//! Work-order completion and close re-checked on rows received by synchronisation (AU-99,
//! `sync::apply_hooks`).
//!
//! A completion or a close made on another PC reaches this one as a work-order row, with its
//! tasks, failure coding, labour, verifications, parts, downtime, tools and status changes as
//! rows of their own. The rules the local commands enforce are checked again here, on the state
//! received, with the same functions (one implementation, no drift):
//! - R1 a work order entering `completed` or `closed` here is a close-out entry; its status not
//!   applied yet makes it wait;
//! - R2 its close-out children still waiting in the inbox (pending, or set aside only for this
//!   missing work order) are applied with it, in its transaction, judged against its status here
//!   before the change (`permit::sync_check::apply_group`); a child that cannot be counted yet (a
//!   row it refers to not here) makes the work order wait;
//! - R3 entering `completed` (or `closed` from anything but `completed`: several steps made
//!   offline), the completion gates (`wo::execution::collect_completion_gates`) on the written
//!   state: open labour, every mandatory task closed with an accepted result (D3), parts
//!   disposition, open downtime, corrective failure coding and root cause -> `WO_COMPLETION_GATE`;
//! - R4 entering `closed`, the close gate (`wo::closeout::close_preflight`) on the written state,
//!   the no-downtime attestation and the FMECA parts override read from the row, the closer from
//!   its `close` transition; the override signer's password cannot be checked again on this PC
//!   (the signer, the role and "distinct from the closer" are) -> `WO_CLOSE_GATE`.
//!   A refused completion or close writes nothing: the work order and its group are rolled back.
//! - R5 a task new, changed or deleted while its work order is completed, closed or cancelled here
//!   -> `WO_TASK_AFTER_COMPLETION`;
//! - R6 a verification new or changed while its work order is not `completed` here ->
//!   `WO_VERIFICATION_WRONG_STATUS`; by the work order's responsible -> `WO_SELF_VERIFICATION`; with
//!   no responsible, by someone who recorded labour on it (D7) -> `WO_VERIFIER_RECORDED_LABOUR`;
//! - R7 a failure coding, a labour line or a tool new, changed or deleted while its work order is
//!   closed or cancelled here -> `WO_FAILURE_DETAIL_AFTER_CLOSE` / `WO_LABOUR_AFTER_CLOSE` /
//!   `WO_TOOL_AFTER_CLOSE`.
//!
//! A mandatory task received cancelled, deferred or NOK without evidence from a PC that has not
//! upgraded is not refused as a row while its work order is open (it is the technician's record):
//! it counts as open, so that work order's completion and close are refused (R3 / R4).
//!
//! The state judged is the one held here: a row identical to the local one passes (sent again). A
//! child applied in its work order's group is judged against the work order's status before the
//! change; inside the group of a work order new here, the children are part of the same received
//! state and pass R5-R7 (R3 / R4 judge them together).
//!
//! A work order new here received by a snapshot (a PC joining the plant, or bootstrapping again)
//! is the plant's history, already judged on the PCs that held it: it is applied with its children
//! (R2) but not judged again by R3 / R4, so a PC joining never refuses a work order closed before
//! these rules existed. Snapshot pages are all stored before any is applied
//! (`sync::transport::worker`), so a work order always finds its children in the inbox, whatever
//! the order of the pages.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::permit::sync_check::{
    conflict, field, group_previous_status, local_id, local_user, local_wire, overlay, status_code, wait, wo_children,
    Group, Payload,
};
use crate::settings::site::InboundVerdict;
use crate::sync::materialize::ApplyOutcome;

/// Conflict codes (shown in the sync centre, `sync.conflictKind`).
pub(crate) mod codes {
    pub const COMPLETION_GATE: &str = "WO_COMPLETION_GATE";
    pub const CLOSE_GATE: &str = "WO_CLOSE_GATE";
    pub const TASK_AFTER_COMPLETION: &str = "WO_TASK_AFTER_COMPLETION";
    pub const VERIFICATION_WRONG_STATUS: &str = "WO_VERIFICATION_WRONG_STATUS";
    pub const SELF_VERIFICATION: &str = "WO_SELF_VERIFICATION";
    pub const VERIFIER_RECORDED_LABOUR: &str = "WO_VERIFIER_RECORDED_LABOUR";
    pub const FAILURE_DETAIL_AFTER_CLOSE: &str = "WO_FAILURE_DETAIL_AFTER_CLOSE";
    pub const LABOUR_AFTER_CLOSE: &str = "WO_LABOUR_AFTER_CLOSE";
    pub const TOOL_AFTER_CLOSE: &str = "WO_TOOL_AFTER_CLOSE";
}

/// The children of a work order applied with its completion or close: (table, payload key of the
/// work order, local column). Labour comes before verifications (the verifier rule reads it).
const CLOSEOUT_CHILDREN: &[(&str, &str, &str)] = &[
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseoutTarget {
    Completed,
    Closed,
}

/// A received work order entering `completed` or `closed` here.
#[derive(Debug, Clone)]
pub(crate) struct CloseoutEntry {
    pub wo_sync_id: String,
    pub target: CloseoutTarget,
    /// Its status here before the change (`None`: new here).
    pub previous_status: Option<String>,
    /// Judged by R3 / R4 (not: a work order new here received by a snapshot).
    pub judged: bool,
}

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("close-out re-check of a received row: {e}"))
}

/// Whether the inbox row being applied for this entity came with a snapshot.
async fn received_by_snapshot(conn: &impl ConnectionTrait, entity_type: &str, sync_id: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT server_batch_id FROM sync_inbox \
             WHERE entity_type = ? AND entity_sync_id = ? AND apply_status IN ('pending', 'deferred') \
             ORDER BY id DESC LIMIT 1",
            [entity_type.into(), sync_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "server_batch_id").map_err(sync_err))
        .transpose()?
        .flatten()
        .is_some_and(|b| b.starts_with("snapshot-")))
}

/// R1: `Some` when the received work order (its local copy overlaid by the row) enters `completed`
/// or `closed` here; `Err`: its status is not applied here yet.
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
    let target = match to.as_str() {
        "completed" => CloseoutTarget::Completed,
        "closed" => CloseoutTarget::Closed,
        _ => return Ok(Ok(None)),
    };
    let previous_status = match local.as_ref().and_then(|l| field(l, "status_sync_id")) {
        Some(from_sync) => status_code(conn, from_sync).await?,
        None => None,
    };
    if previous_status.as_deref() == Some(to.as_str()) {
        return Ok(Ok(None));
    }
    let judged = local.is_some() || !received_by_snapshot(conn, "work_orders", entity_sync_id).await?;
    Ok(Ok(Some(CloseoutEntry {
        wo_sync_id: entity_sync_id.to_string(),
        target,
        previous_status,
        judged,
    })))
}

/// R1 / R2, before the received work order is written: waits while its status is not here, or
/// while one of its waiting close-out children cannot be applied yet.
pub(crate) async fn inbound_closeout_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    match closeout_entry(conn, entity_sync_id, received).await? {
        Err(why) => return wait(why),
        Ok(None) => return Ok(None),
        Ok(Some(_)) => {}
    }
    for (table, key, col) in CLOSEOUT_CHILDREN {
        if let Some(why) = wo_children(conn, table, key, col, entity_sync_id).await?.blocked {
            let code = field(received, "code").unwrap_or(entity_sync_id).to_string();
            return wait(format!("work order {code}: {why}"));
        }
    }
    Ok(None)
}

/// R2: the waiting close-out children of a received completion or close, applied with it; always
/// a group on entry (even without children), so the close-out is judged after it (R3 / R4).
pub(crate) async fn closeout_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<Group>> {
    let Ok(Some(entry)) = closeout_entry(conn, entity_sync_id, received).await? else {
        return Ok(None);
    };
    let mut rows = Vec::new();
    for (table, key, col) in CLOSEOUT_CHILDREN {
        rows.extend(wo_children(conn, table, key, col, entity_sync_id).await?.counted);
    }
    Ok(Some(Group::new(
        entity_sync_id.to_string(),
        entry.previous_status.clone(),
        rows,
        Some(entry),
    )))
}

/// The closer of a received close: the person of its last `close` transition.
async fn closer_of(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT actor_id FROM wo_state_transition_log \
         WHERE wo_id = ? AND to_status = 'closed' ORDER BY acted_at DESC, id DESC LIMIT 1",
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
    let as_received = match entry.target {
        CloseoutTarget::Completed => "completed",
        CloseoutTarget::Closed => "closed",
    };
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

/// The status a close-out child is judged against: inside its work order's group, the status
/// before the change (`Ok(None)`: new here, the child passes); otherwise the status held here
/// (`Ok(None)` too when the work order is not here: the apply waits for it as for any row).
async fn judged_parent_status(conn: &impl ConnectionTrait, wo_sync: &str) -> AppResult<Option<(String, String)>> {
    let Some(wo) = local_wire(conn, "work_orders", wo_sync).await? else {
        return Ok(None);
    };
    let code = field(&wo, "code").unwrap_or(wo_sync).to_string();
    if let Some(previous) = group_previous_status(wo_sync) {
        return Ok(previous.map(|p| (p, code)));
    }
    let Some(status_sync) = field(&wo, "status_sync_id") else {
        return Ok(None);
    };
    Ok(status_code(conn, status_sync).await?.map(|s| (s, code)))
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
    let Some(wo_sync) = field(&row, "work_order_sync_id").map(str::to_string) else {
        return Ok(None);
    };
    let in_group = group_previous_status(&wo_sync).is_some();
    let what = if local.is_some() { "changed" } else { "added" };
    let judged = judged_parent_status(conn, &wo_sync).await?;
    if let Some((refused_in, conflict_code, label)) = child_rule(table) {
        let Some((status, code)) = judged else {
            return Ok(None);
        };
        if refused_in.contains(&status.as_str()) {
            return conflict(
                conflict_code,
                format!("{label} {entity_sync_id} {what} while work order {code} is '{status}' here"),
            );
        }
        return Ok(None);
    }
    if table != "work_order_verifications" {
        return Ok(None);
    }
    // R6: recorded on a completed work order (in its close-out group, the work order's own
    // completion and close are judged with it); the verifier rules hold in the group too, also
    // for a work order new here.
    let code = match judged {
        Some((status, code)) => {
            if !in_group && status != "completed" {
                return conflict(
                    codes::VERIFICATION_WRONG_STATUS,
                    format!("verification {entity_sync_id} {what} while work order {code} is '{status}' here"),
                );
            }
            code
        }
        None if in_group => wo_sync.clone(),
        // The work order is not here yet: the apply waits for it.
        None => return Ok(None),
    };
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
/// cancelled, the evidence of its close).
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
    let Some((status, code)) = judged_parent_status(conn, &wo_sync).await? else {
        return Ok(None);
    };
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
    Ok(None)
}
