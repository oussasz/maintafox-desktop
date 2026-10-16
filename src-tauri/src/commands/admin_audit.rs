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
use crate::require_session;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
//  ACTION DOMAIN SETS
// ═══════════════════════════════════════════════════════════════════════════════

/// Actions visible to holders of `adm.users` (user-lifecycle events).
pub(crate) const USER_ACTIONS: &[&str] = &[
    "user_created",
    "user_updated",
    "user_deactivated",
    "user_reactivated",
    "role_assigned",
    "role_revoked",
    "session_revoked",
    "emergency_grant_created",
    "emergency_grant_revoked",
    "emergency_grant_expired",
    "account_locked",
    "account_unlocked",
    "password_reset_by_admin",
];

/// Actions visible to holders of `adm.roles` (role-governance events).
pub(crate) const ROLE_ACTIONS: &[&str] = &[
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

/// Most rows one export returns (the screen lists 500 at a time).
const EXPORT_MAX_ROWS: i64 = 20_000;

// ═══════════════════════════════════════════════════════════════════════════════
//  DTOs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Default, Deserialize)]
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
    /// Received from another device before history ids travelled as sync ids: the ids inside
    /// this row (target, scope, summary/detail ids) are that device's local ids, not this one's.
    pub from_other_device: bool,
}

/// One choice of a ledger filter (an actor, a target user or a target role).
#[derive(Debug, Serialize)]
pub struct AdminEventFacetOption {
    pub id: i64,
    pub label: String,
    pub count: i64,
}

/// Choices for the ledger filters, taken from the events the caller may see.
#[derive(Debug, Serialize)]
pub struct AdminEventFacets {
    pub actions: Vec<String>,
    pub actors: Vec<AdminEventFacetOption>,
    pub target_users: Vec<AdminEventFacetOption>,
    pub target_roles: Vec<AdminEventFacetOption>,
    pub apply_results: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

async fn holds(state: &AppState, user_id: i32, permission: &str) -> AppResult<bool> {
    crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user_id,
        permission,
        &PermissionScope::Global,
    )
    .await
}

/// Gate: the caller needs one of `adm.users`, `adm.roles`, `adm.permissions`. Returns the visible
/// action set (`None` = every action, for `adm.permissions`).
async fn visible_actions(state: &AppState, user_id: i32) -> AppResult<Option<Vec<&'static str>>> {
    let has_perms = holds(state, user_id, crate::rbac::permissions::ADM_PERMISSIONS).await?;
    let has_users = holds(state, user_id, crate::rbac::permissions::ADM_USERS).await?;
    let has_roles = holds(state, user_id, crate::rbac::permissions::ADM_ROLES).await?;

    if !has_users && !has_roles && !has_perms {
        return Err(AppError::PermissionDenied(
            "Permission requise : adm.users, adm.roles, ou adm.permissions".into(),
        ));
    }
    if has_perms {
        return Ok(None);
    }
    let mut allowed: Vec<&'static str> = Vec::new();
    if has_users {
        allowed.extend_from_slice(USER_ACTIONS);
    }
    if has_roles {
        allowed.extend_from_slice(ROLE_ACTIONS);
    }
    Ok(Some(allowed))
}

/// A filter bound: a date (`YYYY-MM-DD`, start or end of that day) or a full timestamp,
/// as UTC `YYYY-MM-DDTHH:MM:SSZ` (the stored `acted_at` format).
fn normalize_bound(raw: &str, end_of_day: bool) -> AppResult<String> {
    let value = raw.trim();
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let suffix = if end_of_day { "T23:59:59Z" } else { "T00:00:00Z" };
        return Ok(format!("{}{suffix}", date.format("%Y-%m-%d")));
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .map_err(|_| AppError::ValidationFailed(vec![format!("'{value}' is not a date (YYYY-MM-DD).")]))
}

/// WHERE clause + values for the caller's visible actions and the user filters.
fn build_conditions(
    allowed: Option<&[&'static str]>,
    filter: &AdminEventFilter,
) -> AppResult<(Vec<String>, Vec<sea_orm::Value>)> {
    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(actions) = allowed {
        if actions.is_empty() {
            conditions.push("1 = 0".to_string());
        } else {
            let placeholders: String = actions.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            conditions.push(format!("ace.action IN ({placeholders})"));
            for a in actions {
                values.push((*a).into());
            }
        }
    }
    if let Some(action) = filter.action.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        conditions.push("ace.action = ?".to_string());
        values.push(action.to_string().into());
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
    let from = match filter.date_from.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(d) => Some(normalize_bound(d, false)?),
        None => None,
    };
    let to = match filter.date_to.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(d) => Some(normalize_bound(d, true)?),
        None => None,
    };
    if let (Some(f), Some(t)) = (&from, &to) {
        if f > t {
            return Err(AppError::ValidationFailed(vec![
                "The start date must be before the end date.".into(),
            ]));
        }
    }
    if let Some(f) = from {
        conditions.push("ace.acted_at >= ?".to_string());
        values.push(f.into());
    }
    if let Some(t) = to {
        conditions.push("ace.acted_at <= ?".to_string());
        values.push(t.into());
    }
    if let Some(apply_result) = filter.apply_result.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        conditions.push("ace.apply_result = ?".to_string());
        values.push(apply_result.to_string().into());
    }
    Ok((conditions, values))
}

const EVENT_SELECT: &str = "\
    SELECT ace.id, ace.action, ace.actor_id, ace.target_user_id, \
           ace.target_role_id, ace.acted_at, ace.scope_type, \
           ace.scope_reference, ace.summary, ace.diff_json, \
           ace.step_up_used, ace.apply_result, \
           EXISTS (SELECT 1 FROM sync_history_foreign_rows f WHERE f.entity_type = 'admin_change_events' AND f.entity_sync_id = ace.entity_sync_id) AS from_other_device, \
           ua_actor.username  AS actor_username, \
           ua_target.username AS target_username, \
           r_target.name     AS target_role_name \
    FROM admin_change_events ace \
    LEFT JOIN user_accounts ua_actor  ON ua_actor.id  = ace.actor_id \
    LEFT JOIN user_accounts ua_target ON ua_target.id = ace.target_user_id \
    LEFT JOIN roles r_target          ON r_target.id  = ace.target_role_id";

/// Parse a row from `admin_change_events` with actor/target name joins.
fn parse_event_row(r: &sea_orm::QueryResult) -> AppResult<AdminChangeEventDetail> {
    Ok(AdminChangeEventDetail {
        id: r.try_get("", "id")?,
        action: r.try_get("", "action")?,
        actor_id: r.try_get::<Option<i64>>("", "actor_id").unwrap_or(None),
        actor_username: r.try_get::<Option<String>>("", "actor_username").unwrap_or(None),
        target_user_id: r.try_get::<Option<i64>>("", "target_user_id").unwrap_or(None),
        target_username: r.try_get::<Option<String>>("", "target_username").unwrap_or(None),
        target_role_id: r.try_get::<Option<i64>>("", "target_role_id").unwrap_or(None),
        target_role_name: r.try_get::<Option<String>>("", "target_role_name").unwrap_or(None),
        acted_at: r.try_get("", "acted_at")?,
        scope_type: r.try_get::<Option<String>>("", "scope_type").unwrap_or(None),
        scope_reference: r.try_get::<Option<String>>("", "scope_reference").unwrap_or(None),
        summary: r.try_get::<Option<String>>("", "summary").unwrap_or(None),
        diff_json: r.try_get::<Option<String>>("", "diff_json").unwrap_or(None),
        step_up_used: r.try_get::<i32>("", "step_up_used").unwrap_or(0) != 0,
        apply_result: r
            .try_get::<String>("", "apply_result")
            .unwrap_or_else(|_| "applied".to_string()),
        from_other_device: r.try_get::<bool>("", "from_other_device").unwrap_or(false),
    })
}

/// Events matching `filter` among those in `allowed`, newest first.
pub(crate) async fn query_admin_events(
    db: &impl ConnectionTrait,
    allowed: Option<&[&'static str]>,
    filter: &AdminEventFilter,
    limit: i64,
    offset: i64,
) -> AppResult<Vec<AdminChangeEventDetail>> {
    let (conditions, mut values) = build_conditions(allowed, filter)?;
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let sql = format!("{EVENT_SELECT}{where_clause} ORDER BY ace.acted_at DESC, ace.id DESC LIMIT ? OFFSET ?");
    values.push(limit.into());
    values.push(offset.into());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;
    rows.iter().map(parse_event_row).collect()
}

fn map_csv_err(err: csv::Error) -> AppError {
    AppError::Internal(anyhow::anyhow!("CSV export error: {err}"))
}

/// CSV of the events (one row per event, the diff as its JSON text).
pub(crate) fn events_to_csv(events: &[AdminChangeEventDetail]) -> AppResult<String> {
    let mut writer = csv::Writer::from_writer(vec![]);
    writer
        .write_record([
            "id",
            "acted_at",
            "action",
            "apply_result",
            "actor_id",
            "actor",
            "target_user_id",
            "target_user",
            "target_role_id",
            "target_role",
            "scope_type",
            "scope_reference",
            "step_up_used",
            "summary",
            "detail_json",
            "from_other_device",
        ])
        .map_err(map_csv_err)?;
    let opt_i = |v: Option<i64>| v.map(|n| n.to_string()).unwrap_or_default();
    for e in events {
        writer
            .write_record([
                e.id.to_string(),
                e.acted_at.clone(),
                e.action.clone(),
                e.apply_result.clone(),
                opt_i(e.actor_id),
                e.actor_username.clone().unwrap_or_default(),
                opt_i(e.target_user_id),
                e.target_username.clone().unwrap_or_default(),
                opt_i(e.target_role_id),
                e.target_role_name.clone().unwrap_or_default(),
                e.scope_type.clone().unwrap_or_default(),
                e.scope_reference.clone().unwrap_or_default(),
                e.step_up_used.to_string(),
                e.summary.clone().unwrap_or_default(),
                e.diff_json.clone().unwrap_or_default(),
                e.from_other_device.to_string(),
            ])
            .map_err(map_csv_err)?;
    }
    let data = writer
        .into_inner()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("admin events csv finalize failed: {e}")))?;
    String::from_utf8(data).map_err(|e| AppError::Internal(anyhow::anyhow!("admin events csv utf8: {e}")))
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
    let allowed = visible_actions(&state, caller.user_id).await?;
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    let offset = filter.offset.unwrap_or(0).max(0);
    query_admin_events(&state.db, allowed.as_deref(), &filter, limit, offset).await
}

/// Filter choices for the admin ledger: the actions, actors, target users and target roles that
/// occur in the events the caller may see.
#[tauri::command]
pub async fn list_admin_event_facets(state: State<'_, AppState>) -> AppResult<AdminEventFacets> {
    let caller = require_session!(state);
    let allowed = visible_actions(&state, caller.user_id).await?;
    let (conditions, values) = build_conditions(allowed.as_deref(), &AdminEventFilter::default())?;
    let where_clause = if conditions.is_empty() {
        "1 = 1".to_string()
    } else {
        conditions.join(" AND ")
    };

    let actions = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT DISTINCT ace.action AS v FROM admin_change_events ace WHERE {where_clause} ORDER BY v"),
            values.clone(),
        ))
        .await?
        .iter()
        .filter_map(|r| r.try_get::<String>("", "v").ok())
        .collect();
    let apply_results = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT DISTINCT ace.apply_result AS v FROM admin_change_events ace WHERE {where_clause} ORDER BY v"
            ),
            values.clone(),
        ))
        .await?
        .iter()
        .filter_map(|r| r.try_get::<String>("", "v").ok())
        .collect();

    let facet = |column: &str, join: &str, label: &str| {
        format!(
            "SELECT ace.{column} AS id, COALESCE({label}, '#' || ace.{column}) AS label, COUNT(*) AS cnt \
             FROM admin_change_events ace {join} \
             WHERE ace.{column} IS NOT NULL AND {where_clause} \
             GROUP BY ace.{column} ORDER BY label"
        )
    };
    let load = |sql: String| {
        let values = values.clone();
        let db = &state.db;
        async move {
            let rows = db
                .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
                .await?;
            let mut out = Vec::with_capacity(rows.len());
            for r in &rows {
                out.push(AdminEventFacetOption {
                    id: r.try_get("", "id")?,
                    label: r.try_get("", "label")?,
                    count: r.try_get("", "cnt")?,
                });
            }
            AppResult::Ok(out)
        }
    };
    let actors = load(facet(
        "actor_id",
        "LEFT JOIN user_accounts u ON u.id = ace.actor_id",
        "u.username",
    ))
    .await?;
    let target_users = load(facet(
        "target_user_id",
        "LEFT JOIN user_accounts u ON u.id = ace.target_user_id",
        "u.username",
    ))
    .await?;
    let target_roles = load(facet(
        "target_role_id",
        "LEFT JOIN roles r ON r.id = ace.target_role_id",
        "r.name",
    ))
    .await?;

    Ok(AdminEventFacets {
        actions,
        actors,
        target_users,
        target_roles,
        apply_results,
    })
}

/// Export the events matching `filter` (same visibility as the list) as CSV text.
#[tauri::command]
pub async fn export_admin_events_csv(state: State<'_, AppState>, filter: AdminEventFilter) -> AppResult<String> {
    let caller = require_session!(state);
    let allowed = visible_actions(&state, caller.user_id).await?;
    let events = query_admin_events(&state.db, allowed.as_deref(), &filter, EXPORT_MAX_ROWS, 0).await?;
    events_to_csv(&events)
}

/// Get a single admin change event by ID with full diff_json.
///
/// Permission-based action scoping applies: if the event's action is outside
/// the caller's allowed domain, it returns NotFound (not PermissionDenied)
/// to avoid information leakage.
#[tauri::command]
pub async fn get_admin_event(state: State<'_, AppState>, event_id: i64) -> AppResult<AdminChangeEventDetail> {
    let caller = require_session!(state);
    let allowed_actions = visible_actions(&state, caller.user_id).await?;

    let sql = format!("{EVENT_SELECT} WHERE ace.id = ?");
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [event_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "admin_change_event".to_string(),
            id: event_id.to_string(),
        })?;

    let event = parse_event_row(&row)?;

    // Scope check: ensure the event's action is within the caller's allowed domain
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_bounds_cover_whole_days() {
        assert_eq!(normalize_bound("2026-10-16", false).unwrap(), "2026-10-16T00:00:00Z");
        assert_eq!(normalize_bound("2026-10-16", true).unwrap(), "2026-10-16T23:59:59Z");
        assert_eq!(
            normalize_bound("2026-10-16T12:30:00+02:00", true).unwrap(),
            "2026-10-16T10:30:00Z"
        );
        assert!(normalize_bound("16/10/2026", false).is_err());
    }

    #[tokio::test]
    async fn filters_scope_and_export_follow_the_ledger() {
        let db = crate::db::test_db::seeded().await;
        let actor: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'admin'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        for (action, result, at) in [
            ("user_created", "applied", "2026-01-10T08:00:00Z"),
            ("role_updated", "applied", "2026-02-10T08:00:00Z"),
            ("user_deactivated", "blocked", "2026-03-10T08:00:00Z"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO admin_change_events (action, actor_id, acted_at, summary, apply_result) \
                 VALUES (?, ?, ?, 'test, with comma', ?)",
                [action.into(), actor.into(), at.into(), result.into()],
            ))
            .await
            .unwrap();
        }

        let users_only = query_admin_events(&db, Some(USER_ACTIONS), &AdminEventFilter::default(), 100, 0)
            .await
            .unwrap();
        assert!(users_only.iter().all(|e| USER_ACTIONS.contains(&e.action.as_str())));
        assert!(!users_only.iter().any(|e| e.action == "role_updated"));

        let blocked = query_admin_events(
            &db,
            None,
            &AdminEventFilter {
                apply_result: Some("blocked".into()),
                ..Default::default()
            },
            100,
            0,
        )
        .await
        .unwrap();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].action, "user_deactivated");

        let february = query_admin_events(
            &db,
            None,
            &AdminEventFilter {
                date_from: Some("2026-02-10".into()),
                date_to: Some("2026-02-10".into()),
                actor_id: Some(actor),
                ..Default::default()
            },
            100,
            0,
        )
        .await
        .unwrap();
        assert_eq!(february.len(), 1, "a single-day range includes that whole day");
        assert_eq!(february[0].action, "role_updated");

        let csv = events_to_csv(&february).unwrap();
        let mut lines = csv.lines();
        assert!(lines.next().unwrap().starts_with("id,acted_at,action"));
        assert!(lines.next().unwrap().contains("\"test, with comma\""));
    }
}
