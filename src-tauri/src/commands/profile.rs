//! User profile IPC commands.
//!
//! Commands:
//!   get_my_profile      — return profile for the logged-in user
//!   update_my_profile   — update display name, email, phone, language
//!   change_password     — change own password (requires current password)
//!   get_session_history — return recent sessions from app_sessions

use chrono::Utc;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::{device, password, session_manager};
use crate::errors::{AppError, AppResult};
use crate::require_session;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Response types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize)]
pub struct UserProfile {
    pub id: i32,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub language: Option<String>,
    pub identity_mode: String,
    pub created_at: String,
    pub password_changed_at: Option<String>,
    pub pin_configured: bool,
    pub role_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionHistoryEntry {
    pub id: String,
    pub device_label: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub status: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Input types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct UpdateProfilePayload {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordPayload {
    pub current_password: String,
    pub new_password: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) get_my_profile
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_my_profile(state: State<'_, AppState>) -> AppResult<UserProfile> {
    let user = require_session!(state);

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT u.id, u.username, u.display_name,
                     u.identity_mode, u.created_at,
                     u.password_changed_at,
                     CASE WHEN u.pin_hash IS NOT NULL THEN 1 ELSE 0 END AS pin_configured,
                     r.name AS role_name
              FROM user_accounts u
              LEFT JOIN user_scope_assignments usa ON usa.user_id = u.id AND usa.deleted_at IS NULL
              LEFT JOIN roles r ON r.id = usa.role_id
              WHERE u.id = ? AND u.deleted_at IS NULL",
            [user.user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UserProfile".into(),
            id: user.user_id.to_string(),
        })?;

    Ok(UserProfile {
        id: row.try_get::<i32>("", "id").unwrap_or(0),
        username: row
            .try_get::<String>("", "username")
            .unwrap_or_default(),
        display_name: row
            .try_get::<Option<String>>("", "display_name")
            .unwrap_or(None),
        // email, phone, language not yet in user_accounts schema — return null
        email: None,
        phone: None,
        language: None,
        identity_mode: row
            .try_get::<String>("", "identity_mode")
            .unwrap_or_else(|_| "local".to_string()),
        created_at: row
            .try_get::<String>("", "created_at")
            .unwrap_or_default(),
        password_changed_at: row
            .try_get::<Option<String>>("", "password_changed_at")
            .unwrap_or(None),
        pin_configured: row.try_get::<i32>("", "pin_configured").unwrap_or(0) == 1,
        role_name: row
            .try_get::<Option<String>>("", "role_name")
            .unwrap_or(None),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) update_my_profile
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_my_profile(
    payload: UpdateProfilePayload,
    state: State<'_, AppState>,
) -> AppResult<UserProfile> {
    let user = require_session!(state);
    let now = Utc::now().to_rfc3339();

    // Build dynamic SET clause for only provided fields
    let mut set_parts: Vec<String> = Vec::new();
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref dn) = payload.display_name {
        let trimmed = dn.trim();
        if trimmed.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Le nom d'affichage ne peut pas être vide.".into(),
            ]));
        }
        set_parts.push("display_name = ?".into());
        binds.push(trimmed.to_string().into());
    }

    // Note: email, phone, and language columns do not yet exist in
    // user_accounts. When migration adds them, uncomment below.

    if set_parts.is_empty() {
        // Nothing to update — just return current profile
        return get_my_profile(state).await;
    }

    set_parts.push("updated_at = ?".into());
    binds.push(now.into());
    binds.push(user.user_id.into());

    let sql = format!(
        "UPDATE user_accounts SET {} WHERE id = ?",
        set_parts.join(", ")
    );

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
        .await?;

    // Update in-memory session display_name if changed
    if let Some(ref dn) = payload.display_name {
        let mut sm = state.session.write().await;
        if let Some(session) = &mut sm.current {
            session.user.display_name = Some(dn.trim().to_string());
        }
    }

    get_my_profile(state).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) change_password
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn change_password(
    payload: ChangePasswordPayload,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);

    // Validate new password strength
    let new_password = payload.new_password.trim();
    if new_password.len() < 8 {
        return Err(AppError::ValidationFailed(vec![
            "Le mot de passe doit contenir au moins 8 caractères.".into(),
        ]));
    }

    // Fetch current hash to verify old password
    let record = session_manager::find_active_user(&state.db, &user.username)
        .await?
        .ok_or_else(|| AppError::Auth("Utilisateur introuvable.".into()))?;

    let stored_hash = record
        .5
        .ok_or_else(|| AppError::Auth("Aucun mot de passe local configuré.".into()))?;

    // Verify current password
    let ok = password::verify_password(&payload.current_password, &stored_hash)?;
    if !ok {
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: "user.password_change_failed",
                actor_id: Some(user.user_id),
                summary: "Password change failed — wrong current password",
                ..Default::default()
            },
        )
        .await;
        return Err(AppError::Auth("Le mot de passe actuel est incorrect.".into()));
    }

    // Hash and save new password
    let new_hash = password::hash_password(new_password)?;
    let now = Utc::now().to_rfc3339();

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"UPDATE user_accounts
               SET password_hash = ?,
                   password_changed_at = ?,
                   updated_at = ?
               WHERE id = ?",
            [
                new_hash.into(),
                now.clone().into(),
                now.into(),
                user.user_id.into(),
            ],
        ))
        .await?;

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::PASSWORD_CHANGED,
            actor_id: Some(user.user_id),
            summary: "Password changed by user",
            ..Default::default()
        },
    )
    .await;

    tracing::info!(user_id = user.user_id, "change_password completed");

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) get_session_history
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_session_history(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SessionHistoryEntry>> {
    let user = require_session!(state);
    let row_limit = limit.unwrap_or(10).min(50).max(1);

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                s.id,
                td.device_label,
                s.created_at AS started_at,
                s.last_activity_at AS ended_at,
                CASE
                  WHEN s.last_activity_at IS NOT NULL
                  THEN CAST((julianday(s.last_activity_at) - julianday(s.created_at)) * 86400 AS INTEGER)
                  ELSE NULL
                END AS duration_seconds,
                CASE
                  WHEN s.is_revoked = 1 THEN 'revoked'
                  WHEN s.expires_at < datetime('now') THEN 'expired'
                  ELSE 'active'
                END AS status
              FROM app_sessions s
              LEFT JOIN trusted_devices td ON td.device_fingerprint = s.device_id
              WHERE s.user_id = ?
              ORDER BY s.created_at DESC
              LIMIT ?",
            [user.user_id.to_string().into(), row_limit.into()],
        ))
        .await?;

    let mut entries = Vec::with_capacity(rows.len());
    for row in &rows {
        entries.push(SessionHistoryEntry {
            id: row.try_get::<String>("", "id").unwrap_or_default(),
            device_label: row
                .try_get::<Option<String>>("", "device_label")
                .unwrap_or(None),
            started_at: row
                .try_get::<String>("", "started_at")
                .unwrap_or_default(),
            ended_at: row
                .try_get::<Option<String>>("", "ended_at")
                .unwrap_or(None),
            duration_seconds: row
                .try_get::<Option<i64>>("", "duration_seconds")
                .unwrap_or(None),
            status: row
                .try_get::<String>("", "status")
                .unwrap_or_else(|_| "unknown".to_string()),
        });
    }

    Ok(entries)
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) list_trusted_devices
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize)]
pub struct TrustedDeviceEntry {
    pub id: String,
    pub device_label: Option<String>,
    pub trusted_at: String,
    pub last_seen_at: Option<String>,
    pub is_revoked: bool,
}

#[tauri::command]
pub async fn list_trusted_devices(
    state: State<'_, AppState>,
) -> AppResult<Vec<TrustedDeviceEntry>> {
    let user = require_session!(state);

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT id, device_label, trusted_at, last_seen_at, is_revoked
              FROM trusted_devices
              WHERE user_id = ?
              ORDER BY trusted_at DESC",
            [user.user_id.into()],
        ))
        .await?;

    let mut entries = Vec::with_capacity(rows.len());
    for row in &rows {
        entries.push(TrustedDeviceEntry {
            id: row.try_get::<String>("", "id").unwrap_or_default(),
            device_label: row
                .try_get::<Option<String>>("", "device_label")
                .unwrap_or(None),
            trusted_at: row
                .try_get::<String>("", "trusted_at")
                .unwrap_or_default(),
            last_seen_at: row
                .try_get::<Option<String>>("", "last_seen_at")
                .unwrap_or(None),
            is_revoked: row.try_get::<i32>("", "is_revoked").unwrap_or(0) == 1,
        });
    }

    Ok(entries)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) revoke_my_device
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn revoke_my_device(
    device_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    device::revoke_device_trust(&state.db, &device_id, user.user_id).await?;

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::DEVICE_TRUST_REVOKED,
            actor_id: Some(user.user_id),
            entity_type: Some("trusted_device"),
            entity_id: Some(&device_id),
            summary: "Device trust revoked by user from profile page",
            ..Default::default()
        },
    )
    .await;

    Ok(())
}
