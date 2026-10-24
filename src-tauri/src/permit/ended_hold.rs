//! What this PC does by itself once received permit rows are applied (CMMS Advisor answer to
//! AU-101, option B; owner decision D4, condition 2), and how two PCs doing it at once converge.
//!
//! The apply of a received row never writes a change of this PC's own (`sync::materialize`: the
//! received row is applied, checked and raised for review in its own transaction). The local
//! consequences are written by [`sweep`], derived from the state stored here — never from what a
//! pass held in memory — so a stop or a failure between a pass and its sweep is caught up by the
//! next one: it runs after every materialize pass (`sync::materialize`) and once at start-up
//! (`startup`). Each consequence is its own local transaction, written and sent like any local
//! change; a failure is logged, never returned (the received rows, their review conflicts and their
//! notices are already kept), and retried at the next sweep.
//!
//! - Hold: a work order running here (`in_progress`) with an open review conflict
//!   `PERMIT_ENDED_DURING_WORK` or `PERMIT_ENDED_BEFORE_START` on one of its permits
//!   (`permit::ended_under_work`, raised only when a received row moved the permit from a status
//!   that protected the work to an ended one), raised since the work order's last start, resume or
//!   reopening here, and that permit still ended (closed, handed back, cancelled, suspended,
//!   waiting for revalidation): put on hold, recorded as the system
//!   (`wo::execution::hold_for_permit_ended_elsewhere`). Never on a permit's status alone: a permit
//!   already ended here and handed back elsewhere while the work runs under another permit holds
//!   nothing. A work order a person resumed after the conflict was raised is not held again.
//! - Downtime: a mechanically complete work order with open downtime, no lock fitted under its
//!   permits any more and at least one verified removal: its downtime ends at the last verification
//!   (`wo::execution::end_downtime_after_lock_removal`). The PC that verified the last removal
//!   already ended it in the verification's transaction when the work order was complete there;
//!   this covers a removal verified elsewhere over a work order completed here (the same end time on
//!   both: the row then settles as the same change).
//!
//! Two PCs receiving the same permit end both hold the work order. The first hold the server
//! receives is kept; the second PC's own hold is closed without a person when it is the same hold
//! (`settle_crossed_hold_server_conflict` on the push, `settle_crossed_hold_inbound` on the apply):
//! the work order row differs only by its `updated_at`, and the delay segment — whose identity is
//! derived from the fact — only by its `started_at`. Anything else of this PC still unsent on those
//! rows is a person's change: left to the usual conflict. The history rows of both holds are kept
//! (both PCs held the work order).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{Map, Value};

use crate::errors::{AppError, AppResult};

/// Permit statuses that put running work on hold here (with their review conflict).
const HOLDING_ENDS_SQL: &str = "('closed', 'handed_back', 'cancelled', 'suspended', 'revalidation_required')";

/// Outbox and conflict code of this PC's own hold closed because the same hold reached the server
/// first from another PC.
pub(crate) const CROSSED_HOLD_KEPT: &str = "PERMIT_HOLD_CROSSED";

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("permit end after sync: {e}"))
}

/// What one [`sweep`] wrote.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    pub work_orders_held: u32,
    pub downtimes_ended: u32,
    pub failures: u32,
}

/// The local consequences of received permit rows, derived from the stored state (module
/// documentation). Best effort, each in its own transaction; idempotent.
pub async fn sweep(db: &DatabaseConnection) -> SweepReport {
    let mut report = SweepReport::default();
    match holds_due(db).await {
        Ok(due) => {
            for d in due {
                match hold_one(db, &d).await {
                    Ok(true) => report.work_orders_held += 1,
                    Ok(false) => {}
                    Err(err) => {
                        report.failures += 1;
                        tracing::error!(
                            target: "maintafox::sync",
                            work_order_id = d.wo_id,
                            permit = %d.permit_code,
                            error = %err,
                            "work order not put on hold for its permit ended elsewhere; retried at the next sweep"
                        );
                    }
                }
            }
        }
        Err(err) => {
            report.failures += 1;
            tracing::error!(target: "maintafox::sync", error = %err, "permit-end hold sweep not read");
        }
    }
    match downtimes_due(db).await {
        Ok(due) => {
            for (wo_id, verified_at, isolation_sync_id) in due {
                let ended = async {
                    let txn = db.begin().await?;
                    let n = crate::wo::execution::end_downtime_after_lock_removal(
                        &txn,
                        wo_id,
                        &verified_at,
                        None,
                        &isolation_sync_id,
                    )
                    .await?;
                    txn.commit().await?;
                    Ok::<u64, AppError>(n)
                }
                .await;
                match ended {
                    Ok(n) if n > 0 => report.downtimes_ended += 1,
                    Ok(_) => {}
                    Err(err) => {
                        report.failures += 1;
                        tracing::error!(
                            target: "maintafox::sync",
                            work_order_id = wo_id,
                            error = %err,
                            "downtime not ended after the last lock removal; retried at the next sweep"
                        );
                    }
                }
            }
        }
        Err(err) => {
            report.failures += 1;
            tracing::error!(target: "maintafox::sync", error = %err, "downtime-end sweep not read");
        }
    }
    report
}

/// A work order to hold for a permit ended elsewhere.
struct HoldDue {
    wo_id: i64,
    permit_sync_id: String,
    permit_code: String,
    permit_status: String,
}

/// Running work orders with an open permit-ended review raised since their last start here, on a
/// permit still ended (module documentation).
async fn holds_due(db: &DatabaseConnection) -> AppResult<Vec<HoldDue>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT DISTINCT w.id AS wo_id, p.entity_sync_id AS permit_sync_id, p.code AS permit_code, \
                        p.status AS permit_status \
                 FROM sync_conflicts c \
                 JOIN work_permits p ON p.entity_sync_id = c.entity_sync_id \
                 JOIN work_orders w ON w.id = p.linked_work_order_id \
                 JOIN work_order_statuses s ON s.id = w.status_id \
                 WHERE c.entity_type = 'work_permits' \
                   AND c.conflict_type IN ('{during}', '{before}') \
                   AND c.status IN ('new', 'triaged', 'escalated') \
                   AND s.code = 'in_progress' \
                   AND p.status IN {HOLDING_ENDS_SQL} \
                   AND c.created_at >= COALESCE((SELECT MAX(t.acted_at) FROM wo_state_transition_log t \
                                                  WHERE t.wo_id = w.id AND t.to_status = 'in_progress'), '') \
                 ORDER BY w.id",
                during = crate::permit::ended_under_work::codes::ENDED_DURING_WORK,
                before = crate::permit::ended_under_work::codes::ENDED_BEFORE_START,
            ),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(HoldDue {
                wo_id: r.try_get("", "wo_id").map_err(sync_err)?,
                permit_sync_id: r.try_get("", "permit_sync_id").map_err(sync_err)?,
                permit_code: r.try_get("", "permit_code").map_err(sync_err)?,
                permit_status: r.try_get("", "permit_status").map_err(sync_err)?,
            })
        })
        .collect()
}

async fn hold_one(db: &DatabaseConnection, d: &HoldDue) -> AppResult<bool> {
    let txn = db.begin().await?;
    let held = crate::wo::execution::hold_for_permit_ended_elsewhere(
        &txn,
        d.wo_id,
        &d.permit_sync_id,
        &d.permit_code,
        &d.permit_status,
    )
    .await?;
    txn.commit().await?;
    if held {
        tracing::warn!(
            target: "maintafox::sync",
            permit = %d.permit_code,
            permit_status = %d.permit_status,
            work_order_id = d.wo_id,
            "work order put on hold: its permit ended on another PC"
        );
    }
    Ok(held)
}

/// Completed work orders with open downtime, no fitted lock and a verified removal: the work order,
/// its last verification and the isolation it is on.
async fn downtimes_due(db: &DatabaseConnection) -> AppResult<Vec<(i64, String, String)>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT w.id AS wo_id, last.removal_verified_at AS verified_at, last.entity_sync_id AS isolation_sync_id \
             FROM work_orders w \
             JOIN work_order_statuses s ON s.id = w.status_id \
             JOIN permit_isolations last ON last.id = ( \
                 SELECT x.id FROM permit_isolations x JOIN work_permits xp ON xp.id = x.permit_id \
                  WHERE xp.linked_work_order_id = w.id AND x.removal_verified_at IS NOT NULL \
                  ORDER BY x.removal_verified_at DESC, x.id DESC LIMIT 1) \
             WHERE s.code = 'completed' \
               AND EXISTS (SELECT 1 FROM work_order_downtime_segments d \
                            WHERE d.work_order_id = w.id AND d.ended_at IS NULL) \
               AND NOT EXISTS (SELECT 1 FROM permit_isolations pi JOIN work_permits p ON p.id = pi.permit_id \
                                WHERE p.linked_work_order_id = w.id AND pi.applied_at IS NOT NULL \
                                  AND pi.removal_verified_at IS NULL) \
             ORDER BY w.id"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get("", "wo_id").map_err(sync_err)?,
                r.try_get("", "verified_at").map_err(sync_err)?,
                r.try_get::<Option<String>>("", "isolation_sync_id")
                    .map_err(sync_err)?
                    .unwrap_or_default(),
            ))
        })
        .collect()
}

// ── Two PCs holding the same work order for the same permit end ─────────────────────────────

/// The keys of two wire payloads whose values differ (a key missing on one side counts as null).
fn differing_keys(a: &Map<String, Value>, b: &Map<String, Value>) -> Vec<String> {
    let mut keys: Vec<String> = a
        .keys()
        .chain(b.keys())
        .filter(|k| a.get(*k).unwrap_or(&Value::Null) != b.get(*k).unwrap_or(&Value::Null))
        .cloned()
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// `other` (the version the server holds, or the one received) is the same hold as this PC's own
/// current row `entity_sync_id`, which is this PC's hold for a permit ended elsewhere (module
/// documentation). `false` when it is not (a person decides).
async fn same_hold(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    other: &Map<String, Value>,
) -> AppResult<bool> {
    let system_reasons = [
        crate::wo::execution::PERMIT_ENDED_DELAY_REASON,
        crate::wo::execution::PERMIT_SUSPENDED_DELAY_REASON,
    ];
    let allowed: &[&str] = match entity_type {
        "work_orders" => {
            // This PC's work order is on hold by the system for a permit ended elsewhere.
            let held = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT t.actor_id, t.reason_code, s.code AS status FROM work_orders w \
                     JOIN work_order_statuses s ON s.id = w.status_id \
                     LEFT JOIN wo_state_transition_log t ON t.id = ( \
                         SELECT t2.id FROM wo_state_transition_log t2 WHERE t2.wo_id = w.id \
                          ORDER BY t2.acted_at DESC, t2.id DESC LIMIT 1) \
                     WHERE w.entity_sync_id = ?",
                    [entity_sync_id.into()],
                ))
                .await?;
            let Some(held) = held else {
                return Ok(false);
            };
            let actor: Option<i64> = held.try_get("", "actor_id").map_err(sync_err)?;
            let reason: Option<String> = held.try_get("", "reason_code").map_err(sync_err)?;
            let status: String = held.try_get("", "status").map_err(sync_err)?;
            if status != "on_hold" || actor.is_some() || !reason.as_deref().is_some_and(|r| system_reasons.contains(&r))
            {
                return Ok(false);
            }
            &["updated_at"]
        }
        "work_order_delay_segments" => {
            let segment = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT s.entered_by_id, s.ended_at, rv.code FROM work_order_delay_segments s \
                     JOIN reference_values rv ON rv.id = s.delay_reason_id WHERE s.entity_sync_id = ?",
                    [entity_sync_id.into()],
                ))
                .await?;
            let Some(segment) = segment else {
                return Ok(false);
            };
            let by: Option<i64> = segment.try_get("", "entered_by_id").map_err(sync_err)?;
            let ended: Option<String> = segment.try_get("", "ended_at").map_err(sync_err)?;
            let code: String = segment.try_get("", "code").map_err(sync_err)?;
            if by.is_some() || ended.is_some() || !system_reasons.contains(&code.as_str()) {
                return Ok(false);
            }
            &["started_at"]
        }
        _ => return Ok(false),
    };
    let Some(local) = crate::permit::sync_check::local_wire(conn, entity_type, entity_sync_id).await? else {
        return Ok(false);
    };
    // The same status received: on hold for the work order (the only difference allowed is time).
    Ok(differing_keys(&local, other)
        .iter()
        .all(|k| allowed.contains(&k.as_str())))
}

/// Closes this PC's unsent changes of a row (`superseded`) and the conflicts still open for them:
/// the same hold reached the server first from another PC; its version is kept here.
async fn close_own_hold(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    server_row_version: Option<i64>,
) -> AppResult<()> {
    let message = format!(
        "The same hold (permit ended on another PC) reached the server first from another PC{}: its version is \
         kept here.",
        server_row_version
            .map(|v| format!(" (server version {v})"))
            .unwrap_or_default()
    );
    let filter = "entity_type = ? AND entity_sync_id = ? AND status IN ('pending', 'in_flight', 'conflict')";
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "UPDATE sync_conflicts
             SET status = 'resolved_remote', resolution_action = 'same_hold_kept', resolution_note = ?,
                 resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE status IN ('new', 'triaged', 'escalated')
               AND linked_outbox_id IN (SELECT id FROM sync_outbox WHERE {filter})"
        ),
        [message.clone().into(), entity_type.into(), entity_sync_id.into()],
    ))
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "UPDATE sync_outbox
             SET status = 'superseded', rejection_code = ?, rejection_message = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE {filter}"
        ),
        [
            CROSSED_HOLD_KEPT.into(),
            message.into(),
            entity_type.into(),
            entity_sync_id.into(),
        ],
    ))
    .await?;
    tracing::info!(
        target: "maintafox::sync",
        entity_type,
        entity_sync_id,
        "own hold closed: the same hold (permit ended elsewhere) was kept from another PC"
    );
    Ok(())
}

/// A server conflict on this PC's push of its own hold (work order row or its delay segment) when
/// the server holds the same hold from another PC (module documentation): this PC's change is
/// closed, no conflict is recorded; the server's version is applied when it arrives. `true` when
/// settled.
pub(crate) async fn settle_crossed_hold_server_conflict(
    conn: &impl ConnectionTrait,
    conflict: &crate::sync::domain::SyncServerConflictInput,
) -> AppResult<bool> {
    if conflict.server_deleted {
        return Ok(false);
    }
    let Some(server) = conflict
        .server_payload_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Map<String, Value>>(json).ok())
    else {
        return Ok(false);
    };
    if !same_hold(conn, &conflict.entity_type, &conflict.entity_sync_id, &server).await? {
        return Ok(false);
    }
    close_own_hold(
        conn,
        &conflict.entity_type,
        &conflict.entity_sync_id,
        Some(conflict.server_row_version),
    )
    .await?;
    Ok(true)
}

/// A received work order row or delay segment while this PC still has its own unsent hold of it
/// (`sync::materialize`, before the concurrency check): when it is the same hold, this PC's change
/// is closed and the received version is then applied. `true` when settled.
pub(crate) async fn settle_crossed_hold_inbound(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    payload_json: &str,
) -> AppResult<bool> {
    if entity_type != "work_orders" && entity_type != "work_order_delay_segments" {
        return Ok(false);
    }
    let unsent = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS unsent FROM sync_outbox
             WHERE entity_type = ? AND entity_sync_id = ? AND status IN ('pending', 'in_flight', 'conflict')
             LIMIT 1",
            [entity_type.into(), entity_sync_id.into()],
        ))
        .await?;
    if unsent.is_none() {
        return Ok(false);
    }
    let Ok(received) = serde_json::from_str::<Map<String, Value>>(payload_json) else {
        return Ok(false);
    };
    if !same_hold(conn, entity_type, entity_sync_id, &received).await? {
        return Ok(false);
    }
    close_own_hold(conn, entity_type, entity_sync_id, None).await?;
    Ok(true)
}

// ── A downtime end received while a lock is fitted here ─────────────────────────────────────

/// Conflict code: a downtime end received while a lock is fitted here under the work order's
/// permits (owner decision D4, condition 2, re-checked on arrival).
pub(crate) const DOWNTIME_END_LOCKS_FITTED: &str = "WO_DOWNTIME_END_LOCKS_FITTED";

/// A received downtime segment that ends one still open here, while a lock is fitted here under
/// its work order's permits (`permit::wo_gate::downtime_held_by_locks_sql`, the rule of the hand
/// end): it waits when the removal verification of those locks is received and still to apply (the
/// other PC verified them and ended the downtime in the same transaction; they arrive together),
/// else it is a conflict for a person. `None`: nothing to judge.
pub(crate) async fn inbound_downtime_end_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Map<String, Value>,
) -> AppResult<Option<crate::settings::site::InboundVerdict>> {
    use crate::permit::sync_check::{field, local_wire};
    use crate::settings::site::InboundVerdict;
    if field(received, "ended_at").is_none() {
        return Ok(None);
    }
    let Some(local) = local_wire(conn, "work_order_downtime_segments", entity_sync_id).await? else {
        return Ok(None);
    };
    if field(&local, "ended_at").is_some() {
        return Ok(None);
    }
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT s.work_order_id AS wo_id, {} AS held FROM work_order_downtime_segments s \
                 WHERE s.entity_sync_id = ?",
                crate::permit::wo_gate::downtime_held_by_locks_sql("s.work_order_id")
            ),
            [entity_sync_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let wo_id: i64 = row.try_get("", "wo_id").map_err(sync_err)?;
    if row.try_get::<i64>("", "held").map_err(sync_err)? == 0 {
        return Ok(None);
    }
    let verification_waiting = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS w FROM sync_inbox i \
             WHERE i.entity_type = 'permit_isolations' AND i.apply_status IN ('pending', 'deferred') \
               AND json_extract(i.payload_json, '$.removal_verified_at') IS NOT NULL \
               AND i.entity_sync_id IN ( \
                   SELECT pi.entity_sync_id FROM permit_isolations pi JOIN work_permits p ON p.id = pi.permit_id \
                    WHERE p.linked_work_order_id = ? AND pi.applied_at IS NOT NULL \
                      AND pi.removal_verified_at IS NULL) \
             LIMIT 1",
            [wo_id.into()],
        ))
        .await?
        .is_some();
    let message = format!(
        "downtime segment {entity_sync_id} received as ended while a lock is still fitted here under work order \
         {wo_id}'s permits"
    );
    Ok(Some(if verification_waiting {
        InboundVerdict::Defer(format!(
            "{message}: waits for the lock removal verification received with it"
        ))
    } else {
        InboundVerdict::Conflict {
            code: DOWNTIME_END_LOCKS_FITTED,
            message,
        }
    }))
}
