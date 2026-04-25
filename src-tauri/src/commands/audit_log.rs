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
    pub action_code: Option<String>,
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
    pub auth_context: String,
    pub result: String,
    pub happened_at: String,
    pub retention_class: String,
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
        "adm.audit",
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
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            values,
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(AuditEventSummary {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            action_code: row
                .try_get::<String>("", "action_code")
                .unwrap_or_default(),
            target_type: row.try_get::<Option<String>>("", "target_type").unwrap_or(None),
            target_id: row.try_get::<Option<String>>("", "target_id").unwrap_or(None),
            actor_id: row.try_get::<Option<i64>>("", "actor_id").unwrap_or(None),
            actor_username: row
                .try_get::<Option<String>>("", "actor_username")
                .unwrap_or(None),
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
        "adm.audit",
        &PermissionScope::Global,
    )
    .await?;

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ae.id, ae.action_code, ae.target_type, ae.target_id, ae.actor_id, ua.username AS actor_username,
                    ae.auth_context, ae.result, ae.before_hash, ae.after_hash, ae.happened_at, ae.retention_class, ae.details_json
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
        actor_username: row
            .try_get::<Option<String>>("", "actor_username")
            .unwrap_or(None),
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
    })
}

#[tauri::command]
pub async fn export_audit_log(
    payload: ExportAuditInput,
    state: State<'_, AppState>,
) -> AppResult<ExportResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "log.export", PermissionScope::Global);
    require_step_up!(state);

    if payload.export_reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "export_reason must not be empty".to_string(),
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
        "adm.audit",
        &PermissionScope::Global,
    )
    .await?;
    if !has_adm_audit {
        enforced_filter.actor_id = Some(i64::from(user.user_id));
    }

    let (sql, values) = build_audit_filter_query(&enforced_filter, false);
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            values,
        ))
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

fn build_audit_filter_query(filter: &AuditFilter, include_actor_username: bool) -> (String, Vec<Value>) {
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    let offset = filter.offset.unwrap_or(0).max(0);

    let select = if include_actor_username {
        "SELECT ae.id, ae.action_code, ae.target_type, ae.target_id, ae.actor_id, ua.username AS actor_username,
                ae.auth_context, ae.result, ae.before_hash, ae.after_hash, ae.happened_at, ae.retention_class, ae.details_json
         FROM audit_events ae
         LEFT JOIN user_accounts ua ON ua.id = ae.actor_id"
            .to_string()
    } else {
        "SELECT ae.id, ae.action_code, ae.target_type, ae.target_id, ae.actor_id,
                ae.auth_context, ae.result, ae.before_hash, ae.after_hash, ae.happened_at, ae.retention_class, ae.details_json
         FROM audit_events ae"
            .to_string()
    };

    let mut sql = format!("{select} WHERE 1=1");
    let mut values: Vec<Value> = Vec::new();

    if let Some(v) = &filter.action_code {
        sql.push_str(" AND ae.action_code LIKE ?");
        values.push(format!("{v}%").into());
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
        "log.view",
        &PermissionScope::Global,
    )
    .await?;
    if has_global {
        return Ok(());
    }

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt
             FROM user_scope_assignments usa
             INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
             INNER JOIN permissions p ON p.id = rp.permission_id
             WHERE usa.user_id = ?
               AND usa.deleted_at IS NULL
               AND (usa.valid_from IS NULL OR usa.valid_from <= datetime('now'))
               AND (usa.valid_to IS NULL OR usa.valid_to >= datetime('now'))
               AND usa.scope_type IN ('entity', 'org_node', 'site', 'team')
               AND p.name = 'log.view'",
            [i64::from(user_id).into()],
        ))
        .await?;
    let scoped_count = row
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0);
    if scoped_count > 0 {
        Ok(())
    } else {
        Err(AppError::PermissionDenied(
            "Permission denied: log.view is required".to_string(),
        ))
    }
}
