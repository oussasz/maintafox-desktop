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
    /// Sweep after a sync pass: the work order was ended on another PC, or a reservation for it
    /// arrived after it ended.
    CatchUpAfterSync(WoEnd),
}

/// Reason recorded on the releases made by the startup sweep.
pub(crate) const D13_UPGRADE_REASON: &str = "D13 upgrade";
/// Reason recorded on the releases made by the sweep after a sync pass.
pub(crate) const D13_AFTER_SYNC_REASON: &str = "D13 catch-up after sync";

/// Where the sweep of ended work orders runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SweepOrigin {
    Startup,
    AfterSync,
}

impl ReleaseCause {
    fn audit_action(self) -> &'static str {
        match self {
            Self::WorkOrderEnded(WoEnd::Closed) => "inventory.reservation.released_at_wo_close",
            Self::WorkOrderEnded(WoEnd::Cancelled) => "inventory.reservation.released_at_wo_cancel",
            Self::Upgrade(_) => "inventory.reservation.released_d13_upgrade",
            Self::ReturnAfterEnd(_) => "inventory.reservation.released_at_return_after_wo_end",
            Self::CatchUpAfterSync(_) => "inventory.reservation.released_after_sync",
        }
    }

    /// Note written on the RELEASE movement (the stock history shows it).
    fn movement_note(self) -> &'static str {
        match self {
            Self::WorkOrderEnded(WoEnd::Closed) => "Reservation released at work order close",
            Self::WorkOrderEnded(WoEnd::Cancelled) => "Reservation released at work order cancellation",
            Self::Upgrade(_) => "D13 upgrade: reservation of an ended work order released",
            Self::ReturnAfterEnd(_) => "Reservation of an ended work order released at a return",
            Self::CatchUpAfterSync(_) => "Reservation of a work order ended on another PC released after sync",
        }
    }

    fn wo_end(self) -> WoEnd {
        match self {
            Self::WorkOrderEnded(end)
            | Self::Upgrade(end)
            | Self::ReturnAfterEnd(end)
            | Self::CatchUpAfterSync(end) => end,
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
        ReleaseCause::CatchUpAfterSync(_) => Some(D13_AFTER_SYNC_REASON),
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
    origin: SweepOrigin,
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
        let cause = match origin {
            SweepOrigin::Startup => ReleaseCause::Upgrade(end),
            SweepOrigin::AfterSync => ReleaseCause::CatchUpAfterSync(end),
        };
        match release_all_of_work_order(&txn, wo_id, cause, None).await {
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

// ── Reserved stock: a projection of the reservations and their movements ────
//
// `stock_balances.reserved_qty` of an article at a location is computed from synchronised data
// only — the reservation rows and the movement ledger — never kept as a running total, so every
// PC holding the same rows and movements shows the same reserved stock, whatever order they
// arrived in. Per reservation:
//
// - 0 when its row is released, or when its one release movement (seeded id,
//   [`RELEASE_SEED_ENTITY`]) is in the ledger;
// - otherwise `row remaining` = max(0, quantity_reserved − quantity_issued) of the row, and
//   `own` = what its movements hold (RESERVE + RETURN − ISSUE − RELEASE linked to it) when it has
//   a RESERVE movement of its own, else the row remaining (a reservation recorded by the sync
//   engine before Phase 2.4, which dropped the movements' link);
// - contribution = max(0, min(own, row remaining)).
//
// The row bounds the ledger: a legacy issue or release whose link was dropped cannot keep stock
// reserved for ever (the row says consumed or released), an over-issue arriving by sync never takes
// a reservation below 0 (CMMS Advisor decision D12). The ledger bounds the row: an issue arrived
// before the row's own update already counts. While two PCs hold different versions of a row (a
// sync conflict open), each uses its own; once the conflict is resolved the rows, and so the
// figures, are the same.
//
// Only open rows with something left (status active/partial, issued < reserved) can contribute, so
// only those are read (index on article, location, status). Movements are summed in sync-id order
// so the floating-point result is the same on every PC.

/// Seed entity of the one RELEASE movement of a reservation (`seeded_entity_sync_id`).
pub(crate) const RELEASE_SEED_ENTITY: &str = "reservation_release";

/// One movement of a reservation, as the projection reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReservationMovement {
    pub entity_sync_id: Option<String>,
    pub movement_type: String,
    pub quantity: f64,
}

/// The reservation row, as the projection reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReservationRowState {
    pub entity_sync_id: Option<String>,
    pub status: String,
    pub quantity_reserved: f64,
    pub quantity_issued: f64,
}

/// Reserved stock one reservation holds (see the rules above).
pub(crate) fn reservation_reserved(row: &ReservationRowState, movements: &[ReservationMovement]) -> f64 {
    if !matches!(row.status.as_str(), "active" | "partial") {
        return 0.0;
    }
    let row_remaining = (row.quantity_reserved - row.quantity_issued).max(0.0);
    if row_remaining <= 0.0 {
        return 0.0;
    }
    if let Some(sync_id) = row.entity_sync_id.as_deref() {
        let release_id = crate::sync::identity::seeded_entity_sync_id(RELEASE_SEED_ENTITY, sync_id);
        if movements
            .iter()
            .any(|m| m.movement_type == "RELEASE" && m.entity_sync_id.as_deref() == Some(release_id.as_str()))
        {
            return 0.0;
        }
    }
    let own = if movements.iter().any(|m| m.movement_type == "RESERVE") {
        let mut ordered: Vec<&ReservationMovement> = movements.iter().collect();
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
    } else {
        row_remaining
    };
    own.min(row_remaining).max(0.0)
}

/// The reservation's movements with a reserved-stock effect (indexed by reservation).
async fn movements_of_reservation<C: ConnectionTrait>(
    conn: &C,
    reservation_id: i64,
) -> AppResult<Vec<ReservationMovement>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id, movement_type, quantity FROM inventory_transactions
             WHERE reservation_id = ? AND movement_type IN ('RESERVE', 'ISSUE', 'RETURN', 'RELEASE')",
            [reservation_id.into()],
        ))
        .await?;
    let mut movements = Vec::with_capacity(rows.len());
    for row in rows {
        movements.push(ReservationMovement {
            entity_sync_id: row.try_get("", "entity_sync_id")?,
            movement_type: row.try_get("", "movement_type")?,
            quantity: row.try_get("", "quantity")?,
        });
    }
    Ok(movements)
}

fn row_state(row: &sea_orm::QueryResult) -> AppResult<ReservationRowState> {
    Ok(ReservationRowState {
        entity_sync_id: row.try_get("", "entity_sync_id")?,
        status: row.try_get("", "status")?,
        quantity_reserved: row.try_get("", "quantity_reserved")?,
        quantity_issued: row.try_get("", "quantity_issued")?,
    })
}

/// Reserved stock one reservation holds in the projection (what its release frees).
pub(crate) async fn reservation_held<C: ConnectionTrait>(conn: &C, reservation_id: i64) -> AppResult<f64> {
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id, status, quantity_reserved, quantity_issued
             FROM stock_reservations WHERE id = ?",
            [reservation_id.into()],
        ))
        .await?
    else {
        return Ok(0.0);
    };
    let state = row_state(&row)?;
    let movements = movements_of_reservation(conn, reservation_id).await?;
    Ok(reservation_reserved(&state, &movements))
}

/// Reserved stock of an article at a location (the projection, not stored).
pub(crate) async fn reserved_projection<C: ConnectionTrait>(
    conn: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<f64> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, status, quantity_reserved, quantity_issued
             FROM stock_reservations
             WHERE article_id = ? AND location_id = ? AND status IN ('active', 'partial')
               AND quantity_reserved > quantity_issued
             ORDER BY entity_sync_id, id",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    let mut reserved = 0.0;
    for row in &rows {
        let reservation_id: i64 = row.try_get("", "id")?;
        let state = row_state(row)?;
        let movements = movements_of_reservation(conn, reservation_id).await?;
        reserved += reservation_reserved(&state, &movements);
    }
    Ok(reserved)
}

/// Writes the projection into `stock_balances` (reserved and available; on hand is not touched),
/// only when it changed. Called after every local command that moves reserved stock and, for
/// rows and movements received by sync, once per pair at the end of the materialize pass
/// ([`recompute_dirty_reserved`]). Returns the reserved quantity.
pub(crate) async fn recompute_reserved_in<C: ConnectionTrait>(
    conn: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<f64> {
    let reserved = reserved_projection(conn, article_id, location_id).await?;
    let existing = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reserved_qty, available_qty, on_hand_qty FROM stock_balances
             WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    match existing {
        Some(row) => {
            let current: f64 = row.try_get("", "reserved_qty")?;
            let available: f64 = row.try_get("", "available_qty")?;
            let on_hand: f64 = row.try_get("", "on_hand_qty")?;
            if current.to_bits() != reserved.to_bits() || available.to_bits() != (on_hand - reserved).to_bits() {
                conn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE stock_balances
                     SET reserved_qty = ?, available_qty = on_hand_qty - ?,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE article_id = ? AND location_id = ?",
                    [reserved.into(), reserved.into(), article_id.into(), location_id.into()],
                ))
                .await?;
            }
        }
        None if reserved > 0.0 => {
            // No balance row yet (reservations only): created with nothing on hand.
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
        None => {}
    }
    Ok(reserved)
}

/// Every pair marked (by the triggers of migration 257, in the transactions that wrote its
/// reservations or their movements) recomputed once and its mark removed, inside the caller's
/// transaction: the sync pass before it commits, a conflict choice (`sync::conflicts`). Reserved
/// stock and the writes that changed it commit together, so no one sees a figure in between.
/// Returns the number of pairs.
pub(crate) async fn recompute_dirty_reserved_in<C: ConnectionTrait>(conn: &C) -> AppResult<usize> {
    let pairs = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT article_id, location_id FROM stock_reserved_dirty".to_string(),
        ))
        .await?;
    for row in &pairs {
        let article_id: i64 = row.try_get("", "article_id")?;
        let location_id: i64 = row.try_get("", "location_id")?;
        recompute_reserved_in(conn, article_id, location_id).await?;
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM stock_reserved_dirty WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    }
    Ok(pairs.len())
}

/// Safety net, in its own transaction: marks left by a write path that did not recompute (an
/// interrupted pass, a failed recompute). Run after every materialize pass and every sync cycle,
/// also when nothing was received; nothing is written when there is no mark.
pub(crate) async fn recompute_dirty_reserved(db: &DatabaseConnection) -> AppResult<usize> {
    let any = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT 1 AS present FROM stock_reserved_dirty LIMIT 1".to_string(),
        ))
        .await?
        .is_some();
    if !any {
        return Ok(0);
    }
    let txn = db.begin().await?;
    let pairs = recompute_dirty_reserved_in(&txn).await?;
    txn.commit().await?;
    Ok(pairs)
}

/// Startup: recompute reserved stock of every article and location that has reservations or a
/// reserved balance (repairs balances kept as running totals before the projection), and clear
/// the marks. Returns the number of pairs.
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
    txn.execute(Statement::from_string(
        DbBackend::Sqlite,
        "DELETE FROM stock_reserved_dirty".to_string(),
    ))
    .await?;
    txn.commit().await?;
    Ok(pairs.len())
}

/// What the startup upkeep of reservations did.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReservationUpkeepReport {
    pub pairs_recomputed: usize,
    pub recompute_ms: u64,
    pub sweep: EndedWoSweepReport,
}

/// Startup, with sync capture on: reserved stock recomputed everywhere, then the D13 sweep of
/// ended work orders. Each step runs even when the other failed; the errors are returned for the
/// log.
pub(crate) async fn startup_reservation_upkeep(db: &DatabaseConnection) -> (ReservationUpkeepReport, Vec<String>) {
    let mut report = ReservationUpkeepReport::default();
    let mut errors = Vec::new();
    let started = std::time::Instant::now();
    match recompute_all_reserved(db).await {
        Ok(n) => report.pairs_recomputed = n,
        Err(e) => errors.push(format!("reserved stock recompute: {e}")),
    }
    report.recompute_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match release_reservations_of_ended_work_orders(db, SweepOrigin::Startup).await {
        Ok(sweep) => report.sweep = sweep,
        Err(e) => errors.push(format!("D13 sweep: {e}")),
    }
    (report, errors)
}

// ── A reservation released on two PCs ──────────────────────────────────────
//
// Two PCs that both release the same reservation each change its row and each write its one
// RELEASE movement (same seeded sync id). The stock is right on both (a released reservation
// counts 0 in the reserved-stock projection), but the second PC to synchronise meets a conflict
// on the row, and on the movement (two creates of the same sync id). When the two versions are
// both "released" and differ only in who/when, the server's version is adopted (first push wins)
// and this PC's release is audited (`inventory.reservation.released_twice`); no one has to
// decide. Any other difference stays a conflict for a person.
//
// May differ on the reservation row: `released_at`, `updated_at`, `row_version` and
// `quantity_issued`. The issued quantity may legitimately differ (one PC issued more before
// releasing); the converged value is the server's. A released reservation counts 0 whatever its
// issued quantity, and a return checks the issued quantity of the ledger as well
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
/// audited. Returns true when settled; false leaves the conflict for a person.
///
/// Called where conflicts are recorded (server answer, inbound apply), in their transaction. The
/// whole attempt runs in a savepoint and never fails the caller: any error rolls the attempt back,
/// is logged, and leaves the conflict for a person, so a sync answer is never refused for it.
pub(crate) async fn settle_double_release<C: ConnectionTrait>(
    conn: &C,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<bool> {
    if entity_type != "stock_reservations" && entity_type != "inventory_transactions" {
        return Ok(false);
    }
    if let Err(e) = conn.execute_unprepared("SAVEPOINT reservation_double_release").await {
        tracing::warn!(entity_type, entity_sync_id, error = %e, "double release not settled");
        return Ok(false);
    }
    let attempt = settle_double_release_in_savepoint(conn, entity_type, entity_sync_id).await;
    let settled = matches!(attempt, Ok(true));
    if let Err(e) = &attempt {
        tracing::warn!(entity_type, entity_sync_id, error = %e, "double release not settled; left for a person");
    }
    if !settled {
        if let Err(e) = conn.execute_unprepared("ROLLBACK TO reservation_double_release").await {
            tracing::error!(entity_type, entity_sync_id, error = %e, "double release attempt not rolled back");
        }
    }
    if let Err(e) = conn.execute_unprepared("RELEASE reservation_double_release").await {
        tracing::error!(entity_type, entity_sync_id, error = %e, "double release savepoint not released");
        return Ok(false);
    }
    Ok(settled)
}

async fn settle_double_release_in_savepoint<C: ConnectionTrait>(
    conn: &C,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<bool> {
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

    crate::sync::conflicts::take_remote_version(conn, &conflict).await?;
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
    Ok(true)
}
