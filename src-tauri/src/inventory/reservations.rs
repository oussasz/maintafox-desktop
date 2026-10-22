//! Stock reservations at the end of a work order (decision D13).
//!
//! When a work order is closed or cancelled, every reservation still holding stock for it is
//! released in the same transaction as the status change, and each release is audited. A
//! reservation belongs to a work order in two ways: it was made for the work order itself
//! (`source_type` one of the work-order sources, `source_id` = the work order), or a part line of
//! the work order points at it (`work_order_parts.reservation_id`).
//!
//! Reservations still open on a work order that has already ended (closed or cancelled before
//! D13, or a reservation made on another PC while this one closed the work order) are released by
//! the startup sweep [`release_reservations_of_ended_work_orders`], or by the first return made
//! against them (`inventory::queries::return_reserved_stock`).
//!
//! The release itself is `inventory::queries::release_reservation_in` (balance, status, RELEASE
//! movement with its deterministic sync id), the same as a storekeeper's release.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::errors::AppResult;
use crate::inventory::queries::{release_reservation_in, ReservationRelease};

/// How the work order ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WoEnd {
    Closed,
    Cancelled,
}

impl WoEnd {
    fn from_status_code(code: &str) -> Option<Self> {
        match code {
            "closed" => Some(Self::Closed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Why a reservation of a work order is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseCause {
    /// In the close / cancel of the work order.
    WorkOrderEnded(WoEnd),
    /// Startup sweep: the work order ended before D13 or before this PC saw the reservation.
    Upgrade(WoEnd),
    /// A return was made against a reservation of an ended work order.
    ReturnAfterEnd(WoEnd),
}

/// Reason recorded on the releases made by the startup sweep.
pub(crate) const D13_UPGRADE_REASON: &str = "D13 upgrade";

impl ReleaseCause {
    fn audit_action(self) -> &'static str {
        match self {
            Self::WorkOrderEnded(WoEnd::Closed) => "inventory.reservation.released_at_wo_close",
            Self::WorkOrderEnded(WoEnd::Cancelled) => "inventory.reservation.released_at_wo_cancel",
            Self::Upgrade(_) => "inventory.reservation.released_d13_upgrade",
            Self::ReturnAfterEnd(_) => "inventory.reservation.released_at_return_after_wo_end",
        }
    }

    /// Note written on the RELEASE movement (the stock history shows it).
    fn movement_note(self) -> &'static str {
        match self {
            Self::WorkOrderEnded(WoEnd::Closed) => "Reservation released at work order close",
            Self::WorkOrderEnded(WoEnd::Cancelled) => "Reservation released at work order cancellation",
            Self::Upgrade(_) => "D13 upgrade: reservation of an ended work order released",
            Self::ReturnAfterEnd(_) => "Reservation of an ended work order released at a return",
        }
    }

    fn wo_end(self) -> WoEnd {
        match self {
            Self::WorkOrderEnded(end) | Self::Upgrade(end) | Self::ReturnAfterEnd(end) => end,
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

/// Releases one reservation of the work order and audits it (when it was still open).
async fn release_and_audit<C: ConnectionTrait>(
    txn: &C,
    reservation_id: i64,
    wo_id: i64,
    cause: ReleaseCause,
    actor_id: Option<i64>,
) -> AppResult<ReservationRelease> {
    let release = release_reservation_in(txn, reservation_id, Some(cause.movement_note()), false).await?;
    if !release.was_open {
        return Ok(release);
    }
    let r = &release.reservation;
    let reason = match cause {
        ReleaseCause::Upgrade(_) => Some(D13_UPGRADE_REASON),
        _ => None,
    };
    crate::audit::writer::write_audit_event_in(
        txn,
        crate::audit::writer::AuditEventInput {
            action_code: cause.audit_action().to_string(),
            target_type: Some("stock_reservation".to_string()),
            target_id: Some(r.id.to_string()),
            actor_id,
            auth_context: (if actor_id.is_some() { "session" } else { "system" }).to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": cause.movement_note(),
                "reason": reason,
                "work_order_end": match cause.wo_end() {
                    WoEnd::Closed => "closed",
                    WoEnd::Cancelled => "cancelled",
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
    Ok(release)
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
    release_all_of_work_order(txn, wo_id, ReleaseCause::WorkOrderEnded(end), Some(actor_id)).await
}

async fn release_all_of_work_order<C: ConnectionTrait>(
    txn: &C,
    wo_id: i64,
    cause: ReleaseCause,
    actor_id: Option<i64>,
) -> AppResult<Vec<ReservationRelease>> {
    let mut released = Vec::new();
    for reservation_id in open_reservations_of_work_order(txn, wo_id).await? {
        let release = release_and_audit(txn, reservation_id, wo_id, cause, actor_id).await?;
        if release.was_open {
            released.push(release);
        }
    }
    Ok(released)
}

/// The closed or cancelled work order the reservation belongs to (either link kind), if any.
/// Nothing is reserved any more for a work order that has ended.
pub(crate) async fn ended_work_order_of_reservation<C: ConnectionTrait>(
    txn: &C,
    reservation_id: i64,
) -> AppResult<Option<(i64, WoEnd)>> {
    let wo_sources = crate::finance::wo_costing::WO_SOURCE_TYPES_SQL;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id AS wo_id, s.code AS status_code
                 FROM work_orders wo
                 JOIN work_order_statuses s ON s.id = wo.status_id
                 WHERE s.code IN ('closed', 'cancelled')
                   AND (wo.id = (SELECT r.source_id FROM stock_reservations r
                                 WHERE r.id = ?1 AND r.source_type IN ({wo_sources}))
                        OR wo.id IN (SELECT wop.work_order_id FROM work_order_parts wop
                                     WHERE wop.reservation_id = ?1))
                 ORDER BY wo.id
                 LIMIT 1"
            ),
            [reservation_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let wo_id: i64 = row.try_get("", "wo_id")?;
    let code: String = row.try_get("", "status_code")?;
    Ok(WoEnd::from_status_code(&code).map(|end| (wo_id, end)))
}

/// Releases (and audits) a still open reservation of an ended work order before a return is made
/// against it, so its unissued remainder is freed with the reservation's one release movement.
pub(crate) async fn release_before_return_after_end<C: ConnectionTrait>(
    txn: &C,
    reservation_id: i64,
    wo_id: i64,
    end: WoEnd,
    actor_id: Option<i64>,
) -> AppResult<ReservationRelease> {
    release_and_audit(txn, reservation_id, wo_id, ReleaseCause::ReturnAfterEnd(end), actor_id).await
}

/// What the startup sweep did.
#[derive(Debug, Clone, Default)]
pub(crate) struct EndedWoSweepReport {
    /// Work orders whose reservations were released.
    pub work_orders: usize,
    /// Reservations released.
    pub reservations: usize,
    /// Work orders left as they were, with the error (retried at the next start).
    pub failures: Vec<(i64, String)>,
}

/// D13 upgrade and catch-up, run at every start with sync capture on: releases the reservations
/// still open on work orders that are already closed or cancelled (ended before D13, or a
/// reservation made on another PC while this one ended the work order). Each release is audited
/// with the system actor and the reason "D13 upgrade". Idempotent: a released reservation is
/// never selected again.
///
/// Every PC runs it on its own data. The RELEASE movement has the reservation's seeded sync id,
/// so a release made here and the same release received from another PC count once: the copy
/// received second is not applied, and a PC that receives the other's release before sweeping
/// only closes the reservation (`release_reservation_in`, "arrived first").
///
/// One transaction per work order: a work order that cannot be released (closed valuation period,
/// balance inconsistent with the reservation) is reported and left for the next start.
pub(crate) async fn release_reservations_of_ended_work_orders(
    db: &DatabaseConnection,
) -> AppResult<EndedWoSweepReport> {
    let wo_sources = crate::finance::wo_costing::WO_SOURCE_TYPES_SQL;
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id AS wo_id, s.code AS status_code
                 FROM work_orders wo
                 JOIN work_order_statuses s ON s.id = wo.status_id
                 WHERE s.code IN ('closed', 'cancelled')
                   AND EXISTS (
                       SELECT 1 FROM stock_reservations r
                       WHERE LOWER(r.status) IN ('active', 'partial')
                         AND ((r.source_type IN ({wo_sources}) AND r.source_id = wo.id)
                              OR r.id IN (SELECT wop.reservation_id FROM work_order_parts wop
                                          WHERE wop.work_order_id = wo.id AND wop.reservation_id IS NOT NULL)))
                 ORDER BY wo.id"
            ),
        ))
        .await?;
    let mut report = EndedWoSweepReport::default();
    for row in rows {
        let wo_id: i64 = row.try_get("", "wo_id")?;
        let code: String = row.try_get("", "status_code")?;
        let Some(end) = WoEnd::from_status_code(&code) else {
            continue;
        };
        let txn = db.begin().await?;
        match release_all_of_work_order(&txn, wo_id, ReleaseCause::Upgrade(end), None).await {
            Ok(released) => {
                txn.commit().await?;
                if !released.is_empty() {
                    report.work_orders += 1;
                    report.reservations += released.len();
                }
            }
            Err(e) => {
                txn.rollback().await?;
                report.failures.push((wo_id, e.to_string()));
            }
        }
    }
    Ok(report)
}
