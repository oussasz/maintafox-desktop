//! Append-only WO action event log.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::wo::time::now_utc_z;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoActionEvent {
    pub id: i64,
    pub wo_id: i64,
    pub action_code: String,
    pub actor_id: Option<i64>,
    pub acted_at: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub payload_json: Option<String>,
}

pub async fn emit_action_event(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    action_code: &str,
    actor_id: Option<i64>,
    from_status: Option<&str>,
    to_status: Option<&str>,
    payload_json: Option<&str>,
) -> AppResult<()> {
    let now = now_utc_z();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_action_events \
         (wo_id, action_code, actor_id, acted_at, from_status, to_status, payload_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            wo_id.into(),
            action_code.into(),
            actor_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            now.into(),
            from_status
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            to_status
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            payload_json
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn list_action_events(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<Vec<WoActionEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, wo_id, action_code, actor_id, acted_at, from_status, to_status, payload_json \
             FROM wo_action_events WHERE wo_id = ? ORDER BY acted_at ASC, id ASC",
            [wo_id.into()],
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(WoActionEvent {
            id: row
                .try_get("", "id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("id: {e}")))?,
            wo_id: row
                .try_get("", "wo_id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("wo_id: {e}")))?,
            action_code: row
                .try_get("", "action_code")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("action_code: {e}")))?,
            actor_id: row
                .try_get("", "actor_id")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("actor_id: {e}")))?,
            acted_at: row
                .try_get("", "acted_at")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("acted_at: {e}")))?,
            from_status: row
                .try_get("", "from_status")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("from_status: {e}")))?,
            to_status: row
                .try_get("", "to_status")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("to_status: {e}")))?,
            payload_json: row
                .try_get("", "payload_json")
                .map_err(|e| AppError::Internal(anyhow::anyhow!("payload_json: {e}")))?,
        });
    }
    Ok(out)
}
