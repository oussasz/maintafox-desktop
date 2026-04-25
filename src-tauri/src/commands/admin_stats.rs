//! Admin statistics IPC command — Sprint S4 GAP-06.
//!
//! Provides aggregated metrics for the AdminMetricCards component.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[derive(Debug, Serialize)]
pub struct AdminStatsPayload {
    pub active_users: i64,
    pub inactive_users: i64,
    pub total_roles: i64,
    pub system_roles: i64,
    pub custom_roles: i64,
    pub active_sessions: i64,
    pub unassigned_users: i64,
    pub emergency_grants_active: i64,
}

/// Return aggregated admin KPI stats.
///
/// Uses CTEs to compute each metric in a single round-trip.
#[tauri::command]
pub async fn get_admin_stats(state: State<'_, AppState>) -> AppResult<AdminStatsPayload> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);

    let sql = r#"
        WITH
        user_counts AS (
            SELECT
                SUM(CASE WHEN is_active = 1 AND deleted_at IS NULL THEN 1 ELSE 0 END) AS active_users,
                SUM(CASE WHEN is_active = 0 AND deleted_at IS NULL THEN 1 ELSE 0 END) AS inactive_users
            FROM user_accounts
        ),
        role_counts AS (
            SELECT
                COUNT(*)                                       AS total_roles,
                SUM(CASE WHEN is_system = 1 THEN 1 ELSE 0 END) AS system_roles,
                SUM(CASE WHEN is_system = 0 THEN 1 ELSE 0 END) AS custom_roles
            FROM roles
            WHERE status != 'retired'
        ),
        session_counts AS (
            SELECT COUNT(*) AS active_sessions
            FROM app_sessions
            WHERE is_revoked = 0
              AND expires_at > datetime('now')
        ),
        unassigned AS (
            SELECT COUNT(*) AS unassigned_users
            FROM user_accounts ua
            WHERE ua.is_active = 1
              AND ua.deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1 FROM user_scope_assignments usa
                  WHERE usa.user_id = ua.id
                    AND usa.deleted_at IS NULL
                    AND (usa.valid_to IS NULL OR usa.valid_to >= date('now'))
              )
        ),
        emergency AS (
            SELECT COUNT(*) AS emergency_grants_active
            FROM user_scope_assignments
            WHERE is_emergency = 1
              AND deleted_at IS NULL
              AND (emergency_expires_at IS NULL OR emergency_expires_at > datetime('now'))
        )
        SELECT
            COALESCE(uc.active_users, 0)          AS active_users,
            COALESCE(uc.inactive_users, 0)        AS inactive_users,
            COALESCE(rc.total_roles, 0)           AS total_roles,
            COALESCE(rc.system_roles, 0)          AS system_roles,
            COALESCE(rc.custom_roles, 0)          AS custom_roles,
            COALESCE(sc.active_sessions, 0)       AS active_sessions,
            COALESCE(un.unassigned_users, 0)      AS unassigned_users,
            COALESCE(em.emergency_grants_active, 0) AS emergency_grants_active
        FROM user_counts uc, role_counts rc, session_counts sc, unassigned un, emergency em
    "#;

    let row = state
        .db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
        .await?;

    match row {
        Some(r) => Ok(AdminStatsPayload {
            active_users: r.try_get("", "active_users").unwrap_or(0),
            inactive_users: r.try_get("", "inactive_users").unwrap_or(0),
            total_roles: r.try_get("", "total_roles").unwrap_or(0),
            system_roles: r.try_get("", "system_roles").unwrap_or(0),
            custom_roles: r.try_get("", "custom_roles").unwrap_or(0),
            active_sessions: r.try_get("", "active_sessions").unwrap_or(0),
            unassigned_users: r.try_get("", "unassigned_users").unwrap_or(0),
            emergency_grants_active: r.try_get("", "emergency_grants_active").unwrap_or(0),
        }),
        None => Ok(AdminStatsPayload {
            active_users: 0,
            inactive_users: 0,
            total_roles: 0,
            system_roles: 0,
            custom_roles: 0,
            active_sessions: 0,
            unassigned_users: 0,
            emergency_grants_active: 0,
        }),
    }
}
