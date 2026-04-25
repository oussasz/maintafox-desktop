//! Admin audit read commands — Phase 2 SP06-F04.
//!
//! Read-only commands for the `admin_change_events` immutable audit ledger.
//! No mutation, purge, or delete operations exist by design.
//!
//! Permission-based event filtering:
//!   - `adm.permissions` → all events visible
//!   - `adm.users` only  → user-related events only
//!   - `adm.roles` only  → role-related events only
//!
//! Prerequisites: migration 030 (admin_change_events table).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_session};

// ═══════════════════════════════════════════════════════════════════════════════
//  ACTION DOMAIN SETS
// ═══════════════════════════════════════════════════════════════════════════════

/// Actions visible to holders of `adm.users` (user-lifecycle events).
const USER_ACTIONS: &[&str] = &[
    "user_created",
    "user_deactivated",
    "role_assigned",
    "role_revoked",
    "session_revoked",
    "emergency_grant_created",
    "emergency_grant_revoked",
    "emergency_grant_expired",
];

/// Actions visible to holders of `adm.roles` (role-governance events).
const ROLE_ACTIONS: &[&str] = &[
    "role_created",
    "role_updated",
    "role_deleted",
    "role_retired",
    "role_imported",
    "role_exported",
    "permission_granted",
    "permission_revoked",
    "delegation_policy_created",
    "delegation_policy_updated",
    "delegation_policy_deleted",
];

// ═══════════════════════════════════════════════════════════════════════════════
//  DTOs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct AdminEventFilter {
    pub action: Option<String>,
    pub actor_id: Option<i64>,
    pub target_user_id: Option<i64>,
    pub target_role_id: Option<i64>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub apply_result: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminChangeEventDetail {
    pub id: i64,
    pub action: String,
    pub actor_id: Option<i64>,
    pub actor_username: Option<String>,
    pub target_user_id: Option<i64>,
    pub target_username: Option<String>,
    pub target_role_id: Option<i64>,
    pub target_role_name: Option<String>,
    pub acted_at: String,
    pub scope_type: Option<String>,
    pub scope_reference: Option<String>,
    pub summary: Option<String>,
    pub diff_json: Option<String>,
    pub step_up_used: bool,
    pub apply_result: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Determine which action domains the caller can see, based on permissions.
///
/// Returns `None` when the caller holds `adm.permissions` (= see everything).
/// Returns `Some(allowed_actions)` when restricted to a subset.
async fn resolve_allowed_actions(state: &AppState, user_id: i32) -> AppResult<Option<Vec<&'static str>>> {
    let has_full = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user_id,
        "adm.permissions",
        &PermissionScope::Global,
    )
    .await?;

    if has_full {
        return Ok(None); // No filter — all events visible
    }

    let has_users = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user_id,
        "adm.users",
        &PermissionScope::Global,
    )
    .await?;

    let has_roles = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user_id,
        "adm.roles",
        &PermissionScope::Global,
    )
    .await?;

    let mut allowed: Vec<&'static str> = Vec::new();
    if has_users {
        allowed.extend_from_slice(USER_ACTIONS);
    }
    if has_roles {
        allowed.extend_from_slice(ROLE_ACTIONS);
    }

    Ok(Some(allowed))
}

/// Parse a row from `admin_change_events` with actor/target name joins.
fn parse_event_row(r: &sea_orm::QueryResult) -> AppResult<AdminChangeEventDetail> {
    Ok(AdminChangeEventDetail {
        id: r.try_get("", "id")?,
        action: r.try_get("", "action")?,
        actor_id: r.try_get::<Option<i64>>("", "actor_id").unwrap_or(None),
        actor_username: r
            .try_get::<Option<String>>("", "actor_username")
            .unwrap_or(None),
        target_user_id: r
            .try_get::<Option<i64>>("", "target_user_id")
            .unwrap_or(None),
        target_username: r
            .try_get::<Option<String>>("", "target_username")
            .unwrap_or(None),
        target_role_id: r
            .try_get::<Option<i64>>("", "target_role_id")
            .unwrap_or(None),
        target_role_name: r
            .try_get::<Option<String>>("", "target_role_name")
            .unwrap_or(None),
        acted_at: r.try_get("", "acted_at")?,
        scope_type: r
            .try_get::<Option<String>>("", "scope_type")
            .unwrap_or(None),
        scope_reference: r
            .try_get::<Option<String>>("", "scope_reference")
            .unwrap_or(None),
        summary: r
            .try_get::<Option<String>>("", "summary")
            .unwrap_or(None),
        diff_json: r
            .try_get::<Option<String>>("", "diff_json")
            .unwrap_or(None),
        step_up_used: r
            .try_get::<i32>("", "step_up_used")
            .unwrap_or(0)
            != 0,
        apply_result: r
            .try_get::<String>("", "apply_result")
            .unwrap_or_else(|_| "applied".to_string()),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
//  COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

/// List admin change events with filtering and permission-based action scoping.
///
/// Callers must hold at least one of `adm.users`, `adm.roles`, or `adm.permissions`.
/// The visible action set is automatically restricted based on the caller's permissions.
#[tauri::command]
pub async fn list_admin_events(
    state: State<'_, AppState>,
    filter: AdminEventFilter,
) -> AppResult<Vec<AdminChangeEventDetail>> {
    let caller = require_session!(state);

    // Gate: at least one admin permission required
    let has_users = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        caller.user_id,
        "adm.users",
        &PermissionScope::Global,
    )
    .await?;
    let has_roles = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        caller.user_id,
        "adm.roles",
        &PermissionScope::Global,
    )
    .await?;
    let has_perms = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        caller.user_id,
        "adm.permissions",
        &PermissionScope::Global,
    )
    .await?;

    if !has_users && !has_roles && !has_perms {
        return Err(AppError::PermissionDenied(
            "Permission requise : adm.users, adm.roles, ou adm.permissions".into(),
        ));
    }

    // Resolve action domain restriction
    let allowed_actions = resolve_allowed_actions(&state, caller.user_id).await?;

    // ── Build dynamic SQL ────────────────────────────────────────────────
    let base = "\
        SELECT ace.id, ace.action, ace.actor_id, ace.target_user_id, \
               ace.target_role_id, ace.acted_at, ace.scope_type, \
               ace.scope_reference, ace.summary, ace.diff_json, \
               ace.step_up_used, ace.apply_result, \
               ua_actor.username  AS actor_username, \
               ua_target.username AS target_username, \
               r_target.name     AS target_role_name \
        FROM admin_change_events ace \
        LEFT JOIN user_accounts ua_actor  ON ua_actor.id  = ace.actor_id \
        LEFT JOIN user_accounts ua_target ON ua_target.id = ace.target_user_id \
        LEFT JOIN roles r_target          ON r_target.id  = ace.target_role_id";

    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    // Permission-based action filter
    if let Some(ref actions) = allowed_actions {
        if actions.is_empty() {
            // No permissions at all — return empty (shouldn't reach here due to gate)
            return Ok(Vec::new());
        }
        let placeholders: String = actions.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        conditions.push(format!("ace.action IN ({placeholders})"));
        for a in actions {
            values.push((*a).into());
        }
    }

    // User-supplied filters
    if let Some(ref action) = filter.action {
        conditions.push("ace.action = ?".to_string());
        values.push(action.clone().into());
    }
    if let Some(actor_id) = filter.actor_id {
        conditions.push("ace.actor_id = ?".to_string());
        values.push(actor_id.into());
    }
    if let Some(target_user_id) = filter.target_user_id {
        conditions.push("ace.target_user_id = ?".to_string());
        values.push(target_user_id.into());
    }
    if let Some(target_role_id) = filter.target_role_id {
        conditions.push("ace.target_role_id = ?".to_string());
        values.push(target_role_id.into());
    }
    if let Some(ref date_from) = filter.date_from {
        conditions.push("ace.acted_at >= ?".to_string());
        values.push(date_from.clone().into());
    }
    if let Some(ref date_to) = filter.date_to {
        conditions.push("ace.acted_at <= ?".to_string());
        values.push(date_to.clone().into());
    }
    if let Some(ref apply_result) = filter.apply_result {
        conditions.push("ace.apply_result = ?".to_string());
        values.push(apply_result.clone().into());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let limit = filter.limit.unwrap_or(100).min(500);
    let offset = filter.offset.unwrap_or(0).max(0);

    let sql = format!(
        "{base}{where_clause} ORDER BY ace.acted_at DESC LIMIT ? OFFSET ?"
    );
    values.push(limit.into());
    values.push(offset.into());

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            values,
        ))
        .await?;

    let mut events = Vec::with_capacity(rows.len());
    for r in &rows {
        events.push(parse_event_row(r)?);
    }

    Ok(events)
}

/// Get a single admin change event by ID with full diff_json.
///
/// Permission-based action scoping applies: if the event's action is outside
/// the caller's allowed domain, it returns NotFound (not PermissionDenied)
/// to avoid information leakage.
#[tauri::command]
pub async fn get_admin_event(
    state: State<'_, AppState>,
    event_id: i64,
) -> AppResult<AdminChangeEventDetail> {
    let caller = require_session!(state);

    // Gate: at least one admin permission required
    let has_users = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        caller.user_id,
        "adm.users",
        &PermissionScope::Global,
    )
    .await?;
    let has_roles = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        caller.user_id,
        "adm.roles",
        &PermissionScope::Global,
    )
    .await?;
    let has_perms = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        caller.user_id,
        "adm.permissions",
        &PermissionScope::Global,
    )
    .await?;

    if !has_users && !has_roles && !has_perms {
        return Err(AppError::PermissionDenied(
            "Permission requise : adm.users, adm.roles, ou adm.permissions".into(),
        ));
    }

    let sql = "\
        SELECT ace.id, ace.action, ace.actor_id, ace.target_user_id, \
               ace.target_role_id, ace.acted_at, ace.scope_type, \
               ace.scope_reference, ace.summary, ace.diff_json, \
               ace.step_up_used, ace.apply_result, \
               ua_actor.username  AS actor_username, \
               ua_target.username AS target_username, \
               r_target.name     AS target_role_name \
        FROM admin_change_events ace \
        LEFT JOIN user_accounts ua_actor  ON ua_actor.id  = ace.actor_id \
        LEFT JOIN user_accounts ua_target ON ua_target.id = ace.target_user_id \
        LEFT JOIN roles r_target          ON r_target.id  = ace.target_role_id \
        WHERE ace.id = ?";

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [event_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "admin_change_event".to_string(),
            id: event_id.to_string(),
        })?;

    let event = parse_event_row(&row)?;

    // Scope check: ensure the event's action is within the caller's allowed domain
    let allowed_actions = resolve_allowed_actions(&state, caller.user_id).await?;
    if let Some(ref actions) = allowed_actions {
        if !actions.contains(&event.action.as_str()) {
            // Return NotFound to avoid leaking existence of events outside scope
            return Err(AppError::NotFound {
                entity: "admin_change_event".to_string(),
                id: event_id.to_string(),
            });
        }
    }

    Ok(event)
}
