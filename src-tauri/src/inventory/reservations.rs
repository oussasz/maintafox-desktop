//! Stock reservations at the end of a work order (decision D13).
//!
//! When a work order is closed or cancelled, every reservation still holding stock for it is
//! released in the same transaction as the status change, and each release is audited. A
//! reservation belongs to a work order in two ways: it was made for the work order itself
//! (`source_type` one of the work-order sources, `source_id` = the work order), or a part line of
//! the work order points at it (`work_order_parts.reservation_id`).
//!
//! The release itself is `inventory::queries::release_reservation_in` (balance, status, RELEASE
//! movement with its deterministic sync id), the same as a storekeeper's release.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::AppResult;
use crate::inventory::queries::{release_reservation_in, ReservationRelease};

/// How the work order ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WoEnd {
    Closed,
    Cancelled,
}

impl WoEnd {
    /// Audit action of each release.
    pub(crate) fn audit_action(self) -> &'static str {
        match self {
            Self::Closed => "inventory.reservation.released_at_wo_close",
            Self::Cancelled => "inventory.reservation.released_at_wo_cancel",
        }
    }

    /// Note written on the RELEASE movement.
    fn movement_note(self) -> &'static str {
        match self {
            Self::Closed => "Reservation released at work order close",
            Self::Cancelled => "Reservation released at work order cancellation",
        }
    }
}

/// Reservations of the work order that still hold stock (`active` or `partial`), both link kinds.
async fn open_reservations_of_work_order<C: ConnectionTrait>(txn: &C, wo_id: i64) -> AppResult<Vec<i64>> {
    let wo_sources = crate::finance::wo_costing::WO_SOURCE_TYPES_SQL;
    let rows = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT r.id
                 FROM stock_reservations r
                 WHERE LOWER(r.status) IN ('active', 'partial')
                   AND ((r.source_type IN ({wo_sources}) AND r.source_id = ?1)
                        OR r.id IN (SELECT wop.reservation_id FROM work_order_parts wop
                                    WHERE wop.work_order_id = ?1 AND wop.reservation_id IS NOT NULL))
                 ORDER BY r.id"
            ),
            [wo_id.into()],
        ))
        .await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        ids.push(row.try_get::<i64>("", "id")?);
    }
    Ok(ids)
}

/// Releases every reservation still holding stock for the work order, inside the caller's
/// transaction (the close or cancel of the work order), and writes one audit event per release.
///
/// A release only lowers the reserved quantity (no stock moves), so it is not refused for an
/// article deactivated since the reservation was made; the location must still exist.
pub(crate) async fn release_for_closed_wo<C: ConnectionTrait>(
    txn: &C,
    wo_id: i64,
    actor_id: i64,
    end: WoEnd,
) -> AppResult<Vec<ReservationRelease>> {
    let mut released = Vec::new();
    for reservation_id in open_reservations_of_work_order(txn, wo_id).await? {
        let release = release_reservation_in(txn, reservation_id, Some(end.movement_note()), false).await?;
        if !release.was_open {
            continue;
        }
        let r = &release.reservation;
        crate::audit::writer::write_audit_event_in(
            txn,
            crate::audit::writer::AuditEventInput {
                action_code: end.audit_action().to_string(),
                target_type: Some("stock_reservation".to_string()),
                target_id: Some(r.id.to_string()),
                actor_id: Some(actor_id),
                auth_context: "session".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                details_json: Some(serde_json::json!({
                    "summary": match end {
                        WoEnd::Closed => "Remaining reservation released when the work order was closed",
                        WoEnd::Cancelled => "Remaining reservation released when the work order was cancelled",
                    },
                    "reservation_id": r.id,
                    "article_id": r.article_id,
                    "article_code": r.article_code,
                    "location_id": r.location_id,
                    "location_code": r.location_code,
                    "quantity_reserved": r.quantity_reserved,
                    "quantity_issued": r.quantity_issued,
                    "quantity_released": release.quantity_released,
                    "release_received_from_sync": release.release_received_from_sync,
                    "transaction_id": release.movement_id,
                    "work_order_id": wo_id,
                })),
            },
        )
        .await?;
        released.push(release);
    }
    Ok(released)
}

/// True when the reservation belongs to a work order that is closed or cancelled (either link
/// kind). Stock returned against such a reservation goes back to free stock: nothing is reserved
/// any more for a work order that has ended.
pub(crate) async fn reservation_work_order_ended<C: ConnectionTrait>(txn: &C, reservation_id: i64) -> AppResult<bool> {
    let wo_sources = crate::finance::wo_costing::WO_SOURCE_TYPES_SQL;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS n
                 FROM work_orders wo
                 JOIN work_order_statuses s ON s.id = wo.status_id
                 WHERE s.code IN ('closed', 'cancelled')
                   AND (wo.id = (SELECT r.source_id FROM stock_reservations r
                                 WHERE r.id = ?1 AND r.source_type IN ({wo_sources}))
                        OR wo.id IN (SELECT wop.work_order_id FROM work_order_parts wop
                                     WHERE wop.reservation_id = ?1))"
            ),
            [reservation_id.into()],
        ))
        .await?;
    let n: i64 = match row {
        Some(row) => row.try_get("", "n")?,
        None => 0,
    };
    Ok(n > 0)
}
