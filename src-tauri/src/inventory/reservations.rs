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
                 WHERE r.status IN ('active', 'partial')
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
                // The reservation row counted more than its movements held (drift between PCs):
                // released all the same, the difference shown here.
                "quantity_short": release.quantity_short,
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
                // Driven by the few open reservations (status index), not by every ended WO.
                "SELECT DISTINCT wo.id AS wo_id, s.code AS status_code
                 FROM stock_reservations r
                 JOIN work_orders wo
                   ON (r.source_type IN ({wo_sources}) AND wo.id = r.source_id)
                   OR wo.id IN (SELECT wop.work_order_id FROM work_order_parts wop WHERE wop.reservation_id = r.id)
                 JOIN work_order_statuses s ON s.id = wo.status_id
                 WHERE r.status IN ('active', 'partial')
                   AND s.code IN ('closed', 'cancelled')
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

// ── Reserved stock: a projection of the synchronised ledger ─────────────────
//
// `stock_balances.reserved_qty` of an article at a location is computed from the movements of its
// reservations, never kept by increments alone, so every PC with the same movements shows the
// same reserved stock whatever the order they arrived in:
//
//   reserved = Σ over the location's reservations of max(0, RESERVE + RETURN − ISSUE − RELEASE)
//
// - a reservation whose one release movement (seeded id, [`RELEASE_SEED_ENTITY`]) is in the
//   ledger counts 0: it is released, whatever else arrived;
// - each reservation is held at 0 at least (CMMS Advisor decision D12: an over-issue arriving by
//   sync is applied as a physical fact, reserved stock never goes below zero);
// - the baseline RESERVE of an older reservation ([`BASELINE_SEED_ENTITY`]) counts only while the
//   reservation has no RESERVE movement of its own (see [`baseline_reservations_without_ledger`]).
//
// Movements are summed in sync-id order so the floating-point result is the same on every PC.

/// Seed entity of the one RELEASE movement of a reservation (`seeded_entity_sync_id`).
pub(crate) const RELEASE_SEED_ENTITY: &str = "reservation_release";
/// Seed entity of the baseline RESERVE movement of a reservation created without one.
pub(crate) const BASELINE_SEED_ENTITY: &str = "reservation_reserve_baseline";

/// Movement types with a reserved-stock effect.
pub(crate) const RESERVATION_MOVEMENTS: [&str; 4] = ["RESERVE", "ISSUE", "RETURN", "RELEASE"];

/// One movement of a reservation, as the projection reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReservationMovement {
    pub entity_sync_id: Option<String>,
    pub movement_type: String,
    pub quantity: f64,
}

/// Signed reserved effect of the reservation's movements (no floor, no release rule), with the
/// baseline RESERVE counted only when the reservation has no RESERVE of its own.
fn ledger_net(reservation_sync_id: Option<&str>, movements: &[ReservationMovement]) -> f64 {
    let baseline_id =
        reservation_sync_id.map(|s| crate::sync::identity::seeded_entity_sync_id(BASELINE_SEED_ENTITY, s));
    let is_baseline = |m: &ReservationMovement| {
        m.movement_type == "RESERVE" && baseline_id.is_some() && m.entity_sync_id.as_deref() == baseline_id.as_deref()
    };
    let has_own_reserve = movements
        .iter()
        .any(|m| m.movement_type == "RESERVE" && !is_baseline(m));
    let mut ordered: Vec<&ReservationMovement> = movements
        .iter()
        .filter(|m| !(has_own_reserve && is_baseline(m)))
        .collect();
    ordered.sort_by(|a, b| {
        a.entity_sync_id
            .cmp(&b.entity_sync_id)
            .then(a.movement_type.cmp(&b.movement_type))
            .then(a.quantity.total_cmp(&b.quantity))
    });
    ordered.iter().fold(0.0, |acc, m| {
        acc + match m.movement_type.as_str() {
            "RESERVE" | "RETURN" => m.quantity,
            "ISSUE" | "RELEASE" => -m.quantity,
            _ => 0.0,
        }
    })
}

/// Reserved stock one reservation holds, from its movements (see the rules above).
pub(crate) fn reservation_reserved_from_ledger(
    reservation_sync_id: Option<&str>,
    movements: &[ReservationMovement],
) -> f64 {
    if let Some(sync_id) = reservation_sync_id {
        let release_id = crate::sync::identity::seeded_entity_sync_id(RELEASE_SEED_ENTITY, sync_id);
        if movements
            .iter()
            .any(|m| m.movement_type == "RELEASE" && m.entity_sync_id.as_deref() == Some(release_id.as_str()))
        {
            return 0.0;
        }
    }
    ledger_net(reservation_sync_id, movements).max(0.0)
}

/// Movements of the reservations of an article at a location, grouped by reservation (ordered by
/// the reservation's sync id).
async fn reservation_movements_at<C: ConnectionTrait>(
    conn: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<Vec<(Option<String>, Vec<ReservationMovement>)>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT r.id AS reservation_id, r.entity_sync_id AS reservation_sync_id,
                    t.entity_sync_id AS movement_sync_id, t.movement_type, t.quantity
             FROM stock_reservations r
             JOIN inventory_transactions t ON t.reservation_id = r.id
             WHERE r.article_id = ? AND r.location_id = ?
               AND t.movement_type IN ('RESERVE', 'ISSUE', 'RETURN', 'RELEASE')
             ORDER BY r.entity_sync_id, r.id",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    let mut grouped: Vec<(i64, Option<String>, Vec<ReservationMovement>)> = Vec::new();
    for row in rows {
        let reservation_id: i64 = row.try_get("", "reservation_id")?;
        let movement = ReservationMovement {
            entity_sync_id: row.try_get("", "movement_sync_id")?,
            movement_type: row.try_get("", "movement_type")?,
            quantity: row.try_get("", "quantity")?,
        };
        match grouped.last_mut() {
            Some((id, _, list)) if *id == reservation_id => list.push(movement),
            _ => grouped.push((reservation_id, row.try_get("", "reservation_sync_id")?, vec![movement])),
        }
    }
    Ok(grouped.into_iter().map(|(_, sync_id, list)| (sync_id, list)).collect())
}

/// Reserved stock one reservation holds in the projection (its own movements).
pub(crate) async fn reservation_held_from_ledger<C: ConnectionTrait>(conn: &C, reservation_id: i64) -> AppResult<f64> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT r.entity_sync_id AS reservation_sync_id, t.entity_sync_id AS movement_sync_id,
                    t.movement_type, t.quantity
             FROM stock_reservations r
             JOIN inventory_transactions t ON t.reservation_id = r.id
             WHERE r.id = ? AND t.movement_type IN ('RESERVE', 'ISSUE', 'RETURN', 'RELEASE')",
            [reservation_id.into()],
        ))
        .await?;
    let mut reservation_sync_id: Option<String> = None;
    let mut movements = Vec::with_capacity(rows.len());
    for row in rows {
        reservation_sync_id = row.try_get("", "reservation_sync_id")?;
        movements.push(ReservationMovement {
            entity_sync_id: row.try_get("", "movement_sync_id")?,
            movement_type: row.try_get("", "movement_type")?,
            quantity: row.try_get("", "quantity")?,
        });
    }
    Ok(reservation_reserved_from_ledger(
        reservation_sync_id.as_deref(),
        &movements,
    ))
}

/// Reserved stock of an article at a location from the ledger (the projection, not stored).
pub(crate) async fn reserved_from_ledger<C: ConnectionTrait>(
    conn: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<f64> {
    Ok(reservation_movements_at(conn, article_id, location_id)
        .await?
        .iter()
        .fold(0.0, |acc, (sync_id, movements)| {
            acc + reservation_reserved_from_ledger(sync_id.as_deref(), movements)
        }))
}

/// Writes the projection into `stock_balances` (reserved and available; on hand is not touched).
/// Called after every local command that moves reserved stock and after every movement received
/// by sync. Returns the reserved quantity.
pub(crate) async fn recompute_reserved_in<C: ConnectionTrait>(
    conn: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<f64> {
    let reserved = reserved_from_ledger(conn, article_id, location_id).await?;
    let existing = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS present FROM stock_balances WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?
        .is_some();
    if existing {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE stock_balances
             SET reserved_qty = ?, available_qty = on_hand_qty - ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE article_id = ? AND location_id = ?",
            [reserved.into(), reserved.into(), article_id.into(), location_id.into()],
        ))
        .await?;
    } else if reserved > 0.0 {
        // No balance row yet (only reservation movements here): created with nothing on hand.
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO stock_balances
                 (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty, updated_at)
             SELECT ?, sl.warehouse_id, sl.id, 0, ?, -?, strftime('%Y-%m-%dT%H:%M:%SZ','now')
             FROM stock_locations sl WHERE sl.id = ?",
            [article_id.into(), reserved.into(), reserved.into(), location_id.into()],
        ))
        .await?;
    }
    Ok(reserved)
}

/// Startup: recompute reserved stock of every article and location that has reservations or a
/// reserved balance (repairs balances kept by increments before the projection).
pub(crate) async fn recompute_all_reserved(db: &DatabaseConnection) -> AppResult<usize> {
    let pairs = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT article_id, location_id FROM stock_reservations
             UNION
             SELECT article_id, location_id FROM stock_balances WHERE reserved_qty <> 0"
                .to_string(),
        ))
        .await?;
    let txn = db.begin().await?;
    for row in &pairs {
        let article_id: i64 = row.try_get("", "article_id")?;
        let location_id: i64 = row.try_get("", "location_id")?;
        recompute_reserved_in(&txn, article_id, location_id).await?;
    }
    txn.commit().await?;
    Ok(pairs.len())
}

/// Baseline for reservations that are open but have no RESERVE movement of their own (a sync
/// engine before Phase 2.4 applied received movements without their reservation link, so such a
/// reservation would count nothing in the projection). One RESERVE is written per reservation,
/// with a sync id seeded from the reservation (every PC writes the same one; the copy received
/// second is not applied), for its open quantity (`quantity_reserved − quantity_issued`) less what
/// its linked movements already count.
///
/// If the reservation's own RESERVE only arrives later (pulled after this ran), the projection
/// ignores the baseline from then on, so it is never counted twice. Runs at startup with sync
/// capture on. Returns the number of baseline movements written.
pub(crate) async fn baseline_reservations_without_ledger(db: &DatabaseConnection) -> AppResult<usize> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT r.id, r.entity_sync_id, r.article_id, r.warehouse_id, r.location_id, r.source_type,
                    r.source_id, r.source_ref, r.quantity_reserved, r.quantity_issued
             FROM stock_reservations r
             WHERE r.status IN ('active', 'partial')
               AND NOT EXISTS (SELECT 1 FROM inventory_transactions t
                               WHERE t.reservation_id = r.id AND t.movement_type = 'RESERVE')
             ORDER BY r.id"
                .to_string(),
        ))
        .await?;
    let mut written = 0;
    for row in rows {
        let reservation_id: i64 = row.try_get("", "id")?;
        let sync_id = crate::sync::identity::require_entity_sync_id(
            "stock_reservations",
            reservation_id,
            row.try_get("", "entity_sync_id")?,
        )?;
        let baseline_id = crate::sync::identity::seeded_entity_sync_id(BASELINE_SEED_ENTITY, &sync_id);
        let article_id: i64 = row.try_get("", "article_id")?;
        let location_id: i64 = row.try_get("", "location_id")?;
        let txn = db.begin().await?;
        let present = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS present FROM inventory_transactions WHERE entity_sync_id = ?",
                [baseline_id.clone().into()],
            ))
            .await?
            .is_some();
        if !present {
            let mut movements = Vec::new();
            for m in txn
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT entity_sync_id, movement_type, quantity FROM inventory_transactions
                     WHERE reservation_id = ? AND movement_type IN ('ISSUE', 'RETURN', 'RELEASE')",
                    [reservation_id.into()],
                ))
                .await?
            {
                movements.push(ReservationMovement {
                    entity_sync_id: m.try_get("", "entity_sync_id")?,
                    movement_type: m.try_get("", "movement_type")?,
                    quantity: m.try_get("", "quantity")?,
                });
            }
            let reserved: f64 = row.try_get("", "quantity_reserved")?;
            let issued: f64 = row.try_get("", "quantity_issued")?;
            let quantity = (reserved - issued).max(0.0) - ledger_net(Some(&sync_id), &movements);
            if quantity > 1e-9 {
                let source_type: String = row.try_get("", "source_type")?;
                let source_ref: Option<String> = row.try_get("", "source_ref")?;
                crate::inventory::queries::insert_reservation_baseline_movement(
                    &txn,
                    crate::inventory::queries::BaselineMovement {
                        article_id,
                        warehouse_id: row.try_get("", "warehouse_id")?,
                        location_id,
                        reservation_id,
                        quantity,
                        source_type: &source_type,
                        source_id: row.try_get("", "source_id")?,
                        source_ref: source_ref.as_deref(),
                        entity_sync_id: &baseline_id,
                    },
                )
                .await?;
                recompute_reserved_in(&txn, article_id, location_id).await?;
                written += 1;
            }
        }
        txn.commit().await?;
    }
    Ok(written)
}

/// What the startup upkeep of reservations did.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReservationUpkeepReport {
    pub baselines: usize,
    pub pairs_recomputed: usize,
    pub sweep: EndedWoSweepReport,
}

/// Startup, with sync capture on: baseline of older reservations, reserved stock recomputed from
/// the ledger everywhere, then the D13 sweep of ended work orders. Each step runs even when an
/// earlier one failed; the errors are returned for the log.
pub(crate) async fn startup_reservation_upkeep(db: &DatabaseConnection) -> (ReservationUpkeepReport, Vec<String>) {
    let mut report = ReservationUpkeepReport::default();
    let mut errors = Vec::new();
    match baseline_reservations_without_ledger(db).await {
        Ok(n) => report.baselines = n,
        Err(e) => errors.push(format!("reservation baseline: {e}")),
    }
    match recompute_all_reserved(db).await {
        Ok(n) => report.pairs_recomputed = n,
        Err(e) => errors.push(format!("reserved stock recompute: {e}")),
    }
    match release_reservations_of_ended_work_orders(db).await {
        Ok(sweep) => report.sweep = sweep,
        Err(e) => errors.push(format!("D13 sweep: {e}")),
    }
    (report, errors)
}

// ── A reservation released on two PCs ──────────────────────────────────────
//
// Two PCs that both release the same reservation each change its row and each write its one
// RELEASE movement (same seeded sync id). The stock is right on both (reserved stock is the
// ledger projection: the release counts once), but the second PC to synchronise meets a conflict
// on the row, and on the movement (two creates of the same sync id). When the two versions are
// both "released" and differ only in who/when, the server's version is adopted (first push wins)
// and this PC's release is audited (`inventory.reservation.released_twice`); no one has to
// decide. Any other difference stays a conflict for a person.
//
// May differ on the reservation row: `released_at`, `updated_at`, `row_version` and
// `quantity_issued`. The issued quantity may legitimately differ (one PC issued more before
// releasing); the converged value is the server's. Reserved stock does not depend on it (ledger
// projection), and a return checks the issued quantity of the ledger as well
// (`queries::return_reserved_stock_as`), so nothing issued here is lost.
// May differ on the release movement: everything but its type, reservation, article, warehouse
// and location (quantity, time, actor, note): its presence alone counts.

/// Audit action of a release made here that another PC had already sent.
pub(crate) const RELEASED_TWICE_ACTION: &str = "inventory.reservation.released_twice";

const ROW_MAY_DIFFER: [&str; 4] = ["released_at", "updated_at", "row_version", "quantity_issued"];
const RELEASE_MUST_MATCH: [&str; 5] = [
    "movement_type",
    "reservation_sync_id",
    "article_sync_id",
    "warehouse_sync_id",
    "location_sync_id",
];

type Payload = serde_json::Map<String, serde_json::Value>;

fn both_released_rows(local: &Payload, server: &Payload) -> bool {
    let released = |p: &Payload| p.get("status").and_then(serde_json::Value::as_str) == Some("released");
    if !released(local) || !released(server) {
        return false;
    }
    let keys: std::collections::BTreeSet<&String> = local.keys().chain(server.keys()).collect();
    keys.into_iter()
        .filter(|k| !ROW_MAY_DIFFER.contains(&k.as_str()))
        .all(|k| local.get(k) == server.get(k))
}

fn same_seeded_release(entity_sync_id: &str, local: &Payload, server: &Payload) -> bool {
    let is_release = |p: &Payload| p.get("movement_type").and_then(serde_json::Value::as_str) == Some("RELEASE");
    if !is_release(local) || !is_release(server) {
        return false;
    }
    if !RELEASE_MUST_MATCH.iter().all(|k| local.get(*k) == server.get(*k)) {
        return false;
    }
    // Only the reservation's one release (seeded id): another movement keeps its conflict.
    local
        .get("reservation_sync_id")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|r| crate::sync::identity::seeded_entity_sync_id(RELEASE_SEED_ENTITY, r) == entity_sync_id)
}

/// Settles the open conflict of `entity_type`/`entity_sync_id` when it is a reservation released
/// on two PCs (see above): the server's version is taken (`sync::conflicts::take_remote_version`),
/// the entity's open conflicts are closed and, for the reservation row, this PC's release is
/// audited. Returns true when settled; false leaves the conflict for a person. A failure while
/// taking the server's version is rolled back to before the attempt and leaves the conflict.
/// Called where conflicts are recorded (server answer, inbound apply), in their transaction.
pub(crate) async fn settle_double_release<C: ConnectionTrait>(
    conn: &C,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<bool> {
    if entity_type != "stock_reservations" && entity_type != "inventory_transactions" {
        return Ok(false);
    }
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {} FROM sync_conflicts
                 WHERE entity_type = ? AND entity_sync_id = ? AND status IN ('new', 'triaged', 'escalated')
                 ORDER BY id DESC LIMIT 1",
                crate::sync::queries::CONFLICT_COLUMNS
            ),
            [entity_type.into(), entity_sync_id.into()],
        ))
        .await?
    else {
        return Ok(false);
    };
    let conflict = crate::sync::queries::to_sync_conflict(&row)?;
    let parse = |json: &Option<String>| json.as_deref().and_then(|j| serde_json::from_str::<Payload>(j).ok());
    let (Some(local), Some(server)) = (
        parse(&conflict.local_payload_json),
        parse(&conflict.inbound_payload_json),
    ) else {
        return Ok(false);
    };
    let settles = if entity_type == "stock_reservations" {
        both_released_rows(&local, &server)
    } else {
        same_seeded_release(entity_sync_id, &local, &server)
    };
    if !settles {
        return Ok(false);
    }

    conn.execute_unprepared("SAVEPOINT reservation_double_release").await?;
    if crate::sync::conflicts::take_remote_version(conn, &conflict)
        .await
        .is_err()
    {
        conn.execute_unprepared("ROLLBACK TO reservation_double_release")
            .await?;
        conn.execute_unprepared("RELEASE reservation_double_release").await?;
        return Ok(false);
    }
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE sync_conflicts
         SET status = 'resolved_remote', resolution_action = 'auto_double_release',
             resolution_note = 'Released on two PCs: the version sent first is kept.',
             resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE entity_type = ? AND entity_sync_id = ? AND status IN ('new', 'triaged', 'escalated')",
        [entity_type.into(), entity_sync_id.into()],
    ))
    .await?;
    if entity_type == "stock_reservations" {
        let reservation_id: Option<i64> = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM stock_reservations WHERE entity_sync_id = ?",
                [entity_sync_id.into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "id"))
            .transpose()?;
        // Who released it here, when known (the D13 releases are audited with their actor).
        let released_here_by: Option<i64> = match reservation_id {
            Some(id) => conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT actor_id FROM audit_events
                     WHERE target_type = 'stock_reservation' AND target_id = ?
                       AND action_code LIKE 'inventory.reservation.released%' AND action_code <> ?
                     ORDER BY id DESC LIMIT 1",
                    [id.to_string().into(), RELEASED_TWICE_ACTION.into()],
                ))
                .await?
                .and_then(|r| r.try_get::<Option<i64>>("", "actor_id").ok().flatten()),
            None => None,
        };
        crate::audit::writer::write_audit_event_in(
            conn,
            crate::audit::writer::AuditEventInput {
                action_code: RELEASED_TWICE_ACTION.to_string(),
                target_type: Some("stock_reservation".to_string()),
                target_id: reservation_id.map(|id| id.to_string()),
                actor_id: None,
                auth_context: "system".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                details_json: Some(serde_json::json!({
                    "summary": "Reservation released on this PC after another PC had released it: the version sent first is kept",
                    "reservation_sync_id": entity_sync_id,
                    "released_here_at": local.get("released_at"),
                    "released_here_by": released_here_by,
                    "quantity_issued_here": local.get("quantity_issued"),
                    "kept_released_at": server.get("released_at"),
                    "kept_quantity_issued": server.get("quantity_issued"),
                    "conflict_type": conflict.conflict_type,
                })),
            },
        )
        .await?;
    }
    conn.execute_unprepared("RELEASE reservation_double_release").await?;
    Ok(true)
}
