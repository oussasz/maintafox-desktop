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
        "ADJUST_IN" | "TRANSFER_IN" | "GR_ACCEPT" | "REPAIRABLE_RETURN" | "REPAIRABLE_CANCEL_RESTORE" => {
            (quantity, 0.0)
        }
        "ADJUST_OUT" | "TRANSFER_OUT" | "REPAIRABLE_RELEASE" | "REPAIRABLE_SCRAP_WRITEOFF" => (-quantity, 0.0),
        "COUNT_ADJUST" | "COUNT_REVERSAL" => (quantity, 0.0),
        "RESERVE" => (0.0, quantity),
        "RELEASE" => (0.0, -quantity),
        "ISSUE" => (-quantity, -quantity),
        "RETURN" => (quantity, quantity),
        _ => return None,
    })
}

/// Movement types that change on-hand stock, with the sign of that change. The SQL below and
/// `movement_balance_effect` must agree (test `on_hand_sql_matches_balance_effect`).
pub const ON_HAND_MOVEMENTS: &[(&str, f64)] = &[
    ("ADJUST_IN", 1.0),
    ("TRANSFER_IN", 1.0),
    ("GR_ACCEPT", 1.0),
    ("REPAIRABLE_RETURN", 1.0),
    ("REPAIRABLE_CANCEL_RESTORE", 1.0),
    ("ADJUST_OUT", -1.0),
    ("TRANSFER_OUT", -1.0),
    ("REPAIRABLE_RELEASE", -1.0),
    ("REPAIRABLE_SCRAP_WRITEOFF", -1.0),
    ("COUNT_ADJUST", 1.0),
    ("COUNT_REVERSAL", 1.0),
    ("ISSUE", -1.0),
    ("RETURN", 1.0),
];

/// SQL expression: the on-hand change of an `inventory_transactions` row (alias `t`). Used to
/// derive lot stock and as-at valuations from the ledger.
pub fn on_hand_delta_sql(alias: &str) -> String {
    let mut sql = format!("CASE {alias}.movement_type");
    for (movement, sign) in ON_HAND_MOVEMENTS {
        let value = if *sign > 0.0 {
            format!("{alias}.quantity")
        } else {
            format!("-{alias}.quantity")
        };
        sql.push_str(&format!(" WHEN '{movement}' THEN {value}"));
    }
    sql.push_str(" ELSE 0 END");
    sql
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
    // Reserved stock is a projection of the reservations' movements, not a running total: it is
    // recomputed from the ledger, so the same movements give the same reserved stock on every PC
    // whatever order they arrive in (`inventory::reservations`).
    if crate::inventory::reservations::RESERVATION_MOVEMENTS.contains(&movement_type) {
        crate::inventory::reservations::recompute_reserved_in(conn, article_id, location_id).await?;
    }
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
        assert_eq!(
            movement_balance_effect("REPAIRABLE_CANCEL_RESTORE", 2.0),
            Some((2.0, 0.0))
        );
        assert_eq!(
            movement_balance_effect("REPAIRABLE_SCRAP_WRITEOFF", 2.0),
            Some((-2.0, 0.0))
        );
        assert_eq!(movement_balance_effect("UNKNOWN", 1.0), None);
    }

    #[test]
    fn on_hand_sql_matches_balance_effect() {
        for movement in ["REPAIRABLE_CANCEL_RESTORE", "REPAIRABLE_SCRAP_WRITEOFF"] {
            assert!(
                super::ON_HAND_MOVEMENTS.iter().any(|(m, _)| m == &movement),
                "{movement} changes on-hand stock"
            );
        }
        for (movement, sign) in super::ON_HAND_MOVEMENTS {
            let (on_hand, _) = movement_balance_effect(movement, 2.0).expect("known movement");
            assert_eq!(on_hand, 2.0 * sign, "{movement}");
        }
        for movement in ["RESERVE", "RELEASE"] {
            let (on_hand, _) = movement_balance_effect(movement, 2.0).expect("known movement");
            assert_eq!(on_hand, 0.0, "{movement} must not be in ON_HAND_MOVEMENTS");
            assert!(!super::ON_HAND_MOVEMENTS.iter().any(|(m, _)| m == &movement));
        }
        let sql = super::on_hand_delta_sql("t");
        assert!(sql.contains("WHEN 'ISSUE' THEN -t.quantity"));
        assert!(sql.ends_with("ELSE 0 END"));
    }
}
