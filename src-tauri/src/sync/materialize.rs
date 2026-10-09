//! Inbox materialization — apply pending sync_inbox rows to domain tables.
//!
//! Ingest (`apply_sync_batch`) stores envelopes as `pending`.
//! This module upserts real domain rows, then marks `applied` / `conflict`.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    SYNC_ENTITY_ARTICLES, SYNC_ENTITY_EQUIPMENT, SYNC_ENTITY_INTERVENTION_REQUESTS,
    SYNC_ENTITY_MEDIA_ATTACHMENTS, SYNC_ENTITY_ORG_NODES, SYNC_ENTITY_PERSONNEL,
    SYNC_ENTITY_PM_OCCURRENCES, SYNC_ENTITY_PM_PLANS, SYNC_ENTITY_PM_PLAN_VERSIONS,
    SYNC_ENTITY_REFERENCE_VALUES, SYNC_ENTITY_ROLES, SYNC_ENTITY_STOCK_LOCATIONS,
    SYNC_ENTITY_STOCK_MOVEMENTS, SYNC_ENTITY_STOCK_RESERVATIONS, SYNC_ENTITY_TEAMS,
    SYNC_ENTITY_USER_ACCOUNTS, SYNC_ENTITY_WAREHOUSES, SYNC_ENTITY_WORK_ORDERS,
    SYNC_ENTITY_WORK_ORDER_INTERVENERS, SYNC_ENTITY_WORK_ORDER_TASKS,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaterializeInboxResult {
    pub processed: i64,
    pub applied: i64,
    pub conflicts: i64,
    pub skipped: i64,
    pub errors: Vec<String>,
}

/// Dependency-safe apply priority (lower = earlier).
pub fn materialize_priority(entity_type: &str) -> u16 {
    match entity_type {
        x if x == SYNC_ENTITY_ORG_NODES => 10,
        x if x == SYNC_ENTITY_ROLES => 20,
        x if x == SYNC_ENTITY_USER_ACCOUNTS => 30,
        x if x == SYNC_ENTITY_PERSONNEL || x == SYNC_ENTITY_TEAMS => 40,
        x if x == SYNC_ENTITY_EQUIPMENT => 50,
        x if x == SYNC_ENTITY_REFERENCE_VALUES => 55,
        x if x == SYNC_ENTITY_WAREHOUSES || x == SYNC_ENTITY_STOCK_LOCATIONS || x == SYNC_ENTITY_ARTICLES => 60,
        x if x == SYNC_ENTITY_STOCK_RESERVATIONS => 64,
        x if x == SYNC_ENTITY_STOCK_MOVEMENTS => 65,
        x if x == SYNC_ENTITY_INTERVENTION_REQUESTS => 70,
        x if x == SYNC_ENTITY_PM_PLANS => 80,
        x if x == SYNC_ENTITY_PM_PLAN_VERSIONS => 81,
        x if x == SYNC_ENTITY_PM_OCCURRENCES => 82,
        x if x == SYNC_ENTITY_WORK_ORDERS => 90,
        x if x == SYNC_ENTITY_WORK_ORDER_TASKS || x == SYNC_ENTITY_WORK_ORDER_INTERVENERS => 91,
        x if x == SYNC_ENTITY_MEDIA_ATTACHMENTS => 100,
        _ => 500,
    }
}

/// Materialize all pending inbox rows (ordered by apply priority then id).
pub async fn materialize_inbox_batch(
    db: &DatabaseConnection,
    limit: Option<i64>,
) -> AppResult<MaterializeInboxResult> {
    let lim = limit.unwrap_or(200).clamp(1, 2000);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_type, entity_sync_id, operation, row_version, payload_json
             FROM sync_inbox
             WHERE apply_status = 'pending'
             ORDER BY id ASC
             LIMIT ?",
            [lim.into()],
        ))
        .await?;

    let mut items: Vec<(i64, String, String, String, i64, String)> = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push((
            row.try_get("", "id")
                .map_err(|e| AppError::SyncError(format!("inbox id: {e}")))?,
            row.try_get("", "entity_type")
                .map_err(|e| AppError::SyncError(format!("entity_type: {e}")))?,
            row.try_get("", "entity_sync_id")
                .map_err(|e| AppError::SyncError(format!("entity_sync_id: {e}")))?,
            row.try_get("", "operation")
                .map_err(|e| AppError::SyncError(format!("operation: {e}")))?,
            row.try_get("", "row_version")
                .map_err(|e| AppError::SyncError(format!("row_version: {e}")))?,
            row.try_get("", "payload_json")
                .map_err(|e| AppError::SyncError(format!("payload_json: {e}")))?,
        ));
    }

    items.sort_by(|a, b| {
        materialize_priority(&a.1)
            .cmp(&materialize_priority(&b.1))
            .then(a.0.cmp(&b.0))
    });

    let mut result = MaterializeInboxResult {
        processed: 0,
        applied: 0,
        conflicts: 0,
        skipped: 0,
        errors: Vec::new(),
    };

    for (id, entity_type, entity_sync_id, operation, row_version, payload_json) in items {
        result.processed += 1;
        let tx = db.begin().await?;
        match materialize_one(
            &tx,
            &entity_type,
            &entity_sync_id,
            &operation,
            row_version,
            &payload_json,
        )
        .await
        {
            Ok(ApplyOutcome::Applied) => {
                mark_inbox(&tx, id, "applied", None, None).await?;
                tx.commit().await?;
                result.applied += 1;
            }
            Ok(ApplyOutcome::Conflict { code, message }) => {
                mark_inbox(&tx, id, "conflict", Some(&code), Some(&message)).await?;
                tx.commit().await?;
                result.conflicts += 1;
            }
            Ok(ApplyOutcome::Skipped) => {
                mark_inbox(&tx, id, "applied", None, None).await?;
                tx.commit().await?;
                result.skipped += 1;
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = mark_inbox(&tx, id, "conflict", Some("MATERIALIZE_ERROR"), Some(&msg)).await;
                let _ = tx.commit().await;
                result.conflicts += 1;
                result.errors.push(format!("{entity_type}/{entity_sync_id}: {msg}"));
            }
        }
    }

    Ok(result)
}

enum ApplyOutcome {
    Applied,
    Skipped,
    Conflict { code: String, message: String },
}

async fn mark_inbox(
    conn: &impl ConnectionTrait,
    id: i64,
    status: &str,
    rejection_code: Option<&str>,
    rejection_message: Option<&str>,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_inbox
         SET apply_status = ?,
             rejection_code = ?,
             rejection_message = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [
            status.into(),
            rejection_code.into(),
            rejection_message.into(),
            id.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn materialize_one(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    operation: &str,
    row_version: i64,
    payload_json: &str,
) -> AppResult<ApplyOutcome> {
    if operation == "delete" || operation == "archive" {
        return delete_by_sync_id(conn, entity_type, entity_sync_id).await;
    }

    let payload: Value = serde_json::from_str(payload_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("invalid payload_json: {e}")]))?;

    match entity_type {
        x if x == SYNC_ENTITY_EQUIPMENT => upsert_equipment(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_USER_ACCOUNTS => {
            upsert_user_account(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_ROLES => upsert_role(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_ORG_NODES => upsert_org_node(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_INTERVENTION_REQUESTS => {
            upsert_intervention_request(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_WORK_ORDERS => upsert_work_order(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_WORK_ORDER_TASKS => {
            upsert_work_order_task(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_WORK_ORDER_INTERVENERS => {
            upsert_work_order_intervener(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_PM_PLANS => upsert_pm_plan(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_PM_PLAN_VERSIONS => {
            upsert_pm_plan_version(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_PM_OCCURRENCES => {
            upsert_pm_occurrence(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_ARTICLES => upsert_article(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_WAREHOUSES => upsert_warehouse(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_STOCK_LOCATIONS => {
            upsert_stock_location(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_STOCK_MOVEMENTS => {
            upsert_stock_movement(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_STOCK_RESERVATIONS => {
            upsert_stock_reservation(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_PERSONNEL => upsert_personnel(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_TEAMS => upsert_team(conn, entity_sync_id, row_version, &payload).await,
        x if x == SYNC_ENTITY_REFERENCE_VALUES => {
            upsert_reference_value(conn, entity_sync_id, row_version, &payload).await
        }
        x if x == SYNC_ENTITY_MEDIA_ATTACHMENTS => {
            upsert_media_attachment(conn, entity_sync_id, row_version, &payload).await
        }
        _ => Ok(ApplyOutcome::Skipped),
    }
}

async fn local_row_version(
    conn: &impl ConnectionTrait,
    table: &str,
    entity_sync_id: &str,
) -> AppResult<Option<i64>> {
    // table names are internal constants only
    let sql = format!("SELECT row_version FROM {table} WHERE entity_sync_id = ? LIMIT 1");
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [entity_sync_id.to_string().into()],
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => Ok(Some(
            r.try_get::<i64>("", "row_version")
                .map_err(|e| AppError::SyncError(format!("row_version: {e}")))?,
        )),
    }
}

fn json_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn json_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

fn json_f64(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

fn json_bool_i64(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        _ => 0,
    }
}

async fn resolve_local_id(
    conn: &impl ConnectionTrait,
    table: &str,
    entity_sync_id: &str,
) -> AppResult<Option<i64>> {
    let sql = format!("SELECT id FROM {table} WHERE entity_sync_id = ? LIMIT 1");
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [entity_sync_id.to_string().into()],
        ))
        .await?;
    Ok(row
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(|e| AppError::SyncError(format!("resolve id: {e}")))?)
}

async fn check_row_version_conflict(
    conn: &impl ConnectionTrait,
    table: &str,
    entity_sync_id: &str,
    incoming_version: i64,
) -> AppResult<Option<ApplyOutcome>> {
    if let Some(local) = local_row_version(conn, table, entity_sync_id).await? {
        if local > incoming_version {
            return Ok(Some(ApplyOutcome::Conflict {
                code: "STALE_INBOUND".into(),
                message: format!("local row_version {local} > inbound {incoming_version}"),
            }));
        }
        if local == incoming_version {
            return Ok(Some(ApplyOutcome::Skipped));
        }
    }
    Ok(None)
}

async fn delete_by_sync_id(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<ApplyOutcome> {
    let table = match entity_type {
        x if x == SYNC_ENTITY_EQUIPMENT => "equipment",
        x if x == SYNC_ENTITY_USER_ACCOUNTS => "user_accounts",
        x if x == SYNC_ENTITY_ROLES => "roles",
        x if x == SYNC_ENTITY_ORG_NODES => "org_nodes",
        x if x == SYNC_ENTITY_INTERVENTION_REQUESTS => "intervention_requests",
        x if x == SYNC_ENTITY_WORK_ORDERS => "work_orders",
        x if x == SYNC_ENTITY_WORK_ORDER_TASKS => "work_order_tasks",
        x if x == SYNC_ENTITY_WORK_ORDER_INTERVENERS => "work_order_interveners",
        x if x == SYNC_ENTITY_PM_PLANS => "pm_plans",
        x if x == SYNC_ENTITY_PM_PLAN_VERSIONS => "pm_plan_versions",
        x if x == SYNC_ENTITY_PM_OCCURRENCES => "pm_occurrences",
        x if x == SYNC_ENTITY_ARTICLES => "articles",
        x if x == SYNC_ENTITY_WAREHOUSES => "warehouses",
        x if x == SYNC_ENTITY_STOCK_LOCATIONS => "stock_locations",
        x if x == SYNC_ENTITY_STOCK_MOVEMENTS => "inventory_transactions",
        x if x == SYNC_ENTITY_STOCK_RESERVATIONS => "stock_reservations",
        x if x == SYNC_ENTITY_PERSONNEL => "personnel",
        x if x == SYNC_ENTITY_TEAMS => "teams",
        x if x == SYNC_ENTITY_MEDIA_ATTACHMENTS => "media_attachments",
        x if x == SYNC_ENTITY_REFERENCE_VALUES => "reference_values",
        _ => return Ok(ApplyOutcome::Skipped),
    };
    let sql = format!("DELETE FROM {table} WHERE entity_sync_id = ?");
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        &sql,
        [entity_sync_id.to_string().into()],
    ))
    .await?;
    Ok(ApplyOutcome::Applied)
}

async fn upsert_equipment(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "equipment", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let code = json_str(p, "asset_code")
        .or_else(|| json_str(p, "asset_id_code"))
        .unwrap_or_else(|| format!("EQ-{entity_sync_id}"));
    let name = json_str(p, "asset_name")
        .or_else(|| json_str(p, "name"))
        .unwrap_or_else(|| code.clone());
    let status = json_str(p, "status_code")
        .or_else(|| json_str(p, "lifecycle_status"))
        .unwrap_or_else(|| "active_in_service".into());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let org_sync = json_str(p, "org_node_entity_sync_id");
    let org_id = match org_sync.as_deref() {
        Some(s) => resolve_local_id(conn, "org_nodes", s).await?,
        None => None,
    };

    if resolve_local_id(conn, "equipment", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE equipment SET asset_id_code = ?, name = ?, lifecycle_status = ?,
             installed_at_node_id = ?, row_version = ?, updated_at = ?
             WHERE entity_sync_id = ?",
            [
                code.into(),
                name.into(),
                status.into(),
                org_id.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO equipment (
                entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
                row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                name.into(),
                status.into(),
                org_id.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_user_account(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "user_accounts", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let username = json_str(p, "username").ok_or_else(|| {
        AppError::ValidationFailed(vec!["user_accounts.username required".into()])
    })?;
    let display_name = json_str(p, "display_name");
    let password_hash = json_str(p, "password_hash");
    let identity_mode = json_str(p, "identity_mode").unwrap_or_else(|| "local".into());
    let is_active = json_bool_i64(p, "is_active").max(0);
    let is_admin = json_bool_i64(p, "is_admin");
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    if resolve_local_id(conn, "user_accounts", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE user_accounts SET username = ?, display_name = ?, password_hash = COALESCE(?, password_hash),
             identity_mode = ?, is_active = ?, is_admin = ?, row_version = ?, updated_at = ?
             WHERE entity_sync_id = ?",
            [
                username.into(),
                display_name.into(),
                password_hash.into(),
                identity_mode.into(),
                is_active.into(),
                is_admin.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO user_accounts (
                entity_sync_id, username, display_name, password_hash, identity_mode,
                is_active, is_admin, force_password_change, row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                username.into(),
                display_name.into(),
                password_hash.into(),
                identity_mode.into(),
                is_active.into(),
                is_admin.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_role(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "roles", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let name = json_str(p, "name").ok_or_else(|| {
        AppError::ValidationFailed(vec!["roles.name required".into()])
    })?;
    let description = json_str(p, "description");
    let role_type = json_str(p, "role_type").unwrap_or_else(|| "custom".into());
    let status = json_str(p, "status").unwrap_or_else(|| "active".into());
    let is_system = json_bool_i64(p, "is_system");
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    if resolve_local_id(conn, "roles", entity_sync_id).await?.is_some() {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE roles SET name = ?, description = ?, role_type = ?, status = ?, is_system = ?,
             row_version = ?, updated_at = ? WHERE entity_sync_id = ?",
            [
                name.into(),
                description.into(),
                role_type.into(),
                status.into(),
                is_system.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO roles (
                entity_sync_id, name, description, is_system, role_type, status,
                row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                name.into(),
                description.into(),
                is_system.into(),
                role_type.into(),
                status.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_org_node(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "org_nodes", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let code = json_str(p, "code").unwrap_or_else(|| format!("ORG-{entity_sync_id}"));
    let name = json_str(p, "name").unwrap_or_else(|| code.clone());
    let status = json_str(p, "status").unwrap_or_else(|| "active".into());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let parent_sync = json_str(p, "parent_entity_sync_id");
    let parent_id = match parent_sync.as_deref() {
        Some(s) => resolve_local_id(conn, "org_nodes", s).await?,
        None => None,
    };
    let depth = json_i64(p, "depth").unwrap_or(0);
    let ancestor_path = json_str(p, "ancestor_path").unwrap_or_default();

    // Prefer an existing node_type when present
    let node_type_id = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM org_node_types WHERE is_active = 1 ORDER BY id ASC LIMIT 1",
            [],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(|e| AppError::SyncError(format!("node_type: {e}")))?;

    let Some(node_type_id) = node_type_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_NODE_TYPE".into(),
            message: "No org_node_types available for materialization".into(),
        });
    };

    if resolve_local_id(conn, "org_nodes", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE org_nodes SET code = ?, name = ?, parent_id = ?, ancestor_path = ?, depth = ?,
             status = ?, row_version = ?, updated_at = ? WHERE entity_sync_id = ?",
            [
                code.into(),
                name.into(),
                parent_id.into(),
                ancestor_path.into(),
                depth.into(),
                status.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        // structure_model_id may be required on newer schemas
        let structure_model_id = conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id FROM org_structure_models ORDER BY id ASC LIMIT 1",
                [],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "id"))
            .transpose()
            .ok()
            .flatten();

        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO org_nodes (
                entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth,
                status, structure_model_id, row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                name.into(),
                node_type_id.into(),
                parent_id.into(),
                ancestor_path.into(),
                depth.into(),
                status.into(),
                structure_model_id.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_intervention_request(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "intervention_requests", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let code = json_str(p, "code").unwrap_or_else(|| format!("DI-{entity_sync_id}"));
    let title = json_str(p, "title").unwrap_or_else(|| code.clone());
    let description = json_str(p, "description").unwrap_or_default();
    let status = json_str(p, "status").unwrap_or_else(|| "submitted".into());
    let origin_type = json_str(p, "origin_type").unwrap_or_else(|| "field".into());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let equipment_id = match json_str(p, "equipment_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "equipment", s).await?,
        None => None,
    };
    let org_node_id = match json_str(p, "org_node_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "org_nodes", s).await?,
        None => None,
    };
    let submitter_id = match json_str(p, "submitter_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "user_accounts", s).await?,
        None => None,
    };

    let Some(equipment_id) = equipment_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_EQUIPMENT".into(),
            message: "equipment_entity_sync_id could not be resolved".into(),
        });
    };
    let Some(org_node_id) = org_node_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_ORG_NODE".into(),
            message: "org_node_entity_sync_id could not be resolved".into(),
        });
    };

    if resolve_local_id(conn, "intervention_requests", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE intervention_requests SET code = ?, title = ?, description = ?, status = ?,
             equipment_id = ?, org_node_id = ?, submitter_id = COALESCE(?, submitter_id),
             row_version = ?, updated_at = ? WHERE entity_sync_id = ?",
            [
                code.into(),
                title.into(),
                description.into(),
                status.into(),
                equipment_id.into(),
                org_node_id.into(),
                submitter_id.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        let submitter = submitter_id.unwrap_or(1);
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO intervention_requests (
                entity_sync_id, code, equipment_id, org_node_id, status, title, description,
                origin_type, submitter_id, submitted_at, row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                equipment_id.into(),
                org_node_id.into(),
                status.into(),
                title.into(),
                description.into(),
                origin_type.into(),
                submitter.into(),
                updated_at.clone().into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_work_order(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "work_orders", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let code = json_str(p, "code").unwrap_or_else(|| format!("OT-{entity_sync_id}"));
    let title = json_str(p, "title").unwrap_or_else(|| code.clone());
    let description = json_str(p, "description");
    let status_code = json_str(p, "status").unwrap_or_else(|| "draft".into());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let status_id = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = ? LIMIT 1",
            [status_code.clone().into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(|e| AppError::SyncError(format!("wo status: {e}")))?
        .or(Some(1));

    let type_id = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM work_order_types ORDER BY id ASC LIMIT 1",
            [],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(|e| AppError::SyncError(format!("wo type: {e}")))?
        .unwrap_or(1);

    let equipment_id = match json_str(p, "equipment_entity_sync_id")
        .or_else(|| json_str(p, "asset_entity_sync_id"))
        .as_deref()
    {
        Some(s) => resolve_local_id(conn, "equipment", s).await?,
        None => None,
    };
    let responsible_id = match json_str(p, "primary_responsible_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "user_accounts", s).await?,
        None => None,
    };
    let team_id = match json_str(p, "assigned_team_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "teams", s).await?,
        None => None,
    };
    let planned_start = json_str(p, "planned_start");
    let planned_end = json_str(p, "planned_end");
    let scheduled_at = json_str(p, "scheduled_at");

    let Some(status_id) = status_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_WO_STATUS".into(),
            message: format!("status '{status_code}' not found"),
        });
    };

    if resolve_local_id(conn, "work_orders", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE work_orders SET code = ?, title = ?, description = ?, status_id = ?,
             equipment_id = ?, primary_responsible_id = ?, assigned_team_id = ?,
             planned_start = ?, planned_end = ?, scheduled_at = ?,
             row_version = ?, updated_at = ? WHERE entity_sync_id = ?",
            [
                code.into(),
                title.into(),
                description.into(),
                status_id.into(),
                equipment_id.into(),
                responsible_id.into(),
                team_id.into(),
                planned_start.into(),
                planned_end.into(),
                scheduled_at.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO work_orders (
                entity_sync_id, code, type_id, status_id, title, description, equipment_id,
                primary_responsible_id, assigned_team_id, planned_start, planned_end, scheduled_at,
                row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                type_id.into(),
                status_id.into(),
                title.into(),
                description.into(),
                equipment_id.into(),
                responsible_id.into(),
                team_id.into(),
                planned_start.into(),
                planned_end.into(),
                scheduled_at.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_work_order_task(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "work_order_tasks", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let wo_sync = json_str(p, "work_order_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["work_order_entity_sync_id required".into()])
    })?;
    let Some(wo_id) = resolve_local_id(conn, "work_orders", &wo_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_WO".into(),
            message: format!("work_order {wo_sync} not found"),
        });
    };
    let desc = json_str(p, "task_description").unwrap_or_default();
    let seq = json_i64(p, "sequence_order").unwrap_or(0);
    let mandatory = json_bool_i64(p, "is_mandatory");
    let completed = json_bool_i64(p, "is_completed");
    let result_code = json_str(p, "result_code");
    let notes = json_str(p, "notes");

    if resolve_local_id(conn, "work_order_tasks", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE work_order_tasks SET work_order_id = ?, task_description = ?, sequence_order = ?,
             is_mandatory = ?, is_completed = ?, result_code = ?, notes = ?, row_version = ?
             WHERE entity_sync_id = ?",
            [
                wo_id.into(),
                desc.into(),
                seq.into(),
                mandatory.into(),
                completed.into(),
                result_code.into(),
                notes.into(),
                row_version.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO work_order_tasks (
                entity_sync_id, work_order_id, task_description, sequence_order, is_mandatory,
                is_completed, result_code, notes, row_version
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                wo_id.into(),
                desc.into(),
                seq.into(),
                mandatory.into(),
                completed.into(),
                result_code.into(),
                notes.into(),
                row_version.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_work_order_intervener(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "work_order_interveners", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let wo_sync = json_str(p, "work_order_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["work_order_entity_sync_id required".into()])
    })?;
    let Some(wo_id) = resolve_local_id(conn, "work_orders", &wo_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_WO".into(),
            message: format!("work_order {wo_sync} not found"),
        });
    };
    let intervener_id = match json_str(p, "intervener_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "user_accounts", s).await?.unwrap_or(1),
        None => 1,
    };
    let hours = json_f64(p, "hours_worked");
    let notes = json_str(p, "notes");
    let started = json_str(p, "started_at");
    let ended = json_str(p, "ended_at");

    if resolve_local_id(conn, "work_order_interveners", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE work_order_interveners SET work_order_id = ?, intervener_id = ?,
             started_at = ?, ended_at = ?, hours_worked = ?, notes = ?, row_version = ?
             WHERE entity_sync_id = ?",
            [
                wo_id.into(),
                intervener_id.into(),
                started.into(),
                ended.into(),
                hours.into(),
                notes.into(),
                row_version.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO work_order_interveners (
                entity_sync_id, work_order_id, intervener_id, started_at, ended_at,
                hours_worked, notes, row_version
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                wo_id.into(),
                intervener_id.into(),
                started.into(),
                ended.into(),
                hours.into(),
                notes.into(),
                row_version.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_pm_plan(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "pm_plans", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let code = json_str(p, "code").unwrap_or_else(|| format!("PM-{entity_sync_id}"));
    let title = json_str(p, "title").unwrap_or_else(|| code.clone());
    let strategy = json_str(p, "strategy_type").unwrap_or_else(|| "calendar".into());
    let scope = json_str(p, "asset_scope_type").unwrap_or_else(|| "equipment".into());
    let lifecycle = json_str(p, "lifecycle_status").unwrap_or_else(|| "draft".into());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let team_id = match json_str(p, "assigned_team_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "teams", s).await?,
        None => None,
    };

    if resolve_local_id(conn, "pm_plans", entity_sync_id).await?.is_some() {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE pm_plans SET code = ?, title = ?, strategy_type = ?, asset_scope_type = ?,
             lifecycle_status = ?, assigned_team_id = ?, row_version = ?, updated_at = ?
             WHERE entity_sync_id = ?",
            [
                code.into(),
                title.into(),
                strategy.into(),
                scope.into(),
                lifecycle.into(),
                team_id.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO pm_plans (
                entity_sync_id, code, title, asset_scope_type, strategy_type, lifecycle_status,
                assigned_team_id, row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                title.into(),
                scope.into(),
                strategy.into(),
                lifecycle.into(),
                team_id.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_pm_plan_version(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "pm_plan_versions", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let plan_sync = json_str(p, "pm_plan_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["pm_plan_entity_sync_id required".into()])
    })?;
    let Some(plan_id) = resolve_local_id(conn, "pm_plans", &plan_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_PM_PLAN".into(),
            message: plan_sync,
        });
    };
    let version_no = json_i64(p, "version_no").unwrap_or(1);
    let status = json_str(p, "status").unwrap_or_else(|| "draft".into());
    let effective_from = json_str(p, "effective_from").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let trigger = json_str(p, "trigger_definition_json").unwrap_or_else(|| "{}".into());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    if resolve_local_id(conn, "pm_plan_versions", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE pm_plan_versions SET pm_plan_id = ?, version_no = ?, status = ?, effective_from = ?,
             trigger_definition_json = ?, row_version = ?, updated_at = ? WHERE entity_sync_id = ?",
            [
                plan_id.into(),
                version_no.into(),
                status.into(),
                effective_from.into(),
                trigger.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO pm_plan_versions (
                entity_sync_id, pm_plan_id, version_no, status, effective_from,
                trigger_definition_json, row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                plan_id.into(),
                version_no.into(),
                status.into(),
                effective_from.into(),
                trigger.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_pm_occurrence(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "pm_occurrences", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let plan_sync = json_str(p, "pm_plan_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["pm_plan_entity_sync_id required".into()])
    })?;
    let ver_sync = json_str(p, "pm_plan_version_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["pm_plan_version_entity_sync_id required".into()])
    })?;
    let Some(plan_id) = resolve_local_id(conn, "pm_plans", &plan_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_PM_PLAN".into(),
            message: plan_sync,
        });
    };
    let Some(ver_id) = resolve_local_id(conn, "pm_plan_versions", &ver_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_PM_VERSION".into(),
            message: ver_sync,
        });
    };
    let due_basis = json_str(p, "due_basis").unwrap_or_else(|| "calendar".into());
    let status = json_str(p, "status").unwrap_or_else(|| "forecasted".into());
    let due_at = json_str(p, "due_at");
    let wo_id = match json_str(p, "linked_work_order_entity_sync_id").as_deref() {
        Some(s) => resolve_local_id(conn, "work_orders", s).await?,
        None => None,
    };

    if resolve_local_id(conn, "pm_occurrences", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE pm_occurrences SET pm_plan_id = ?, plan_version_id = ?, due_basis = ?, due_at = ?,
             status = ?, linked_work_order_id = ?, row_version = ? WHERE entity_sync_id = ?",
            [
                plan_id.into(),
                ver_id.into(),
                due_basis.into(),
                due_at.into(),
                status.into(),
                wo_id.into(),
                row_version.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO pm_occurrences (
                entity_sync_id, pm_plan_id, plan_version_id, due_basis, due_at, status,
                linked_work_order_id, row_version
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                plan_id.into(),
                ver_id.into(),
                due_basis.into(),
                due_at.into(),
                status.into(),
                wo_id.into(),
                row_version.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_article(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "articles", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let code = json_str(p, "article_code").unwrap_or_else(|| format!("ART-{entity_sync_id}"));
    let name = json_str(p, "article_name").unwrap_or_else(|| code.clone());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let unit_id = json_i64(p, "unit_value_id").unwrap_or(1);
    let is_active = json_bool_i64(p, "is_active").max(1);

    if resolve_local_id(conn, "articles", entity_sync_id).await?.is_some() {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE articles SET article_code = ?, article_name = ?, unit_value_id = ?,
             is_active = ?, row_version = ?, updated_at = ? WHERE entity_sync_id = ?",
            [
                code.into(),
                name.into(),
                unit_id.into(),
                is_active.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO articles (
                entity_sync_id, article_code, article_name, unit_value_id, is_active,
                row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                name.into(),
                unit_id.into(),
                is_active.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_warehouse(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "warehouses", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let code = json_str(p, "code").unwrap_or_else(|| format!("WH-{entity_sync_id}"));
    let name = json_str(p, "name").unwrap_or_else(|| code.clone());
    let is_active = json_bool_i64(p, "is_active").max(1);
    let created_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    if resolve_local_id(conn, "warehouses", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE warehouses SET code = ?, name = ?, is_active = ?, row_version = ?
             WHERE entity_sync_id = ?",
            [
                code.into(),
                name.into(),
                is_active.into(),
                row_version.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO warehouses (entity_sync_id, code, name, is_active, row_version, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                name.into(),
                is_active.into(),
                row_version.into(),
                created_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_stock_location(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "stock_locations", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let wh_sync = json_str(p, "warehouse_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["warehouse_entity_sync_id required".into()])
    })?;
    let Some(wh_id) = resolve_local_id(conn, "warehouses", &wh_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_WAREHOUSE".into(),
            message: wh_sync,
        });
    };
    let code = json_str(p, "code").unwrap_or_else(|| "DEFAULT".into());
    let name = json_str(p, "name").unwrap_or_else(|| code.clone());

    if resolve_local_id(conn, "stock_locations", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE stock_locations SET warehouse_id = ?, code = ?, name = ?, row_version = ?
             WHERE entity_sync_id = ?",
            [
                wh_id.into(),
                code.into(),
                name.into(),
                row_version.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO stock_locations (
                entity_sync_id, warehouse_id, code, name, row_version, created_at
             ) VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            [
                entity_sync_id.to_string().into(),
                wh_id.into(),
                code.into(),
                name.into(),
                row_version.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_stock_movement(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    // Append-only: skip if already present
    if resolve_local_id(conn, "inventory_transactions", entity_sync_id)
        .await?
        .is_some()
    {
        return Ok(ApplyOutcome::Skipped);
    }
    let article_sync = json_str(p, "article_entity_sync_id");
    let loc_sync = json_str(p, "location_entity_sync_id");
    let wh_sync = json_str(p, "warehouse_entity_sync_id");
    let article_id = match article_sync.as_deref() {
        Some(s) => resolve_local_id(conn, "articles", s).await?,
        None => None,
    };
    let location_id = match loc_sync.as_deref() {
        Some(s) => resolve_local_id(conn, "stock_locations", s).await?,
        None => None,
    };
    let warehouse_id = match wh_sync.as_deref() {
        Some(s) => resolve_local_id(conn, "warehouses", s).await?,
        None => None,
    };
    let Some(article_id) = article_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_ARTICLE".into(),
            message: "article_entity_sync_id required".into(),
        });
    };
    let Some(location_id) = location_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_LOCATION".into(),
            message: "location_entity_sync_id required".into(),
        });
    };
    let Some(warehouse_id) = warehouse_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_WAREHOUSE".into(),
            message: "warehouse_entity_sync_id required".into(),
        });
    };
    let qty = json_f64(p, "quantity").unwrap_or(0.0);
    let movement_type = json_str(p, "movement_type").unwrap_or_else(|| "adjust".into());
    let source_type = json_str(p, "source_type").unwrap_or_else(|| "sync".into());
    let reason = json_str(p, "notes").or_else(|| json_str(p, "reason"));

    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO inventory_transactions (
            entity_sync_id, article_id, warehouse_id, location_id, movement_type, quantity,
            source_type, reason, row_version, performed_at, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                   strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [
            entity_sync_id.to_string().into(),
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            movement_type.into(),
            qty.into(),
            source_type.into(),
            reason.into(),
            row_version.into(),
        ],
    ))
    .await
    .map_err(|e| AppError::SyncError(format!("inventory_transactions insert: {e}")))?;
    Ok(ApplyOutcome::Applied)
}

async fn upsert_stock_reservation(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "stock_reservations", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let article_sync = json_str(p, "article_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["article_entity_sync_id required".into()])
    })?;
    let warehouse_sync = json_str(p, "warehouse_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["warehouse_entity_sync_id required".into()])
    })?;
    let location_sync = json_str(p, "location_entity_sync_id").ok_or_else(|| {
        AppError::ValidationFailed(vec!["location_entity_sync_id required".into()])
    })?;
    let Some(article_id) = resolve_local_id(conn, "articles", &article_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_ARTICLE".into(),
            message: article_sync,
        });
    };
    let Some(warehouse_id) = resolve_local_id(conn, "warehouses", &warehouse_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_WAREHOUSE".into(),
            message: warehouse_sync,
        });
    };
    let Some(location_id) = resolve_local_id(conn, "stock_locations", &location_sync).await? else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_LOCATION".into(),
            message: location_sync,
        });
    };
    let wo_sync = json_str(p, "work_order_entity_sync_id");
    let (source_type, source_id) = match wo_sync.as_deref() {
        Some(s) => match resolve_local_id(conn, "work_orders", s).await? {
            Some(id) => (Some("WORK_ORDER".to_string()), Some(id)),
            None => {
                return Ok(ApplyOutcome::Conflict {
                    code: "MISSING_WORK_ORDER".into(),
                    message: s.to_string(),
                });
            }
        },
        None => (None, None),
    };
    let qty = json_f64(p, "quantity").unwrap_or(0.0);
    let status = json_str(p, "status").unwrap_or_else(|| "active".into());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    if resolve_local_id(conn, "stock_reservations", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE stock_reservations
             SET article_id = ?, warehouse_id = ?, location_id = ?,
                 quantity_reserved = ?, status = ?,
                 source_type = COALESCE(?, source_type),
                 source_id = COALESCE(?, source_id),
                 row_version = ?, updated_at = ?
             WHERE entity_sync_id = ?",
            [
                article_id.into(),
                warehouse_id.into(),
                location_id.into(),
                qty.into(),
                status.into(),
                source_type.clone().into(),
                source_id.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO stock_reservations (
                entity_sync_id, article_id, warehouse_id, location_id,
                source_type, source_id, quantity_reserved, quantity_issued, status,
                row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                article_id.into(),
                warehouse_id.into(),
                location_id.into(),
                source_type
                    .unwrap_or_else(|| "SYNC".to_string())
                    .into(),
                source_id.into(),
                qty.into(),
                status.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_personnel(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "personnel", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let employee_code = json_str(p, "employee_code").unwrap_or_else(|| format!("P-{entity_sync_id}"));
    let full_name = json_str(p, "full_name")
        .or_else(|| {
            let first = json_str(p, "first_name").unwrap_or_default();
            let last = json_str(p, "last_name").unwrap_or_default();
            let joined = format!("{first} {last}").trim().to_string();
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        })
        .unwrap_or_else(|| employee_code.clone());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let availability = json_str(p, "availability_status").unwrap_or_else(|| "available".into());

    if resolve_local_id(conn, "personnel", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE personnel SET employee_code = ?, full_name = ?, availability_status = ?,
             row_version = ?, updated_at = ? WHERE entity_sync_id = ?",
            [
                employee_code.into(),
                full_name.into(),
                availability.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO personnel (
                entity_sync_id, employee_code, full_name, availability_status,
                row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                employee_code.into(),
                full_name.into(),
                availability.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_team(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) = check_row_version_conflict(conn, "teams", entity_sync_id, row_version).await? {
        return Ok(c);
    }
    let code = json_str(p, "code").unwrap_or_else(|| format!("T-{entity_sync_id}"));
    let name = json_str(p, "name").unwrap_or_else(|| code.clone());
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let status = json_str(p, "status").unwrap_or_else(|| {
        if json_bool_i64(p, "is_active") == 0 {
            "inactive".into()
        } else {
            "active".into()
        }
    });
    let team_type = json_str(p, "team_type").unwrap_or_else(|| "maintenance".into());

    if resolve_local_id(conn, "teams", entity_sync_id).await?.is_some() {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE teams SET code = ?, name = ?, status = ?, team_type = ?, row_version = ?, updated_at = ?
             WHERE entity_sync_id = ?",
            [
                code.into(),
                name.into(),
                status.into(),
                team_type.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO teams (
                entity_sync_id, code, name, team_type, status, row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                code.into(),
                name.into(),
                team_type.into(),
                status.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_media_attachment(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "media_attachments", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let parent_type = json_str(p, "parent_entity_type").unwrap_or_else(|| "unknown".into());
    let parent_sync = json_str(p, "parent_entity_sync_id").unwrap_or_default();
    let content_sha = json_str(p, "content_sha256").unwrap_or_default();
    let mime = json_str(p, "mime_type").unwrap_or_else(|| "application/octet-stream".into());
    let byte_size = json_i64(p, "byte_size").unwrap_or(0);
    let object_key = json_str(p, "object_key").unwrap_or_default();
    let file_name = json_str(p, "file_name").unwrap_or_else(|| "attachment.bin".into());
    let relative_path = json_str(p, "relative_path");
    let capture_meta = json_str(p, "capture_meta_json");
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    if resolve_local_id(conn, "media_attachments", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE media_attachments SET parent_entity_type = ?, parent_entity_sync_id = ?,
             content_sha256 = ?, mime_type = ?, byte_size = ?, object_key = ?, file_name = ?,
             relative_path = ?, capture_meta_json = ?, row_version = ?, updated_at = ?
             WHERE entity_sync_id = ?",
            [
                parent_type.into(),
                parent_sync.into(),
                content_sha.into(),
                mime.into(),
                byte_size.into(),
                object_key.into(),
                file_name.into(),
                relative_path.into(),
                capture_meta.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO media_attachments (
                entity_sync_id, parent_entity_type, parent_entity_sync_id, content_sha256,
                mime_type, byte_size, object_key, file_name, relative_path, capture_meta_json,
                row_version, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                entity_sync_id.to_string().into(),
                parent_type.into(),
                parent_sync.into(),
                content_sha.into(),
                mime.into(),
                byte_size.into(),
                object_key.into(),
                file_name.into(),
                relative_path.into(),
                capture_meta.into(),
                row_version.into(),
                updated_at.clone().into(),
                updated_at.into(),
            ],
        ))
        .await?;
    }
    Ok(ApplyOutcome::Applied)
}

async fn upsert_reference_value(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    row_version: i64,
    p: &Value,
) -> AppResult<ApplyOutcome> {
    if let Some(c) =
        check_row_version_conflict(conn, "reference_values", entity_sync_id, row_version).await?
    {
        return Ok(c);
    }
    let set_code = json_str(p, "set_code").ok_or_else(|| {
        AppError::ValidationFailed(vec!["reference_values.set_code required".into()])
    })?;
    let code = json_str(p, "code").ok_or_else(|| {
        AppError::ValidationFailed(vec!["reference_values.code required".into()])
    })?;
    let label = json_str(p, "label").unwrap_or_else(|| code.clone());
    let is_active = json_bool_i64(p, "is_active").max(0);
    let updated_at = json_str(p, "updated_at").unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let set_id = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rs.id AS id
             FROM reference_sets rs
             JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE rd.code = ?
             ORDER BY CASE WHEN rs.status = 'published' THEN 0 ELSE 1 END, rs.version_no DESC
             LIMIT 1",
            [set_code.clone().into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(|e| AppError::SyncError(format!("reference set resolve: {e}")))?;

    let Some(set_id) = set_id else {
        return Ok(ApplyOutcome::Conflict {
            code: "MISSING_REFERENCE_SET".into(),
            message: format!("No reference set for domain '{set_code}'"),
        });
    };

    let parent_id = match json_str(p, "parent_code").as_deref() {
        Some(pc) if !pc.is_empty() => {
            conn.query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id FROM reference_values WHERE set_id = ? AND code = ? LIMIT 1",
                [set_id.into(), pc.into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "id"))
            .transpose()
            .ok()
            .flatten()
        }
        _ => None,
    };

    if resolve_local_id(conn, "reference_values", entity_sync_id)
        .await?
        .is_some()
    {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE reference_values SET set_id = ?, code = ?, label = ?, parent_id = ?,
             is_active = ?, row_version = ?, updated_at = ?
             WHERE entity_sync_id = ?",
            [
                set_id.into(),
                code.into(),
                label.into(),
                parent_id.into(),
                is_active.into(),
                row_version.into(),
                updated_at.into(),
                entity_sync_id.to_string().into(),
            ],
        ))
        .await?;
    } else {
        let existing = conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id FROM reference_values WHERE set_id = ? AND code = ? LIMIT 1",
                [set_id.into(), code.clone().into()],
            ))
            .await?;
        if let Some(row) = existing {
            let id: i64 = row
                .try_get("", "id")
                .map_err(|e| AppError::SyncError(format!("reference value id: {e}")))?;
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE reference_values SET entity_sync_id = ?, label = ?, parent_id = ?,
                 is_active = ?, row_version = ?, updated_at = ? WHERE id = ?",
                [
                    entity_sync_id.to_string().into(),
                    label.into(),
                    parent_id.into(),
                    is_active.into(),
                    row_version.into(),
                    updated_at.into(),
                    id.into(),
                ],
            ))
            .await?;
        } else {
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO reference_values (
                    entity_sync_id, set_id, code, label, parent_id, is_active,
                    row_version, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    entity_sync_id.to_string().into(),
                    set_id.into(),
                    code.into(),
                    label.into(),
                    parent_id.into(),
                    is_active.into(),
                    row_version.into(),
                    updated_at.into(),
                ],
            ))
            .await?;
        }
    }
    Ok(ApplyOutcome::Applied)
}
