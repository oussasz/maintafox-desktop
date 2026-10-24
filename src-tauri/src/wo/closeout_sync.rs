//! Work-order completion and close re-checked on rows received by synchronisation (AU-99,
//! `sync::apply_hooks`).
//!
//! A completion or a close made on another PC reaches this one as a work-order row, with its
//! tasks, failure coding, labour, verifications, parts, downtime, tools and status changes as
//! rows of their own. The rules the local commands enforce are checked again here, on the state
//! received, with the same functions (one implementation, no drift):
//! - R1 a work order whose status changes here, or that is new here, is an entry; its status not
//!   applied yet makes it wait;
//! - R2 its close-out children still waiting in the inbox (pending, or set aside only for this
//!   missing work order; with the stock reservations its part lines name) are applied with it, in
//!   its transaction, judged against its status here before the change
//!   (`permit::sync_check::apply_group`); a child that cannot be counted yet (a row it refers to
//!   not here) makes the work order wait. So a work order received cancelled, reopened or new here
//!   brings its children with it, never refused against the status it has just been given;
//! - R3 entering `completed` (or `closed` from anything but `completed`: several steps made
//!   offline), the completion gates (`wo::execution::collect_completion_gates`) on the written
//!   state: open labour, every mandatory task closed with an accepted result (D3), parts
//!   disposition, open downtime, corrective failure coding and root cause -> `WO_COMPLETION_GATE`;
//! - R4 entering `closed`, the close gate (`wo::closeout::close_preflight`) on the written state,
//!   the no-downtime attestation and the FMECA parts override read from the row, the closer from
//!   the `close` transition of this close (dated at its `closed_at`); the override signer's
//!   password cannot be checked again on this PC (the signer, the role and "distinct from the
//!   closer" are) -> `WO_CLOSE_GATE`.
//!   A refused completion or close writes nothing: the work order and its group are rolled back.
//! - R5 a task new, changed or deleted while its work order is completed, closed or cancelled here
//!   (in its group: before and after the change) -> `WO_TASK_AFTER_COMPLETION`; a mandatory task
//!   deleted, or made optional, unless its work order is a draft or in planning here ->
//!   `WO_MANDATORY_TASK_REMOVED`;
//! - R6 a verification by the work order's responsible -> `WO_SELF_VERIFICATION`; with no
//!   responsible, by someone who recorded labour on it (D7) -> `WO_VERIFIER_RECORDED_LABOUR`; new
//!   or changed while its work order is closed or cancelled here -> `WO_VERIFICATION_WRONG_STATUS`;
//! - R7 a failure coding, a labour line or a tool new, changed or deleted while its work order is
//!   closed or cancelled here (in its group: before and after the change) ->
//!   `WO_FAILURE_DETAIL_AFTER_CLOSE` / `WO_LABOUR_AFTER_CLOSE` / `WO_TOOL_AFTER_CLOSE`.
//!
//! Rows recorded after a status change this PC has not received yet wait (deferred, with the reason
//! shown in the sync centre), never conflict: a task result while the work order is a draft, in
//! planning or ready here; a verification while it is not completed here (draft to on hold); labour
//! or tool use while it is not executed here (draft, planning, ready, completed). The delta brings a
//! work order's latest state after the rows recorded before it, and a page may end between them;
//! the waiting row is then counted in the work order's next status-change group and judged there.
//! A row whose work order never reaches that status here waits for good (fail safe, visible as
//! deferred). Inside a group only the two ends of the change are seen, so a result recorded during
//! an execution between two pre-execution statuses is not refused; any later completion or close
//! is judged (R3 / R4).
//!
//! A work order received completed or closed again with the same status here, but completed or
//! closed at another time (reopened and completed or closed again elsewhere), is judged as coming
//! back from execution (`in_progress` before the change): its re-recorded children come with it,
//! and R3 / R4 run. A child that R5 / R6 / R7 would refuse waits for that return instead only when
//! a reopening has actually been received here (the work order's log holds more `reopen`
//! transitions than this PC's `reopen_count`) and the row was recorded after this PC's completion
//! or close; anything else is refused as before. Such a row waits until the return arrives; one
//! whose return never comes waits for good, deferred with its reason.
//!
//! A mandatory task received cancelled, deferred or NOK without evidence from a PC that has not
//! upgraded is not refused as a row while its work order is open (it is the technician's record):
//! it counts as open, so that work order's completion and close are refused (R3 / R4).
//!
//! The state judged is the one held here: a row identical to the local one passes (sent again).
//!
//! A work order new here received by a snapshot this PC asked for (joining the plant, or
//! bootstrapping again; `sync::materialize::applying_snapshot_row`) is the plant's history, already
//! judged on the PCs that held it: it is applied with its children (R2), and neither it (R3 / R4)
//! nor its children that came with the snapshot (R5-R7, the verifier rules) are judged again, so a
//! PC joining never refuses a work order closed before these rules existed; a child received as a
//! change since, applied in that group, is judged. Snapshot pages are all stored before any is
//! applied (`sync::transport::worker`), so a work order always finds its children in the inbox,
//! whatever the order of the pages.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::permit::sync_check::{
    conflict, field, group_parent_judged, group_previous_status, local_id, local_user, local_wire, overlay,
    status_code, wait, wo_children_with, Group, GroupRow, Payload,
};
use crate::settings::site::InboundVerdict;
use crate::sync::materialize::ApplyOutcome;

/// Conflict codes (shown in the sync centre, `sync.conflictKind`).
pub(crate) mod codes {
    pub const COMPLETION_GATE: &str = "WO_COMPLETION_GATE";
    pub const CLOSE_GATE: &str = "WO_CLOSE_GATE";
    pub const TASK_AFTER_COMPLETION: &str = "WO_TASK_AFTER_COMPLETION";
    pub const MANDATORY_TASK_REMOVED: &str = "WO_MANDATORY_TASK_REMOVED";
    pub const VERIFICATION_WRONG_STATUS: &str = "WO_VERIFICATION_WRONG_STATUS";
    pub const SELF_VERIFICATION: &str = "WO_SELF_VERIFICATION";
    pub const VERIFIER_RECORDED_LABOUR: &str = "WO_VERIFIER_RECORDED_LABOUR";
    pub const FAILURE_DETAIL_AFTER_CLOSE: &str = "WO_FAILURE_DETAIL_AFTER_CLOSE";
    pub const LABOUR_AFTER_CLOSE: &str = "WO_LABOUR_AFTER_CLOSE";
    pub const TOOL_AFTER_CLOSE: &str = "WO_TOOL_AFTER_CLOSE";
}

/// The children of a work order applied with its status change: (table, payload key of the work
/// order, local column). Stock reservations come before the part lines that name them, labour
/// before verifications (the verifier rule reads it).
const CLOSEOUT_CHILDREN: &[(&str, &str, &str)] = &[
    ("stock_reservations", "source_sync_id", "source_id"),
    ("work_order_tasks", "work_order_sync_id", "work_order_id"),
    ("work_order_failure_details", "work_order_sync_id", "work_order_id"),
    ("work_order_interveners", "work_order_sync_id", "work_order_id"),
    ("work_order_parts", "work_order_sync_id", "work_order_id"),
    ("work_order_downtime_segments", "work_order_sync_id", "work_order_id"),
    ("work_order_tools", "work_order_sync_id", "work_order_id"),
    ("work_order_verifications", "work_order_sync_id", "work_order_id"),
    ("wo_state_transition_log", "wo_sync_id", "wo_id"),
];

/// Keys that change with every save and say nothing about the close-out.
const IGNORED_KEYS: &[&str] = &["updated_at", crate::sync::row_layout::PAYLOAD_FORMAT_KEY];

/// The fields of a task's result (AU-15: recorded only while its work order is executed).
const TASK_RESULT_KEYS: &[&str] = &[
    "is_completed",
    "result_code",
    "completed_at",
    "completed_by_sync_id",
    "finding_note",
    "follow_up_work_order_sync_id",
];

/// Work-order statuses before execution: a task result received then waits for the start.
const BEFORE_EXECUTION: &[&str] = &["draft", "planning", "ready"];
/// Work-order statuses outside execution in which labour and tool use received wait for the next
/// status change (closed and cancelled refuse them, R7).
const OUTSIDE_EXECUTION: &[&str] = &["draft", "planning", "ready", "completed"];
/// Work-order statuses before completion: a verification received then waits for it.
const BEFORE_COMPLETION: &[&str] = &["draft", "planning", "ready", "in_progress", "on_hold"];
/// Work-order statuses in which a mandatory task may still be removed or made optional.
const BEFORE_PLANNING_FROZEN: &[&str] = &["draft", "planning"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseoutTarget {
    Completed,
    Closed,
    /// Any other status change (or a work order new here): its children come with it; nothing
    /// more is judged after it.
    Other,
}

/// A received work order whose status changes here, or that is new here.
#[derive(Debug, Clone)]
pub(crate) struct CloseoutEntry {
    pub wo_sync_id: String,
    pub target: CloseoutTarget,
    /// Its status here before the change (`None`: new here).
    pub previous_status: Option<String>,
    /// Judged (R3 / R4, and its children by R5-R7): not for a work order new here received by a
    /// snapshot.
    pub judged: bool,
}

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("close-out re-check of a received row: {e}"))
}

/// R1: `Some` when the received work order (its local copy overlaid by the row) changes status
/// here or is new here; `Err`: its status is not applied here yet.
pub(crate) async fn closeout_entry(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Result<Option<CloseoutEntry>, String>> {
    let local = local_wire(conn, "work_orders", entity_sync_id).await?;
    let wo = overlay(local.as_ref(), received);
    let Some(status_sync) = field(&wo, "status_sync_id") else {
        return Ok(Ok(None));
    };
    let Some(to) = status_code(conn, status_sync).await? else {
        return Ok(Err(format!("work-order status {status_sync} is not applied yet")));
    };
    let mut previous_status = match local.as_ref().and_then(|l| field(l, "status_sync_id")) {
        Some(from_sync) => status_code(conn, from_sync).await?,
        None => None,
    };
    if local.is_some() && previous_status.as_deref() == Some(to.as_str()) {
        // The same status here, but completed or closed again elsewhere after a reopening (its
        // completion or close is stamped at another time, or it was reopened more often there):
        // judged as coming back from execution, so the children re-recorded during that execution
        // come with it and R3 / R4 run (review S2, round 3 SF1).
        let stamp = match to.as_str() {
            "completed" => Some("mechanically_completed_at"),
            "closed" => Some("closed_at"),
            _ => None,
        };
        let restamped = stamp.is_some_and(|k| {
            let before = local.as_ref().and_then(|l| field(l, k));
            let after = field(received, k);
            before.is_some() && after.is_some() && before != after
        });
        let count = |p: Option<&Payload>| p.and_then(|p| p.get("reopen_count")).and_then(Value::as_i64);
        let reopened = matches!((count(local.as_ref()), count(Some(received))), (Some(l), Some(r)) if r > l);
        if stamp.is_none() || !(restamped || reopened) {
            return Ok(Ok(None));
        }
        previous_status = Some("in_progress".to_string());
    }
    let target = match to.as_str() {
        "completed" => CloseoutTarget::Completed,
        "closed" => CloseoutTarget::Closed,
        _ => CloseoutTarget::Other,
    };
    let judged = local.is_some() || !crate::sync::materialize::applying_snapshot_row();
    Ok(Ok(Some(CloseoutEntry {
        wo_sync_id: entity_sync_id.to_string(),
        target,
        previous_status,
        judged,
    })))
}

/// The marker of a row waiting because it was recorded after this PC's completion or close of its
/// work order ([`recorded_after`]): such a row comes only with that work order's return from
/// execution (a round trip), never with a change from completed or closed.
const RECORDED_AFTER_MARKER: &str = "recorded after this PC's completion or close";

/// The waiting close-out children of a work order (in [`CLOSEOUT_CHILDREN`] order) and why one
/// cannot be applied yet. A part line naming a reservation applied in the same group is not held
/// back by it. With `from_end` (the work order was completed or closed here before this change, and
/// the change is not a return from execution), a row waiting because it was recorded after that
/// completion or close here is left out: it waits for the round trip, judged on its own meanwhile.
async fn closeout_children(
    conn: &impl ConnectionTrait,
    wo_sync: &str,
    from_end: bool,
) -> AppResult<(Vec<GroupRow>, Option<String>)> {
    let mut rows = Vec::new();
    let mut applied_with: Vec<(&str, String)> = Vec::new();
    let mut blocked = None;
    for (table, key, col) in CLOSEOUT_CHILDREN {
        let children = wo_children_with(conn, table, key, col, wo_sync, &applied_with).await?;
        if blocked.is_none() {
            blocked = children.blocked;
        }
        applied_with.extend(children.rows.into_iter().map(|(sid, _)| (*table, sid)));
        rows.extend(children.counted);
    }
    if from_end && !rows.is_empty() {
        let ids: Vec<i64> = rows.iter().map(GroupRow::inbox_id).collect();
        let after: std::collections::HashSet<i64> = conn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM sync_inbox WHERE id IN (SELECT value FROM json_each(?)) \
                 AND apply_status = 'deferred' AND instr(COALESCE(rejection_message, ''), ?) > 0",
                [serde_json::to_string(&ids)?.into(), RECORDED_AFTER_MARKER.into()],
            ))
            .await?
            .iter()
            .filter_map(|r| r.try_get::<i64>("", "id").ok())
            .collect();
        rows.retain(|r| !after.contains(&r.inbox_id()));
    }
    Ok((rows, blocked))
}

/// Whether the entry starts from the work order completed or closed here (not a return from
/// execution): see [`closeout_children`].
fn from_end(entry: &CloseoutEntry) -> bool {
    matches!(entry.previous_status.as_deref(), Some("completed" | "closed"))
}

/// R1 / R2, before the received work order is written: waits while its status is not here, or
/// while one of its waiting close-out children cannot be applied yet.
pub(crate) async fn inbound_closeout_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let entry = match closeout_entry(conn, entity_sync_id, received).await? {
        Err(why) => return wait(why),
        Ok(None) => return Ok(None),
        Ok(Some(entry)) => entry,
    };
    if let (_, Some(why)) = closeout_children(conn, entity_sync_id, from_end(&entry)).await? {
        let code = field(received, "code").unwrap_or(entity_sync_id).to_string();
        return wait(format!("work order {code}: {why}"));
    }
    Ok(None)
}

/// R2: the waiting close-out children of a received status change (or of a work order new here),
/// applied with it; always a group on entry (even without children), so a completion or close is
/// judged after it (R3 / R4).
pub(crate) async fn closeout_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<Group>> {
    let Ok(Some(entry)) = closeout_entry(conn, entity_sync_id, received).await? else {
        return Ok(None);
    };
    let (rows, _) = closeout_children(conn, entity_sync_id, from_end(&entry)).await?;
    Ok(Some(Group::new(
        entity_sync_id.to_string(),
        entry.previous_status.clone(),
        rows,
        entry.judged,
        Some(entry),
    )))
}

/// The closer of a received close: the person of the `close` transition of this close (dated at
/// the work order's `closed_at`, as `close_wo` writes both); `None` when it is not here.
pub(crate) async fn closer_of(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT t.actor_id FROM wo_state_transition_log t JOIN work_orders w ON w.id = t.wo_id \
         WHERE t.wo_id = ? AND t.to_status = 'closed' AND t.acted_at = w.closed_at \
         ORDER BY t.id DESC LIMIT 1",
        [wo_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<Option<i64>>("", "actor_id").map_err(sync_err))
    .transpose()
    .map(Option::flatten)
}

/// R3 / R4, after the work order and its group were written (same transaction): the completion
/// and close gates on the stored state. `Some` (a conflict) rolls everything back.
pub(crate) async fn judge_closeout(
    conn: &impl ConnectionTrait,
    entry: &CloseoutEntry,
) -> AppResult<Option<ApplyOutcome>> {
    let as_received = match entry.target {
        CloseoutTarget::Completed => "completed",
        CloseoutTarget::Closed => "closed",
        CloseoutTarget::Other => return Ok(None),
    };
    if !entry.judged {
        return Ok(None);
    }
    let Some(wo_id) = local_id(conn, "work_orders", &entry.wo_sync_id).await? else {
        return Ok(None);
    };
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code, no_downtime_attestation, no_downtime_attestation_reason, fmeca_parts_override_reason, \
                    fmeca_parts_override_signed_by_id \
             FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| sync_err("work order vanished after its apply"))?;
    let text = |c: &str| row.try_get::<Option<String>>("", c).map_err(sync_err);
    let code = text("code")?.unwrap_or_else(|| entry.wo_sync_id.clone());
    let multi_step = entry.target == CloseoutTarget::Closed && entry.previous_status.as_deref() != Some("completed");
    if entry.target == CloseoutTarget::Completed || multi_step {
        let failing: Vec<String> = crate::wo::execution::collect_completion_gates(conn, wo_id)
            .await?
            .into_iter()
            .filter(|g| g.required && !g.passed)
            .map(|g| match g.detail {
                Some(d) => format!("{} ({d})", g.code),
                None => g.code,
            })
            .collect();
        if !failing.is_empty() {
            return Ok(Some(ApplyOutcome::Conflict {
                code: codes::COMPLETION_GATE.into(),
                message: format!(
                    "work order {code} received as {as_received} while here it does not pass the completion checks: {}",
                    failing.join("; ")
                ),
            }));
        }
    }
    if entry.target == CloseoutTarget::Closed {
        let reason = text("no_downtime_attestation_reason")?;
        let override_reason = text("fmeca_parts_override_reason")?;
        let attestation: Option<i64> = row.try_get("", "no_downtime_attestation").map_err(sync_err)?;
        let signer: Option<i64> = row.try_get("", "fmeca_parts_override_signed_by_id").map_err(sync_err)?;
        let evidence = crate::wo::closeout::CloseEvidence {
            closer_user_id: closer_of(conn, wo_id).await?,
            no_downtime_attestation: attestation.map(|v| v != 0),
            no_downtime_attestation_reason: reason.as_deref(),
            fmeca_override_reason: override_reason.as_deref(),
            fmeca_override_signer_id: signer,
            fmeca_override_signer_password: crate::wo::closeout::SignerPassword::NotVerifiableHere,
        };
        let rpn = crate::reliability::advanced_rams::queries::fmeca_rpn_critical_threshold_i64_in(conn).await?;
        // The signer password outcome is `None` here: not checked on a received close (no lockout
        // counter of this PC is touched).
        let (errors, _, _) = crate::wo::closeout::close_preflight(conn, wo_id, &evidence, rpn).await?;
        if !errors.is_empty() {
            return Ok(Some(ApplyOutcome::Conflict {
                code: codes::CLOSE_GATE.into(),
                message: format!(
                    "work order {code} received as closed while here it does not pass the close checks: {}",
                    errors.join("; ")
                ),
            }));
        }
    }
    Ok(None)
}

/// How a close-out child is judged.
enum Parent {
    /// Its work order is not here: the apply waits for it as for any row.
    Missing,
    /// Applied in the group of its work order's received change: the status before it (`None`:
    /// new here) and after it; `judged` false for snapshot history (nothing judged).
    InGroup {
        before: Option<String>,
        after: String,
        judged: bool,
        code: String,
    },
    /// Received on its own: the status held here.
    Held { status: String, code: String },
}

async fn parent_of(conn: &impl ConnectionTrait, wo_sync: &str) -> AppResult<Parent> {
    let Some(wo) = local_wire(conn, "work_orders", wo_sync).await? else {
        return Ok(Parent::Missing);
    };
    let code = field(&wo, "code").unwrap_or(wo_sync).to_string();
    let now = match field(&wo, "status_sync_id") {
        Some(status_sync) => status_code(conn, status_sync).await?,
        None => None,
    };
    let Some(now) = now else {
        return Ok(Parent::Missing);
    };
    Ok(match group_previous_status(wo_sync) {
        Some(before) => Parent::InGroup {
            before,
            after: now,
            judged: group_parent_judged(wo_sync).unwrap_or(true),
            code,
        },
        None => Parent::Held { status: now, code },
    })
}

/// The time a close-out child records its own work at, per table (a task's result, a labour line's
/// start and end, a verification, a tool added); failure coding has none.
fn record_time_keys(table: &str) -> &'static [&'static str] {
    match table {
        "work_order_tasks" => &["completed_at"],
        "work_order_interveners" => &["started_at", "ended_at"],
        "work_order_verifications" => &["verified_at"],
        "work_order_tools" => &["created_at"],
        _ => &[],
    }
}

/// Whether a child that a rule would refuse belongs to a reopening of its work order made on
/// another PC whose return has not arrived yet (review round 3 SF1, final B2, final-2 SF1): a
/// reopening of this PC's completion or close has been received here — a `reopen` transition from
/// the status held here (`completed` or `closed`), dated at or after this PC's completion or close
/// stamp (the log is applied whatever the work order's status, and captured before the records
/// re-made during the reopening; each local completion or close resets the signal, a reopening of
/// an earlier one never counts) — and the row was recorded after that stamp. A later time alone is
/// no evidence.
async fn awaiting_return(
    conn: &impl ConnectionTrait,
    table: &str,
    wo_sync: &str,
    status: &str,
    row: &Payload,
) -> AppResult<bool> {
    let stamp_key = match status {
        "completed" => "mechanically_completed_at",
        "closed" => "closed_at",
        _ => return Ok(false),
    };
    let reopened = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT EXISTS (SELECT 1 FROM wo_state_transition_log t \
                   WHERE t.wo_id = w.id AND t.action = 'reopen' AND t.from_status = ? \
                     AND w.{stamp_key} IS NOT NULL AND julianday(t.acted_at) >= julianday(w.{stamp_key})) AS pending \
                 FROM work_orders w WHERE w.entity_sync_id = ?"
            ),
            [status.into(), wo_sync.into()],
        ))
        .await?
        .map(|r| r.try_get::<bool>("", "pending").map_err(sync_err))
        .transpose()?
        .unwrap_or(false);
    if !reopened {
        return Ok(false);
    }
    let Some(stamp) = local_wire(conn, "work_orders", wo_sync)
        .await?
        .as_ref()
        .and_then(|wo| field(wo, stamp_key))
        .and_then(crate::settings::site::instant)
    else {
        return Ok(false);
    };
    Ok(record_time_keys(table)
        .iter()
        .filter_map(|k| field(row, k).and_then(crate::settings::site::instant))
        .any(|t| t > stamp))
}

/// Why a received child waits for its work order's return from a reopening ([`RECORDED_AFTER_MARKER`]).
fn after_end(what: &str, code: &str, status: &str) -> String {
    format!(
        "{what}: {RECORDED_AFTER_MARKER} of work order {code} (here '{status}'), during a reopening of it made on \
         another PC and received here; waits for the work order to come back from that reopening (completed or \
         closed again), applied and judged with it"
    )
}

/// Why a received child waits: the status it was recorded in has not been reached here yet.
fn behind(what: &str, code: &str, status: &str) -> String {
    format!(
        "{what}: waits for work order {code} (here '{status}') to receive the status change it was recorded \
         after; applied with it"
    )
}

fn same_state(local: Option<&Payload>, row: &Payload) -> bool {
    let Some(local) = local else {
        return false;
    };
    let keys: std::collections::BTreeSet<&String> = local.keys().chain(row.keys()).collect();
    keys.into_iter()
        .filter(|k| !IGNORED_KEYS.contains(&k.as_str()))
        .all(|k| local.get(k).unwrap_or(&Value::Null) == row.get(k).unwrap_or(&Value::Null))
}

fn differs(local: Option<&Payload>, row: &Payload, keys: &[&str]) -> bool {
    keys.iter()
        .any(|k| local.and_then(|l| l.get(*k)).unwrap_or(&Value::Null) != row.get(*k).unwrap_or(&Value::Null))
}

fn is_set(p: &Payload, key: &str) -> bool {
    match p.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

/// The rule of a close-out child table: (statuses of its work order in which a change is refused,
/// conflict code, what the row is). Verifications are judged apart (R6).
fn child_rule(table: &str) -> Option<(&'static [&'static str], &'static str, &'static str)> {
    match table {
        "work_order_tasks" => Some((
            &["completed", "closed", "cancelled"],
            codes::TASK_AFTER_COMPLETION,
            "task",
        )),
        "work_order_failure_details" => Some((
            &["closed", "cancelled"],
            codes::FAILURE_DETAIL_AFTER_CLOSE,
            "failure coding",
        )),
        "work_order_interveners" => Some((&["closed", "cancelled"], codes::LABOUR_AFTER_CLOSE, "labour line")),
        "work_order_tools" => Some((&["closed", "cancelled"], codes::TOOL_AFTER_CLOSE, "tool")),
        _ => None,
    }
}

/// R5-R7 on a received task, failure coding, labour line, tool or verification (see the module
/// documentation).
pub(crate) async fn inbound_closeout_child_check(
    conn: &impl ConnectionTrait,
    table: &str,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, table, entity_sync_id).await?;
    let row = overlay(local.as_ref(), received);
    if same_state(local.as_ref(), &row) {
        return Ok(None);
    }
    // A history row already here (append-only, e.g. a verification) that the apply skips: nothing
    // to judge (same test as the apply, `row_codec::append_only_replay_skipped`).
    if local.is_some() {
        let layout = crate::sync::row_layout::layout(conn, table).await?;
        if crate::sync::row_codec::append_only_replay_skipped(conn, &layout, entity_sync_id, received).await? {
            return Ok(None);
        }
    }
    let Some(wo_sync) = field(&row, "work_order_sync_id").map(str::to_string) else {
        return Ok(None);
    };
    let what = if local.is_some() { "changed" } else { "added" };
    let parent = parent_of(conn, &wo_sync).await?;
    let code = match &parent {
        Parent::Missing => return Ok(None),
        // Snapshot history: applied as the plant holds it — a child that came with the snapshot too;
        // one received as a change since is judged (review N1).
        Parent::InGroup { judged: false, .. } if crate::sync::materialize::applying_snapshot_row() => return Ok(None),
        Parent::InGroup { code, .. } | Parent::Held { code, .. } => code.clone(),
    };
    if table == "work_order_tasks" {
        // A mandatory task made optional, only while the work order is a draft or in planning.
        let downgraded = local.as_ref().is_some_and(|l| is_set(l, "is_mandatory")) && !is_set(&row, "is_mandatory");
        let may_remove = match &parent {
            Parent::InGroup { before, after, .. } => {
                before.as_deref().is_none_or(|b| BEFORE_PLANNING_FROZEN.contains(&b))
                    || BEFORE_PLANNING_FROZEN.contains(&after.as_str())
            }
            Parent::Held { status, .. } => BEFORE_PLANNING_FROZEN.contains(&status.as_str()),
            Parent::Missing => true,
        };
        if downgraded && !may_remove {
            return conflict(
                codes::MANDATORY_TASK_REMOVED,
                format!("mandatory task {entity_sync_id} made optional while work order {code} is past planning here"),
            );
        }
        // A result recorded while the work order is not executed here yet (AU-15): its start (or
        // completion) is still on its way, made on the PC that recorded it; the row waits for the
        // work order's next status change here and is judged in that group (review B3).
        if let Parent::Held { status, .. } = &parent {
            let has_result = differs(local.as_ref(), &row, TASK_RESULT_KEYS)
                && (is_set(&row, "is_completed") || local.as_ref().is_some_and(|l| is_set(l, "is_completed")));
            if has_result && BEFORE_EXECUTION.contains(&status.as_str()) {
                return wait(behind(&format!("task {entity_sync_id}: result {what}"), &code, status));
            }
        }
    }
    if let Some((refused_in, conflict_code, label)) = child_rule(table) {
        let refused = match &parent {
            // Refused when the work order was in such a status both before and after the change
            // received with it (a cancel, a reopen or a completion brings its own children).
            Parent::InGroup { before, after, .. } => {
                before.as_deref().is_some_and(|b| refused_in.contains(&b)) && refused_in.contains(&after.as_str())
            }
            Parent::Held { status, .. } => refused_in.contains(&status.as_str()),
            Parent::Missing => false,
        };
        if refused {
            let status = match &parent {
                Parent::InGroup { after, .. } => after.clone(),
                Parent::Held { status, .. } => status.clone(),
                Parent::Missing => String::new(),
            };
            // Refused here, unless it belongs to a reopening received from another PC whose return
            // (a page may end between them) has not arrived yet: then it waits for that return,
            // visibly, and is judged with it (review round 3 SF1, final B1 / B2).
            if matches!(parent, Parent::Held { .. }) && awaiting_return(conn, table, &wo_sync, &status, &row).await? {
                return wait(after_end(&format!("{label} {entity_sync_id} {what}"), &code, &status));
            }
            return conflict(
                conflict_code,
                format!("{label} {entity_sync_id} {what} while work order {code} is '{status}' here"),
            );
        }
        // Labour and tool use recorded while the work order is not executed here (draft, planning,
        // ready, completed): made during an execution this PC has not received yet; waits for the
        // work order's next status change (review S1).
        if let Parent::Held { status, .. } = &parent {
            let execution_record = match table {
                // A labour line booked, or its time changed (a note may be corrected any time).
                "work_order_interveners" => {
                    local.is_none() || differs(local.as_ref(), &row, &["started_at", "ended_at", "hours_worked"])
                }
                "work_order_tools" => match &local {
                    Some(l) => l.get("usage_status") != row.get("usage_status"),
                    None => field(&row, "usage_status").is_some_and(|u| u != "planned"),
                },
                _ => false,
            };
            if execution_record && OUTSIDE_EXECUTION.contains(&status.as_str()) {
                return wait(behind(&format!("{label} {entity_sync_id} {what}"), &code, status));
            }
        }
        return Ok(None);
    }
    if table != "work_order_verifications" {
        return Ok(None);
    }
    // R6: recorded on a completed work order (in its group, the work order's own completion and
    // close are judged with it). Received before that completion here: waits for it (review B3);
    // on a work order closed or cancelled here: refused.
    if let Parent::Held { status, .. } = &parent {
        if ["closed", "cancelled"].contains(&status.as_str()) {
            if awaiting_return(conn, table, &wo_sync, status, &row).await? {
                return wait(after_end(
                    &format!("verification {entity_sync_id} {what}"),
                    &code,
                    status,
                ));
            }
            return conflict(
                codes::VERIFICATION_WRONG_STATUS,
                format!("verification {entity_sync_id} {what} while work order {code} is '{status}' here"),
            );
        }
        if BEFORE_COMPLETION.contains(&status.as_str()) {
            return wait(behind(&format!("verification {entity_sync_id} {what}"), &code, status));
        }
    }
    let Some(verifier_sync) = field(&row, "verified_by_sync_id") else {
        return Ok(None);
    };
    let Some(verifier) = local_user(conn, verifier_sync).await? else {
        return wait(format!(
            "verification {entity_sync_id}: verifier {verifier_sync} is not applied yet"
        ));
    };
    let Some(wo_id) = local_id(conn, "work_orders", &wo_sync).await? else {
        return Ok(None);
    };
    match crate::wo::closeout::verifier_conflict(conn, wo_id, verifier).await? {
        Some(crate::wo::closeout::VERIFIER_IS_RESPONSIBLE) => conflict(
            codes::SELF_VERIFICATION,
            format!("verification {entity_sync_id} of work order {code} by its own responsible"),
        ),
        Some(_) => conflict(
            codes::VERIFIER_RECORDED_LABOUR,
            format!(
                "verification {entity_sync_id} of work order {code} (no responsible) by someone who recorded labour on it"
            ),
        ),
        None => Ok(None),
    }
}

/// R5 / R7 on a received delete of a task, failure coding, labour line, tool or verification: refused
/// while its work order is in a status in which that row is kept (a verification: once closed or
/// cancelled, the evidence of its close); a mandatory task is deleted only while its work order is
/// a draft or in planning here.
pub(crate) async fn inbound_closeout_child_delete_check(
    conn: &impl ConnectionTrait,
    table: &str,
    entity_sync_id: &str,
) -> AppResult<Option<InboundVerdict>> {
    let Some(local) = local_wire(conn, table, entity_sync_id).await? else {
        return Ok(None);
    };
    let Some(wo_sync) = field(&local, "work_order_sync_id").map(str::to_string) else {
        return Ok(None);
    };
    let Parent::Held { status, code } = parent_of(conn, &wo_sync).await? else {
        return Ok(None);
    };
    let mandatory_removed = table == "work_order_tasks"
        && is_set(&local, "is_mandatory")
        && !BEFORE_PLANNING_FROZEN.contains(&status.as_str());
    let (refused_in, conflict_code, label): (&'static [&'static str], &'static str, &'static str) =
        match child_rule(table) {
            Some(rule) => rule,
            None if table == "work_order_verifications" => (
                &["closed", "cancelled"],
                codes::VERIFICATION_WRONG_STATUS,
                "verification",
            ),
            None => return Ok(None),
        };
    if refused_in.contains(&status.as_str()) {
        return conflict(
            conflict_code,
            format!("{label} {entity_sync_id} deleted while work order {code} is '{status}' here"),
        );
    }
    if mandatory_removed {
        return conflict(
            codes::MANDATORY_TASK_REMOVED,
            format!("mandatory task {entity_sync_id} deleted while work order {code} is '{status}' here"),
        );
    }
    Ok(None)
}
