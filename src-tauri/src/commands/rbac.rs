//! RBAC IPC commands.
//!
//! Exposes permission queries and step-up verification to the frontend.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::password_policy::PasswordPolicy;
use crate::auth::rbac;
use crate::auth::rbac::PermissionScope;
use crate::auth::{password, session_manager};
use crate::errors::{AppError, AppResult};
use crate::{require_permission, require_session};
use crate::state::AppState;

/// Return the effective permission set for the currently authenticated user.
///
/// The frontend calls this after login to pre-load permissions into the
/// `usePermissions` hook so that `<PermissionGate>` can work without
/// round-tripping to the backend for every check.
#[tauri::command]
pub async fn get_my_permissions(state: State<'_, AppState>) -> AppResult<Vec<rbac::PermissionRecord>> {
    let user = require_session!(state);
    rbac::get_user_permissions(&state.db, user.user_id).await
}

/// Input for the `verify_step_up` command.
#[derive(Debug, Deserialize)]
pub struct StepUpRequest {
    pub password: String,
}

/// Response from a successful step-up verification.
#[derive(Debug, Serialize)]
pub struct StepUpResponse {
    pub success: bool,
    pub expires_at: String,
}

/// Generic key/value row returned from `rbac_settings`.
#[derive(Debug, Serialize)]
pub struct RbacSettingEntry {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
}

/// Password policy settings consumed by the admin panel.
#[derive(Debug, Serialize)]
pub struct PasswordPolicySettings {
    pub max_age_days: i64,
    pub warn_days_before_expiry: i64,
    pub min_length: usize,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_digit: bool,
    pub require_special: bool,
}

/// Verify the user's password for a dangerous-action step-up.
///
/// On success, records the verification timestamp in the in-memory session.
/// The step-up window (`STEP_UP_DURATION_SECS`) starts from this moment.
#[tauri::command]
pub async fn verify_step_up(payload: StepUpRequest, state: State<'_, AppState>) -> AppResult<StepUpResponse> {
    let user = require_session!(state);

    // Look up the current password hash from the database
    let user_record = session_manager::find_active_user(&state.db, &user.username).await?;

    let pw_hash = match user_record {
        Some(row) => match row.5 {
            Some(hash) => hash,
            None => {
                return Err(AppError::Auth("Aucun mot de passe configuré.".into()));
            }
        },
        None => {
            return Err(AppError::Auth("Utilisateur introuvable.".into()));
        }
    };

    // Verify the provided password against the stored hash
    let valid = password::verify_password(&payload.password, &pw_hash)?;
    if !valid {
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::STEP_UP_FAILURE,
                actor_id: Some(user.user_id),
                summary: "Step-up reauthentication failed: wrong password",
                ..Default::default()
            },
        )
        .await;
        return Err(AppError::Auth("Mot de passe incorrect.".into()));
    }

    // Record the step-up timestamp in the session
    let mut sm = state.session.write().await;
    sm.record_step_up();

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::STEP_UP_SUCCESS,
            actor_id: Some(user.user_id),
            summary: "Step-up reauthentication verified",
            ..Default::default()
        },
    )
    .await;

    let expires_at =
        (chrono::Utc::now() + chrono::TimeDelta::seconds(session_manager::STEP_UP_DURATION_SECS as i64)).to_rfc3339();

    Ok(StepUpResponse {
        success: true,
        expires_at,
    })
}

/// List RBAC settings by key prefix. Requires `adm.settings`.
#[tauri::command]
pub async fn get_rbac_settings(prefix: String, state: State<'_, AppState>) -> AppResult<Vec<RbacSettingEntry>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);

    let pattern = format!("{}%", prefix.trim());
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT key, value, description
               FROM rbac_settings
               WHERE key LIKE ?
               ORDER BY key",
            [pattern.into()],
        ))
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| RbacSettingEntry {
            key: row.try_get::<String>("", "key").unwrap_or_default(),
            value: row.try_get::<String>("", "value").unwrap_or_default(),
            description: row.try_get::<Option<String>>("", "description").unwrap_or(None),
        })
        .collect())
}

/// Update one `rbac_settings` value. Requires `adm.settings`.
#[tauri::command]
pub async fn update_rbac_setting(key: String, value: String, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);

    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "La clé de configuration est obligatoire.".into(),
        ]));
    }

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO rbac_settings (key, value, description)
              VALUES (?, ?, NULL)
              ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key.into(), value.into()],
        ))
        .await?;

    Ok(())
}

/// Return the resolved password policy used by login/password-change flows.
/// Requires `adm.settings`.
#[tauri::command]
pub async fn get_password_policy(state: State<'_, AppState>) -> AppResult<PasswordPolicySettings> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);

    let policy = PasswordPolicy::load(&state.db).await;
    Ok(PasswordPolicySettings {
        max_age_days: policy.max_age_days,
        warn_days_before_expiry: policy.warn_days_before_expiry,
        min_length: policy.min_length,
        require_uppercase: policy.require_uppercase,
        require_lowercase: policy.require_lowercase,
        require_digit: policy.require_digit,
        require_special: policy.require_special,
    })
}
