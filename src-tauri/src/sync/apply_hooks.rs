//! Local consequences of applying an inbound row (Phase 2.4).
//!
//! The generic materializer writes the replicated columns. Some tables also feed data that each
//! device derives for itself; those derivations run here, in the same transaction as the apply:
//! - `inventory_transactions`: a received movement changes `stock_balances` of its location
//!   (`inventory::ledger`);
//! - `org_nodes`: `ancestor_path` / `depth` (paths of local ids) are recomputed for the node and
//!   its subtree;
//! - `user_dismissals`: `scope_key` (`<issue_code>:<local equipment id>`) is rebuilt;
//! - `report_schedules`: the next run of this device is computed from the cron expression;
//! - `work_order_delay_segments`: before the apply, `hold_kind` must be `pause` or `prerequisite`
//!   (anything else is refused as a conflict) and present (a row from an older app version without
//!   it is deferred, never written with a guessed kind: it decides the PRD lifecycle state);
//! - `media_attachments`: the file is queued for download from object storage
//!   (`sync_media_downloads`, worked by the sync worker); a deleted row drops a pending download.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::row_codec::{located_row_id, RowLocator};
use crate::sync::row_layout::EntityLayout;

/// Checks an inbound row before it is written; `Some` stops the apply with that outcome.
pub(crate) fn before_upsert(
    layout: &EntityLayout,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> Option<crate::sync::materialize::ApplyOutcome> {
    use crate::sync::materialize::{ApplyOutcome, DEFER_MISSING_FIELD};
    match layout.def.entity_type {
        "work_order_delay_segments" => match payload.get("hold_kind") {
            None | Some(serde_json::Value::Null) => Some(ApplyOutcome::Deferred {
                code: DEFER_MISSING_FIELD.into(),
                message: "hold_kind missing: sent by an older app version; waiting for an update of that device".into(),
            }),
            Some(serde_json::Value::String(k)) if crate::wo::lifecycle::HoldKind::try_from_str(k).is_some() => None,
            Some(other) => Some(ApplyOutcome::Conflict {
                code: "INVALID_VALUE".into(),
                message: format!("hold_kind must be 'pause' or 'prerequisite', got {other}"),
            }),
        },
        _ => None,
    }
}

fn hook_err(table: &str, e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("{table} apply hook: {e}"))
}

/// After an inbound row was inserted (`inserted`) or updated.
pub(crate) async fn after_upsert(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    row: &RowLocator,
    inserted: bool,
) -> AppResult<()> {
    match layout.def.entity_type {
        "inventory_transactions" if inserted => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "movement not found after insert"))?;
            apply_movement_to_balance(conn, id).await
        }
        "org_nodes" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "node not found after apply"))?;
            recompute_org_paths(conn, id).await
        }
        "user_dismissals" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "dismissal not found after apply"))?;
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE user_dismissals SET scope_key = issue_code || ':' || CAST(equipment_id AS TEXT) WHERE id = ?",
                [id.into()],
            ))
            .await?;
            Ok(())
        }
        "report_schedules" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "schedule not found after apply"))?;
            schedule_next_run(conn, id).await
        }
        "media_attachments" if inserted => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "media row not found after insert"))?;
            queue_received_media(conn, id).await
        }
        _ => Ok(()),
    }
}

async fn queue_received_media(conn: &impl ConnectionTrait, media_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, object_key, content_sha256, byte_size, relative_path
             FROM media_attachments WHERE id = ?",
            [media_id.into()],
        ))
        .await?
        .ok_or_else(|| hook_err("media_attachments", "media row vanished"))?;
    let text = |c: &str| {
        row.try_get::<String>("", c)
            .map_err(|e| hook_err("media_attachments", e))
    };
    let relative_path: Option<String> = row
        .try_get("", "relative_path")
        .map_err(|e| hook_err("media_attachments", e))?;
    let byte_size: i64 = row
        .try_get("", "byte_size")
        .map_err(|e| hook_err("media_attachments", e))?;
    crate::sync::media::queue_media_download(
        conn,
        &text("entity_sync_id")?,
        &text("object_key")?,
        &text("content_sha256")?,
        byte_size,
        relative_path.as_deref(),
    )
    .await
}

async fn schedule_next_run(conn: &impl ConnectionTrait, schedule_id: i64) -> AppResult<()> {
    use std::str::FromStr;
    let cron_expr: String = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT cron_expr FROM report_schedules WHERE id = ?",
            [schedule_id.into()],
        ))
        .await?
        .ok_or_else(|| hook_err("report_schedules", "schedule vanished"))?
        .try_get("", "cron_expr")
        .map_err(|e| hook_err("report_schedules", e))?;
    let schedule = cron::Schedule::from_str(cron_expr.trim())
        .map_err(|e| hook_err("report_schedules", format!("cron expression '{cron_expr}': {e}")))?;
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE report_schedules SET next_run_at = ? WHERE id = ?",
        [
            crate::reports::scheduler::next_run_rfc3339(&schedule).into(),
            schedule_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// After an inbound delete removed a row.
pub(crate) async fn after_delete(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
) -> AppResult<()> {
    match layout.def.entity_type {
        "media_attachments" => crate::sync::media::forget_media_download(conn, entity_sync_id).await,
        _ => Ok(()),
    }
}

async fn apply_movement_to_balance(conn: &impl ConnectionTrait, movement_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT article_id, warehouse_id, location_id, movement_type, quantity
             FROM inventory_transactions WHERE id = ?",
            [movement_id.into()],
        ))
        .await?
        .ok_or_else(|| hook_err("inventory_transactions", "movement vanished"))?;
    let get_i64 = |c: &str| {
        row.try_get::<i64>("", c)
            .map_err(|e| hook_err("inventory_transactions", e))
    };
    let movement_type: String = row
        .try_get("", "movement_type")
        .map_err(|e| hook_err("inventory_transactions", e))?;
    let quantity: f64 = row
        .try_get("", "quantity")
        .map_err(|e| hook_err("inventory_transactions", e))?;
    crate::inventory::ledger::apply_received_movement(
        conn,
        get_i64("article_id")?,
        get_i64("warehouse_id")?,
        get_i64("location_id")?,
        &movement_type,
        quantity,
    )
    .await
}

/// `ancestor_path` (root `/{id}/`, child `{parent path}{id}/`) and `depth` for a node and every
/// node below it, as the org commands write them.
async fn recompute_org_paths(conn: &impl ConnectionTrait, node_id: i64) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "WITH RECURSIVE tree(id, path, depth) AS (
             SELECT n.id,
                    COALESCE(p.ancestor_path, '/') || n.id || '/',
                    COALESCE(p.depth + 1, 0)
             FROM org_nodes n LEFT JOIN org_nodes p ON p.id = n.parent_id
             WHERE n.id = ?
             UNION ALL
             SELECT c.id, tree.path || c.id || '/', tree.depth + 1
             FROM org_nodes c JOIN tree ON c.parent_id = tree.id
             WHERE tree.depth < 64
         )
         UPDATE org_nodes
         SET ancestor_path = (SELECT path FROM tree WHERE tree.id = org_nodes.id),
             depth = (SELECT depth FROM tree WHERE tree.id = org_nodes.id)
         WHERE id IN (SELECT id FROM tree)",
        [node_id.into()],
    ))
    .await?;
    Ok(())
}
