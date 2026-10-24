//! What this PC does by itself once received permit rows are applied (CMMS Advisor answer to
//! AU-101, option B; owner decision D4, condition 2), and how two PCs doing it at once converge.
//!
//! The apply of a received row never writes a change of this PC's own (`sync::materialize`: the
//! received row is applied, checked and raised for review in its own transaction). After the pass
//! that applied them has committed, with capture active again, [`after_sync_pass`] looks at the
//! permit and isolation rows it applied, each in its own local transaction, written and sent like
//! any local change. A failure is logged, never returned: the received rows, their review conflicts
//! and their notices are already kept.
//!
//! - A permit received as ended (closed, handed back, cancelled) or suspended (or waiting for
//!   revalidation) while its work order runs here: the work order is put on hold, recorded as the
//!   system (`wo::execution::hold_for_permit_ended_elsewhere`). Nothing when it is not running here
//!   (already on hold, completed, closed): idempotent. Resume stays refused by the permit gates.
//! - A lock removal verified on another PC: when it was the last lock fitted under the permits of
//!   a mechanically complete work order, its downtime ends at that verification
//!   (`wo::execution::end_downtime_after_lock_removal`). The PC that verified it already ended the
//!   downtime in the verification's transaction when the work order was complete there, and that
//!   change arrives with it; this covers a work order completed here only (the same end time on
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

/// Entity types whose applied rows [`after_sync_pass`] looks at.
pub(crate) const AFTER_SYNC_TYPES: &[&str] = &["work_permits", "permit_isolations"];

/// Permit statuses received that put running work on hold here.
const HOLDING_ENDS: &[&str] = &[
    "closed",
    "handed_back",
    "cancelled",
    "suspended",
    "revalidation_required",
];

/// Outbox and conflict code of this PC's own hold closed because the same hold reached the server
/// first from another PC.
pub(crate) const CROSSED_HOLD_KEPT: &str = "PERMIT_HOLD_CROSSED";

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("permit end after sync: {e}"))
}

/// After a materialize pass has committed: the local consequences of the permit and isolation rows
/// it applied (module documentation). Best effort, each in its own transaction.
pub async fn after_sync_pass(db: &DatabaseConnection, applied: &[(String, String)]) {
    for (entity_type, sync_id) in applied {
        let done = match entity_type.as_str() {
            "work_permits" => match hold_work_of_ended_permit(db, sync_id).await {
                // Its isolations may have come with it (judged as its group, not listed here).
                Ok(()) => end_downtime_of_permit_work(db, sync_id).await,
                Err(err) => Err(err),
            },
            "permit_isolations" => end_downtime_after_received_removal(db, sync_id).await,
            _ => Ok(()),
        };
        if let Err(err) = done {
            tracing::error!(
                target: "maintafox::sync",
                entity_type = %entity_type,
                entity_sync_id = %sync_id,
                error = %err,
                "local consequence of a received permit row not written; the received row is kept"
            );
        }
    }
}

/// A received permit that ended (or is suspended) with its work order running here: the work order
/// is put on hold (module documentation).
pub(crate) async fn hold_work_of_ended_permit(db: &DatabaseConnection, permit_sync_id: &str) -> AppResult<()> {
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.code, p.status, p.linked_work_order_id FROM work_permits p WHERE p.entity_sync_id = ?",
            [permit_sync_id.into()],
        ))
        .await?
    else {
        return Ok(());
    };
    let status: String = row.try_get("", "status").map_err(sync_err)?;
    let Some(wo_id) = row
        .try_get::<Option<i64>>("", "linked_work_order_id")
        .map_err(sync_err)?
    else {
        return Ok(());
    };
    if !HOLDING_ENDS.contains(&status.as_str()) {
        return Ok(());
    }
    let code: String = row.try_get("", "code").map_err(sync_err)?;
    let txn = db.begin().await?;
    let held =
        crate::wo::execution::hold_for_permit_ended_elsewhere(&txn, wo_id, permit_sync_id, &code, &status).await?;
    txn.commit().await?;
    if held {
        tracing::warn!(
            target: "maintafox::sync",
            permit = %code,
            permit_status = %status,
            work_order_id = wo_id,
            "work order put on hold: its permit ended on another PC"
        );
    }
    Ok(())
}

/// The work order and the time of its last lock removal verification, of the permit (or the
/// isolation) received: `FROM` part over `p` (the permit).
const LAST_VERIFICATION_SQL: &str = "SELECT p.linked_work_order_id AS wo_id, \
     (SELECT MAX(x.removal_verified_at) FROM permit_isolations x \
        JOIN work_permits xp ON xp.id = x.permit_id \
       WHERE xp.linked_work_order_id = p.linked_work_order_id) AS last_verified_at";

/// A received lock removal verification: the downtime of a completed work order ends when no lock
/// is fitted under its permits any more (module documentation), at the last verification's time.
pub(crate) async fn end_downtime_after_received_removal(
    db: &DatabaseConnection,
    isolation_sync_id: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{LAST_VERIFICATION_SQL} FROM permit_isolations pi JOIN work_permits p ON p.id = pi.permit_id \
                 WHERE pi.entity_sync_id = ? AND pi.removal_verified_at IS NOT NULL"
            ),
            [isolation_sync_id.into()],
        ))
        .await?;
    end_downtime_at_last_verification(db, row, isolation_sync_id).await
}

/// [`end_downtime_after_received_removal`] for the work order of a received permit row: its
/// isolations may have been applied with it (its group), not listed on their own.
async fn end_downtime_of_permit_work(db: &DatabaseConnection, permit_sync_id: &str) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LAST_VERIFICATION_SQL} FROM work_permits p WHERE p.entity_sync_id = ?"),
            [permit_sync_id.into()],
        ))
        .await?;
    end_downtime_at_last_verification(db, row, permit_sync_id).await
}

/// Ends the downtime (`wo::execution::end_downtime_after_lock_removal`, which does nothing unless
/// the work order is complete with no lock fitted and downtime open) of the work order of `row` at
/// its last verification, in its own transaction; `origin_sync_id`: the received row.
async fn end_downtime_at_last_verification(
    db: &DatabaseConnection,
    row: Option<sea_orm::QueryResult>,
    origin_sync_id: &str,
) -> AppResult<()> {
    let Some(row) = row else {
        return Ok(());
    };
    let (Some(wo_id), Some(verified_at)) = (
        row.try_get::<Option<i64>>("", "wo_id").map_err(sync_err)?,
        row.try_get::<Option<String>>("", "last_verified_at")
            .map_err(sync_err)?,
    ) else {
        return Ok(());
    };
    let txn = db.begin().await?;
    crate::wo::execution::end_downtime_after_lock_removal(&txn, wo_id, &verified_at, None, origin_sync_id).await?;
    txn.commit().await?;
    Ok(())
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
