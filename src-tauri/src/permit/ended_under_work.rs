//! A permit ended on another PC while, here, its work order ran or started after that end (AU-03
//! re-checked on arrival, AU-101; owner decisions D5 and D9).
//!
//! On the PC that ends a permit, the local rules hold: a permit is neither closed nor cancelled while
//! its work order runs, and its suspension puts the running work on hold in the same transaction
//! (`permit::queries`). Two PCs working offline can still cross: PC2 closes or suspends permit P at
//! 09:50 while PC1, still holding P active, starts (or resumes, or reopens) its work order at 10:00.
//! When PC1 receives "P ended at 09:50", the end is a fact of the plant: the row is applied as
//! received, so the two PCs never diverge, and the crossing is raised for review here, in the
//! apply's transaction (`sync::apply_hooks::before_upsert_checked`, after the permit's own checks
//! passed; a row then refused or left waiting rolls both back):
//! - [`codes::ENDED_BEFORE_START`] (`PERMIT_ENDED_BEFORE_START`): the permit ended (closed, handed
//!   back, cancelled, suspended or waiting for revalidation) at a time before the work order's last
//!   start, resume or reopening recorded here (D9);
//! - [`codes::ENDED_DURING_WORK`] (`PERMIT_ENDED_DURING_WORK`): the work order runs here
//!   (`in_progress`) and the permit ended after its last start here, or at a time the row does not
//!   give (a cancellation carries no time) — what the local close, cancel and suspension rules
//!   prevent on a single PC (AU-03, D5), received from a PC that did not know the work had started;
//! - a cancellation (no time) over a work order started here and not closed — on hold or completed
//!   — is raised as [`codes::ENDED_BEFORE_START`] too: its order with the start cannot be proved
//!   (CMMS Advisor answer to S4); its notice says the time is not known.
//!
//! Each is a sync conflict for a person (the received permit row, with the copy held here before it
//! and the received version), never a refusal, and only "dismiss" (with its reason, once the work is
//! stopped) or "escalate" are accepted on it (`sync::queries::REVIEW_ONLY_CONFLICTS`). The work
//! order's responsible and the permit close-right holders of the permit's site get a safety notice
//! they acknowledge (`settings::received_notice::PERMIT_ENDED_UNDER_WORK_CATEGORY`). Nothing of the
//! work order is written in the apply's transaction (this PC never writes a change of its own
//! while applying a received row): a work order still running here is put on hold afterwards, in
//! its own local transaction, by `permit::ended_hold` (CMMS Advisor answer, option B).
//!
//! A suspension received while the work order is on hold here (normally the hold made with it on
//! the other PC, received first) is not a conflict: the work order's responsible is told the work is
//! held by the suspension, as on the PC that suspended it (D5).
//!
//! A permit suspended and reinstated on another PC, both before this PC receives either, arrives
//! `active` as held here; its suspension row arrives already lifted. When work recorded here
//! overlaps that suspension window ([`after_suspension_check`]) — a start (or resume, or
//! reopening) inside it ([`codes::ENDED_BEFORE_START`]), or work already running when it began
//! ([`codes::ENDED_DURING_WORK`]) — a review conflict and a notice are raised, one per window
//! (recorded on the suspension row); the permit is active again, so nothing is held.
//!
//! Known limit (two clocks): the end time is the sending PC's clock, the start time this PC's. When
//! the sender's clock runs ahead of this one by more than the gap between them, an end really
//! before the start is judged after it: on a work order running here it is still raised (during
//! the work), on one on hold or completed here it is not raised. The other D9 checks share this
//! model.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::permit::sync_check::{field, local_id, local_wire, overlay, payloads, permit_children, Payload};
use crate::settings::site::instant;

/// Conflict codes (shown in the sync centre, `sync.conflictKind`) and notice message keys.
pub(crate) mod codes {
    /// The permit ended before the work order's last start here (owner decision D9).
    pub const ENDED_BEFORE_START: &str = "PERMIT_ENDED_BEFORE_START";
    /// The permit ended while the work order runs here (AU-03 re-checked on arrival).
    pub const ENDED_DURING_WORK: &str = "PERMIT_ENDED_DURING_WORK";
    /// Message keys of the notice (`notifications:messages.*`).
    pub const NOTICE_BEFORE_START: &str = "permit_ended_before_start";
    pub const NOTICE_DURING_WORK: &str = "permit_ended_during_work";
    /// The same, for an end the received row gives no time of (a cancellation carries none): the
    /// notice says so instead of showing an empty time.
    pub const NOTICE_BEFORE_START_UNTIMED: &str = "permit_ended_before_start_untimed";
    pub const NOTICE_DURING_WORK_UNTIMED: &str = "permit_ended_during_work_untimed";
    /// A suspension made and lifted on another PC whose window covers a start here.
    pub const NOTICE_SUSPENDED_AROUND_START: &str = "permit_suspended_around_start";
    /// A suspension made and lifted on another PC while the work was already running here.
    pub const NOTICE_SUSPENDED_DURING_WORK: &str = "permit_suspended_during_work";
}

/// Statuses in which a permit no longer protects work: ended for good, or suspended.
const ENDED: &[&str] = &[
    "closed",
    "handed_back",
    "cancelled",
    "expired",
    "suspended",
    "revalidation_required",
];
/// Statuses of a suspension.
const SUSPENDED: &[&str] = &["suspended", "revalidation_required"];
/// Work-order statuses in which its work has started here.
const STARTED: &[&str] = &["in_progress", "on_hold", "completed", "closed"];

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("permit end received under work: {e}"))
}

/// The time a received permit ended there, when its row (or its open suspension) gives one.
async fn end_time(conn: &impl ConnectionTrait, sync_id: &str, permit: &Payload, to: &str) -> AppResult<Option<String>> {
    Ok(match to {
        "closed" | "handed_back" => field(permit, "closed_at")
            .or_else(|| field(permit, "handed_back_at"))
            .map(str::to_string),
        _ if SUSPENDED.contains(&to) => {
            let suspensions = permit_children(conn, "permit_suspensions", sync_id).await?;
            payloads(&suspensions)
                .into_iter()
                .filter(|s| field(s, "reinstated_at").is_none())
                .filter_map(|s| field(s, "suspended_at"))
                .filter_map(|t| instant(t).map(|i| (i, t.to_string())))
                .max_by_key(|(i, _)| *i)
                .map(|(_, t)| t)
        }
        _ => None,
    })
}

/// The work order a permit is linked to, as held here: id, code, status and the time of its last
/// start, resume or reopening here (its `in_progress` history row; else its actual start).
struct HeldWork {
    id: i64,
    code: String,
    status: String,
    last_start: Option<String>,
}

async fn held_work(conn: &impl ConnectionTrait, wo_sync_id: &str) -> AppResult<Option<HeldWork>> {
    let Some(id) = local_id(conn, "work_orders", wo_sync_id).await? else {
        return Ok(None);
    };
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT w.code, s.code AS status, \
                    COALESCE((SELECT t.acted_at FROM wo_state_transition_log t \
                              WHERE t.wo_id = w.id AND t.to_status = 'in_progress' \
                              ORDER BY t.acted_at DESC, t.id DESC LIMIT 1), w.actual_start) AS last_start \
             FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = ?",
            [id.into()],
        ))
        .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some(HeldWork {
        id,
        code: r
            .try_get::<Option<String>>("", "code")
            .map_err(sync_err)?
            .unwrap_or_default(),
        status: r.try_get("", "status").map_err(sync_err)?,
        last_start: r.try_get("", "last_start").map_err(sync_err)?,
    }))
}

fn at(t: Option<&str>) -> Option<DateTime<Utc>> {
    t.and_then(instant)
}

/// The version of the received row being applied (its inbox row, when it came through the inbox).
async fn received_version(conn: &impl ConnectionTrait, sync_id: &str) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT row_version FROM sync_inbox WHERE entity_type = 'work_permits' AND entity_sync_id = ? \
         AND apply_status IN ('pending', 'deferred') ORDER BY row_version DESC, id DESC LIMIT 1",
        [sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "row_version").map_err(sync_err))
    .transpose()
}

/// After a received permit passed its checks, before it is written (same transaction): when it
/// ends here (from a status that protected the work) while its work order ran here, or started
/// after that end, the review conflict and the notice (see the module documentation).
pub(crate) async fn after_permit_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<()> {
    // New here: it never protected work on this PC.
    let Some(local) = local_wire(conn, "work_permits", entity_sync_id).await? else {
        return Ok(());
    };
    let permit = overlay(Some(&local), received);
    let (Some(from), Some(to)) = (field(&local, "status"), field(&permit, "status")) else {
        return Ok(());
    };
    if from == to || ENDED.contains(&from) || !ENDED.contains(&to) || to == "expired" {
        return Ok(());
    }
    let Some(wo_sync) = field(&permit, "linked_work_order_sync_id") else {
        return Ok(());
    };
    let Some(work) = held_work(conn, wo_sync).await? else {
        return Ok(());
    };
    let Some(permit_id) = local_id(conn, "work_permits", entity_sync_id).await? else {
        return Ok(());
    };
    let permit_code = field(&permit, "code").unwrap_or(entity_sync_id).to_string();
    let end = end_time(conn, entity_sync_id, &permit, to).await?;
    let (end_at, start_at) = (at(end.as_deref()), at(work.last_start.as_deref()));
    let started_after_end =
        STARTED.contains(&work.status.as_str()) && matches!((end_at, start_at), (Some(e), Some(s)) if e < s);
    // A cancellation carries no time: over work started here and not closed, its order with the
    // start cannot be proved, so it is raised for review (CMMS Advisor answer to S4).
    let untimed_cancel_over_started_work = to == "cancelled"
        && end_at.is_none()
        && matches!(work.status.as_str(), "on_hold" | "completed")
        && work.last_start.is_some();
    let kind = if started_after_end || untimed_cancel_over_started_work {
        Some((
            codes::ENDED_BEFORE_START,
            if end_at.is_some() {
                codes::NOTICE_BEFORE_START
            } else {
                codes::NOTICE_BEFORE_START_UNTIMED
            },
        ))
    } else if work.status == "in_progress" {
        Some((
            codes::ENDED_DURING_WORK,
            if end_at.is_some() {
                codes::NOTICE_DURING_WORK
            } else {
                codes::NOTICE_DURING_WORK_UNTIMED
            },
        ))
    } else {
        None
    };
    let Some((code, notice_key)) = kind else {
        // A suspension received while the work is on hold here: its responsible is told (D5).
        if SUSPENDED.contains(&to) && work.status == "on_hold" {
            let reason = suspension_reason(conn, entity_sync_id).await?;
            crate::settings::received_notice::announce_work_held_by_suspension_of(
                conn,
                permit_id,
                entity_sync_id,
                &permit_code,
                &work.code,
                reason.as_deref().unwrap_or("?"),
            )
            .await;
        }
        return Ok(());
    };
    let shown = |t: &Option<String>| t.clone().unwrap_or_else(|| "unknown".to_string());
    let detail = if code == codes::ENDED_BEFORE_START {
        format!(
            "permit {permit_code} received as '{to}' (ended {}) before work order {} started here ({}): applied, to review",
            shown(&end),
            work.code,
            shown(&work.last_start)
        )
    } else {
        format!(
            "permit {permit_code} received as '{to}' (ended {}) while work order {} runs here (since {}): applied, to review",
            shown(&end),
            work.code,
            shown(&work.last_start)
        )
    };
    crate::sync::queries::record_sync_conflict(
        conn,
        crate::sync::queries::NewSyncConflict {
            source_scope: "inbound",
            source_batch_id: None,
            linked_outbox_id: None,
            // Not linked to its inbox row: the row is applied, a choice on the review must not mark it
            // otherwise.
            linked_inbox_id: None,
            entity_type: "work_permits".to_string(),
            entity_sync_id: entity_sync_id.to_string(),
            operation: "upsert".to_string(),
            conflict_type: code.to_string(),
            local_payload_json: Some(serde_json::to_string(&local)?),
            inbound_payload_json: Some(serde_json::to_string(received)?),
            checkpoint_token: None,
            local_row_version: None,
            inbound_row_version: received_version(conn, entity_sync_id).await?,
            detail_message: Some(detail),
        },
    )
    .await?;
    tracing::warn!(
        permit = %permit_code,
        work_order = %work.code,
        work_order_id = work.id,
        code,
        "permit ended elsewhere under work held here: applied and raised for review"
    );
    crate::settings::received_notice::announce_permit_ended_under_work(
        conn,
        &crate::settings::received_notice::PermitEndedNotice {
            permit_id,
            permit_sync_id: entity_sync_id,
            permit_code: &permit_code,
            wo_code: &work.code,
            key: notice_key,
            status: to,
            end_at: end_at.and(end.as_deref()),
            until: None,
            start_at: work.last_start.as_deref(),
        },
    )
    .await;
    Ok(())
}

/// The reason of the permit's open suspension (received with it, or held here).
async fn suspension_reason(conn: &impl ConnectionTrait, sync_id: &str) -> AppResult<Option<String>> {
    let suspensions = permit_children(conn, "permit_suspensions", sync_id).await?;
    Ok(payloads(&suspensions)
        .into_iter()
        .filter(|s| field(s, "reinstated_at").is_none())
        .filter_map(|s| {
            field(s, "suspended_at")
                .and_then(instant)
                .map(|i| (i, field(s, "reason")))
        })
        .max_by_key(|(i, _)| *i)
        .and_then(|(_, r)| r.map(str::to_string)))
}

/// A received suspension of a permit, new here and already lifted (suspended and reinstated on
/// another PC before this PC received either), before it is written (same transaction): when work
/// of the permit's work order recorded here overlaps its window, the work ran under a suspended
/// permit. A start, resume or reopening inside the window: [`codes::ENDED_BEFORE_START`]; work
/// already running when the window began (its last status change before is a start, or, with no
/// change recorded, its actual start is before): [`codes::ENDED_DURING_WORK`]. The conflict is
/// recorded on the suspension row (one review per window) and the same people are told. The permit
/// is active again: nothing is held (CMMS Advisor answer to S3; review round 2, G3 and G4).
pub(crate) async fn after_suspension_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<()> {
    // Known here already: this PC saw the permit suspended, the permit's own arrival judged it.
    if local_wire(conn, "permit_suspensions", entity_sync_id).await?.is_some() {
        return Ok(());
    }
    let (Some(from), Some(until)) = (field(received, "suspended_at"), field(received, "reinstated_at")) else {
        return Ok(());
    };
    let (Some(from_at), Some(until_at)) = (instant(from), instant(until)) else {
        return Ok(());
    };
    let Some(permit_sync) = field(received, "permit_sync_id") else {
        return Ok(());
    };
    let Some(permit) = local_wire(conn, "work_permits", permit_sync).await? else {
        return Ok(());
    };
    let Some(permit_id) = local_id(conn, "work_permits", permit_sync).await? else {
        return Ok(());
    };
    let Some(wo_sync) = field(&permit, "linked_work_order_sync_id") else {
        return Ok(());
    };
    let Some(wo_id) = local_id(conn, "work_orders", wo_sync).await? else {
        return Ok(());
    };
    // Every status change of the work order recorded here, in order: a start inside the window, or
    // work already running when the window began (its last change before is a start).
    let changes = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.acted_at, t.to_status FROM wo_state_transition_log t WHERE t.wo_id = ? \
             ORDER BY t.acted_at, t.id",
            [wo_id.into()],
        ))
        .await?;
    let mut started_inside: Option<String> = None;
    let mut last_before: Option<(String, String)> = None;
    let mut any_change = false;
    for row in &changes {
        let acted: String = row.try_get("", "acted_at").map_err(sync_err)?;
        let to: String = row.try_get("", "to_status").map_err(sync_err)?;
        let Some(at) = instant(&acted) else {
            continue;
        };
        any_change = true;
        if at <= from_at {
            last_before = Some((acted, to));
        } else if at < until_at && to == "in_progress" && started_inside.is_none() {
            started_inside = Some(acted);
        }
    }
    let running_at_start = match last_before {
        Some((acted, to)) if to == "in_progress" => Some(acted),
        Some(_) => None,
        // No change recorded before the window: its actual start, when nothing else is recorded.
        None if !any_change => field(
            &local_wire(conn, "work_orders", wo_sync).await?.unwrap_or_default(),
            "actual_start",
        )
        .filter(|s| instant(s).is_some_and(|s| s <= from_at))
        .map(str::to_string),
        None => None,
    };
    let (code, notice_key, start, how) = match (started_inside, running_at_start) {
        (Some(start), _) => (
            codes::ENDED_BEFORE_START,
            codes::NOTICE_SUSPENDED_AROUND_START,
            start,
            "started here inside that window",
        ),
        (None, Some(since)) => (
            codes::ENDED_DURING_WORK,
            codes::NOTICE_SUSPENDED_DURING_WORK,
            since,
            "was running here when that window began",
        ),
        (None, None) => return Ok(()),
    };
    let wo_code = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "code"))
        .transpose()
        .map_err(sync_err)?
        .flatten()
        .unwrap_or_default();
    let permit_code = field(&permit, "code").unwrap_or(permit_sync).to_string();
    crate::sync::queries::record_sync_conflict(
        conn,
        crate::sync::queries::NewSyncConflict {
            source_scope: "inbound",
            source_batch_id: None,
            linked_outbox_id: None,
            // Not linked to its inbox row: the row is applied. Recorded on the suspension itself, so
            // each window has its own review (a later window never merges into an earlier one, open
            // or dismissed); the permit is the record shown.
            linked_inbox_id: None,
            entity_type: "permit_suspensions".to_string(),
            entity_sync_id: entity_sync_id.to_string(),
            operation: "upsert".to_string(),
            conflict_type: code.to_string(),
            local_payload_json: Some(serde_json::to_string(&permit)?),
            inbound_payload_json: Some(serde_json::to_string(received)?),
            checkpoint_token: None,
            local_row_version: None,
            inbound_row_version: None,
            detail_message: Some(format!(
                "permit {permit_code} was suspended on another PC from {from} to {until} (suspension {entity_sync_id}); \
                 work order {wo_code} {how} ({start}): applied, to review"
            )),
        },
    )
    .await?;
    tracing::warn!(
        permit = %permit_code,
        work_order = %wo_code,
        work_order_id = wo_id,
        code,
        "work here under a suspension made and lifted elsewhere: raised for review"
    );
    crate::settings::received_notice::announce_permit_ended_under_work(
        conn,
        &crate::settings::received_notice::PermitEndedNotice {
            permit_id,
            permit_sync_id: permit_sync,
            permit_code: &permit_code,
            wo_code: &wo_code,
            key: notice_key,
            status: "suspended",
            end_at: Some(from),
            until: Some(until),
            start_at: Some(&start),
        },
    )
    .await;
    Ok(())
}
