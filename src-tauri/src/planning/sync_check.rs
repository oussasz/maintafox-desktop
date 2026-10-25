//! Planning rows received by synchronisation are judged again before they are written
//! (`sync::apply_hooks::before_upsert_checked`), with the rules of this PC (audit findings AU-06,
//! AU-102, S4; owner decision D6; CMMS Advisor answers Q1-Q4):
//!
//! - `schedule_commitments`: work entering a week frozen here (new here, or moved: slot or crew
//!   changed), or a commitment frozen here moved anywhere, comes with its break-in: a matching
//!   break-in (same commitment, same new slot) already here, or waiting in the inbox (then applied
//!   with it in one transaction, [`commitment_group`], and judged too). Without one it is a
//!   `PLANNING_WEEK_FROZEN` conflict at once, never an open wait: the sender's clock is never
//!   trusted to say the change came before the freeze (the conflict shows both times). A matching
//!   valid break-in that arrives later closes that conflict, audited
//!   ([`retry_frozen_week_conflicts`]). A commitment received un-frozen while its week is still
//!   frozen here waits only while the week's reopening is in the inbox; otherwise it is the same
//!   conflict.
//! - `schedule_break_ins`: an urgent break-in (or one with an unknown reason here), or one that
//!   forced a blocking check, names an approver; a named approver is never its requester, signed
//!   with a password (`approver_signed_at`) and holds plan.confirm at the crew's site here, now
//!   (waiting only while rows that could give that right are in the inbox). The password itself
//!   cannot be checked again here (docs/KNOWN_LIMITS.md). Break-ins recorded by older versions
//!   (named only, self-approved) are conflicts for review.
//! - `weekly_schedules`: a week reopened (frozen or published back to approved or draft, same
//!   baseline) comes with its `unfrozen` decision, here or in the inbox, decided by someone other
//!   than the requester who holds plan.confirm at the site here, now.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};
use crate::permit::sync_check::{conflict, field, local_wire, overlay, wait, Payload};
use crate::settings::site::InboundVerdict;

use super::break_in_approval::{break_in_approvers, team_planning_site};

/// Work entering a frozen week without its break-in (same code as the local refusal).
pub(crate) const WEEK_FROZEN: &str = "PLANNING_WEEK_FROZEN";
/// An urgent or forcing break-in that names no approver.
pub(crate) const BREAK_IN_APPROVAL_REQUIRED: &str = "PLANNING_BREAK_IN_APPROVAL_REQUIRED";
/// A break-in approved by its requester (or whose requester is not recorded).
pub(crate) const BREAK_IN_SELF_APPROVAL: &str = "PLANNING_BREAK_IN_SELF_APPROVAL";
/// A break-in approved by someone without plan.confirm at the crew's site here.
pub(crate) const BREAK_IN_APPROVER_UNAUTHORISED: &str = "PLANNING_BREAK_IN_APPROVER_UNAUTHORISED";
/// A break-in naming an approver who did not sign with a password (older versions).
pub(crate) const BREAK_IN_UNSIGNED: &str = "PLANNING_BREAK_IN_UNSIGNED";
/// A week reopened without its `unfrozen` decision.
pub(crate) const WEEK_UNFREEZE_UNDECIDED: &str = "WEEK_UNFREEZE_UNDECIDED";
/// A week reopened by a decision of its own requester (or with either person unrecorded).
pub(crate) const WEEK_UNFREEZE_SAME_PERSON: &str = "WEEK_UNFREEZE_SAME_PERSON";
/// A week reopened by a decider without plan.confirm at the site here.
pub(crate) const WEEK_UNFREEZE_UNAUTHORISED: &str = "WEEK_UNFREEZE_UNAUTHORISED";

/// Inbox rows still to be applied (not a delete).
const WAITING: &str = "apply_status IN ('pending', 'deferred') AND operation NOT IN ('delete', 'archive')";

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("planning re-check of a received row: {e}"))
}

fn key(column: &str) -> String {
    crate::sync::row_layout::ref_payload_key(column)
}

/// Local id of a row named by its sync id; `None` when not here.
async fn local_id(conn: &impl ConnectionTrait, table: &str, sync_id: &str) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT id FROM \"{table}\" WHERE entity_sync_id = ?"),
        [sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "id").map_err(sync_err))
    .transpose()
}

fn int(p: &Payload, k: &str) -> Option<i64> {
    match p.get(k) {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The slot and crew of a commitment as the payload states them.
fn slot_of(p: &Payload) -> (Option<String>, Option<String>, Option<String>) {
    (
        field(p, "committed_start").map(str::to_string),
        field(p, "committed_end").map(str::to_string),
        field(p, &key("assigned_team_id")).map(str::to_string),
    )
}

/// A break-in of the commitment `sid` to the slot `[start, end)` applied here.
async fn break_in_applied_here(conn: &impl ConnectionTrait, sid: &str, start: &str, end: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS m FROM schedule_break_ins sb JOIN schedule_commitments c ON c.id = sb.schedule_commitment_id \
              WHERE c.entity_sync_id = ? AND julianday(sb.new_slot_start) = julianday(?) \
                AND julianday(sb.new_slot_end) = julianday(?) LIMIT 1",
            [sid.into(), start.into(), end.into()],
        ))
        .await?
        .is_some())
}

/// Inbox ids of the break-ins of the commitment `sid` to the slot `[start, end)` still waiting.
async fn break_ins_waiting(conn: &impl ConnectionTrait, sid: &str, start: &str, end: &str) -> AppResult<Vec<i64>> {
    let commitment_key = key("schedule_commitment_id");
    conn.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "SELECT id FROM sync_inbox WHERE entity_type = 'schedule_break_ins' AND {WAITING} \
               AND json_extract(payload_json, '$.{commitment_key}') = ? \
               AND julianday(json_extract(payload_json, '$.new_slot_start')) = julianday(?) \
               AND julianday(json_extract(payload_json, '$.new_slot_end')) = julianday(?) \
             ORDER BY id"
        ),
        [sid.into(), start.into(), end.into()],
    ))
    .await?
    .iter()
    .map(|r| r.try_get::<i64>("", "id").map_err(sync_err))
    .collect()
}

/// The waiting break-ins of `sid` to `[start, end)` that can be applied with it now
/// (`permit::sync_check::children`: admissible, nothing else missing), and why a matching one
/// cannot be counted yet.
async fn matching_children(
    conn: &impl ConnectionTrait,
    sid: &str,
    start: &str,
    end: &str,
) -> AppResult<(Vec<crate::permit::sync_check::GroupRow>, Option<String>)> {
    let matching = break_ins_waiting(conn, sid, start, end).await?;
    if matching.is_empty() {
        return Ok((Vec::new(), None));
    }
    let children = crate::permit::sync_check::children(
        conn,
        "schedule_break_ins",
        "schedule_commitments",
        "schedule_commitment_id",
        &key("schedule_commitment_id"),
        sid,
    )
    .await?;
    let counted: Vec<_> = children
        .counted
        .into_iter()
        .filter(|row| matching.contains(&row.inbox_id()))
        .collect();
    let blocked = counted.is_empty().then(|| {
        children
            .blocked
            .unwrap_or_else(|| format!("the break-in of commitment {sid} received with it cannot be applied yet"))
    });
    Ok((counted, blocked))
}

/// A reopening of the week starting `week_start` (draft or approved again) waiting in the inbox.
async fn reopening_waiting(conn: &impl ConnectionTrait, week_start: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS w FROM sync_inbox WHERE entity_type = 'weekly_schedules' AND {WAITING} \
                   AND json_extract(payload_json, '$.week_start') = ? \
                   AND json_extract(payload_json, '$.status') IN ('draft', 'approved') LIMIT 1"
            ),
            [week_start.into()],
        ))
        .await?
        .is_some())
}

/// See the module documentation (`schedule_commitments`).
pub(crate) async fn inbound_commitment_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "schedule_commitments", entity_sync_id).await?;
    let merged = overlay(local.as_ref(), received);
    // Released or cancelled: leaving the schedule is not entering a week.
    if field(&merged, "status") != Some("committed") {
        return Ok(None);
    }
    let (Some(start), Some(end), team_sync) = slot_of(&merged) else {
        return Ok(None);
    };
    let moved = local.as_ref().is_none_or(|l| slot_of(l) != slot_of(&merged));
    let frozen_here = local.as_ref().and_then(|l| field(l, "frozen_at")).is_some();
    let unfrozen = frozen_here && field(&merged, "frozen_at").is_none();
    if !moved && !unfrozen {
        return Ok(None);
    }
    let frozen_week = match team_sync.as_deref() {
        Some(team_sync) => {
            let Some(team) = local_id(conn, "teams", team_sync).await? else {
                return wait(format!("crew {team_sync} is not applied yet"));
            };
            super::weekly_schedule::frozen_week_for_slot(conn, team, &start, &end, Utc::now()).await?
        }
        None => None,
    };
    let sent_at = field(received, "updated_at")
        .or_else(|| field(received, "created_at"))
        .unwrap_or("not recorded")
        .to_string();
    if moved && (frozen_week.is_some() || frozen_here) {
        if break_in_applied_here(conn, entity_sync_id, &start, &end).await? {
            return Ok(None);
        }
        let (counted, blocked) = matching_children(conn, entity_sync_id, &start, &end).await?;
        if !counted.is_empty() {
            // Applied with it, and judged (`commitment_group`).
            return Ok(None);
        }
        if let Some(blocked) = blocked {
            // Its break-in is in the inbox but needs another row first: a bounded wait.
            return wait(blocked);
        }
        let (week, frozen_at) = match &frozen_week {
            Some(w) => (w.week_start.clone(), w.frozen_at.clone().unwrap_or_else(|| "not recorded".into())),
            None => (
                "of its current slot".to_string(),
                local
                    .as_ref()
                    .and_then(|l| field(l, "frozen_at"))
                    .unwrap_or("not recorded")
                    .to_string(),
            ),
        };
        return conflict(
            WEEK_FROZEN,
            format!(
                "commitment {entity_sync_id} enters the frozen week {week} ({start} -> {end}) without its break-in; \
                 changed on its PC at {sent_at} (that PC's clock), week frozen here at {frozen_at}"
            ),
        );
    }
    let Some(week) = frozen_week.filter(|_| unfrozen) else {
        return Ok(None);
    };
    if reopening_waiting(conn, &week.week_start).await? {
        return wait(format!("the reopening of the week {} is still in the inbox", week.week_start));
    }
    conflict(
        WEEK_FROZEN,
        format!(
            "commitment {entity_sync_id} received un-frozen while its week {} is still frozen here (frozen at {}); \
             changed on its PC at {sent_at} (that PC's clock)",
            week.week_start,
            week.frozen_at.as_deref().unwrap_or("not recorded")
        ),
    )
}

/// The waiting break-ins of a received commitment that match its slot, applied right after it in
/// its transaction (a break-in refused by its own check refuses the commitment too).
pub(crate) async fn commitment_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<crate::permit::sync_check::Group>> {
    let local = local_wire(conn, "schedule_commitments", entity_sync_id).await?;
    let merged = overlay(local.as_ref(), received);
    let (Some(start), Some(end), _) = slot_of(&merged) else {
        return Ok(None);
    };
    let (counted, _) = matching_children(conn, entity_sync_id, &start, &end).await?;
    if counted.is_empty() {
        return Ok(None);
    }
    Ok(Some(crate::permit::sync_check::Group::new(
        entity_sync_id.to_string(),
        local.as_ref().and_then(|l| field(l, "status")).map(str::to_string),
        counted,
        true,
        None,
    )))
}

/// See the module documentation (`schedule_break_ins`).
pub(crate) async fn inbound_break_in_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    // Append-only: a break-in held here is never changed.
    if local_id(conn, "schedule_break_ins", entity_sync_id).await?.is_some() {
        return Ok(None);
    }
    let commitment_key = key("schedule_commitment_id");
    let Some(commitment_sync) = field(received, &commitment_key) else {
        return Ok(None);
    };
    let team: Option<i64> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT assigned_team_id FROM schedule_commitments WHERE entity_sync_id = ?",
            [commitment_sync.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<i64>>("", "assigned_team_id").map_err(sync_err))
        .transpose()?
        .flatten();
    if local_id(conn, "schedule_commitments", commitment_sync).await?.is_none() {
        return wait(format!("commitment {commitment_sync} is not applied yet"));
    }
    // A reason this PC does not know is judged as urgent (fail closed).
    let reason = field(received, "break_in_reason").unwrap_or_default();
    let urgent = super::break_in_reasons::find_break_in_reason(conn, reason)
        .await?
        .is_none_or(|r| r.urgent);
    // An unreadable or unknown forced check counts as forced.
    let forced = match field(received, "forced_checks_json") {
        None => false,
        Some(json) => serde_json::from_str::<Vec<String>>(json).map_or(true, |codes| !codes.is_empty()),
    };
    let approver_sync = field(received, &key("approved_by_user_id"));
    let requester_sync = field(received, &key("created_by_id"));
    let Some(approver_sync) = approver_sync else {
        if urgent || forced {
            return conflict(
                BREAK_IN_APPROVAL_REQUIRED,
                format!(
                    "break-in {entity_sync_id} ({}{}) names no approver: an urgent or forcing break-in is signed \
                     by a second planner",
                    if urgent { "urgent reason" } else { "reason" },
                    if forced { ", blocking checks forced" } else { "" }
                ),
            );
        }
        return Ok(None);
    };
    if requester_sync.is_none_or(|r| r == approver_sync) {
        return conflict(
            BREAK_IN_SELF_APPROVAL,
            format!("break-in {entity_sync_id} is approved by its own requester (or its requester is not recorded)"),
        );
    }
    if field(received, "approver_signed_at").is_none() {
        return conflict(
            BREAK_IN_UNSIGNED,
            format!(
                "break-in {entity_sync_id} names an approver who did not sign it with their password (recorded by an \
                 older version)"
            ),
        );
    }
    let Some(approver) = local_id(conn, "user_accounts", approver_sync).await? else {
        return wait(format!("approver {approver_sync} is not applied yet"));
    };
    let site = match team {
        Some(team) => team_planning_site(conn, team).await?,
        None => None,
    };
    if !break_in_approvers(conn, site).await?.contains(&approver) {
        if crate::permit::sync_check::rights_may_still_arrive(
            conn,
            approver_sync,
            crate::rbac::permissions::PLAN_CONFIRM,
        )
        .await?
        {
            return wait(format!("the rights of approver {approver_sync} are still arriving"));
        }
        return conflict(
            BREAK_IN_APPROVER_UNAUTHORISED,
            format!(
                "break-in {entity_sync_id} is approved by someone who does not hold plan.confirm at the crew's site here"
            ),
        );
    }
    Ok(None)
}

/// The `unfrozen` decision of a week: requester and decider (sync ids), when found.
struct UnfreezeDecision {
    requester: Option<String>,
    decider: Option<String>,
}

/// See the module documentation (`weekly_schedules`). Called after the lifecycle check
/// (`weekly_schedule::inbound_lifecycle_conflict`) found nothing.
pub(crate) async fn inbound_unfreeze_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let (Some(status), Some(week_start)) = (field(received, "status"), field(received, "week_start")) else {
        return Ok(None);
    };
    let Some(baseline_no) = int(received, "baseline_no") else {
        return Ok(None);
    };
    if !matches!(status, "draft" | "approved") || baseline_no < 1 {
        return Ok(None);
    }
    let site_sync = field(received, &key("entity_id"));
    let Some(local) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_id, entity_sync_id, status, baseline_no FROM weekly_schedules \
              WHERE entity_sync_id = ? \
                 OR (week_start = ? AND entity_id = (SELECT id FROM org_nodes WHERE entity_sync_id = ?)) \
              ORDER BY (entity_sync_id = ?) DESC LIMIT 1",
            [
                entity_sync_id.into(),
                week_start.into(),
                site_sync.map(str::to_string).into(),
                entity_sync_id.into(),
            ],
        ))
        .await?
    else {
        return Ok(None);
    };
    let local_status: String = local.try_get("", "status").map_err(sync_err)?;
    let local_baseline: i64 = local.try_get("", "baseline_no").map_err(sync_err)?;
    if !matches!(local_status.as_str(), "frozen" | "published") || local_baseline != baseline_no {
        return Ok(None);
    }
    let week_id: i64 = local.try_get("", "id").map_err(sync_err)?;
    let site: i64 = local.try_get("", "entity_id").map_err(sync_err)?;
    let local_sid: Option<String> = local.try_get("", "entity_sync_id").map_err(sync_err)?;
    let Some(decision) = unfreeze_decision(conn, week_id, entity_sync_id, local_sid.as_deref(), baseline_no).await?
    else {
        return conflict(
            WEEK_UNFREEZE_UNDECIDED,
            format!(
                "week of {week_start}: received reopened ('{status}', baseline {baseline_no}) without its unfreeze \
                 decision, here or received with it"
            ),
        );
    };
    let (Some(requester), Some(decider)) = (decision.requester, decision.decider) else {
        return conflict(
            WEEK_UNFREEZE_SAME_PERSON,
            format!("week of {week_start}: its unfreeze decision does not name both the requester and the decider"),
        );
    };
    if requester == decider {
        return conflict(
            WEEK_UNFREEZE_SAME_PERSON,
            format!("week of {week_start}: unfrozen by a decision of its own requester"),
        );
    }
    let Some(decider_id) = local_id(conn, "user_accounts", &decider).await? else {
        return wait(format!("decider {decider} is not applied yet"));
    };
    if !break_in_approvers(conn, Some(site)).await?.contains(&decider_id) {
        if crate::permit::sync_check::rights_may_still_arrive(conn, &decider, crate::rbac::permissions::PLAN_CONFIRM)
            .await?
        {
            return wait(format!("the rights of decider {decider} are still arriving"));
        }
        return conflict(
            WEEK_UNFREEZE_UNAUTHORISED,
            format!("week of {week_start}: unfrozen by someone who does not hold plan.confirm at the site here"),
        );
    }
    Ok(None)
}

/// The `unfrozen` event of baseline `baseline_no` of the week: here, else waiting in the inbox.
async fn unfreeze_decision(
    conn: &impl ConnectionTrait,
    week_id: i64,
    received_sid: &str,
    local_sid: Option<&str>,
    baseline_no: i64,
) -> AppResult<Option<UnfreezeDecision>> {
    let here = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.entity_sync_id AS requester, d.entity_sync_id AS decider FROM weekly_schedule_events e \
               LEFT JOIN user_accounts a ON a.id = e.actor_id LEFT JOIN user_accounts d ON d.id = e.approver_id \
              WHERE e.weekly_schedule_id = ? AND e.action = 'unfrozen' AND e.baseline_no = ? \
              ORDER BY e.id DESC LIMIT 1",
            [week_id.into(), baseline_no.into()],
        ))
        .await?;
    if let Some(r) = here {
        return Ok(Some(UnfreezeDecision {
            requester: r.try_get("", "requester").map_err(sync_err)?,
            decider: r.try_get("", "decider").map_err(sync_err)?,
        }));
    }
    let week_key = key("weekly_schedule_id");
    let (actor_key, approver_key) = (key("actor_id"), key("approver_id"));
    let waiting = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT json_extract(payload_json, '$.{actor_key}') AS requester, \
                        json_extract(payload_json, '$.{approver_key}') AS decider \
                   FROM sync_inbox WHERE entity_type = 'weekly_schedule_events' AND {WAITING} \
                    AND json_extract(payload_json, '$.{week_key}') IN (?, ?) \
                    AND json_extract(payload_json, '$.action') = 'unfrozen' \
                    AND CAST(json_extract(payload_json, '$.baseline_no') AS INTEGER) = ? \
                  ORDER BY id DESC LIMIT 1"
            ),
            [
                received_sid.into(),
                local_sid.unwrap_or(received_sid).into(),
                baseline_no.into(),
            ],
        ))
        .await?;
    waiting
        .map(|r| {
            Ok(UnfreezeDecision {
                requester: r.try_get::<Option<String>>("", "requester").map_err(sync_err)?,
                decider: r.try_get::<Option<String>>("", "decider").map_err(sync_err)?,
            })
        })
        .transpose()
}

/// Audit action of a `PLANNING_WEEK_FROZEN` conflict closed by its break-in received later.
pub(crate) const WEEK_FROZEN_CLOSED_EVENT: &str = "planning.week_frozen_conflict.closed_by_break_in";

/// Commitments refused as `PLANNING_WEEK_FROZEN` whose matching break-in has arrived since (here, or
/// waiting in the inbox): each is applied again, with its break-in, which is judged too. Applied:
/// its conflict is closed (`resolved_remote`, `break_in_received`) and audited. Still not
/// applicable: the conflict stays open with the new reason. Returns how many applied.
pub(crate) async fn retry_frozen_week_conflicts(db: &DatabaseConnection) -> AppResult<i64> {
    use crate::sync::materialize::ApplyOutcome;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT i.id AS inbox_id, c.id AS conflict_id, i.entity_sync_id AS sid, i.payload_json AS payload \
               FROM sync_inbox i JOIN sync_conflicts c ON c.linked_inbox_id = i.id \
              WHERE i.entity_type = 'schedule_commitments' AND i.apply_status = 'conflict' \
                AND i.rejection_code = ? AND c.conflict_type = ? \
                AND c.status IN ('new', 'triaged', 'escalated') \
              ORDER BY i.id",
            [WEEK_FROZEN.into(), WEEK_FROZEN.into()],
        ))
        .await?;
    let mut applied = 0;
    for row in rows {
        let inbox_id: i64 = row.try_get("", "inbox_id").map_err(sync_err)?;
        let conflict_id: i64 = row.try_get("", "conflict_id").map_err(sync_err)?;
        let sid: String = row.try_get("", "sid").map_err(sync_err)?;
        let payload: String = row.try_get("", "payload").map_err(sync_err)?;
        let Ok(Value::Object(received)) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        let local = local_wire(db, "schedule_commitments", &sid).await?;
        let merged = overlay(local.as_ref(), &received);
        let (Some(start), Some(end), _) = slot_of(&merged) else {
            continue;
        };
        if !break_in_applied_here(db, &sid, &start, &end).await?
            && break_ins_waiting(db, &sid, &start, &end).await?.is_empty()
        {
            continue;
        }
        let Some(item) = crate::sync::materialize::load_inbox_row(db, inbox_id).await? else {
            continue;
        };
        let tx = db.begin().await?;
        crate::sync::capture::drain_capture_journal(&tx).await?;
        let outcome = crate::sync::materialize::apply_inbox_row_in(&tx, &item).await;
        crate::sync::row_codec::clear_ref_cache();
        let still = match outcome {
            Ok(ApplyOutcome::Applied | ApplyOutcome::Unchanged) => {
                tx.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE sync_conflicts \
                        SET status = 'resolved_remote', resolution_action = 'break_in_received', \
                            resolution_note = 'Its break-in was received and judged valid: the commitment is applied with it.', \
                            resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1, \
                            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                      WHERE id = ?",
                    [conflict_id.into()],
                ))
                .await?;
                tx.commit().await?;
                applied += 1;
                audit_closed(db, conflict_id, &sid).await;
                continue;
            }
            // Waits for a row of its break-in: tried again at the next synchronisation.
            Ok(ApplyOutcome::Deferred { .. }) => {
                tx.rollback().await?;
                continue;
            }
            Ok(ApplyOutcome::Conflict { code, message }) => format!("{code}: {message}"),
            Err(e) => e.to_string(),
        };
        tx.rollback().await?;
        // The break-in received for it does not let it in: the conflict stays open, with why.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sync_conflicts SET detail_message = ?, row_version = row_version + 1, \
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
              WHERE id = ? AND COALESCE(detail_message, '') <> ?",
            [still.clone().into(), conflict_id.into(), still.into()],
        ))
        .await?;
    }
    Ok(applied)
}

async fn audit_closed(db: &DatabaseConnection, conflict_id: i64, commitment_sync_id: &str) {
    let commitment = match local_id(db, "schedule_commitments", commitment_sync_id).await {
        Ok(Some(id)) => id.to_string(),
        _ => return,
    };
    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type: WEEK_FROZEN_CLOSED_EVENT,
            actor_id: None,
            entity_type: Some("schedule_commitment"),
            entity_id: Some(&commitment),
            summary: "Frozen-week conflict closed: the commitment's break-in was received",
            detail_json: Some(
                json!({ "sync_conflict_id": conflict_id, "commitment_sync_id": commitment_sync_id }).to_string(),
            ),
            ..Default::default()
        },
    )
    .await;
}
