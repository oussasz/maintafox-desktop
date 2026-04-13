use std::collections::{HashSet, VecDeque};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::require_session;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ActivityFilter {
    pub event_class: Option<String>,
    pub event_code: Option<String>,
    pub source_module: Option<String>,
    pub source_record_type: Option<String>,
    pub source_record_id: Option<String>,
    pub entity_scope_id: Option<i64>,
    pub actor_id: Option<i64>,
    pub severity: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub correlation_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ActivityEventSummary {
    pub id: i64,
    pub event_class: String,
    pub event_code: String,
    pub source_module: String,
    pub source_record_type: Option<String>,
    pub source_record_id: Option<String>,
    pub entity_scope_id: Option<i64>,
    pub actor_id: Option<i64>,
    pub actor_username: Option<String>,
    pub happened_at: String,
    pub severity: String,
    pub summary_json: Option<serde_json::Value>,
    pub correlation_id: Option<String>,
    pub visibility_scope: String,
}

#[derive(Debug, Serialize)]
pub struct ActivityEventDetail {
    pub event: ActivityEventSummary,
    pub correlated_events: Vec<ActivityEventSummary>,
    pub source_record_link: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveFilterInput {
    pub view_name: String,
    pub filter_json: serde_json::Value,
    pub is_default: bool,
}

#[derive(Debug, Serialize)]
pub struct SavedActivityFilter {
    pub id: i64,
    pub user_id: i64,
    pub view_name: String,
    pub filter_json: serde_json::Value,
    pub is_default: bool,
}

#[derive(Debug, Deserialize)]
pub struct EventChainInput {
    pub root_event_id: i64,
    pub root_table: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct EventChainNode {
    pub table: String,
    pub event_id: i64,
    pub happened_at: String,
    pub event_code: Option<String>,
    pub action_code: Option<String>,
    pub source_module: Option<String>,
    pub link_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EventChain {
    pub events: Vec<EventChainNode>,
}

#[derive(Debug, Clone)]
struct LogViewAccess {
    has_global: bool,
    accessible_entities: Vec<i64>,
}

#[tauri::command]
pub async fn list_activity_events(
    filter: Option<ActivityFilter>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ActivityEventSummary>> {
    let user = require_session!(state);
    let access = resolve_log_view_access(&state, user.user_id).await?;

    let filter = filter.unwrap_or_default();
    let limit = filter.limit.unwrap_or(50).clamp(1, 500);
    let offset = filter.offset.unwrap_or(0).max(0);

    let mut sql = String::from(
        "SELECT
            ae.id,
            ae.event_class,
            ae.event_code,
            ae.source_module,
            ae.source_record_type,
            ae.source_record_id,
            ae.entity_scope_id,
            ae.actor_id,
            ua.username AS actor_username,
            ae.happened_at,
            ae.severity,
            ae.summary_json,
            ae.correlation_id,
            ae.visibility_scope
         FROM activity_events ae
         LEFT JOIN user_accounts ua ON ua.id = ae.actor_id
         WHERE 1=1",
    );
    let mut values: Vec<Value> = Vec::new();

    if let Some(v) = filter.event_class {
        sql.push_str(" AND ae.event_class = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.event_code {
        sql.push_str(" AND ae.event_code = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.source_module {
        sql.push_str(" AND ae.source_module = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.source_record_type {
        sql.push_str(" AND ae.source_record_type = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.source_record_id {
        sql.push_str(" AND ae.source_record_id = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.entity_scope_id {
        sql.push_str(" AND ae.entity_scope_id = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.actor_id {
        sql.push_str(" AND ae.actor_id = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.severity {
        sql.push_str(" AND ae.severity = ?");
        values.push(v.into());
    }
    if let Some(v) = filter.date_from {
        sql.push_str(" AND ae.happened_at >= ?");
        values.push(v.into());
    }
    if let Some(v) = filter.date_to {
        sql.push_str(" AND ae.happened_at <= ?");
        values.push(v.into());
    }
    if let Some(v) = filter.correlation_id {
        sql.push_str(" AND ae.correlation_id = ?");
        values.push(v.into());
    }

    if !access.has_global {
        sql.push_str(" AND ae.entity_scope_id IN (");
        for (idx, entity_id) in access.accessible_entities.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
            values.push((*entity_id).into());
        }
        sql.push(')');
    }

    sql.push_str(" ORDER BY ae.happened_at DESC LIMIT ? OFFSET ?");
    values.push(limit.into());
    values.push(offset.into());

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
        out.push(parse_activity_summary(&row));
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_activity_event(
    event_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ActivityEventDetail> {
    let user = require_session!(state);
    let access = resolve_log_view_access(&state, user.user_id).await?;

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                ae.id, ae.event_class, ae.event_code, ae.source_module, ae.source_record_type,
                ae.source_record_id, ae.entity_scope_id, ae.actor_id, ua.username AS actor_username,
                ae.happened_at, ae.severity, ae.summary_json, ae.correlation_id, ae.visibility_scope
             FROM activity_events ae
             LEFT JOIN user_accounts ua ON ua.id = ae.actor_id
             WHERE ae.id = ?
             LIMIT 1",
            [event_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "activity_event".to_string(),
            id: event_id.to_string(),
        })?;

    let event = parse_activity_summary(&row);
    if !access.has_global {
        match event.entity_scope_id {
            Some(entity_id) if access.accessible_entities.contains(&entity_id) => {}
            _ => {
                return Err(AppError::PermissionDenied(
                    "Permission denied: log.view access does not cover this entity scope".to_string(),
                ));
            }
        }
    }
    let correlation = event.correlation_id.clone();

    let correlated_rows = if let Some(correlation_id) = correlation {
        let mut corr_sql = String::from(
            "SELECT
                    ae.id, ae.event_class, ae.event_code, ae.source_module, ae.source_record_type,
                    ae.source_record_id, ae.entity_scope_id, ae.actor_id, ua.username AS actor_username,
                    ae.happened_at, ae.severity, ae.summary_json, ae.correlation_id, ae.visibility_scope
                 FROM activity_events ae
                 LEFT JOIN user_accounts ua ON ua.id = ae.actor_id
                 WHERE ae.correlation_id = ?
                   AND ae.id != ?
            ",
        );
        let mut corr_values: Vec<Value> = vec![correlation_id.into(), event_id.into()];
        if !access.has_global {
            corr_sql.push_str(" AND ae.entity_scope_id IN (");
            for (idx, entity_id) in access.accessible_entities.iter().enumerate() {
                if idx > 0 {
                    corr_sql.push_str(", ");
                }
                corr_sql.push('?');
                corr_values.push((*entity_id).into());
            }
            corr_sql.push(')');
        }
        corr_sql.push_str(" ORDER BY ae.happened_at DESC LIMIT 100");

        state
            .db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                corr_sql,
                corr_values,
            ))
            .await?
    } else {
        Vec::new()
    };

    let mut correlated_events = Vec::with_capacity(correlated_rows.len());
    for row in correlated_rows {
        correlated_events.push(parse_activity_summary(&row));
    }

    let source_record_link = event.source_record_id.as_ref().map(|source_id| {
        if event.source_module == "wo" {
            format!("/work-orders/{source_id}")
        } else if event.source_module == "di" {
            format!("/requests/{source_id}")
        } else {
            format!("/{}/{}", event.source_module, source_id)
        }
    });

    Ok(ActivityEventDetail {
        event,
        correlated_events,
        source_record_link,
    })
}

#[tauri::command]
pub async fn save_activity_filter(
    payload: SaveFilterInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    let _ = resolve_log_view_access(&state, user.user_id).await?;

    if payload.view_name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "view_name must not be empty".to_string(),
        ]));
    }

    if payload.is_default {
        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE saved_activity_filters
                 SET is_default = 0
                 WHERE user_id = ?",
                [user.user_id.into()],
            ))
            .await?;
    }

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO saved_activity_filters
                (user_id, view_name, filter_json, is_default)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(user_id, view_name)
             DO UPDATE SET
                filter_json = excluded.filter_json,
                is_default = excluded.is_default",
            [
                user.user_id.into(),
                payload.view_name.into(),
                payload.filter_json.to_string().into(),
                i32::from(payload.is_default).into(),
            ],
        ))
        .await?;

    Ok(())
}

#[tauri::command]
pub async fn list_saved_activity_filters(
    state: State<'_, AppState>,
) -> AppResult<Vec<SavedActivityFilter>> {
    let user = require_session!(state);
    let _ = resolve_log_view_access(&state, user.user_id).await?;

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, user_id, view_name, filter_json, is_default
             FROM saved_activity_filters
             WHERE user_id = ?
             ORDER BY is_default DESC, view_name ASC",
            [user.user_id.into()],
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let raw_filter = row
            .try_get::<String>("", "filter_json")
            .unwrap_or_else(|_| "{}".to_string());
        let parsed = serde_json::from_str::<serde_json::Value>(&raw_filter)
            .unwrap_or_else(|_| serde_json::json!({}));
        out.push(SavedActivityFilter {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            user_id: row.try_get::<i64>("", "user_id").unwrap_or_default(),
            view_name: row.try_get::<String>("", "view_name").unwrap_or_default(),
            filter_json: parsed,
            is_default: row.try_get::<i32>("", "is_default").unwrap_or(0) == 1,
        });
    }

    Ok(out)
}

/// Build a correlated event chain from [`event_links`] (BFS). Used by [`get_event_chain`]
/// and by integration tests that need the same graph walk without IPC.
pub async fn build_event_chain(
    db: &DatabaseConnection,
    payload: &EventChainInput,
) -> AppResult<EventChain> {
    let mut queue: VecDeque<(String, i64)> = VecDeque::new();
    let mut visited: HashSet<(String, i64)> = HashSet::new();
    let mut events: Vec<EventChainNode> = Vec::new();

    let root_table = normalize_event_table(&payload.root_table);
    queue.push_back((root_table, payload.root_event_id));

    while let Some((table, event_id)) = queue.pop_front() {
        if visited.len() >= 20 {
            break;
        }

        if !visited.insert((table.clone(), event_id)) {
            continue;
        }

        if let Some(node) = fetch_chain_node_db(db, &table, event_id, None).await? {
            events.push(node);
        }

        let links = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT parent_event_id, child_event_id, parent_table, child_table, link_type
                 FROM event_links
                 WHERE (parent_table = ? AND parent_event_id = ?)
                    OR (child_table = ? AND child_event_id = ?)
                 LIMIT 100",
                [
                    table.clone().into(),
                    event_id.into(),
                    table.clone().into(),
                    event_id.into(),
                ],
            ))
            .await?;

        for link in links {
            let parent_id = link.try_get::<i64>("", "parent_event_id").unwrap_or_default();
            let child_id = link.try_get::<i64>("", "child_event_id").unwrap_or_default();
            let parent_table = normalize_event_table(
                &link
                    .try_get::<String>("", "parent_table")
                    .unwrap_or_else(|_| "activity_events".to_string()),
            );
            let child_table = normalize_event_table(
                &link
                    .try_get::<String>("", "child_table")
                    .unwrap_or_else(|_| "activity_events".to_string()),
            );

            let neighbor = if parent_table == table && parent_id == event_id {
                (child_table, child_id)
            } else {
                (parent_table, parent_id)
            };

            if visited.contains(&(neighbor.0.clone(), neighbor.1)) {
                continue;
            }

            if visited.len() + queue.len() >= 20 {
                break;
            }

            queue.push_back(neighbor);
        }
    }

    events.sort_by(|a, b| a.happened_at.cmp(&b.happened_at));

    Ok(EventChain { events })
}

#[tauri::command]
pub async fn get_event_chain(
    payload: EventChainInput,
    state: State<'_, AppState>,
) -> AppResult<EventChain> {
    let user = require_session!(state);
    let access = resolve_log_view_access(&state, user.user_id).await?;
    if !access.has_global {
        if normalize_event_table(&payload.root_table) != "activity_events" {
            return Err(AppError::PermissionDenied(
                "Permission denied: entity-scoped log.view can only open activity event chains"
                    .to_string(),
            ));
        }
        let entity_scope_id =
            fetch_activity_entity_scope_id(&state.db, payload.root_event_id).await?;
        match entity_scope_id {
            Some(entity_id) if access.accessible_entities.contains(&entity_id) => {}
            _ => {
                return Err(AppError::PermissionDenied(
                    "Permission denied: log.view access does not cover this chain root".to_string(),
                ));
            }
        }
    }

    build_event_chain(&state.db, &payload).await
}

fn parse_activity_summary(row: &sea_orm::QueryResult) -> ActivityEventSummary {
    let summary_json = row
        .try_get::<Option<String>>("", "summary_json")
        .unwrap_or(None)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());

    ActivityEventSummary {
        id: row.try_get::<i64>("", "id").unwrap_or_default(),
        event_class: row.try_get::<String>("", "event_class").unwrap_or_default(),
        event_code: row.try_get::<String>("", "event_code").unwrap_or_default(),
        source_module: row.try_get::<String>("", "source_module").unwrap_or_default(),
        source_record_type: row
            .try_get::<Option<String>>("", "source_record_type")
            .unwrap_or(None),
        source_record_id: row
            .try_get::<Option<String>>("", "source_record_id")
            .unwrap_or(None),
        entity_scope_id: row.try_get::<Option<i64>>("", "entity_scope_id").unwrap_or(None),
        actor_id: row.try_get::<Option<i64>>("", "actor_id").unwrap_or(None),
        actor_username: row
            .try_get::<Option<String>>("", "actor_username")
            .unwrap_or(None),
        happened_at: row.try_get::<String>("", "happened_at").unwrap_or_default(),
        severity: row
            .try_get::<String>("", "severity")
            .unwrap_or_else(|_| "info".to_string()),
        summary_json,
        correlation_id: row
            .try_get::<Option<String>>("", "correlation_id")
            .unwrap_or(None),
        visibility_scope: row
            .try_get::<String>("", "visibility_scope")
            .unwrap_or_else(|_| "global".to_string()),
    }
}

async fn resolve_accessible_entity_scope_ids(
    state: &State<'_, AppState>,
    user_id: i32,
) -> AppResult<Vec<i64>> {
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT usa.scope_reference
             FROM user_scope_assignments usa
             INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
             INNER JOIN permissions p ON p.id = rp.permission_id
             WHERE usa.user_id = ?
               AND usa.deleted_at IS NULL
               AND (usa.valid_from IS NULL OR usa.valid_from <= datetime('now'))
               AND (usa.valid_to IS NULL OR usa.valid_to >= datetime('now'))
               AND usa.scope_type IN ('entity', 'org_node')
               AND p.name = 'log.view'",
            [i64::from(user_id).into()],
        ))
        .await?;

    let mut ids = Vec::new();
    for row in rows {
        if let Some(raw) = row
            .try_get::<Option<String>>("", "scope_reference")
            .unwrap_or(None)
        {
            if let Ok(parsed) = raw.parse::<i64>() {
                ids.push(parsed);
            }
        }
    }
    Ok(ids)
}

async fn resolve_log_view_access(
    state: &State<'_, AppState>,
    user_id: i32,
) -> AppResult<LogViewAccess> {
    let has_global = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user_id,
        "log.view",
        &PermissionScope::Global,
    )
    .await?;
    if has_global {
        return Ok(LogViewAccess {
            has_global: true,
            accessible_entities: Vec::new(),
        });
    }

    let accessible_entities = resolve_accessible_entity_scope_ids(state, user_id).await?;
    if accessible_entities.is_empty() {
        return Err(AppError::PermissionDenied(
            "Permission denied: no accessible entity scopes for log.view".to_string(),
        ));
    }

    Ok(LogViewAccess {
        has_global: false,
        accessible_entities,
    })
}

async fn fetch_activity_entity_scope_id(
    db: &DatabaseConnection,
    event_id: i64,
) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_scope_id FROM activity_events WHERE id = ? LIMIT 1",
            [event_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "activity_event".to_string(),
            id: event_id.to_string(),
        })?;
    Ok(row
        .try_get::<Option<i64>>("", "entity_scope_id")
        .unwrap_or(None))
}

fn normalize_event_table(table: &str) -> String {
    match table {
        "activity_events" | "audit_events" | "notification_events" => table.to_string(),
        _ => "activity_events".to_string(),
    }
}

async fn fetch_chain_node_db(
    db: &DatabaseConnection,
    table: &str,
    event_id: i64,
    link_type: Option<String>,
) -> AppResult<Option<EventChainNode>> {
    let sql = match table {
        "audit_events" => {
            "SELECT id, happened_at, action_code, NULL AS event_code, NULL AS source_module
             FROM audit_events WHERE id = ? LIMIT 1"
        }
        "notification_events" => {
            "SELECT id, occurred_at AS happened_at, event_code, event_code AS action_code, source_module
             FROM notification_events WHERE id = ? LIMIT 1"
        }
        _ => {
            "SELECT id, happened_at, event_code, NULL AS action_code, source_module
             FROM activity_events WHERE id = ? LIMIT 1"
        }
    };

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [event_id.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(EventChainNode {
        table: table.to_string(),
        event_id: row.try_get::<i64>("", "id").unwrap_or(event_id),
        happened_at: row.try_get::<String>("", "happened_at").unwrap_or_default(),
        event_code: row.try_get::<Option<String>>("", "event_code").unwrap_or(None),
        action_code: row.try_get::<Option<String>>("", "action_code").unwrap_or(None),
        source_module: row
            .try_get::<Option<String>>("", "source_module")
            .unwrap_or(None),
        link_type,
    }))
}
