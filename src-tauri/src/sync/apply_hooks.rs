//! Local consequences of applying an inbound row (Phase 2.4).
//!
//! The generic materializer writes the replicated columns. Some tables also feed data that each
//! device derives for itself; those derivations run here, in the same transaction as the apply:
//! - `inventory_transactions`: a received movement changes `stock_balances` of its location
//!   (`inventory::ledger`);
//! - `org_nodes`: `ancestor_path` / `depth` (paths of local ids) are recomputed for the node and
//!   its subtree.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::row_codec::{located_row_id, RowLocator};
use crate::sync::row_layout::EntityLayout;

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
        _ => Ok(()),
    }
}

/// After an inbound delete removed a row.
pub(crate) async fn after_delete(
    _conn: &impl ConnectionTrait,
    _layout: &EntityLayout,
    _entity_sync_id: &str,
) -> AppResult<()> {
    Ok(())
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
    let get_i64 = |c: &str| row.try_get::<i64>("", c).map_err(|e| hook_err("inventory_transactions", e));
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
