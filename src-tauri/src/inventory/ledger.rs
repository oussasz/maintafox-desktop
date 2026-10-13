//! Effect of a stock movement on the stock balance of its location.
//!
//! `stock_balances` is a projection of the movement ledger (`inventory_transactions`). Each
//! inventory command updates the balance and appends the movement in one transaction; this table
//! states, per movement type, the balance change those commands make, so a movement received from
//! another device (sync) changes the local balance exactly as it did on the device that made it.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};

/// `(on_hand delta, reserved delta)` of a movement of `quantity` (as stored on the movement).
///
/// Mirrors the inventory commands: adjustments, transfers, goods receipts and repairables move
/// on-hand stock; reservations move reserved stock; an issue consumes both; a return restores both;
/// count postings store a signed variance.
pub fn movement_balance_effect(movement_type: &str, quantity: f64) -> Option<(f64, f64)> {
    Some(match movement_type {
        "ADJUST_IN" | "TRANSFER_IN" | "GR_ACCEPT" | "REPAIRABLE_RETURN" => (quantity, 0.0),
        "ADJUST_OUT" | "TRANSFER_OUT" | "REPAIRABLE_RELEASE" => (-quantity, 0.0),
        "COUNT_ADJUST" | "COUNT_REVERSAL" => (quantity, 0.0),
        "RESERVE" => (0.0, quantity),
        "RELEASE" => (0.0, -quantity),
        "ISSUE" => (-quantity, -quantity),
        "RETURN" => (quantity, quantity),
        _ => return None,
    })
}

/// Apply a movement made on another device to the local balance of its location.
pub async fn apply_received_movement(
    conn: &impl ConnectionTrait,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    movement_type: &str,
    quantity: f64,
) -> AppResult<()> {
    let (on_hand_delta, reserved_delta) = movement_balance_effect(movement_type, quantity).ok_or_else(|| {
        AppError::SyncError(format!(
            "stock movement type '{movement_type}' has no known effect on stock balances"
        ))
    })?;
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO stock_balances (
             article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(article_id, location_id) DO UPDATE SET
             warehouse_id = excluded.warehouse_id,
             on_hand_qty = stock_balances.on_hand_qty + excluded.on_hand_qty,
             reserved_qty = stock_balances.reserved_qty + excluded.reserved_qty,
             available_qty = (stock_balances.on_hand_qty + excluded.on_hand_qty)
                           - (stock_balances.reserved_qty + excluded.reserved_qty),
             updated_at = excluded.updated_at",
        [
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            on_hand_delta.into(),
            reserved_delta.into(),
            (on_hand_delta - reserved_delta).into(),
        ],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::movement_balance_effect;

    #[test]
    fn effects_follow_the_inventory_commands() {
        assert_eq!(movement_balance_effect("ADJUST_IN", 3.0), Some((3.0, 0.0)));
        assert_eq!(movement_balance_effect("ADJUST_OUT", 3.0), Some((-3.0, 0.0)));
        assert_eq!(movement_balance_effect("RESERVE", 2.0), Some((0.0, 2.0)));
        assert_eq!(movement_balance_effect("RELEASE", 2.0), Some((0.0, -2.0)));
        assert_eq!(movement_balance_effect("ISSUE", 1.0), Some((-1.0, -1.0)));
        assert_eq!(movement_balance_effect("RETURN", 1.0), Some((1.0, 1.0)));
        assert_eq!(movement_balance_effect("COUNT_ADJUST", -4.0), Some((-4.0, 0.0)));
        assert_eq!(movement_balance_effect("UNKNOWN", 1.0), None);
    }
}
