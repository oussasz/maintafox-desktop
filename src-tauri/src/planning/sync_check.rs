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

use super::break_in_approval::{break_in_approvers, holds_at_crew_sites, team_planning_site};

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

/// Code of a received planning row too malformed to be judged (no slot, no commitment).
pub(crate) const INVALID_VALUE: &str = "INVALID_VALUE";

/// The columns a break-in held here may still receive: the refusal marks a planner set on a PC
/// that received it (CMMS Advisor decision 2026-10-25). Nothing else of a break-in ever changes.
pub(crate) const REFUSAL_COLUMNS: &[&str] = &["sync_refused_at", "sync_refused_reason"];

/// A break-in that would let commitment `sid` in at `[start, end)`: its new slot is that slot and,
/// when the commitment is held here at `from`, its old slot is the slot held here (the move it
/// describes, review S1: an older break-in to the same slot is never replayed). A break-in refused
/// on a PC (`sync_refused_at`) never counts.
struct Target<'a> {
    sid: &'a str,
    start: &'a str,
    end: &'a str,
    /// The commitment's slot here (`None`: new here).
    from: Option<(&'a str, &'a str)>,
}

/// The break-ins applied here that match `t` (their payloads, as this PC would send them).
async fn matching_here(conn: &impl ConnectionTrait, t: &Target<'_>) -> AppResult<Vec<(String, Payload)>> {
    let mut sql = "SELECT sb.entity_sync_id AS sid FROM schedule_break_ins sb \
                     JOIN schedule_commitments c ON c.id = sb.schedule_commitment_id \
                    WHERE c.entity_sync_id = ? AND sb.sync_refused_at IS NULL \
                      AND julianday(sb.new_slot_start) = julianday(?) AND julianday(sb.new_slot_end) = julianday(?)"
        .to_string();
    let mut values: Vec<sea_orm::Value> = vec![t.sid.into(), t.start.into(), t.end.into()];
    if let Some((from_start, from_end)) = t.from {
        sql.push_str(
            " AND julianday(sb.old_slot_start) = julianday(?) AND julianday(sb.old_slot_end) = julianday(?)",
        );
        values.extend([from_start.into(), from_end.into()]);
    }
    sql.push_str(" ORDER BY sb.id DESC");
    let mut out = Vec::new();
    for r in conn
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?
    {
        let Some(sid) = r.try_get::<Option<String>>("", "sid").map_err(sync_err)? else {
            continue;
        };
        if let Some(p) = local_wire(conn, "schedule_break_ins", &sid).await? {
            out.push((sid, p));
        }
    }
    Ok(out)
}

/// The break-ins waiting in the inbox that match `t`: inbox id, sync id and payload.
async fn matching_waiting(conn: &impl ConnectionTrait, t: &Target<'_>) -> AppResult<Vec<(i64, String, Payload)>> {
    let commitment_key = key("schedule_commitment_id");
    let mut sql = format!(
        "SELECT id, entity_sync_id AS sid, payload_json FROM sync_inbox \
          WHERE entity_type = 'schedule_break_ins' AND {WAITING} \
            AND json_extract(payload_json, '$.{commitment_key}') = ? \
            AND json_extract(payload_json, '$.sync_refused_at') IS NULL \
            AND julianday(json_extract(payload_json, '$.new_slot_start')) = julianday(?) \
            AND julianday(json_extract(payload_json, '$.new_slot_end')) = julianday(?)"
    );
    let mut values: Vec<sea_orm::Value> = vec![t.sid.into(), t.start.into(), t.end.into()];
    if let Some((from_start, from_end)) = t.from {
        sql.push_str(
            " AND julianday(json_extract(payload_json, '$.old_slot_start')) = julianday(?) \
              AND julianday(json_extract(payload_json, '$.old_slot_end')) = julianday(?)",
        );
        values.extend([from_start.into(), from_end.into()]);
    }
    sql.push_str(" ORDER BY id");
    let mut out = Vec::new();
    for r in conn
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?
    {
        let id: i64 = r.try_get("", "id").map_err(sync_err)?;
        let sid: String = r.try_get("", "sid").map_err(sync_err)?;
        let json: String = r.try_get("", "payload_json").map_err(sync_err)?;
        if let Ok(Value::Object(p)) = serde_json::from_str::<Value>(&json) {
            out.push((id, sid, p));
        }
    }
    Ok(out)
}

/// What the matching break-ins of a commitment say.
enum BreakInSupport {
    /// A valid one is applied here: the commitment may enter.
    Here,
    /// Valid ones wait in the inbox and can be applied with it now (the group's rows).
    Waiting(Vec<crate::permit::sync_check::GroupRow>),
    /// A matching one needs another row first (its approver, crew, rights still arriving).
    Wait(String),
    /// None is valid: the refusal of the first one judged, or `None` when there is none.
    None(Option<String>),
}

/// The matching break-ins of `t`, each judged ([`judge_break_in`]); `fallback_team`: the crew a
/// break-in recorded before 0.9.2 (no crews on it) is judged at.
async fn break_in_support(
    conn: &impl ConnectionTrait,
    t: &Target<'_>,
    fallback_team: Option<i64>,
) -> AppResult<BreakInSupport> {
    let mut refused: Option<String> = None;
    for (sid, p) in matching_here(conn, t).await? {
        match judge_break_in(conn, &sid, &p, fallback_team).await? {
            None => return Ok(BreakInSupport::Here),
            Some(InboundVerdict::Defer(m)) => return Ok(BreakInSupport::Wait(m)),
            Some(InboundVerdict::Conflict { code, message }) => {
                refused.get_or_insert(format!("{code}: {message}"));
            }
        }
    }
    let waiting = matching_waiting(conn, t).await?;
    if waiting.is_empty() {
        return Ok(BreakInSupport::None(refused));
    }
    let children = crate::permit::sync_check::children(
        conn,
        "schedule_break_ins",
        "schedule_commitments",
        "schedule_commitment_id",
        &key("schedule_commitment_id"),
        t.sid,
    )
    .await?;
    let mut valid = Vec::new();
    let mut wait_for: Option<String> = None;
    for (inbox_id, sid, p) in &waiting {
        match judge_break_in(conn, sid, p, fallback_team).await? {
            None => match children.counted.iter().find(|row| row.inbox_id() == *inbox_id) {
                Some(row) => valid.push(row.clone()),
                None => {
                    wait_for.get_or_insert_with(|| {
                        children.blocked.clone().unwrap_or_else(|| {
                            format!("the break-in {sid} of commitment {} cannot be applied yet", t.sid)
                        })
                    });
                }
            },
            Some(InboundVerdict::Defer(m)) => {
                wait_for.get_or_insert(m);
            }
            Some(InboundVerdict::Conflict { code, message }) => {
                refused.get_or_insert(format!("{code}: {message}"));
            }
        }
    }
    Ok(if !valid.is_empty() {
        BreakInSupport::Waiting(valid)
    } else if let Some(m) = wait_for {
        BreakInSupport::Wait(m)
    } else {
        BreakInSupport::None(refused)
    })
}

/// The commitment's slot here, its local crew id and the received crew's local id.
struct CommitmentContext {
    local: Option<Payload>,
    merged: Payload,
    start: String,
    end: String,
    team: Option<i64>,
}

async fn commitment_context(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Result<CommitmentContext, InboundVerdict>> {
    let local = local_wire(conn, "schedule_commitments", entity_sync_id).await?;
    let merged = overlay(local.as_ref(), received);
    let (Some(start), Some(end), team_sync) = slot_of(&merged) else {
        return Ok(Err(InboundVerdict::Conflict {
            code: INVALID_VALUE,
            message: format!("commitment {entity_sync_id} received without its slot"),
        }));
    };
    let team = match team_sync.as_deref() {
        Some(team_sync) => match local_id(conn, "teams", team_sync).await? {
            Some(team) => Some(team),
            None => return Ok(Err(InboundVerdict::Defer(format!("crew {team_sync} is not applied yet")))),
        },
        None => None,
    };
    Ok(Ok(CommitmentContext {
        local,
        merged,
        start,
        end,
        team,
    }))
}

impl CommitmentContext {
    fn target<'a>(&'a self, sid: &'a str) -> Target<'a> {
        Target {
            sid,
            start: &self.start,
            end: &self.end,
            from: self.local.as_ref().and_then(|l| {
                Some((field(l, "committed_start")?, field(l, "committed_end")?))
            }),
        }
    }
}

/// A reopening of the week starting `week_start` of the site `site_sync` (draft or approved again)
/// waiting in the inbox (review N11: the site too).
async fn reopening_waiting(conn: &impl ConnectionTrait, week_start: &str, site: i64) -> AppResult<bool> {
    let site_key = key("entity_id");
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS w FROM sync_inbox WHERE entity_type = 'weekly_schedules' AND {WAITING} \
                   AND json_extract(payload_json, '$.week_start') = ? \
                   AND json_extract(payload_json, '$.{site_key}') = (SELECT entity_sync_id FROM org_nodes WHERE id = ?) \
                   AND json_extract(payload_json, '$.status') IN ('draft', 'approved') LIMIT 1"
            ),
            [week_start.into(), site.into()],
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
    let ctx = match commitment_context(conn, entity_sync_id, received).await? {
        Ok(ctx) => ctx,
        Err(verdict) => return Ok(Some(verdict)),
    };
    // Released or cancelled: leaving the schedule is not entering a week.
    if field(&ctx.merged, "status") != Some("committed") {
        return Ok(None);
    }
    let local = ctx.local.as_ref();
    let moved = local.is_none_or(|l| slot_of(l) != slot_of(&ctx.merged));
    let frozen_here = local.and_then(|l| field(l, "frozen_at")).is_some();
    let unfrozen = frozen_here && field(&ctx.merged, "frozen_at").is_none();
    if !moved && !unfrozen {
        return Ok(None);
    }
    let frozen_week = match ctx.team {
        Some(team) => {
            super::weekly_schedule::frozen_week_for_slot(conn, team, &ctx.start, &ctx.end, Utc::now()).await?
        }
        None => None,
    };
    let sent_at = field(received, "updated_at")
        .or_else(|| field(received, "created_at"))
        .unwrap_or("not recorded")
        .to_string();
    let (start, end) = (&ctx.start, &ctx.end);
    if moved && (frozen_week.is_some() || frozen_here) {
        let refused = match break_in_support(conn, &ctx.target(entity_sync_id), ctx.team).await? {
            // Applied with it, and judged again (`commitment_group`), or applied here and valid.
            BreakInSupport::Here | BreakInSupport::Waiting(_) => return Ok(None),
            // Its break-in is in the inbox but needs another row first: a bounded wait.
            BreakInSupport::Wait(m) => return wait(m),
            BreakInSupport::None(refused) => refused,
        };
        let (week, frozen_at) = match &frozen_week {
            Some(w) => (
                w.week_start.clone(),
                w.frozen_at.clone().unwrap_or_else(|| "not recorded".into()),
            ),
            None => (
                "of its current slot".to_string(),
                local
                    .and_then(|l| field(l, "frozen_at"))
                    .unwrap_or("not recorded")
                    .to_string(),
            ),
        };
        let why = match refused {
            Some(r) => format!("its break-in is refused ({r})"),
            None => "without its break-in".to_string(),
        };
        return conflict(
            WEEK_FROZEN,
            format!(
                "commitment {entity_sync_id} enters the frozen week {week} ({start} -> {end}) {why}; \
                 changed on its PC at {sent_at} (that PC's clock), week frozen here at {frozen_at}"
            ),
        );
    }
    let Some(week) = frozen_week.filter(|_| unfrozen) else {
        return Ok(None);
    };
    let site = match ctx.team {
        Some(team) => team_planning_site(conn, team).await?,
        None => None,
    };
    if let Some(site) = site {
        if reopening_waiting(conn, &week.week_start, site).await? {
            return wait(format!(
                "the reopening of the week {} is still in the inbox",
                week.week_start
            ));
        }
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

/// The waiting, valid break-ins of a received commitment that match its move, applied right after
/// it in its transaction (each is judged again by its own check there).
pub(crate) async fn commitment_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<crate::permit::sync_check::Group>> {
    let Ok(ctx) = commitment_context(conn, entity_sync_id, received).await? else {
        return Ok(None);
    };
    if field(&ctx.merged, "status") != Some("committed") {
        return Ok(None);
    }
    let BreakInSupport::Waiting(rows) = break_in_support(conn, &ctx.target(entity_sync_id), ctx.team).await? else {
        return Ok(None);
    };
    Ok(Some(crate::permit::sync_check::Group::new(
        entity_sync_id.to_string(),
        ctx.local.as_ref().and_then(|l| field(l, "status")).map(str::to_string),
        rows,
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
    // Append-only: a break-in held here only ever receives its refusal marks
    // (`refusal_marks_only`, applied by `sync::apply_hooks::adopt_existing_append_only`); any other
    // rewrite is skipped by the apply.
    if local_id(conn, "schedule_break_ins", entity_sync_id).await?.is_some() {
        return Ok(None);
    }
    // A break-in refused on another PC is kept as history: it never counts as valid
    // (`matching_here` / `matching_waiting`), so it is not judged.
    if field(received, "sync_refused_at").is_some() {
        return Ok(None);
    }
    let commitment_key = key("schedule_commitment_id");
    let Some(commitment_sync) = field(received, &commitment_key) else {
        return conflict(
            INVALID_VALUE,
            format!("break-in {entity_sync_id} received without its commitment"),
        );
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
    judge_break_in(conn, entity_sync_id, received, team).await
}

/// Whether a received break-in differs from the one held here only by refusal marks it did not
/// have here (CMMS Advisor decision 2026-10-25): those marks are applied, nothing else.
pub(crate) async fn refusal_marks_only(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<bool> {
    let Some(local) = local_wire(conn, "schedule_break_ins", entity_sync_id).await? else {
        return Ok(false);
    };
    if field(&local, "sync_refused_at").is_some() || field(received, "sync_refused_at").is_none() {
        return Ok(false);
    }
    // Every other value the received row carries is the one held here.
    Ok(received.iter().all(|(k, v)| {
        REFUSAL_COLUMNS.contains(&k.as_str())
            || matches!(k.as_str(), "row_version" | "updated_at")
            || local.get(k).is_none_or(|held| held == v)
    }))
}

/// The four-eyes rules of a break-in (received, or held here and relied on again): an urgent one
/// (or one whose reason this PC does not know), or one that forced a blocking check, names an
/// approver; a named approver is not the requester, signed with a password, is linked to a
/// personnel record here and holds plan.confirm at the sites of the crews the break-in names
/// (the crew it goes to and the crew it leaves; `fallback_team` for a break-in recorded before
/// 0.9.2 without them), here, now.
pub(crate) async fn judge_break_in(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
    fallback_team: Option<i64>,
) -> AppResult<Option<InboundVerdict>> {
    if field(received, "sync_refused_at").is_some() {
        return conflict(
            BREAK_IN_UNSIGNED,
            format!("break-in {entity_sync_id} was refused on a PC that received it"),
        );
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
    // The approver is a person of the plant here (review N10).
    let linked = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS l FROM user_accounts WHERE id = ? AND personnel_id IS NOT NULL",
            [approver.into()],
        ))
        .await?
        .is_some();
    if !linked {
        return conflict(
            BREAK_IN_APPROVER_UNAUTHORISED,
            format!("break-in {entity_sync_id} is approved by an account linked to no personnel record here"),
        );
    }
    // The crews the break-in names (review S2), else the commitment's crew.
    let mut crews: Vec<i64> = Vec::new();
    for column in ["new_team_id", "old_team_id"] {
        if let Some(team_sync) = field(received, &key(column)) {
            match local_id(conn, "teams", team_sync).await? {
                Some(team) => crews.push(team),
                None => return wait(format!("crew {team_sync} is not applied yet")),
            }
        }
    }
    let allowed = match (crews.first(), fallback_team) {
        (Some(team), _) => holds_at_crew_sites(conn, approver, *team, crews.get(1).copied()).await?,
        (None, Some(team)) => holds_at_crew_sites(conn, approver, team, None).await?,
        (None, None) => break_in_approvers(conn, None).await?.contains(&approver),
    };
    if !allowed {
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
                "break-in {entity_sync_id} is approved by someone who does not hold plan.confirm at the site of each \
                 crew it names, here"
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
///
/// Every reopening between the week held here and the received one needs its decision (review
/// B1): from the baseline frozen here (or the next one, when the week is open here) up to the
/// received baseline when the week is received open again (the one before, when it is received
/// frozen or published again).
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
    let frozen_here = matches!(local_status.as_str(), "frozen" | "published");
    let received_open = matches!(status, "draft" | "approved");
    let first = if frozen_here { local_baseline } else { local_baseline + 1 };
    let last = if received_open { baseline_no } else { baseline_no - 1 };
    let first = first.max(1);
    if first > last {
        return Ok(None);
    }
    let week_id: i64 = local.try_get("", "id").map_err(sync_err)?;
    let site: i64 = local.try_get("", "entity_id").map_err(sync_err)?;
    let local_sid: Option<String> = local.try_get("", "entity_sync_id").map_err(sync_err)?;
    for baseline in first..=last {
        let Some(decision) = unfreeze_decision(conn, week_id, entity_sync_id, local_sid.as_deref(), baseline).await?
        else {
            return conflict(
                WEEK_UNFREEZE_UNDECIDED,
                format!(
                    "week of {week_start}: received '{status}' (baseline {baseline_no}) without the unfreeze decision \
                     of baseline {baseline}, here or received with it"
                ),
            );
        };
        let (Some(requester), Some(decider)) = (decision.requester, decision.decider) else {
            return conflict(
                WEEK_UNFREEZE_SAME_PERSON,
                format!(
                    "week of {week_start}: the unfreeze decision of baseline {baseline} does not name both the \
                     requester and the decider"
                ),
            );
        };
        if requester == decider {
            return conflict(
                WEEK_UNFREEZE_SAME_PERSON,
                format!("week of {week_start}: baseline {baseline} unfrozen by a decision of its own requester"),
            );
        }
        let Some(decider_id) = local_id(conn, "user_accounts", &decider).await? else {
            return wait(format!("decider {decider} is not applied yet"));
        };
        if !break_in_approvers(conn, Some(site)).await?.contains(&decider_id) {
            if crate::permit::sync_check::rights_may_still_arrive(
                conn,
                &decider,
                crate::rbac::permissions::PLAN_CONFIRM,
            )
            .await?
            {
                return wait(format!("the rights of decider {decider} are still arriving"));
            }
            return conflict(
                WEEK_UNFREEZE_UNAUTHORISED,
                format!(
                    "week of {week_start}: baseline {baseline} unfrozen by someone who does not hold plan.confirm at \
                     the site here"
                ),
            );
        }
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
        let Ok(ctx) = commitment_context(db, &sid, &received).await? else {
            continue;
        };
        // Only a break-in of this very move (review S1), not refused anywhere, is worth a retry; the
        // retry judges it again (review S2).
        let target = ctx.target(&sid);
        if matching_here(db, &target).await?.is_empty() && matching_waiting(db, &target).await?.is_empty() {
            continue;
        }
        let closed = reapply_conflicted(
            db,
            inbox_id,
            conflict_id,
            "break_in_received",
            "Its break-in was received and judged valid: the commitment is applied with it.",
        )
        .await?;
        if closed {
            applied += 1;
            audit_closed(
                db,
                WEEK_FROZEN_CLOSED_EVENT,
                "Frozen-week conflict closed: the commitment's break-in was received",
                "schedule_commitment",
                "schedule_commitments",
                conflict_id,
                &sid,
            )
            .await;
        }
    }
    Ok(applied)
}

/// Audit action of a `WEEK_UNFREEZE_UNDECIDED` conflict closed by its decision received later.
pub(crate) const UNFREEZE_CLOSED_EVENT: &str = "planning.week_unfreeze_conflict.closed_by_decision";

/// Weeks refused as `WEEK_UNFREEZE_UNDECIDED` for which an `unfrozen` decision has arrived since
/// (here, or waiting in the inbox; review S4): each is applied again, judged again with every
/// decision it needs. Applied: its conflict is closed (`resolved_remote`, `decision_received`) and
/// audited, and its commitments follow at the next pass. Still not applicable: the conflict stays
/// open with the new reason. Returns how many applied.
pub(crate) async fn retry_unfreeze_conflicts(db: &DatabaseConnection) -> AppResult<i64> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT i.id AS inbox_id, c.id AS conflict_id, i.entity_sync_id AS sid, i.payload_json AS payload \
               FROM sync_inbox i JOIN sync_conflicts c ON c.linked_inbox_id = i.id \
              WHERE i.entity_type = 'weekly_schedules' AND i.apply_status = 'conflict' \
                AND i.rejection_code = ? AND c.conflict_type = ? \
                AND c.status IN ('new', 'triaged', 'escalated') \
              ORDER BY i.id",
            [WEEK_UNFREEZE_UNDECIDED.into(), WEEK_UNFREEZE_UNDECIDED.into()],
        ))
        .await?;
    let week_key = key("weekly_schedule_id");
    let mut applied = 0;
    for row in rows {
        let inbox_id: i64 = row.try_get("", "inbox_id").map_err(sync_err)?;
        let conflict_id: i64 = row.try_get("", "conflict_id").map_err(sync_err)?;
        let sid: String = row.try_get("", "sid").map_err(sync_err)?;
        let payload: String = row.try_get("", "payload").map_err(sync_err)?;
        let Ok(Value::Object(received)) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        // An `unfrozen` decision of this week, here or waiting: worth judging the week again.
        let here = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS d FROM weekly_schedule_events e JOIN weekly_schedules w ON w.id = e.weekly_schedule_id \
                  WHERE e.action = 'unfrozen' AND (w.entity_sync_id = ? \
                     OR (w.week_start = ? AND w.entity_id = (SELECT id FROM org_nodes WHERE entity_sync_id = ?))) \
                  LIMIT 1",
                [
                    sid.clone().into(),
                    field(&received, "week_start").map(str::to_string).into(),
                    field(&received, &key("entity_id")).map(str::to_string).into(),
                ],
            ))
            .await?
            .is_some();
        let waiting = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT 1 AS d FROM sync_inbox WHERE entity_type = 'weekly_schedule_events' AND {WAITING} \
                       AND json_extract(payload_json, '$.{week_key}') = ? \
                       AND json_extract(payload_json, '$.action') = 'unfrozen' LIMIT 1"
                ),
                [sid.clone().into()],
            ))
            .await?
            .is_some();
        if !here && !waiting {
            continue;
        }
        let closed = reapply_conflicted(
            db,
            inbox_id,
            conflict_id,
            "decision_received",
            "Its unfreeze decision was received and judged valid: the reopened week is applied.",
        )
        .await?;
        if closed {
            applied += 1;
            audit_closed(
                db,
                UNFREEZE_CLOSED_EVENT,
                "Week unfreeze conflict closed: its unfreeze decision was received",
                "weekly_schedule",
                "weekly_schedules",
                conflict_id,
                &sid,
            )
            .await;
        }
    }
    Ok(applied)
}

/// Applies a conflicted inbox row again through every check, in one transaction. Applied: its
/// conflict is closed (`resolved_remote`, `action`, `note`) and `true` is returned. Waiting: left
/// as it was, tried again at the next synchronisation. Refused again: the conflict stays open with
/// the new reason.
async fn reapply_conflicted(
    db: &DatabaseConnection,
    inbox_id: i64,
    conflict_id: i64,
    action: &str,
    note: &str,
) -> AppResult<bool> {
    use crate::sync::materialize::ApplyOutcome;
    let Some(item) = crate::sync::materialize::load_inbox_row(db, inbox_id).await? else {
        return Ok(false);
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
                    SET status = 'resolved_remote', resolution_action = ?, resolution_note = ?, \
                        resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1, \
                        updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                  WHERE id = ?",
                [action.into(), note.into(), conflict_id.into()],
            ))
            .await?;
            tx.commit().await?;
            return Ok(true);
        }
        Ok(ApplyOutcome::Deferred { .. }) => {
            tx.rollback().await?;
            return Ok(false);
        }
        Ok(ApplyOutcome::Conflict { code, message }) => format!("{code}: {message}"),
        Err(e) => e.to_string(),
    };
    tx.rollback().await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE sync_conflicts SET detail_message = ?, row_version = row_version + 1, \
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE id = ? AND COALESCE(detail_message, '') <> ?",
        [still.clone().into(), conflict_id.into(), still.into()],
    ))
    .await?;
    Ok(false)
}

/// Audits a planning conflict closed by the row it waited for (target: the row, by its local id;
/// sent by sync with its own sync id).
#[allow(clippy::too_many_arguments)]
async fn audit_closed(
    db: &DatabaseConnection,
    event: &str,
    summary: &str,
    target_type: &str,
    table: &str,
    conflict_id: i64,
    sync_id: &str,
) {
    let target = match local_id(db, table, sync_id).await {
        Ok(Some(id)) => id.to_string(),
        _ => return,
    };
    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type: event,
            actor_id: None,
            entity_type: Some(target_type),
            entity_id: Some(&target),
            summary,
            detail_json: Some(json!({ "sync_conflict_id": conflict_id, "entity_sync_id": sync_id }).to_string()),
            ..Default::default()
        },
    )
    .await;
}

// ── "Keep this PC's plan" (CMMS Advisor decision 2026-10-25) ─────────────────────────────────

/// The planning four-eyes conflict codes (`resolve_sync_conflict` offers only "keep this PC's
/// plan" and "escalate" on them).
pub(crate) const PLANNING_CONFLICTS: &[&str] = &[
    WEEK_FROZEN,
    BREAK_IN_APPROVAL_REQUIRED,
    BREAK_IN_SELF_APPROVAL,
    BREAK_IN_APPROVER_UNAUTHORISED,
    BREAK_IN_UNSIGNED,
    WEEK_UNFREEZE_UNDECIDED,
    WEEK_UNFREEZE_SAME_PERSON,
    WEEK_UNFREEZE_UNAUTHORISED,
];

fn keep_refused(message: String) -> AppError {
    AppError::ValidationFailed(vec![message])
}

/// The crews a planning conflict is decided at: the crew of the received row and of the row held
/// here (a commitment), the crews a break-in names (else its commitment's crew here).
async fn conflict_crews(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    received: &Payload,
    local: Option<&Payload>,
) -> AppResult<Vec<i64>> {
    fn push(crews: &mut Vec<i64>, team: Option<i64>) {
        if let Some(team) = team {
            if !crews.contains(&team) {
                crews.push(team);
            }
        }
    }
    let mut crews = Vec::new();
    let team_of = |p: &Payload, column: &str| field(p, &key(column)).map(str::to_string);
    match entity_type {
        "schedule_commitments" => {
            for p in [Some(received), local].into_iter().flatten() {
                if let Some(team_sync) = team_of(p, "assigned_team_id") {
                    push(&mut crews, local_id(conn, "teams", &team_sync).await?);
                }
            }
        }
        "schedule_break_ins" => {
            for column in ["new_team_id", "old_team_id"] {
                if let Some(team_sync) = team_of(received, column) {
                    push(&mut crews, local_id(conn, "teams", &team_sync).await?);
                }
            }
            if crews.is_empty() {
                if let Some(commitment) = field(received, &key("schedule_commitment_id")) {
                    let team: Option<i64> = conn
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT assigned_team_id FROM schedule_commitments WHERE entity_sync_id = ?",
                            [commitment.into()],
                        ))
                        .await?
                        .map(|r| r.try_get::<Option<i64>>("", "assigned_team_id").map_err(sync_err))
                        .transpose()?
                        .flatten();
                    push(&mut crews, team);
                }
            }
        }
        _ => {}
    }
    Ok(crews)
}

/// "Keep this PC's plan" on a planning four-eyes conflict, before the generic keep
/// (`sync::conflicts::keep_local_version`, which then sends this PC's version):
/// - the person holds plan.confirm at the site of every crew involved (a week: its site here);
/// - a commitment refused while new here is stored here cancelled ("refusé (sync)", by this
///   person, now), a break-in refused while new here is stored here marked refused: that version is
///   what is sent, so every PC ends with the same row (never a delete);
/// - a row held here is simply sent again (a week stays frozen; its unfreeze event stays history).
pub(crate) async fn keep_planning_plan(
    conn: &impl ConnectionTrait,
    conflict: &crate::sync::domain::SyncConflictRecord,
    user_id: i64,
    reason: &str,
) -> AppResult<()> {
    let received: Payload = conflict
        .inbound_payload_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<Value>(j).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    let entity_type = conflict.entity_type.as_str();
    let sid = conflict.entity_sync_id.as_str();
    let local = local_wire(conn, entity_type, sid).await?;

    // Who may keep the plan: plan.confirm at each site involved.
    let allowed = if entity_type == "weekly_schedules" {
        let site: Option<i64> = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT entity_id FROM weekly_schedules WHERE entity_sync_id = ? \
                    OR (week_start = ? AND entity_id = (SELECT id FROM org_nodes WHERE entity_sync_id = ?)) \
                  ORDER BY (entity_sync_id = ?) DESC LIMIT 1",
                [
                    sid.into(),
                    field(&received, "week_start").map(str::to_string).into(),
                    field(&received, &key("entity_id")).map(str::to_string).into(),
                    sid.into(),
                ],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "entity_id").map_err(sync_err))
            .transpose()?;
        break_in_approvers(conn, site).await?.contains(&user_id)
    } else {
        let crews = conflict_crews(conn, entity_type, &received, local.as_ref()).await?;
        match crews.as_slice() {
            [] => break_in_approvers(conn, None).await?.contains(&user_id),
            [team, rest @ ..] => {
                let mut ok = holds_at_crew_sites(conn, user_id, *team, None).await?;
                for other in rest {
                    ok = ok && holds_at_crew_sites(conn, user_id, *other, None).await?;
                }
                ok
            }
        }
    };
    if !allowed {
        return Err(keep_refused(
            "GATE_SYNC_KEEP_PLAN_NOT_ALLOWED: Keeping this PC's plan needs plan.confirm at the site of each crew \
             involved."
                .to_string(),
        ));
    }
    if local.is_some() || entity_type == "weekly_schedules" {
        return Ok(());
    }

    // Absent here: stored here in its refused form, then sent as this PC's version.
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let refusal = format!("refusé (sync) : {} — {reason}", conflict.conflict_type);
    let mut refused = received.clone();
    match entity_type {
        "schedule_commitments" => {
            let user_sync = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
                    [user_id.into()],
                ))
                .await?
                .and_then(|r| r.try_get::<Option<String>>("", "entity_sync_id").ok().flatten());
            refused.insert("status".into(), "cancelled".into());
            refused.insert("status_reason".into(), refusal.into());
            refused.insert("status_changed_at".into(), now.clone().into());
            refused.insert(key("status_changed_by_id"), user_sync.map_or(Value::Null, Value::from));
            refused.insert("frozen_at".into(), Value::Null);
        }
        "schedule_break_ins" => {
            refused.insert("sync_refused_at".into(), now.clone().into());
            refused.insert("sync_refused_reason".into(), refusal.into());
        }
        _ => return Ok(()),
    }
    let version = conflict.inbound_row_version.unwrap_or(1);
    match crate::sync::materialize::apply_inbound_payload(
        conn,
        entity_type,
        sid,
        "upsert",
        version,
        &Value::Object(refused).to_string(),
    )
    .await?
    {
        crate::sync::materialize::ApplyOutcome::Applied | crate::sync::materialize::ApplyOutcome::Unchanged => Ok(()),
        crate::sync::materialize::ApplyOutcome::Deferred { message, .. }
        | crate::sync::materialize::ApplyOutcome::Conflict { message, .. } => Err(keep_refused(format!(
            "GATE_SYNC_KEEP_PLAN_NOT_YET: The refused version cannot be recorded here yet: {message}"
        ))),
    }
}
