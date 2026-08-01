//! Append-only WO Execution Log (technician timeline — not Audit).

use crate::errors::{AppError, AppResult};
use crate::wo::time::now_utc_z;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoExecutionEvent {
    pub id: i64,
    pub work_order_id: i64,
    pub occurred_at: String,
    pub event_type: String,
    pub summary_key: String,
    pub summary_params_json: Option<String>,
    pub entity_kind: Option<String>,
    pub entity_id: Option<i64>,
    pub actor_id: Option<i64>,
}

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("WoExecutionEvent decode error for '{field}': {e}"))
}

pub async fn emit_execution_event(
    db: &impl ConnectionTrait,
    wo_id: i64,
    event_type: &str,
    summary_key: &str,
    summary_params: serde_json::Value,
    entity_kind: Option<&str>,
    entity_id: Option<i64>,
    actor_id: Option<i64>,
) -> AppResult<()> {
    let now = now_utc_z();
    let params = summary_params.to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_execution_events \
         (work_order_id, occurred_at, event_type, summary_key, summary_params_json, \
          entity_kind, entity_id, actor_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            wo_id.into(),
            now.into(),
            event_type.into(),
            summary_key.into(),
            params.into(),
            entity_kind
                .map(|s| sea_orm::Value::from(s.to_string()))
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            entity_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            actor_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn list_execution_events(db: &DatabaseConnection, wo_id: i64) -> AppResult<Vec<WoExecutionEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, occurred_at, event_type, summary_key, \
                    summary_params_json, entity_kind, entity_id, actor_id \
               FROM work_order_execution_events \
              WHERE work_order_id = ? \
              ORDER BY occurred_at ASC, id ASC",
            [wo_id.into()],
        ))
        .await?;

    rows.iter()
        .map(|row| {
            Ok(WoExecutionEvent {
                id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
                work_order_id: row
                    .try_get("", "work_order_id")
                    .map_err(|e| decode_err("work_order_id", e))?,
                occurred_at: row
                    .try_get("", "occurred_at")
                    .map_err(|e| decode_err("occurred_at", e))?,
                event_type: row.try_get("", "event_type").map_err(|e| decode_err("event_type", e))?,
                summary_key: row
                    .try_get("", "summary_key")
                    .map_err(|e| decode_err("summary_key", e))?,
                summary_params_json: row
                    .try_get("", "summary_params_json")
                    .map_err(|e| decode_err("summary_params_json", e))?,
                entity_kind: row
                    .try_get("", "entity_kind")
                    .map_err(|e| decode_err("entity_kind", e))?,
                entity_id: row.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?,
                actor_id: row.try_get("", "actor_id").map_err(|e| decode_err("actor_id", e))?,
            })
        })
        .collect()
}

/// Convenience for part label in log params.
pub fn part_label_json(label: &str) -> serde_json::Value {
    json!({ "label": label })
}
