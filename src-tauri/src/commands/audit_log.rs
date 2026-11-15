use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::audit::writer::{write_audit_event, AuditEventInput};
use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct AuditFilter {
    /// Start of the action code (`auth.`), kept for older callers and exports.
    pub action_code: Option<String>,
    /// One action code exactly (`wo.closed`), as chosen in the screen's list of actions.
    #[serde(default)]
    pub action: Option<String>,
    /// The readable code of the object the events are about, as users see it: an OT number
    /// (`OT-2026-0042`), an equipment code or a permit number (part of it is enough). Resolved
    /// here to the stored object ids; never an internal id typed by the user.
    #[serde(default)]
    pub object_search: Option<String>,
    pub actor_id: Option<i64>,
    pub target_type: Option<String>,
    pub result: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub retention_class: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuditEventSummary {
    pub id: i64,
    pub action_code: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub actor_id: Option<i64>,
    pub actor_username: Option<String>,
    /// The actor's name as people know it (display name, else the account name).
    pub actor_display_name: Option<String>,
    /// The object as it reads on screen ("OT-2026-0042 — Fuite pompe"), when this database
    /// holds it; `None` for an object it does not know or a row from another device.
    pub target_label: Option<String>,
    pub auth_context: String,
    pub result: String,
    pub happened_at: String,
    pub retention_class: String,
    /// Received from another device before history ids travelled as sync ids: the ids inside
    /// this row (target, scope, summary/detail ids) are that device's local ids, not this one's.
    pub from_other_device: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuditEventDetail {
    pub id: i64,
    pub action_code: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub actor_id: Option<i64>,
    pub actor_username: Option<String>,
    pub auth_context: String,
    pub result: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub happened_at: String,
    pub retention_class: String,
    pub details_json: Option<serde_json::Value>,
    /// Received from another device before history ids travelled as sync ids: the ids inside
    /// this row (target, scope, summary/detail ids) are that device's local ids, not this one's.
    pub from_other_device: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExportAuditInput {
    pub filter: AuditFilter,
    pub export_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub event_export_run_id: i64,
    pub row_count: i64,
    pub rows_json: serde_json::Value,
}

#[tauri::command]
pub async fn list_audit_events(
    filter: Option<AuditFilter>,
    state: State<'_, AppState>,
) -> AppResult<Vec<AuditEventSummary>> {
    let user = require_session!(state);
    ensure_log_view_access(&state, user.user_id).await?;
    let has_adm_audit = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::ADM_AUDIT,
        &PermissionScope::Global,
    )
    .await?;

    let mut filter = filter.unwrap_or_default();
    if !has_adm_audit {
        filter.actor_id = Some(i64::from(user.user_id));
    }

    let (sql, values) = build_audit_filter_query(&filter, true);
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(AuditEventSummary {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            action_code: row.try_get::<String>("", "action_code").unwrap_or_default(),
            target_type: row.try_get::<Option<String>>("", "target_type").unwrap_or(None),
            target_id: row.try_get::<Option<String>>("", "target_id").unwrap_or(None),
            actor_id: row.try_get::<Option<i64>>("", "actor_id").unwrap_or(None),
            actor_username: row.try_get::<Option<String>>("", "actor_username").unwrap_or(None),
            actor_display_name: row.try_get::<Option<String>>("", "actor_display_name").unwrap_or(None),
            target_label: row.try_get::<Option<String>>("", "target_label").unwrap_or(None),
            auth_context: row
                .try_get::<String>("", "auth_context")
                .unwrap_or_else(|_| "password".to_string()),
            result: row
                .try_get::<String>("", "result")
                .unwrap_or_else(|_| "success".to_string()),
            happened_at: row.try_get::<String>("", "happened_at").unwrap_or_default(),
            retention_class: row
                .try_get::<String>("", "retention_class")
                .unwrap_or_else(|_| "standard".to_string()),
            from_other_device: row.try_get::<bool>("", "from_other_device").unwrap_or(false),
        });
    }

    Ok(out)
}

#[tauri::command]
pub async fn get_audit_event(event_id: i64, state: State<'_, AppState>) -> AppResult<AuditEventDetail> {
    let user = require_session!(state);
    ensure_log_view_access(&state, user.user_id).await?;
    let has_adm_audit = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::ADM_AUDIT,
        &PermissionScope::Global,
    )
    .await?;

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ae.id, ae.action_code, ae.target_type, ae.target_id, ae.actor_id, ua.username AS actor_username,
                    ae.auth_context, ae.result, ae.before_hash, ae.after_hash, ae.happened_at, ae.retention_class, ae.details_json, EXISTS (SELECT 1 FROM sync_history_foreign_rows f WHERE f.entity_type = 'audit_events' AND f.entity_sync_id = ae.entity_sync_id) AS from_other_device
             FROM audit_events ae
             LEFT JOIN user_accounts ua ON ua.id = ae.actor_id
             WHERE ae.id = ?
             LIMIT 1",
            [event_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "audit_event".to_string(),
            id: event_id.to_string(),
        })?;

    let actor_id = row.try_get::<Option<i64>>("", "actor_id").unwrap_or(None);
    if !has_adm_audit && actor_id != Some(i64::from(user.user_id)) {
        return Err(AppError::PermissionDenied(
            "Permission required: adm.audit or own actor_id event".to_string(),
        ));
    }

    let details_json = row
        .try_get::<Option<String>>("", "details_json")
        .unwrap_or(None)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());

    Ok(AuditEventDetail {
        id: row.try_get::<i64>("", "id").unwrap_or_default(),
        action_code: row.try_get::<String>("", "action_code").unwrap_or_default(),
        target_type: row.try_get::<Option<String>>("", "target_type").unwrap_or(None),
        target_id: row.try_get::<Option<String>>("", "target_id").unwrap_or(None),
        actor_id,
        actor_username: row.try_get::<Option<String>>("", "actor_username").unwrap_or(None),
        auth_context: row
            .try_get::<String>("", "auth_context")
            .unwrap_or_else(|_| "password".to_string()),
        result: row
            .try_get::<String>("", "result")
            .unwrap_or_else(|_| "success".to_string()),
        before_hash: row.try_get::<Option<String>>("", "before_hash").unwrap_or(None),
        after_hash: row.try_get::<Option<String>>("", "after_hash").unwrap_or(None),
        happened_at: row.try_get::<String>("", "happened_at").unwrap_or_default(),
        retention_class: row
            .try_get::<String>("", "retention_class")
            .unwrap_or_else(|_| "standard".to_string()),
        details_json,
        from_other_device: row.try_get::<bool>("", "from_other_device").unwrap_or(false),
    })
}

#[tauri::command]
pub async fn export_audit_log(payload: ExportAuditInput, state: State<'_, AppState>) -> AppResult<ExportResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::LOG_EXPORT,
        PermissionScope::Global
    );
    require_step_up!(state);

    if payload.export_reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_CMD_AUDIT_EXPORT_REASON_REQUIRED: export_reason must not be empty".to_string(),
        ]));
    }

    let export_insert = state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO event_export_runs
                (requested_by_id, export_scope, started_at, status, row_count, output_path)
             VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), 'running', NULL, NULL)",
            [user.user_id.into(), "audit_log".into()],
        ))
        .await?;
    let event_export_run_id = export_insert.last_insert_id() as i64;

    write_audit_event(
        &state.db,
        AuditEventInput {
            action_code: "export.audit_log".to_string(),
            target_type: Some("audit_events".to_string()),
            target_id: Some(event_export_run_id.to_string()),
            actor_id: Some(i64::from(user.user_id)),
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "compliance".to_string(),
            details_json: Some(serde_json::json!({
                "export_reason": payload.export_reason,
                "filter": payload.filter,
                "event_export_run_id": event_export_run_id
            })),
        },
    )
    .await?;

    let mut enforced_filter = payload.filter.clone();
    let has_adm_audit = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::ADM_AUDIT,
        &PermissionScope::Global,
    )
    .await?;
    if !has_adm_audit {
        enforced_filter.actor_id = Some(i64::from(user.user_id));
    }

    let (sql, values) = build_audit_filter_query(&enforced_filter, false);
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;

    let mut serialized_rows = Vec::with_capacity(rows.len());
    for row in rows {
        serialized_rows.push(serde_json::json!({
            "id": row.try_get::<i64>("", "id").unwrap_or_default(),
            "action_code": row.try_get::<String>("", "action_code").unwrap_or_default(),
            "target_type": row.try_get::<Option<String>>("", "target_type").unwrap_or(None),
            "target_id": row.try_get::<Option<String>>("", "target_id").unwrap_or(None),
            "actor_id": row.try_get::<Option<i64>>("", "actor_id").unwrap_or(None),
            "auth_context": row.try_get::<String>("", "auth_context").unwrap_or_default(),
            "result": row.try_get::<String>("", "result").unwrap_or_default(),
            "before_hash": row.try_get::<Option<String>>("", "before_hash").unwrap_or(None),
            "after_hash": row.try_get::<Option<String>>("", "after_hash").unwrap_or(None),
            "happened_at": row.try_get::<String>("", "happened_at").unwrap_or_default(),
            "retention_class": row.try_get::<String>("", "retention_class").unwrap_or_default(),
            "details_json": row
                .try_get::<Option<String>>("", "details_json")
                .unwrap_or(None)
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok()),
        }));
    }
    let row_count = serialized_rows.len() as i64;

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE event_export_runs
             SET completed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 status = 'completed',
                 row_count = ?
             WHERE id = ?",
            [row_count.into(), event_export_run_id.into()],
        ))
        .await?;

    Ok(ExportResult {
        event_export_run_id,
        row_count,
        rows_json: serde_json::Value::Array(serialized_rows),
    })
}

/// The row was received from another device before history ids travelled as sync ids.
const FOREIGN_ROW_SQL: &str = "EXISTS (SELECT 1 FROM sync_history_foreign_rows f WHERE f.entity_type = 'audit_events' AND f.entity_sync_id = ae.entity_sync_id)";

/// The object of an event as people read it: an OT, an equipment, a permit, an account. Objects
/// are stored by their local id (`target_id`); other kinds give `NULL` (the screen shows the kind).
const TARGET_LABEL_SQL: &str = "CASE ae.target_type
        WHEN 'work_order' THEN (SELECT w.code || ' — ' || w.title FROM work_orders w WHERE w.id = CAST(ae.target_id AS INTEGER))
        WHEN 'equipment' THEN (SELECT e.asset_id_code || ' — ' || e.name FROM equipment e WHERE e.id = CAST(ae.target_id AS INTEGER))
        WHEN 'work_permit' THEN (SELECT p.code FROM work_permits p WHERE p.id = CAST(ae.target_id AS INTEGER))
        WHEN 'user_account' THEN (SELECT COALESCE(u.display_name, u.username) FROM user_accounts u WHERE u.id = CAST(ae.target_id AS INTEGER))
        END";

/// Events whose object's readable code contains the searched text (OT number, equipment code,
/// permit number): [`OBJECT_SEARCH_BINDS`] binds of the same `LIKE` pattern.
const OBJECT_SEARCH_SQL: &str = "(ae.target_type = 'work_order' AND CAST(ae.target_id AS INTEGER) IN (SELECT w.id FROM work_orders w WHERE w.code LIKE ? ESCAPE '\\'))
        OR (ae.target_type = 'equipment' AND CAST(ae.target_id AS INTEGER) IN (SELECT e.id FROM equipment e WHERE e.asset_id_code LIKE ? ESCAPE '\\'))
        OR (ae.target_type = 'work_permit' AND CAST(ae.target_id AS INTEGER) IN (SELECT p.id FROM work_permits p WHERE p.code LIKE ? ESCAPE '\\'))";
const OBJECT_SEARCH_BINDS: usize = 3;

/// `%text%` for `LIKE … ESCAPE '\'`: the text's own `%`, `_` and `\` are taken literally.
fn like_contains(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

pub(crate) fn build_audit_filter_query(filter: &AuditFilter, include_actor_username: bool) -> (String, Vec<Value>) {
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    let offset = filter.offset.unwrap_or(0).max(0);

    let select = if include_actor_username {
        format!(
            "SELECT ae.id, ae.action_code, ae.target_type, ae.target_id, ae.actor_id, ua.username AS actor_username,
                    COALESCE(ua.display_name, ua.username) AS actor_display_name,
                    CASE WHEN {FOREIGN_ROW_SQL} THEN NULL ELSE {TARGET_LABEL_SQL} END AS target_label,
                    ae.auth_context, ae.result, ae.before_hash, ae.after_hash, ae.happened_at, ae.retention_class, ae.details_json, {FOREIGN_ROW_SQL} AS from_other_device
             FROM audit_events ae
             LEFT JOIN user_accounts ua ON ua.id = ae.actor_id"
        )
    } else {
        "SELECT ae.id, ae.action_code, ae.target_type, ae.target_id, ae.actor_id,
                ae.auth_context, ae.result, ae.before_hash, ae.after_hash, ae.happened_at, ae.retention_class, ae.details_json, EXISTS (SELECT 1 FROM sync_history_foreign_rows f WHERE f.entity_type = 'audit_events' AND f.entity_sync_id = ae.entity_sync_id) AS from_other_device
         FROM audit_events ae"
            .to_string()
    };

    let mut sql = format!("{select} WHERE 1=1");
    let mut values: Vec<Value> = Vec::new();

    if let Some(v) = &filter.action_code {
        sql.push_str(" AND ae.action_code LIKE ?");
        values.push(format!("{v}%").into());
    }
    if let Some(v) = filter.action.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        sql.push_str(" AND ae.action_code = ?");
        values.push(v.to_string().into());
    }
    if let Some(term) = filter.object_search.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        // Rows from another device name that device's objects: never matched to ours.
        sql.push_str(&format!(" AND NOT {FOREIGN_ROW_SQL} AND ({OBJECT_SEARCH_SQL})"));
        let pattern = like_contains(term);
        for _ in 0..OBJECT_SEARCH_BINDS {
            values.push(pattern.clone().into());
        }
    }
    if let Some(v) = filter.actor_id {
        sql.push_str(" AND ae.actor_id = ?");
        values.push(v.into());
    }
    if let Some(v) = &filter.target_type {
        sql.push_str(" AND ae.target_type = ?");
        values.push(v.clone().into());
    }
    if let Some(v) = &filter.result {
        sql.push_str(" AND ae.result = ?");
        values.push(v.clone().into());
    }
    if let Some(v) = &filter.date_from {
        sql.push_str(" AND ae.happened_at >= ?");
        values.push(v.clone().into());
    }
    if let Some(v) = &filter.date_to {
        sql.push_str(" AND ae.happened_at <= ?");
        values.push(v.clone().into());
    }
    if let Some(v) = &filter.retention_class {
        sql.push_str(" AND ae.retention_class = ?");
        values.push(v.clone().into());
    }

    sql.push_str(" ORDER BY ae.happened_at DESC LIMIT ? OFFSET ?");
    values.push(limit.into());
    values.push(offset.into());

    (sql, values)
}

async fn ensure_log_view_access(state: &State<'_, AppState>, user_id: i32) -> AppResult<()> {
    let has_global = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user_id,
        crate::rbac::permissions::LOG_VIEW,
        &PermissionScope::Global,
    )
    .await?;
    if has_global {
        return Ok(());
    }

    // Assignments in force only (validity window, emergency grants before their expiry).
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![i64::from(user_id).into()];
    values.extend(crate::rbac::resolver::in_force_binds(&now));
    values.push(crate::rbac::permissions::LOG_VIEW.into());
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS cnt
                 FROM user_scope_assignments usa
                 INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
                 INNER JOIN permissions p ON p.id = rp.permission_id
                 WHERE usa.user_id = ?
                   AND {}
                   AND usa.scope_type IN ('entity', 'org_node', 'site', 'team')
                   AND p.name = ?",
                crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
            ),
            values,
        ))
        .await?;
    let scoped_count = row.and_then(|r| r.try_get::<i64>("", "cnt").ok()).unwrap_or(0);
    if scoped_count > 0 {
        Ok(())
    } else {
        Err(AppError::PermissionDenied(
            "Permission denied: log.view is required".to_string(),
        ))
    }
}

/// Verifies the hash chain of every device's audit trail held in this database (`audit::chain`).
#[tauri::command]
pub async fn verify_audit_chain(state: State<'_, AppState>) -> AppResult<crate::audit::chain::AuditChainReport> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_AUDIT,
        PermissionScope::Global
    );
    crate::audit::chain::verify_audit_chain(&state.db).await
}

/// A person who appears as the actor of audit events, for the screen's "Utilisateur" list.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct AuditActorOption {
    pub id: i64,
    /// Display name, else the account name.
    pub label: String,
}

/// The choices of the audit log's filters, read from the trail itself: the actions it holds and
/// the people who acted (by name, never a number to type).
#[derive(Debug, Serialize, Clone, Default)]
pub struct AuditFilterOptions {
    pub actions: Vec<String>,
    pub actors: Vec<AuditActorOption>,
}

/// [`AuditFilterOptions`] of the whole trail, or of one actor's events only (`only_actor`: a
/// reader without `adm.audit`, who sees their own events, `list_audit_events`).
pub async fn audit_filter_options(db: &impl ConnectionTrait, only_actor: Option<i64>) -> AppResult<AuditFilterOptions> {
    let (where_sql, values): (&str, Vec<Value>) = match only_actor {
        Some(id) => (" WHERE ae.actor_id = ?", vec![id.into()]),
        None => ("", Vec::new()),
    };
    let actions = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT DISTINCT ae.action_code AS code FROM audit_events ae{where_sql} ORDER BY ae.action_code"),
            values.clone(),
        ))
        .await?
        .into_iter()
        .filter_map(|row| row.try_get::<String>("", "code").ok())
        .collect();
    let actors = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ua.id AS id, COALESCE(ua.display_name, ua.username) AS label
                 FROM user_accounts ua
                 WHERE ua.id IN (SELECT DISTINCT ae.actor_id FROM audit_events ae{where_sql})
                 ORDER BY label COLLATE NOCASE"
            ),
            values,
        ))
        .await?
        .into_iter()
        .filter_map(|row| {
            Some(AuditActorOption {
                id: row.try_get::<i64>("", "id").ok()?,
                label: row.try_get::<String>("", "label").ok()?,
            })
        })
        .collect();
    Ok(AuditFilterOptions { actions, actors })
}

/// The audit log's filter choices (actions, people), with the same reading rights as
/// `list_audit_events`: `log.view`, and only one's own events without `adm.audit`.
#[tauri::command]
pub async fn list_audit_filter_options(state: State<'_, AppState>) -> AppResult<AuditFilterOptions> {
    let user = require_session!(state);
    ensure_log_view_access(&state, user.user_id).await?;
    let has_adm_audit = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::ADM_AUDIT,
        &PermissionScope::Global,
    )
    .await?;
    let only_actor = (!has_adm_audit).then(|| i64::from(user.user_id));
    audit_filter_options(&state.db, only_actor).await
}
