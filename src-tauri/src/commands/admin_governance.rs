//! Admin governance IPC commands — Phase 2 SP06-F03.
//!
//! Four command groups:
//!   1. Session visibility   — list_active_sessions, revoke_session
//!   2. Delegation management — list/create/update/delete delegation_policies
//!   3. Emergency elevation   — list/grant/revoke (grant+revoke in admin_users.rs;
//!                              this module adds list_emergency_grants)
//!   4. Role import/export    — export_role_model, import_role_model
//!
//! All mutation commands write to `admin_change_events` (migration 030).

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::rbac::resolver;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

// ═══════════════════════════════════════════════════════════════════════════════
//  DTOs
// ═══════════════════════════════════════════════════════════════════════════════

// ── Session Visibility ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub user_id: String,
    pub username: String,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub device_trust_status: String,
    pub session_started_at: String,
    pub last_activity_at: Option<String>,
    pub is_current_session: bool,
    pub current_role_names: Vec<String>,
}

// ── Delegation Management ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DelegationPolicyView {
    pub id: i64,
    pub admin_role_id: i64,
    pub admin_role_name: String,
    pub managed_scope_type: String,
    pub managed_scope_reference: Option<String>,
    pub allowed_domains: Vec<String>,
    pub requires_step_up_for_publish: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateDelegationInput {
    pub admin_role_id: i64,
    pub managed_scope_type: String,
    pub managed_scope_reference: Option<String>,
    pub allowed_domains: Vec<String>,
    pub requires_step_up_for_publish: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDelegationInput {
    pub policy_id: i64,
    pub allowed_domains: Option<Vec<String>>,
    pub requires_step_up_for_publish: Option<bool>,
}

// ── Emergency Elevation ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct EmergencyGrantView {
    pub assignment_id: i64,
    pub user_id: i64,
    pub username: String,
    pub role_id: i64,
    pub role_name: String,
    pub scope_type: String,
    pub scope_reference: Option<String>,
    pub emergency_reason: Option<String>,
    pub emergency_expires_at: Option<String>,
    pub assigned_by_username: Option<String>,
    pub created_at: String,
    pub is_expired: bool,
}

// ── Role Import/Export ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct RoleExportEntry {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub is_system: bool,
}

#[derive(Debug, Serialize)]
pub struct RoleExportPayload {
    pub roles: Vec<RoleExportEntry>,
    pub exported_at: String,
    pub exported_by: String,
}

#[derive(Debug, Deserialize)]
pub struct RoleImportEntry {
    pub name: String,
    pub description: Option<String>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RoleImportPayload {
    pub roles: Vec<RoleImportEntry>,
}

#[derive(Debug, Serialize)]
pub struct SkippedRole {
    pub name: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub imported_count: u32,
    pub skipped: Vec<SkippedRole>,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Write an immutable row to `admin_change_events`.
pub(crate) async fn record_admin_event(
    db: &sea_orm::DatabaseConnection,
    action: &str,
    actor_id: i64,
    target_user_id: Option<i64>,
    target_role_id: Option<i64>,
    scope_type: Option<&str>,
    scope_reference: Option<&str>,
    summary: Option<&str>,
    diff_json: Option<&str>,
    step_up_used: bool,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO admin_change_events \
         (action, actor_id, target_user_id, target_role_id, \
          scope_type, scope_reference, summary, diff_json, step_up_used) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            action.into(),
            actor_id.into(),
            target_user_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            target_role_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            scope_type
                .map(|s| sea_orm::Value::from(s.to_string()))
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            scope_reference
                .map(|s| sea_orm::Value::from(s.to_string()))
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            summary
                .map(|s| sea_orm::Value::from(s.to_string()))
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            diff_json
                .map(|s| sea_orm::Value::from(s.to_string()))
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            i32::from(step_up_used).into(),
        ],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
//  SESSION VISIBILITY
// ═══════════════════════════════════════════════════════════════════════════════

/// List all active (non-expired, non-revoked) sessions with device and role info.
#[tauri::command]
pub async fn list_active_sessions(
    state: State<'_, AppState>,
) -> AppResult<Vec<SessionSummary>> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);

    // Get the caller's current session id for is_current_session flag
    let current_session_id = {
        let guard = state.session.read().await;
        guard
            .current
            .as_ref()
            .map(|s| s.session_db_id.clone())
            .unwrap_or_default()
    };

    let sql = "\
        SELECT s.id AS session_id, s.user_id, ua.username, \
               s.device_id, td.device_label AS device_name, \
               CASE \
                 WHEN td.id IS NULL THEN 'unknown' \
                 WHEN td.is_revoked = 1 THEN 'revoked' \
                 ELSE 'trusted' \
               END AS device_trust_status, \
               s.created_at AS session_started_at, \
               s.last_activity_at \
        FROM app_sessions s \
        INNER JOIN user_accounts ua ON CAST(ua.id AS TEXT) = s.user_id \
        LEFT JOIN trusted_devices td ON td.id = s.device_id \
        WHERE s.is_revoked = 0 \
          AND s.expires_at > datetime('now') \
        ORDER BY s.created_at DESC";

    let rows = state
        .db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?;

    let mut sessions = Vec::with_capacity(rows.len());
    for r in &rows {
        let session_id: String = r.try_get("", "session_id")?;
        let user_id: String = r.try_get("", "user_id")?;

        // Load role names for this user
        let role_names = load_user_role_names(&state.db, &user_id).await?;

        sessions.push(SessionSummary {
            is_current_session: session_id == current_session_id,
            session_id,
            user_id,
            username: r.try_get("", "username")?,
            device_id: r.try_get("", "device_id").ok(),
            device_name: r.try_get("", "device_name").ok(),
            device_trust_status: r
                .try_get("", "device_trust_status")
                .unwrap_or_else(|_| "unknown".to_string()),
            session_started_at: r.try_get("", "session_started_at")?,
            last_activity_at: r.try_get("", "last_activity_at").ok(),
            current_role_names: role_names,
        });
    }

    Ok(sessions)
}

/// Load active role names for a user (by text user_id from app_sessions).
async fn load_user_role_names(
    db: &sea_orm::DatabaseConnection,
    user_id_text: &str,
) -> AppResult<Vec<String>> {
    let sql = "\
        SELECT DISTINCT r.name \
        FROM user_scope_assignments usa \
        INNER JOIN roles r ON r.id = usa.role_id \
        WHERE CAST(usa.user_id AS TEXT) = ? \
          AND usa.deleted_at IS NULL \
          AND (usa.is_emergency = 0 \
               OR usa.emergency_expires_at > datetime('now'))";

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [user_id_text.to_string().into()],
        ))
        .await?;

    let names = rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .collect();

    Ok(names)
}

/// Revoke a session. Cannot revoke your own current session.
#[tauri::command]
pub async fn revoke_session(
    session_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    require_step_up!(state);

    // Guard: cannot revoke your own current session
    let current_session_id = {
        let guard = state.session.read().await;
        guard
            .current
            .as_ref()
            .map(|s| s.session_db_id.clone())
            .unwrap_or_default()
    };

    if session_id == current_session_id {
        return Err(AppError::ValidationFailed(vec![
            "Cannot revoke your own current session".into(),
        ]));
    }

    // Verify session exists and is active
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT user_id FROM app_sessions \
             WHERE id = ? AND is_revoked = 0 AND expires_at > datetime('now')",
            [session_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "session".into(),
            id: session_id.clone(),
        })?;

    let target_user_id: String = row.try_get("", "user_id")?;

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE app_sessions SET is_revoked = 1 WHERE id = ?",
            [session_id.into()],
        ))
        .await?;

    record_admin_event(
        &state.db,
        "session_revoked",
        i64::from(caller.user_id),
        target_user_id.parse::<i64>().ok(),
        None,
        None,
        None,
        Some(&format!("Session revoked for user {target_user_id}")),
        None,
        true,
    )
    .await?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
//  DELEGATION MANAGEMENT
// ═══════════════════════════════════════════════════════════════════════════════

/// List all delegation policies with the admin role name resolved.
#[tauri::command]
pub async fn list_delegation_policies(
    state: State<'_, AppState>,
) -> AppResult<Vec<DelegationPolicyView>> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.roles", PermissionScope::Global);

    let sql = "\
        SELECT dap.id, dap.admin_role_id, r.name AS admin_role_name, \
               dap.managed_scope_type, dap.managed_scope_reference, \
               dap.allowed_domains_json, dap.requires_step_up_for_publish \
        FROM delegated_admin_policies dap \
        INNER JOIN roles r ON r.id = dap.admin_role_id \
        ORDER BY r.name ASC";

    let rows = state
        .db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?;

    let mut policies = Vec::with_capacity(rows.len());
    for r in &rows {
        let domains_json: String = r
            .try_get("", "allowed_domains_json")
            .unwrap_or_else(|_| "[]".to_string());
        let allowed_domains: Vec<String> =
            serde_json::from_str(&domains_json).unwrap_or_default();

        policies.push(DelegationPolicyView {
            id: r.try_get("", "id")?,
            admin_role_id: r.try_get("", "admin_role_id")?,
            admin_role_name: r.try_get("", "admin_role_name")?,
            managed_scope_type: r.try_get("", "managed_scope_type")?,
            managed_scope_reference: r.try_get("", "managed_scope_reference").ok(),
            allowed_domains,
            requires_step_up_for_publish: r
                .try_get::<i32>("", "requires_step_up_for_publish")
                .map(|v| v != 0)
                .unwrap_or(true),
        });
    }

    Ok(policies)
}

/// Create a new delegation policy.
#[tauri::command]
pub async fn create_delegation_policy(
    input: CreateDelegationInput,
    state: State<'_, AppState>,
) -> AppResult<DelegationPolicyView> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.roles", PermissionScope::Global);
    require_step_up!(state);

    // ── Validate admin_role_id exists ───────────────────────────────────
    let role_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, is_system FROM roles WHERE id = ?",
            [input.admin_role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "role".into(),
            id: input.admin_role_id.to_string(),
        })?;

    let role_name: String = role_row.try_get("", "name")?;

    // ── Validate allowed_domains against known categories ────────────────
    if input.allowed_domains.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "At least one allowed domain is required".into(),
        ]));
    }

    let known_categories = load_known_categories(&state.db).await?;
    let mut invalid_domains = Vec::new();
    for domain in &input.allowed_domains {
        if !known_categories.contains(domain.as_str()) {
            invalid_domains.push(domain.clone());
        }
    }
    if !invalid_domains.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Unknown permission domains: {}",
            invalid_domains.join(", ")
        )]));
    }

    // ── Validate scope_reference exists in org_units if not tenant ───────
    if input.managed_scope_type != "tenant" {
        if let Some(ref scope_ref) = input.managed_scope_reference {
            let exists = state
                .db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM org_units WHERE id = ?",
                    [scope_ref.clone().into()],
                ))
                .await?;
            if exists.is_none() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Scope reference '{scope_ref}' not found in org_units"
                )]));
            }
        }
    }

    let domains_json = serde_json::to_string(&input.allowed_domains)?;

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO delegated_admin_policies \
             (admin_role_id, managed_scope_type, managed_scope_reference, \
              allowed_domains_json, requires_step_up_for_publish) \
             VALUES (?, ?, ?, ?, ?)",
            [
                input.admin_role_id.into(),
                input.managed_scope_type.clone().into(),
                input
                    .managed_scope_reference
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                domains_json.into(),
                i32::from(input.requires_step_up_for_publish).into(),
            ],
        ))
        .await?;

    let id_row = state
        .db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to get last insert id")))?;
    let id: i64 = id_row.try_get("", "id")?;

    record_admin_event(
        &state.db,
        "delegation_policy_created",
        i64::from(caller.user_id),
        None,
        Some(input.admin_role_id),
        Some(&input.managed_scope_type),
        input.managed_scope_reference.as_deref(),
        Some(&format!("Delegation policy created for role '{role_name}'")),
        None,
        true,
    )
    .await?;

    Ok(DelegationPolicyView {
        id,
        admin_role_id: input.admin_role_id,
        admin_role_name: role_name,
        managed_scope_type: input.managed_scope_type,
        managed_scope_reference: input.managed_scope_reference,
        allowed_domains: input.allowed_domains,
        requires_step_up_for_publish: input.requires_step_up_for_publish,
    })
}

/// Update an existing delegation policy.
#[tauri::command]
pub async fn update_delegation_policy(
    input: UpdateDelegationInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.roles", PermissionScope::Global);
    require_step_up!(state);

    // Fetch current policy
    let current = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, allowed_domains_json, requires_step_up_for_publish, admin_role_id \
             FROM delegated_admin_policies WHERE id = ?",
            [input.policy_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "delegation_policy".into(),
            id: input.policy_id.to_string(),
        })?;

    let admin_role_id: i64 = current.try_get("", "admin_role_id")?;
    let old_domains: String = current
        .try_get("", "allowed_domains_json")
        .unwrap_or_else(|_| "[]".to_string());
    let old_step_up: i32 = current
        .try_get("", "requires_step_up_for_publish")
        .unwrap_or(1);

    // Build diff
    let mut diff = serde_json::Map::new();

    if let Some(ref domains) = input.allowed_domains {
        // Validate domains
        let known_categories = load_known_categories(&state.db).await?;
        let mut invalid = Vec::new();
        for d in domains {
            if !known_categories.contains(d.as_str()) {
                invalid.push(d.clone());
            }
        }
        if !invalid.is_empty() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Unknown permission domains: {}",
                invalid.join(", ")
            )]));
        }

        let new_json = serde_json::to_string(domains)?;
        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE delegated_admin_policies SET allowed_domains_json = ?, \
                 updated_at = datetime('now') WHERE id = ?",
                [new_json.into(), input.policy_id.into()],
            ))
            .await?;
        diff.insert("allowed_domains".into(), serde_json::json!({
            "old": serde_json::from_str::<serde_json::Value>(&old_domains).unwrap_or_default(),
            "new": domains,
        }));
    }

    if let Some(step_up) = input.requires_step_up_for_publish {
        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE delegated_admin_policies SET requires_step_up_for_publish = ?, \
                 updated_at = datetime('now') WHERE id = ?",
                [i32::from(step_up).into(), input.policy_id.into()],
            ))
            .await?;
        diff.insert("requires_step_up_for_publish".into(), serde_json::json!({
            "old": old_step_up != 0,
            "new": step_up,
        }));
    }

    let diff_json = serde_json::to_string(&diff)?;
    record_admin_event(
        &state.db,
        "delegation_policy_updated",
        i64::from(caller.user_id),
        None,
        Some(admin_role_id),
        None,
        None,
        Some(&format!("Delegation policy {} updated", input.policy_id)),
        Some(&diff_json),
        true,
    )
    .await?;

    Ok(())
}

/// Delete a delegation policy.
#[tauri::command]
pub async fn delete_delegation_policy(
    policy_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.roles", PermissionScope::Global);
    require_step_up!(state);

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT admin_role_id FROM delegated_admin_policies WHERE id = ?",
            [policy_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "delegation_policy".into(),
            id: policy_id.to_string(),
        })?;
    let admin_role_id: i64 = row.try_get("", "admin_role_id")?;

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM delegated_admin_policies WHERE id = ?",
            [policy_id.into()],
        ))
        .await?;

    record_admin_event(
        &state.db,
        "delegation_policy_deleted",
        i64::from(caller.user_id),
        None,
        Some(admin_role_id),
        None,
        None,
        Some(&format!("Delegation policy {policy_id} deleted")),
        None,
        true,
    )
    .await?;

    Ok(())
}

/// Load the set of known permission category names from the permissions table.
async fn load_known_categories(
    db: &sea_orm::DatabaseConnection,
) -> AppResult<HashSet<String>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT category FROM permissions".to_string(),
        ))
        .await?;

    let categories = rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "category").ok())
        .collect();

    Ok(categories)
}

// ═══════════════════════════════════════════════════════════════════════════════
//  EMERGENCY ELEVATION — listing
// ═══════════════════════════════════════════════════════════════════════════════

/// List emergency grants (both active and recently expired).
/// Grant and revoke commands remain in `admin_users.rs` (already implemented).
#[tauri::command]
pub async fn list_emergency_grants(
    state: State<'_, AppState>,
) -> AppResult<Vec<EmergencyGrantView>> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);

    let sql = "\
        SELECT usa.id AS assignment_id, usa.user_id, ua.username, \
               usa.role_id, r.name AS role_name, \
               usa.scope_type, usa.scope_reference, \
               usa.emergency_reason, usa.emergency_expires_at, \
               usa.created_at, \
               granter.username AS assigned_by_username \
        FROM user_scope_assignments usa \
        INNER JOIN user_accounts ua ON ua.id = usa.user_id \
        INNER JOIN roles r ON r.id = usa.role_id \
        LEFT JOIN user_accounts granter ON granter.id = usa.assigned_by_id \
        WHERE usa.is_emergency = 1 \
          AND usa.deleted_at IS NULL \
        ORDER BY usa.emergency_expires_at DESC";

    let rows = state
        .db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?;

    let now = chrono::Utc::now().to_rfc3339();

    let mut grants = Vec::with_capacity(rows.len());
    for r in &rows {
        let expires_at: Option<String> = r.try_get("", "emergency_expires_at").ok();
        let is_expired = expires_at
            .as_deref()
            .map(|ea| ea < now.as_str())
            .unwrap_or(false);

        grants.push(EmergencyGrantView {
            assignment_id: r.try_get("", "assignment_id")?,
            user_id: r.try_get("", "user_id")?,
            username: r.try_get("", "username")?,
            role_id: r.try_get("", "role_id")?,
            role_name: r.try_get("", "role_name")?,
            scope_type: r.try_get("", "scope_type")?,
            scope_reference: r.try_get("", "scope_reference").ok(),
            emergency_reason: r.try_get("", "emergency_reason").ok(),
            emergency_expires_at: expires_at,
            assigned_by_username: r.try_get("", "assigned_by_username").ok(),
            created_at: r.try_get("", "created_at")?,
            is_expired,
        });
    }

    Ok(grants)
}

// ═══════════════════════════════════════════════════════════════════════════════
//  ROLE IMPORT / EXPORT
// ═══════════════════════════════════════════════════════════════════════════════

/// Export selected roles as a portable JSON payload.
#[tauri::command]
pub async fn export_role_model(
    role_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> AppResult<RoleExportPayload> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.roles", PermissionScope::Global);

    if role_ids.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "At least one role must be selected for export".into(),
        ]));
    }

    let mut roles = Vec::with_capacity(role_ids.len());

    for role_id in &role_ids {
        let role_row = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, name, description, is_system FROM roles WHERE id = ?",
                [(*role_id).into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "role".into(),
                id: role_id.to_string(),
            })?;

        let perm_rows = state
            .db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT p.name \
                 FROM role_permissions rp \
                 INNER JOIN permissions p ON p.id = rp.permission_id \
                 WHERE rp.role_id = ? \
                 ORDER BY p.name",
                [(*role_id).into()],
            ))
            .await?;

        let permissions: Vec<String> = perm_rows
            .iter()
            .filter_map(|r| r.try_get::<String>("", "name").ok())
            .collect();

        roles.push(RoleExportEntry {
            id: role_row.try_get("", "id")?,
            name: role_row.try_get("", "name")?,
            description: role_row.try_get("", "description").ok(),
            permissions,
            is_system: role_row.try_get::<i32>("", "is_system").map(|v| v != 0).unwrap_or(false),
        });
    }

    let exported_at = chrono::Utc::now().to_rfc3339();

    record_admin_event(
        &state.db,
        "role_exported",
        i64::from(caller.user_id),
        None,
        None,
        None,
        None,
        Some(&format!(
            "Exported {} role(s): {}",
            roles.len(),
            roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")
        )),
        None,
        false,
    )
    .await?;

    Ok(RoleExportPayload {
        roles,
        exported_at,
        exported_by: caller.username.clone(),
    })
}

/// Import roles from a portable JSON payload.
///
/// Each role is validated independently. Roles with missing hard dependencies
/// are skipped (not the whole import). Successfully validated roles are
/// inserted as custom, non-system roles.
#[tauri::command]
pub async fn import_role_model(
    input: RoleImportPayload,
    state: State<'_, AppState>,
) -> AppResult<ImportResult> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.roles", PermissionScope::Global);
    require_step_up!(state);

    if input.roles.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Import payload contains no roles".into(),
        ]));
    }

    let mut imported_count: u32 = 0;
    let mut skipped = Vec::new();

    for entry in &input.roles {
        // ── Validate permission set ──────────────────────────────────────
        let names: HashSet<String> = entry.permissions.iter().cloned().collect();

        // Check for unknown permissions
        let mut errors = Vec::new();
        if !names.is_empty() {
            let placeholders: String = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT name FROM permissions WHERE name IN ({placeholders})"
            );
            let values: Vec<sea_orm::Value> = names.iter().map(|n| n.clone().into()).collect();
            let rows = state
                .db
                .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
                .await?;
            let known: HashSet<String> = rows
                .iter()
                .filter_map(|r| r.try_get::<String>("", "name").ok())
                .collect();
            let unknown: Vec<&String> = names.iter().filter(|n| !known.contains(*n)).collect();
            for u in &unknown {
                errors.push(format!("Unknown permission: {u}"));
            }
        }

        // Check hard dependency violations
        let deps = resolver::dependency_warnings_for(&state.db, &names).await?;
        for dep in &deps {
            if dep.dependency_type == "hard" && !names.contains(&dep.required_permission_name) {
                errors.push(format!(
                    "{} requires {} (hard dependency)",
                    dep.permission_name, dep.required_permission_name
                ));
            }
        }

        if !errors.is_empty() {
            skipped.push(SkippedRole {
                name: entry.name.clone(),
                errors,
            });
            continue;
        }

        // ── Check for name collision ─────────────────────────────────────
        let existing = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM roles WHERE name = ?",
                [entry.name.clone().into()],
            ))
            .await?;

        if existing.is_some() {
            skipped.push(SkippedRole {
                name: entry.name.clone(),
                errors: vec![format!("Role '{}' already exists", entry.name)],
            });
            continue;
        }

        // ── Insert role ──────────────────────────────────────────────────
        let now = chrono::Utc::now().to_rfc3339();
        let sync_id = Uuid::new_v4().to_string();

        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO roles \
                 (sync_id, name, description, role_type, status, is_system, \
                  created_at, updated_at, row_version) \
                 VALUES (?, ?, ?, 'custom', 'active', 0, ?, ?, 1)",
                [
                    sync_id.into(),
                    entry.name.clone().into(),
                    entry
                        .description
                        .clone()
                        .map(sea_orm::Value::from)
                        .unwrap_or(sea_orm::Value::from(None::<String>)),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;

        let role_id_row = state
            .db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to get role id")))?;
        let new_role_id: i64 = role_id_row.try_get("", "id")?;

        // ── Insert role_permissions ───────────────────────────────────────
        for perm_name in &entry.permissions {
            state
                .db
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT OR IGNORE INTO role_permissions (role_id, permission_id) \
                     SELECT ?, p.id FROM permissions p WHERE p.name = ?",
                    [new_role_id.into(), perm_name.clone().into()],
                ))
                .await?;
        }

        // ── Audit event ──────────────────────────────────────────────────
        let diff_json = serde_json::to_string(&entry.permissions)?;
        record_admin_event(
            &state.db,
            "role_imported",
            i64::from(caller.user_id),
            None,
            Some(new_role_id),
            None,
            None,
            Some(&format!("Role '{}' imported with {} permissions", entry.name, entry.permissions.len())),
            Some(&diff_json),
            true,
        )
        .await?;

        imported_count += 1;
    }

    Ok(ImportResult {
        imported_count,
        skipped,
    })
}
