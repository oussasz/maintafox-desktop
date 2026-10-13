//! Per-row synchronisation state for screens that must show what is still only on this device
//! (e.g. the inspection runner's offline queue). Read-only; derived from the capture journal and
//! the outbox, which are the real offline queue of the app.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

/// Saved on this device, not yet accepted by the server (captured or queued, or being sent).
pub const ROW_SYNC_WAITING: &str = "waiting";
/// Accepted by the server, or received from it.
pub const ROW_SYNC_SYNCED: &str = "synced";
/// The server refused it, or it could not be staged: see the Sync Center.
pub const ROW_SYNC_ERROR: &str = "error";
/// The server holds a different version: a decision is waiting in the Sync Center.
pub const ROW_SYNC_CONFLICT: &str = "conflict";

/// Keeps each statement small (well under SQLite's bound-parameter limit).
const CHUNK: usize = 200;

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("row sync state decode '{field}': {err}"))
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// State of each given row of `entity_type` (keyed by `entity_sync_id`). Rows with no capture and
/// no outbox entry were received from the server or already sent: `synced`.
pub async fn row_sync_states(
    db: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_ids: &[String],
) -> AppResult<HashMap<String, &'static str>> {
    let mut out: HashMap<String, &'static str> =
        entity_sync_ids.iter().map(|id| (id.clone(), ROW_SYNC_SYNCED)).collect();
    for chunk in entity_sync_ids.chunks(CHUNK) {
        let mut values: Vec<sea_orm::Value> = vec![entity_type.into()];
        values.extend(chunk.iter().map(|id| sea_orm::Value::from(id.clone())));

        // The latest outbox entry of a row decides, unless a newer change is still in the journal.
        let outbox = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT o.entity_sync_id, o.status FROM sync_outbox o \
                     WHERE o.entity_type = ? AND o.status <> 'superseded' AND o.entity_sync_id IN ({}) \
                     AND o.id = (SELECT MAX(o2.id) FROM sync_outbox o2 WHERE o2.entity_type = o.entity_type \
                                 AND o2.entity_sync_id = o.entity_sync_id AND o2.status <> 'superseded')",
                    placeholders(chunk.len())
                ),
                values.clone(),
            ))
            .await?;
        for r in &outbox {
            let id: String = r
                .try_get("", "entity_sync_id")
                .map_err(|e| decode_err("entity_sync_id", e))?;
            let status: String = r.try_get("", "status").map_err(|e| decode_err("status", e))?;
            let state = match status.as_str() {
                "pending" | "in_flight" => ROW_SYNC_WAITING,
                "conflict" => ROW_SYNC_CONFLICT,
                "rejected" | "dead_letter" => ROW_SYNC_ERROR,
                _ => ROW_SYNC_SYNCED,
            };
            out.insert(id, state);
        }

        let journal = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT entity_sync_id, MAX(CASE WHEN last_error IS NOT NULL THEN 1 ELSE 0 END) AS failed \
                     FROM sync_capture_journal WHERE entity_type = ? AND entity_sync_id IN ({}) \
                     GROUP BY entity_sync_id",
                    placeholders(chunk.len())
                ),
                values,
            ))
            .await?;
        for r in &journal {
            let id: String = r
                .try_get("", "entity_sync_id")
                .map_err(|e| decode_err("entity_sync_id", e))?;
            let failed: i64 = r.try_get("", "failed").map_err(|e| decode_err("failed", e))?;
            out.insert(id, if failed != 0 { ROW_SYNC_ERROR } else { ROW_SYNC_WAITING });
        }
    }
    Ok(out)
}
