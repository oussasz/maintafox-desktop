//! Planning rows received by synchronisation are judged again before they are written
//! (`sync::apply_hooks::before_upsert_checked`), with the rules of this PC (audit findings AU-06,
//! AU-102, S4; owner decision D6; CMMS Advisor answers Q1-Q4):
//!
//! - `schedule_commitments`: work entering a week frozen here (new here, or moved: slot or crew
//!   changed), or a commitment frozen here moved anywhere, comes with its break-ins: a run of valid
//!   break-ins leading from its slot and crew here to the received ones, each from where the one
//!   before left it ([`chains`]: already here when they arrived before it, or waiting in the inbox,
//!   then applied with it in one transaction, [`commitment_group`], and judged too). Without one it
//!   is a `PLANNING_WEEK_FROZEN` conflict at once, never an open wait: the sender's clock is never
//!   trusted to say the change came before the freeze (the conflict shows both times). A version
//!   that puts the work back on the plan held here — its slot and crew in the week's frozen
//!   baseline, or where break-ins refused on a PC took it from — is let in: that is how "keep this
//!   PC's plan" converges on every PC. A conflict this PC's rules let in later (its break-ins
//!   arrived, its week reopened here) closes by itself, audited ([`retry_frozen_week_conflicts`]).
//!   A commitment received un-frozen while its week is still frozen here waits only while the
//!   week's reopening is in the inbox; otherwise it is the same conflict.
//! - `schedule_break_ins`: an urgent break-in (or one with an unknown reason here), or one that
//!   forced a blocking check, names an approver; a named approver is never its requester, signed
//!   with a password (`approver_signed_at`) and holds plan.confirm at the crew's site here, now
//!   (waiting only while rows that could give that right are in the inbox). The password itself
//!   cannot be checked again here (docs/KNOWN_LIMITS.md). Break-ins recorded by older versions
//!   (named only, self-approved) are conflicts for review.
//! - `teams`: a crew's site change that would carry its committed jobs across a week frozen here
//!   (out of a frozen week of the site it leaves, or into one of the site it joins) is a
//!   `PLANNING_WEEK_FROZEN` conflict for review, the same rule as the local edit
//!   (`planning::crew_site`, AU-129); it closes by itself once this PC's rules let it in (the jobs
//!   ended, moved or released, or the week reopened here).
//! - `weekly_schedules`: a week reopened (frozen or published back to approved or draft, same
//!   baseline) comes with its `unfrozen` decision, here or in the inbox, decided by someone other
//!   than the requester who holds plan.confirm at the site here, now.

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

/// Whether two planned times are the same instant (as stored, RFC 3339; else as typed).
fn same_instant(a: &str, b: &str) -> bool {
    match (
        crate::wo::shifts::planned_instant(a),
        crate::wo::shifts::planned_instant(b),
    ) {
        (Some(x), Some(y)) => x == y,
        _ => a.trim() == b.trim(),
    }
}

/// Where a commitment is: its slot and its crew (the crew's sync id).
#[derive(Debug, Clone)]
struct Place {
    start: String,
    end: String,
    crew: Option<String>,
}

impl Place {
    fn of(p: &Payload) -> Option<Place> {
        let (Some(start), Some(end), crew) = slot_of(p) else {
            return None;
        };
        Some(Place { start, end, crew })
    }

    fn same(&self, other: &Place) -> bool {
        same_instant(&self.start, &other.start) && same_instant(&self.end, &other.end) && self.crew == other.crew
    }
}

/// A break-in of a commitment, read as the move it describes.
#[derive(Debug, Clone)]
struct BreakInMove {
    sid: String,
    payload: Payload,
    /// Waiting in the inbox (its inbox row); `None`: applied here.
    inbox_id: Option<i64>,
    /// Refused on a PC: marked here, or its refusal marks wait in the inbox. Never counts as valid
    /// (CMMS Advisor decision 3); only its undoing is let in ([`undoes_refused`]).
    refused: bool,
    /// Received from another PC (waiting, or applied from the inbox); `false`: made on this PC.
    by_sync: bool,
}

impl BreakInMove {
    /// A break-in that leaves slot and crew as they were (an assignee change only): it moves the job
    /// nowhere, so runs and undoings step over it (review R3-S3).
    fn no_op(&self) -> bool {
        match (
            self.slot("old_slot_start", "old_slot_end"),
            self.slot("new_slot_start", "new_slot_end"),
        ) {
            (Some((os, oe)), Some((ns, ne))) => {
                let (old, new) = self.crews();
                same_instant(os, ns) && same_instant(oe, ne) && old == new
            }
            _ => false,
        }
    }

    fn slot(&self, start: &str, end: &str) -> Option<(&str, &str)> {
        Some((field(&self.payload, start)?, field(&self.payload, end)?))
    }

    /// The crews it records: the one the work leaves and the one it goes to (sync ids).
    fn crews(&self) -> (Option<&str>, Option<&str>) {
        (
            field(&self.payload, &key("old_team_id")),
            field(&self.payload, &key("new_team_id")),
        )
    }

    /// Where it takes the commitment when the commitment is at `from`: only when it describes a move
    /// from there, slot and crew (review S1, R2-S1). A break-in recorded before 0.9.2 names no crew:
    /// it then describes a move within the crew (the crew is unchanged). One naming only one of the
    /// two crews describes no move that can be checked: none.
    fn step(&self, from: &Place) -> Option<Place> {
        let (old_start, old_end) = self.slot("old_slot_start", "old_slot_end")?;
        let (new_start, new_end) = self.slot("new_slot_start", "new_slot_end")?;
        if !same_instant(old_start, &from.start) || !same_instant(old_end, &from.end) {
            return None;
        }
        let crew = match self.crews() {
            (Some(old), Some(new)) if from.crew.as_deref() == Some(old) => Some(new.to_string()),
            (None, None) => from.crew.clone(),
            _ => return None,
        };
        Some(Place {
            start: new_start.to_string(),
            end: new_end.to_string(),
            crew,
        })
    }

    /// Where the commitment was before this break-in, when it took it to `to` (the reverse of
    /// [`BreakInMove::step`]).
    fn back(&self, to: &Place) -> Option<Place> {
        let (old_start, old_end) = self.slot("old_slot_start", "old_slot_end")?;
        let (new_start, new_end) = self.slot("new_slot_start", "new_slot_end")?;
        if !same_instant(new_start, &to.start) || !same_instant(new_end, &to.end) {
            return None;
        }
        let crew = match self.crews() {
            (Some(old), Some(new)) if to.crew.as_deref() == Some(new) => Some(old.to_string()),
            (None, None) => to.crew.clone(),
            _ => return None,
        };
        Some(Place {
            start: old_start.to_string(),
            end: old_end.to_string(),
            crew,
        })
    }

    /// For a commitment new here: whether it brings the work in at `to` (its new slot and the crew
    /// it goes to; a break-in naming no crew never does).
    fn enters_at(&self, to: &Place) -> bool {
        self.slot("new_slot_start", "new_slot_end")
            .is_some_and(|(start, end)| same_instant(start, &to.start) && same_instant(end, &to.end))
            && self.crews().1.is_some()
            && self.crews().1 == to.crew.as_deref()
    }
}

/// Whether a break-in reached this PC from another one: a row of it in the inbox (review R3-S3: one
/// made here is never refused by a keep).
const ARRIVED_BY_SYNC: &str = "EXISTS (SELECT 1 FROM sync_inbox r WHERE r.entity_type = 'schedule_break_ins' \
     AND r.entity_sync_id = sb.entity_sync_id)";

/// Every break-in of commitment `sid`: those applied here (oldest first) and those received and not
/// applied (in arrival order; with `conflicted`, those refused on their own too). Refusal marks
/// waiting in the inbox for a break-in held here count as its refusal. One move per break-in (review
/// N3-c): its latest received row, refused when any of its rows is.
async fn break_in_moves(
    conn: &impl ConnectionTrait,
    sid: &str,
    conflicted: bool,
) -> AppResult<(Vec<BreakInMove>, Vec<BreakInMove>)> {
    let commitment_key = key("schedule_commitment_id");
    let here_rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT sb.entity_sync_id AS sid, {ARRIVED_BY_SYNC} AS by_sync, \
                        (sb.sync_refused_at IS NOT NULL OR EXISTS (SELECT 1 FROM sync_inbox m \
                            WHERE m.entity_type = 'schedule_break_ins' AND m.entity_sync_id = sb.entity_sync_id \
                              AND {WAITING} \
                              AND json_extract(m.payload_json, '$.sync_refused_at') IS NOT NULL)) AS refused \
                   FROM schedule_break_ins sb JOIN schedule_commitments c ON c.id = sb.schedule_commitment_id \
                  WHERE c.entity_sync_id = ? ORDER BY sb.id"
            ),
            [sid.into()],
        ))
        .await?;
    let mut here = Vec::new();
    for r in here_rows {
        let Some(break_in) = r.try_get::<Option<String>>("", "sid").map_err(sync_err)? else {
            continue;
        };
        let refused: bool = r.try_get("", "refused").map_err(sync_err)?;
        let by_sync: bool = r.try_get("", "by_sync").map_err(sync_err)?;
        if let Some(payload) = local_wire(conn, "schedule_break_ins", &break_in).await? {
            here.push(BreakInMove {
                sid: break_in,
                payload,
                inbox_id: None,
                refused,
                by_sync,
            });
        }
    }
    let statuses = if conflicted {
        "apply_status IN ('pending', 'deferred', 'conflict')"
    } else {
        "apply_status IN ('pending', 'deferred')"
    };
    let waiting_rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT i.id, i.entity_sync_id AS sid, i.payload_json FROM sync_inbox i \
                  WHERE i.entity_type = 'schedule_break_ins' AND i.{statuses} \
                    AND i.operation NOT IN ('delete', 'archive') \
                    AND json_extract(i.payload_json, '$.{commitment_key}') = ? \
                    AND NOT EXISTS (SELECT 1 FROM schedule_break_ins sb WHERE sb.entity_sync_id = i.entity_sync_id) \
                  ORDER BY i.id"
            ),
            [sid.into()],
        ))
        .await?;
    let mut waiting: Vec<BreakInMove> = Vec::new();
    for r in waiting_rows {
        let id: i64 = r.try_get("", "id").map_err(sync_err)?;
        let break_in: String = r.try_get("", "sid").map_err(sync_err)?;
        let json: String = r.try_get("", "payload_json").map_err(sync_err)?;
        let Ok(Value::Object(payload)) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        let refused = field(&payload, "sync_refused_at").is_some();
        match waiting.iter_mut().find(|m| m.sid == break_in) {
            // A later row of the same break-in: its content, refused if either is.
            Some(known) => {
                known.refused |= refused;
                known.payload = payload;
                known.inbox_id = Some(id);
            }
            None => waiting.push(BreakInMove {
                sid: break_in,
                payload,
                inbox_id: Some(id),
                refused,
                by_sync: true,
            }),
        }
    }
    Ok((here, waiting))
}

/// At most this many chains are judged for one received commitment, of at most this many moves.
const MAX_CHAINS: usize = 16;
const MAX_CHAIN_LEN: usize = 8;
/// At most this many steps are tried while looking for chains (review N3-a): beyond, the search
/// stops and the received commitment is refused (a conflict), never applied on a partial search.
const MAX_CHAIN_VISITS: usize = 2_000;

/// The chain search gave up ([`MAX_CHAIN_VISITS`]).
#[derive(Debug)]
struct ChainSearchExhausted;

/// The runs of break-ins that lead a commitment from `from` (its place here) to `to` (the received
/// place), shortest first (review S1, R2-S1, R2-S4, N-a):
/// - first the latest break-ins applied here, when they describe moves not made here yet (a break-in
///   that arrived before its commitment): a run that ends with the latest break-in held here and
///   starts at `from` — an older break-in followed by a later one here is history, never replayed;
/// - then break-ins received and not applied, each once;
/// - each one from where the previous one left the commitment, slot and crew; none refused (unless
///   `with_refused`, to find the break-ins of a refused move); break-ins that move nothing
///   (assignee only) are stepped over (review R3-S3).
fn chains(
    from: &Place,
    to: &Place,
    here: &[BreakInMove],
    waiting: &[BreakInMove],
    with_refused: bool,
) -> Result<Vec<Vec<BreakInMove>>, ChainSearchExhausted> {
    struct Search<'a> {
        to: &'a Place,
        waiting: Vec<&'a BreakInMove>,
        used: Vec<bool>,
        path: Vec<BreakInMove>,
        out: Vec<Vec<BreakInMove>>,
        visits: usize,
    }
    fn walk(s: &mut Search<'_>, at: &Place) -> Result<(), ChainSearchExhausted> {
        s.visits += 1;
        if s.visits > MAX_CHAIN_VISITS {
            return Err(ChainSearchExhausted);
        }
        if !s.path.is_empty() && at.same(s.to) {
            s.out.push(s.path.clone());
            return Ok(());
        }
        if s.out.len() >= MAX_CHAINS || s.path.len() >= MAX_CHAIN_LEN {
            return Ok(());
        }
        for i in 0..s.waiting.len() {
            if s.used[i] {
                continue;
            }
            let m = s.waiting[i];
            if let Some(next) = m.step(at) {
                s.used[i] = true;
                s.path.push(m.clone());
                let result = walk(s, &next);
                s.path.pop();
                s.used[i] = false;
                result?;
            }
        }
        Ok(())
    }
    let counts = |m: &&BreakInMove| !m.no_op() && (with_refused || !m.refused);
    let here: Vec<&BreakInMove> = here.iter().filter(|m| !m.no_op()).collect();
    let mut search = Search {
        to,
        waiting: waiting.iter().filter(counts).collect(),
        used: Vec::new(),
        path: Vec::new(),
        out: Vec::new(),
        visits: 0,
    };
    search.used = vec![false; search.waiting.len()];
    for taken in 0..=here.len().min(MAX_CHAIN_LEN) {
        let run = &here[here.len() - taken..];
        if !with_refused && run.iter().any(|m| m.refused) {
            break;
        }
        let mut at = from.clone();
        if !run.iter().all(|m| match m.step(&at) {
            Some(next) => {
                at = next;
                true
            }
            None => false,
        }) {
            continue;
        }
        search.path = run.iter().map(|m| (*m).clone()).collect();
        walk(&mut search, &at)?;
    }
    let mut out = search.out;
    out.sort_by_key(Vec::len);
    Ok(out)
}

/// Whether `to` puts the commitment back where refused break-ins took it from: from `at` (its place
/// here) back through the latest break-ins held here that move it, each refused (CMMS Advisor
/// decisions 2-3, review R2-B1: the plan kept on the PC that refused them, never a move into the
/// frozen week). Break-ins that move nothing are stepped over (review R3-S3).
fn undoes_refused(here: &[BreakInMove], at: &Place, to: &Place) -> bool {
    let mut place = at.clone();
    for m in here.iter().rev().filter(|m| !m.no_op()) {
        if !m.refused {
            return false;
        }
        match m.back(&place) {
            Some(before) => place = before,
            None => return false,
        }
        if place.same(to) {
            return true;
        }
    }
    false
}

/// Whether a valid (not refused) break-in held here brought the job to `at`, its place here: then
/// the place is a signed decision, and a version going back to the baseline is a move like any other
/// (review R3-S1).
fn reached_by_valid_break_in(here: &[BreakInMove], at: &Place) -> bool {
    here.iter().any(|m| !m.no_op() && !m.refused && m.back(at).is_some())
}

/// Whether `to` is where the commitment (local id `commitment`) stands in the baseline of the week
/// frozen here for that slot (`schedule`: that week's schedule id and current baseline): same slot
/// and crew as its baseline item (review R2-B1, R3-S1, CMMS Advisor Q2). Putting work back on the
/// frozen plan is not entering the week.
async fn back_to_baseline(
    conn: &impl ConnectionTrait,
    commitment: i64,
    to: &Place,
    to_team: Option<i64>,
    schedule: (i64, i64),
) -> AppResult<bool> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT committed_start, committed_end, team_id FROM weekly_schedule_baseline_items \
              WHERE schedule_commitment_id = ? AND weekly_schedule_id = ? AND baseline_no = ?",
            [commitment.into(), schedule.0.into(), schedule.1.into()],
        ))
        .await?;
    for r in rows {
        let start: String = r.try_get("", "committed_start").map_err(sync_err)?;
        let end: String = r.try_get("", "committed_end").map_err(sync_err)?;
        let team: Option<i64> = r.try_get("", "team_id").map_err(sync_err)?;
        if same_instant(&start, &to.start) && same_instant(&end, &to.end) && team.is_some() && team == to_team {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the baseline `baseline` of the week (local id `week`, sync id `week_sid`) was received
/// from another PC rather than frozen here (review R3-S2): its freeze event is held here and came by
/// sync (an inbox row, never staged by this PC), or it waits in the inbox. A week this PC froze
/// itself never takes another PC's baseline items as leave to enter.
async fn baseline_received(conn: &impl ConnectionTrait, week: i64, week_sid: &str, baseline: i64) -> AppResult<bool> {
    let held = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.entity_sync_id AS sid, \
                    EXISTS (SELECT 1 FROM sync_inbox i WHERE i.entity_type = 'weekly_schedule_events' \
                              AND i.entity_sync_id = e.entity_sync_id) AS received, \
                    (EXISTS (SELECT 1 FROM sync_outbox o WHERE o.entity_type = 'weekly_schedule_events' \
                              AND o.entity_sync_id = e.entity_sync_id) \
                     OR EXISTS (SELECT 1 FROM sync_capture_journal j WHERE j.entity_type = 'weekly_schedule_events' \
                              AND j.entity_sync_id = e.entity_sync_id)) AS staged \
               FROM weekly_schedule_events e \
              WHERE e.weekly_schedule_id = ? AND e.baseline_no = ? AND e.action IN ('frozen', 'auto_frozen')",
            [week.into(), baseline.into()],
        ))
        .await?;
    let mut any_held = false;
    for r in held {
        let received: bool = r.try_get("", "received").map_err(sync_err)?;
        let staged: bool = r.try_get("", "staged").map_err(sync_err)?;
        if !received || staged {
            // Frozen by this PC.
            return Ok(false);
        }
        any_held = true;
    }
    if any_held {
        return Ok(true);
    }
    let week_key = key("weekly_schedule_id");
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS w FROM sync_inbox WHERE entity_type = 'weekly_schedule_events' AND {WAITING} \
                   AND json_extract(payload_json, '$.{week_key}') = ? \
                   AND CAST(json_extract(payload_json, '$.baseline_no') AS INTEGER) = ? \
                   AND json_extract(payload_json, '$.action') IN ('frozen', 'auto_frozen') LIMIT 1"
            ),
            [week_sid.into(), baseline.into()],
        ))
        .await?
        .is_some())
}

/// For a commitment new here: whether `to` is its place in the baseline of the week frozen here for
/// that slot (`schedule`: id and baseline), by a baseline item of that week and baseline waiting in
/// the inbox, when that baseline was received from another PC (a PC receiving a frozen week with its
/// commitments: they are the plan, not work entering it; review R3-S2).
async fn in_received_baseline(
    conn: &impl ConnectionTrait,
    sid: &str,
    to: &Place,
    schedule: (i64, i64),
) -> AppResult<bool> {
    let Some(week_sid) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM weekly_schedules WHERE id = ?",
            [schedule.0.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<String>>("", "entity_sync_id").ok().flatten())
    else {
        return Ok(false);
    };
    let commitment_key = key("schedule_commitment_id");
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT payload_json FROM sync_inbox WHERE entity_type = 'weekly_schedule_baseline_items' \
                   AND {WAITING} AND json_extract(payload_json, '$.{commitment_key}') = ?"
            ),
            [sid.into()],
        ))
        .await?;
    let mut listed = false;
    for r in rows {
        let json: String = r.try_get("", "payload_json").map_err(sync_err)?;
        let Ok(Value::Object(item)) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        let (Some(start), Some(end), Some(week), Some(baseline)) = (
            field(&item, "committed_start"),
            field(&item, "committed_end"),
            field(&item, &key("weekly_schedule_id")),
            int(&item, "baseline_no"),
        ) else {
            continue;
        };
        listed |= week == week_sid
            && baseline == schedule.1
            && same_instant(start, &to.start)
            && same_instant(end, &to.end)
            && to.crew.is_some()
            && field(&item, &key("team_id")) == to.crew.as_deref();
    }
    Ok(listed && baseline_received(conn, schedule.0, &week_sid, schedule.1).await?)
}

/// What the break-ins of a received commitment say.
enum BreakInSupport {
    /// A valid run is applied here: the commitment may enter.
    Here,
    /// A valid run waits in the inbox (with, maybe, break-ins applied here) and can be applied with
    /// it now (the group's rows, in the order of the run).
    Waiting(Vec<crate::permit::sync_check::GroupRow>),
    /// A run needs another row first (its approver, crew, rights still arriving).
    Wait(String),
    /// None is valid: the refusal of the first one judged, or `None` when there is none.
    None(Option<String>),
}

/// The break-ins that would let the commitment `sid` in at `to`, each judged ([`judge_break_in`]);
/// `from`: its place here (`None`: new here); `fallback_team`: the crew a break-in recorded before
/// 0.9.2 (no crews on it) is judged at.
async fn break_in_support(
    conn: &impl ConnectionTrait,
    sid: &str,
    from: Option<&Place>,
    to: &Place,
    fallback_team: Option<i64>,
) -> AppResult<BreakInSupport> {
    let (here, waiting) = break_in_moves(conn, sid, false).await?;
    let candidates: Vec<Vec<BreakInMove>> = match from {
        Some(from) => match chains(from, to, &here, &waiting, false) {
            Ok(found) => found,
            // Too many break-ins to search through: refused, never let in on a partial search.
            Err(ChainSearchExhausted) => {
                return Ok(BreakInSupport::None(Some(format!(
                    "too many break-ins of commitment {sid} to judge (more than {MAX_CHAIN_VISITS} steps searched)"
                ))))
            }
        },
        // New here: one break-in that brings it in at that slot and crew.
        None => waiting
            .iter()
            .filter(|m| !m.refused && m.enters_at(to))
            .map(|m| vec![m.clone()])
            .collect(),
    };
    let mut refused: Option<String> = None;
    let mut wait_for: Option<String> = None;
    let mut children: Option<crate::permit::sync_check::Children> = None;
    'chain: for chain in candidates {
        for m in &chain {
            match judge_break_in(conn, &m.sid, &m.payload, fallback_team).await? {
                None => {}
                Some(InboundVerdict::Defer(message)) => {
                    wait_for.get_or_insert(message);
                    continue 'chain;
                }
                Some(InboundVerdict::Conflict { code, message }) => {
                    refused.get_or_insert(format!("{code}: {message}"));
                    continue 'chain;
                }
            }
        }
        let mut rows = Vec::new();
        for m in &chain {
            let Some(inbox_id) = m.inbox_id else {
                continue;
            };
            if children.is_none() {
                children = Some(
                    crate::permit::sync_check::children(
                        conn,
                        "schedule_break_ins",
                        "schedule_commitments",
                        "schedule_commitment_id",
                        &key("schedule_commitment_id"),
                        sid,
                    )
                    .await?,
                );
            }
            let counted = children
                .as_ref()
                .and_then(|c| c.counted.iter().find(|row| row.inbox_id() == inbox_id));
            match counted {
                Some(row) => rows.push(row.clone()),
                None => {
                    wait_for.get_or_insert_with(|| {
                        children.as_ref().and_then(|c| c.blocked.clone()).unwrap_or_else(|| {
                            format!("the break-in {} of commitment {sid} cannot be applied yet", m.sid)
                        })
                    });
                    continue 'chain;
                }
            }
        }
        return Ok(if rows.is_empty() {
            BreakInSupport::Here
        } else {
            BreakInSupport::Waiting(rows)
        });
    }
    Ok(match wait_for {
        Some(message) => BreakInSupport::Wait(message),
        None => BreakInSupport::None(refused),
    })
}

/// The commitment's place here and the received one, with the received crew's local id.
struct CommitmentContext {
    local: Option<Payload>,
    local_id: Option<i64>,
    merged: Payload,
    from: Option<Place>,
    to: Place,
    team: Option<i64>,
}

async fn commitment_context(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Result<CommitmentContext, InboundVerdict>> {
    let local = local_wire(conn, "schedule_commitments", entity_sync_id).await?;
    let merged = overlay(local.as_ref(), received);
    let Some(to) = Place::of(&merged) else {
        return Ok(Err(InboundVerdict::Conflict {
            code: INVALID_VALUE,
            message: format!("commitment {entity_sync_id} received without its slot"),
        }));
    };
    let team = match to.crew.as_deref() {
        Some(team_sync) => match local_id(conn, "teams", team_sync).await? {
            Some(team) => Some(team),
            None => {
                return Ok(Err(InboundVerdict::Defer(format!(
                    "crew {team_sync} is not applied yet"
                ))))
            }
        },
        None => None,
    };
    let local_id = match local {
        Some(_) => local_id(conn, "schedule_commitments", entity_sync_id).await?,
        None => None,
    };
    Ok(Ok(CommitmentContext {
        from: local.as_ref().and_then(Place::of),
        local,
        local_id,
        merged,
        to,
        team,
    }))
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

/// Why a received commitment is let in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admitted {
    /// It enters no week frozen here (left the schedule, not moved, or the week is open here).
    NotEntering,
    /// Its move is a run of valid break-ins (here, or applied with it).
    BreakIn,
    /// It puts the work back on the plan: the week's frozen baseline here, or where break-ins
    /// refused on a PC took it from (review R2-B1).
    PlanRestored,
}

/// The judgement of a received commitment: let in (why), or a verdict.
enum Judgement {
    Admitted(Admitted),
    Verdict(InboundVerdict),
}

/// See the module documentation (`schedule_commitments`).
pub(crate) async fn inbound_commitment_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    Ok(match judge_commitment(conn, entity_sync_id, received).await? {
        Judgement::Admitted(_) => None,
        Judgement::Verdict(verdict) => Some(verdict),
    })
}

async fn judge_commitment(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Judgement> {
    let ctx = match commitment_context(conn, entity_sync_id, received).await? {
        Ok(ctx) => ctx,
        Err(verdict) => return Ok(Judgement::Verdict(verdict)),
    };
    // Released or cancelled: leaving the schedule is not entering a week.
    if field(&ctx.merged, "status") != Some("committed") {
        return Ok(Judgement::Admitted(Admitted::NotEntering));
    }
    let local = ctx.local.as_ref();
    // Cancelled or released here (by a keep, for one) and received committed again: it re-enters the
    // schedule, even at the same slot, and is judged like new work (review N4-d).
    let reentering = local.is_some_and(|l| field(l, "status") != Some("committed"));
    let moved = reentering || ctx.from.as_ref().is_none_or(|from| !from.same(&ctx.to));
    let frozen_here = local.and_then(|l| field(l, "frozen_at")).is_some();
    let unfrozen = frozen_here && field(&ctx.merged, "frozen_at").is_none();
    if !moved && !unfrozen {
        return Ok(Judgement::Admitted(Admitted::NotEntering));
    }
    let frozen_week = match ctx.team {
        Some(team) => {
            super::weekly_schedule::frozen_week_for_slot(conn, team, &ctx.to.start, &ctx.to.end, crate::clock::now())
                .await?
        }
        None => None,
    };
    let sent_at = field(received, "updated_at")
        .or_else(|| field(received, "created_at"))
        .unwrap_or("not recorded")
        .to_string();
    let (start, end) = (&ctx.to.start, &ctx.to.end);
    let mut admitted = Admitted::NotEntering;
    if moved && (frozen_week.is_some() || frozen_here) {
        // Back on the plan: the frozen baseline here, or undoing break-ins refused on a PC.
        // The baseline counted is the one of the week frozen here for the received slot (R3-S1).
        let schedule = frozen_week.as_ref().and_then(|w| w.schedule);
        let restored = match (ctx.local_id, ctx.from.as_ref()) {
            // Re-entering work is never "back on the plan": it needs its break-in (N4-d).
            _ if reentering => false,
            (Some(id), Some(from)) => {
                let (here, _) = break_in_moves(conn, entity_sync_id, false).await?;
                undoes_refused(&here, from, &ctx.to)
                    // Back to the baseline only when no valid break-in here led the job where it is
                    // (a signed move is never undone silently, review R3-S1).
                    || match schedule {
                        Some(schedule) if !reached_by_valid_break_in(&here, from) => {
                            back_to_baseline(conn, id, &ctx.to, ctx.team, schedule).await?
                        }
                        _ => false,
                    }
            }
            // New here (a PC receiving the plan): its baseline item of the week frozen here for that
            // slot, waiting in the inbox with it, when that baseline came from another PC (R3-S2).
            (None, _) => match schedule {
                Some(schedule) => in_received_baseline(conn, entity_sync_id, &ctx.to, schedule).await?,
                None => false,
            },
            _ => false,
        };
        if restored {
            admitted = Admitted::PlanRestored;
        } else {
            let refused = match break_in_support(conn, entity_sync_id, ctx.from.as_ref(), &ctx.to, ctx.team).await? {
                // Applied with it, and judged again (`commitment_group`), or applied here and valid.
                BreakInSupport::Here | BreakInSupport::Waiting(_) => return Ok(Judgement::Admitted(Admitted::BreakIn)),
                // Its break-in is in the inbox but needs another row first: a bounded wait.
                BreakInSupport::Wait(m) => return Ok(Judgement::Verdict(InboundVerdict::Defer(m))),
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
            return Ok(Judgement::Verdict(InboundVerdict::Conflict {
                code: WEEK_FROZEN,
                message: format!(
                    "commitment {entity_sync_id} enters the frozen week {week} ({start} -> {end}) {why}; \
                     changed on its PC at {sent_at} (that PC's clock), week frozen here at {frozen_at}"
                ),
            }));
        }
    }
    let Some(week) = frozen_week.filter(|_| unfrozen) else {
        return Ok(Judgement::Admitted(admitted));
    };
    let site = match ctx.team {
        Some(team) => team_planning_site(conn, team).await?,
        None => None,
    };
    if let Some(site) = site {
        if reopening_waiting(conn, &week.week_start, site).await? {
            return Ok(Judgement::Verdict(InboundVerdict::Defer(format!(
                "the reopening of the week {} is still in the inbox",
                week.week_start
            ))));
        }
    }
    Ok(Judgement::Verdict(InboundVerdict::Conflict {
        code: WEEK_FROZEN,
        message: format!(
            "commitment {entity_sync_id} received un-frozen while its week {} is still frozen here (frozen at {}); \
             changed on its PC at {sent_at} (that PC's clock)",
            week.week_start,
            week.frozen_at.as_deref().unwrap_or("not recorded")
        ),
    }))
}

/// See the module documentation (`teams`). A crew new here has no job here; a node not here yet
/// is left to the generic apply (it waits for it). A crew moved off a node deleted here to the node
/// that replaced it when a new organisation structure was published
/// (`org::validation::node_replaces_deleted_node`) is no crew move; while that publish has only
/// partly arrived (`org::validation::node_copy_awaits_activation`) the row waits. Any other move,
/// off a deactivated node too, is judged as on this PC.
pub(crate) async fn inbound_crew_site_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let Some(team) = local_id(conn, "teams", entity_sync_id).await? else {
        return Ok(None);
    };
    let local = local_wire(conn, "teams", entity_sync_id).await?;
    let merged = overlay(local.as_ref(), received);
    let new_node = match field(&merged, &key("primary_node_id")) {
        Some(node_sync) => match local_id(conn, "org_nodes", node_sync).await? {
            Some(node) => Some(node),
            None => return Ok(None),
        },
        None => None,
    };
    let held_node: Option<i64> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT primary_node_id FROM teams WHERE id = ?",
            [team.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "primary_node_id").ok().flatten());
    if let (Some(held), Some(node)) = (held_node, new_node) {
        if held != node {
            if crate::org::validation::node_replaces_deleted_node(conn, held, node).await? {
                return Ok(None);
            }
            if crate::org::validation::node_copy_awaits_activation(conn, Some(held), node).await? {
                return Ok(Some(InboundVerdict::Defer(format!(
                    "crew {entity_sync_id}: the organisation structure holding its new node is not activated here yet"
                ))));
            }
        }
    }
    let Some(block) = super::crew_site::crew_site_change_block(conn, team, new_node, crate::clock::now()).await? else {
        return Ok(None);
    };
    let sent_at = field(received, "updated_at").unwrap_or("not recorded");
    Ok(Some(InboundVerdict::Conflict {
        code: WEEK_FROZEN,
        message: format!(
            "crew {entity_sync_id} received with another site: {} committed job(s) of the crew would cross the \
             frozen week(s) {} here; changed on its PC at {sent_at} (that PC's clock)",
            block.jobs,
            block.weeks.join(", ")
        ),
    }))
}

/// The waiting, valid break-ins of a received commitment that lead it to the received place,
/// applied right after it in its transaction (each is judged again by its own check there).
pub(crate) async fn commitment_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<crate::permit::sync_check::Group>> {
    let Ok(ctx) = commitment_context(conn, entity_sync_id, received).await? else {
        return Ok(None);
    };
    if field(&ctx.merged, "status") != Some("committed") || ctx.from.as_ref().is_some_and(|f| f.same(&ctx.to)) {
        return Ok(None);
    }
    let BreakInSupport::Waiting(rows) =
        break_in_support(conn, entity_sync_id, ctx.from.as_ref(), &ctx.to, ctx.team).await?
    else {
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
    let differing: Vec<&str> = received
        .iter()
        .filter(|(k, v)| {
            !REFUSAL_COLUMNS.contains(&k.as_str())
                && !matches!(k.as_str(), "row_version" | "updated_at")
                && local.get(k.as_str()).is_some_and(|held| held != *v)
        })
        .map(|(k, _)| k.as_str())
        .collect();
    if !differing.is_empty() {
        // Marks arriving with another difference (a value normalised differently by the refusing
        // PC) are not taken: logged, never silent (review N-d).
        tracing::warn!(
            target: "maintafox::sync",
            break_in = entity_sync_id,
            fields = ?differing,
            "refusal marks of a break-in held here not applied: other values differ"
        );
        return Ok(false);
    }
    Ok(true)
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
    // The crews the break-in names (review S2); the commitment's crew as well when it names no crew
    // the work goes to (review N-c), or no crew at all (recorded before 0.9.2).
    let mut crews: Vec<i64> = Vec::new();
    for column in ["new_team_id", "old_team_id"] {
        if let Some(team_sync) = field(received, &key(column)) {
            match local_id(conn, "teams", team_sync).await? {
                Some(team) => crews.push(team),
                None => return wait(format!("crew {team_sync} is not applied yet")),
            }
        }
    }
    if field(received, &key("new_team_id")).is_none() {
        if let Some(team) = fallback_team.filter(|t| !crews.contains(t)) {
            crews.push(team);
        }
    }
    let mut allowed = !crews.is_empty() || break_in_approvers(conn, None).await?.contains(&approver);
    for team in &crews {
        allowed = allowed && holds_at_crew_sites(conn, approver, *team, None).await?;
    }
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
    // A week without its status, start or baseline cannot be judged: refused, never applied as a
    // partial row that keeps the values held here (review N-b).
    let (Some(status), Some(week_start), Some(baseline_no)) = (
        field(received, "status"),
        field(received, "week_start"),
        int(received, "baseline_no"),
    ) else {
        return conflict(
            INVALID_VALUE,
            format!("week {entity_sync_id} received without its status, week start or baseline number"),
        );
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
    let first = if frozen_here {
        local_baseline
    } else {
        local_baseline + 1
    };
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
/// Audit action of a `PLANNING_WEEK_FROZEN` conflict closed because the week is open here since
/// (its reopening was received and judged; review R2-S2).
pub(crate) const WEEK_FROZEN_REOPENED_EVENT: &str = "planning.week_frozen_conflict.closed_by_reopening";
/// Audit action of a `PLANNING_WEEK_FROZEN` conflict closed because the commitment is back on the
/// plan here (the frozen baseline, or the undoing of break-ins refused on a PC; review R2-B1).
pub(crate) const WEEK_FROZEN_RESTORED_EVENT: &str = "planning.week_frozen_conflict.closed_by_restored_plan";

/// Commitments refused as `PLANNING_WEEK_FROZEN` that this PC's rules let in now, judged again in
/// full: their run of break-ins has arrived since (here, or waiting in the inbox: applied with them
/// and judged too), the week is open here since (its reopening received and judged, review R2-S2),
/// or the received version puts the work back on the plan. Each is applied again through every
/// check; applied: its conflict is closed (`resolved_remote`, with why) and audited. Still not
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
        // A newer version received from the server took the job off the schedule since, applied
        // here, while it is off the schedule here too: the refused version is superseded (review
        // N5-b).
        if close_superseded_by_exit(db, inbox_id, conflict_id, &sid).await? {
            continue;
        }
        // Only a commitment this PC would let in now is worth a retry; the retry judges it again,
        // with its break-ins (review S2).
        let Judgement::Admitted(why) = judge_commitment(db, &sid, &received).await? else {
            continue;
        };
        let (action, note, event, summary) = match why {
            Admitted::BreakIn => (
                "break_in_received",
                "Its break-in was received and judged valid: the commitment is applied with it.",
                WEEK_FROZEN_CLOSED_EVENT,
                "Frozen-week conflict closed: the commitment's break-in was received",
            ),
            Admitted::NotEntering => (
                "week_reopened",
                "The week is no longer frozen here (its reopening was received and judged): the commitment is \
                 applied.",
                WEEK_FROZEN_REOPENED_EVENT,
                "Frozen-week conflict closed: the week is open here since",
            ),
            Admitted::PlanRestored => (
                "plan_restored",
                "The received version puts the work back on the plan held here: the commitment is applied.",
                WEEK_FROZEN_RESTORED_EVENT,
                "Frozen-week conflict closed: the commitment is back on the plan",
            ),
        };
        let closed = reapply_conflicted(db, inbox_id, conflict_id, action, note).await?;
        if closed {
            applied += 1;
            audit_closed(
                db,
                event,
                summary,
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

/// Audit action of a `PLANNING_WEEK_FROZEN` conflict closed because a newer version of the
/// commitment received from the server, taking the job off the schedule (closed, cancelled,
/// released), was applied here since (review N5-b).
pub(crate) const WEEK_FROZEN_SUPERSEDED_EVENT: &str = "planning.week_frozen_conflict.closed_by_newer_version";

/// A `PLANNING_WEEK_FROZEN` conflict on a commitment (`conflict_id`, its refused inbox row
/// `inbox_id`) whose PC sent a newer version since that takes the job off the schedule, received
/// and applied here, while the job is off the schedule here too (a job closed on both PCs, one of
/// them having edited it first, review N5-b): the refused version can no longer enter anything. Its
/// inbox row is superseded and the conflict closed (`resolved_remote`, `newer_version_applied`,
/// noted) and audited. No data is changed. `true` when closed. A newer version still waiting or
/// refused, or a job committed here, closes nothing: it is judged as before.
async fn close_superseded_by_exit(
    db: &DatabaseConnection,
    inbox_id: i64,
    conflict_id: i64,
    sid: &str,
) -> AppResult<bool> {
    let newer = db
        // The highest applied off-schedule version, not necessarily the latest one: what makes the
        // close safe is the EXISTS on this PC's own commitment (off the schedule here now).
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.row_version AS v, json_extract(n.payload_json, '$.status') AS s \
               FROM sync_inbox i JOIN sync_inbox n \
                 ON n.entity_type = i.entity_type AND n.entity_sync_id = i.entity_sync_id \
              WHERE i.id = ? AND n.apply_status = 'applied' AND n.row_version > i.row_version \
                AND json_extract(n.payload_json, '$.status') IS NOT NULL \
                AND json_extract(n.payload_json, '$.status') <> 'committed' \
                AND EXISTS (SELECT 1 FROM schedule_commitments c \
                             WHERE c.entity_sync_id = i.entity_sync_id AND c.status <> 'committed') \
              ORDER BY n.row_version DESC LIMIT 1",
            [inbox_id.into()],
        ))
        .await?;
    let Some(newer) = newer else {
        return Ok(false);
    };
    let version: i64 = newer.try_get("", "v").map_err(sync_err)?;
    let status: String = newer.try_get("", "s").map_err(sync_err)?;
    let note = format!(
        "A newer version of the commitment, received from the server (version {version}, {status}) was applied here since: the \
         job is off the schedule, the refused version enters no frozen week."
    );
    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE sync_inbox SET apply_status = 'superseded', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE id = ? AND apply_status = 'conflict'",
        [inbox_id.into()],
    ))
    .await?;
    let closed = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sync_conflicts \
                SET status = 'resolved_remote', resolution_action = 'newer_version_applied', resolution_note = ?, \
                    resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1, \
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
              WHERE id = ? AND status IN ('new', 'triaged', 'escalated')",
            [note.into(), conflict_id.into()],
        ))
        .await?
        .rows_affected()
        > 0;
    tx.commit().await?;
    if closed {
        audit_closed(
            db,
            WEEK_FROZEN_SUPERSEDED_EVENT,
            "Frozen-week conflict closed: a newer version received from the server took the job off the schedule",
            "schedule_commitment",
            "schedule_commitments",
            conflict_id,
            sid,
        )
        .await;
    }
    Ok(closed)
}

/// Audit action of a `WEEK_UNFREEZE_*` conflict closed once its reopening is judged valid here (its
/// decision, or the decider's right, received later).
pub(crate) const UNFREEZE_CLOSED_EVENT: &str = "planning.week_unfreeze_conflict.closed_by_decision";
/// Audit action of a break-in conflict closed once the break-in is judged valid here (its approver's
/// account or right received or granted later; review R3-S4).
pub(crate) const BREAK_IN_CLOSED_EVENT: &str = "planning.break_in_conflict.closed_by_rights";

/// The week codes judged again by [`retry_unfreeze_conflicts`].
const WEEK_CONFLICTS: &[&str] = &[
    WEEK_UNFREEZE_UNDECIDED,
    WEEK_UNFREEZE_SAME_PERSON,
    WEEK_UNFREEZE_UNAUTHORISED,
];
/// The break-in codes judged again by [`retry_break_in_conflicts`].
const BREAK_IN_CONFLICTS: &[&str] = &[
    BREAK_IN_APPROVAL_REQUIRED,
    BREAK_IN_SELF_APPROVAL,
    BREAK_IN_APPROVER_UNAUTHORISED,
    BREAK_IN_UNSIGNED,
];

/// Whether a received week is still refused here (moving it back, or a reopening not judged valid;
/// a wait counts as refused).
async fn week_still_refused(conn: &impl ConnectionTrait, sid: &str, received: &Payload) -> AppResult<bool> {
    Ok(super::weekly_schedule::inbound_lifecycle_conflict(conn, sid, received)
        .await?
        .is_some()
        || inbound_unfreeze_check(conn, sid, received).await?.is_some())
}

/// The open conflicts of `entity_type` with one of `codes`, linked to their refused inbox row:
/// inbox id, conflict id, sync id and received payload.
pub(crate) async fn open_planning_conflicts(
    db: &DatabaseConnection,
    entity_type: &str,
    codes: &[&str],
) -> AppResult<Vec<(i64, i64, String, Payload)>> {
    let codes_json = serde_json::to_string(codes)?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT i.id AS inbox_id, c.id AS conflict_id, i.entity_sync_id AS sid, i.payload_json AS payload \
               FROM sync_inbox i JOIN sync_conflicts c ON c.linked_inbox_id = i.id \
              WHERE i.entity_type = ? AND i.apply_status = 'conflict' \
                AND c.conflict_type IN (SELECT value FROM json_each(?)) \
                AND c.status IN ('new', 'triaged', 'escalated') \
              ORDER BY i.id",
            [entity_type.into(), codes_json.into()],
        ))
        .await?;
    let mut out = Vec::new();
    for row in rows {
        let payload: String = row.try_get("", "payload").map_err(sync_err)?;
        let Ok(Value::Object(received)) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        out.push((
            row.try_get("", "inbox_id").map_err(sync_err)?,
            row.try_get("", "conflict_id").map_err(sync_err)?,
            row.try_get("", "sid").map_err(sync_err)?,
            received,
        ));
    }
    Ok(out)
}

/// Weeks refused as `WEEK_UNFREEZE_*` that this PC's rules let in now (their decision arrived, the
/// decider's right arrived or was granted here; review S4, R3-S4): each is applied again through
/// every check. Applied: its conflict is closed (`resolved_remote`, `decision_received`) and
/// audited; its commitments refused meanwhile follow through [`retry_frozen_week_conflicts`]. Still
/// not applicable: the conflict stays open with the new reason. Returns how many applied.
pub(crate) async fn retry_unfreeze_conflicts(db: &DatabaseConnection) -> AppResult<i64> {
    let mut applied = 0;
    for (inbox_id, conflict_id, sid, received) in
        open_planning_conflicts(db, "weekly_schedules", WEEK_CONFLICTS).await?
    {
        if week_still_refused(db, &sid, &received).await? {
            continue;
        }
        let closed = reapply_conflicted(
            db,
            inbox_id,
            conflict_id,
            "decision_received",
            "Its reopening is judged valid here now (its unfreeze decision, or the decider's right, has arrived): \
             the week is applied.",
        )
        .await?;
        if closed {
            applied += 1;
            audit_closed(
                db,
                UNFREEZE_CLOSED_EVENT,
                "Week unfreeze conflict closed: its reopening is judged valid here",
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

/// Break-ins refused on arrival (`PLANNING_BREAK_IN_*`) that this PC's rules let in now (their
/// approver's account, personnel link or right arrived or was granted here; review R3-S4): each is
/// applied again through every check; applied: its conflict is closed (`resolved_remote`,
/// `judged_valid`) and audited, and the commitment it moves follows through
/// [`retry_frozen_week_conflicts`]. Returns how many applied.
pub(crate) async fn retry_break_in_conflicts(db: &DatabaseConnection) -> AppResult<i64> {
    let mut applied = 0;
    for (inbox_id, conflict_id, sid, received) in
        open_planning_conflicts(db, "schedule_break_ins", BREAK_IN_CONFLICTS).await?
    {
        if inbound_break_in_check(db, &sid, &received).await?.is_some() {
            continue;
        }
        // Held here since (another row of it applied, refused or not): nothing was judged valid
        // (review N4-b).
        let held = local_id(db, "schedule_break_ins", &sid).await?.is_some();
        let (action, note, summary) = if held {
            (
                "held_here",
                "The break-in is held here already (another version of it was applied): nothing more to apply.",
                "Break-in conflict closed: the break-in is held here already",
            )
        } else {
            (
                "judged_valid",
                "The break-in is judged valid here now (its approver's account or right has arrived or been \
                 granted): it is applied.",
                "Break-in conflict closed: the break-in is judged valid here",
            )
        };
        let closed = reapply_conflicted(db, inbox_id, conflict_id, action, note).await?;
        if closed {
            applied += 1;
            audit_closed(
                db,
                BREAK_IN_CLOSED_EVENT,
                summary,
                "schedule_break_in",
                "schedule_break_ins",
                conflict_id,
                &sid,
            )
            .await;
        }
    }
    Ok(applied)
}

/// Crews refused as `PLANNING_WEEK_FROZEN` (a site change, AU-129) that this PC's rules let in now
/// (their jobs ended, moved or released, or the week reopened here): each is applied again through
/// every check; applied: its conflict is closed (`resolved_remote`, `judged_valid`) and audited.
/// Returns how many applied.
pub(crate) async fn retry_crew_site_conflicts(db: &DatabaseConnection) -> AppResult<i64> {
    let mut applied = 0;
    for (inbox_id, conflict_id, sid, received) in open_planning_conflicts(db, "teams", &[WEEK_FROZEN]).await? {
        if inbound_crew_site_check(db, &sid, &received).await?.is_some() {
            continue;
        }
        let closed = reapply_conflicted(
            db,
            inbox_id,
            conflict_id,
            "judged_valid",
            "The crew's site change crosses no week frozen here any more (its jobs ended, moved or were released, \
             or the week reopened): it is applied.",
        )
        .await?;
        if closed {
            applied += 1;
            audit_closed(
                db,
                CREW_SITE_CLOSED_EVENT,
                "Crew site change conflict closed: it crosses no frozen week here any more",
                "team",
                "teams",
                conflict_id,
                &sid,
            )
            .await;
        }
    }
    Ok(applied)
}

/// Audit action of a crew site change conflict closed by this PC's rules letting it in.
pub(crate) const CREW_SITE_CLOSED_EVENT: &str = "planning.crew_site_conflict.closed";

/// Every planning conflict judged again (review R3-S4): weeks, then break-ins, then the commitments
/// they let in, then crews whose site change crossed a frozen week; then the break-ins received after their job's plan was kept are refused with that
/// decision ([`refuse_break_ins_after_kept_plan`], review R3-S5). Returns how many rows applied.
pub(crate) async fn retry_planning_conflicts(db: &DatabaseConnection) -> AppResult<i64> {
    let mut applied = retry_unfreeze_conflicts(db).await?;
    applied += retry_break_in_conflicts(db).await?;
    applied += retry_frozen_week_conflicts(db).await?;
    applied += retry_crew_site_conflicts(db).await?;
    refuse_break_ins_after_kept_plan(db).await?;
    Ok(applied)
}

/// Break-ins received from another PC after their job's move was refused here and this PC's plan
/// kept (`PLANNING_WEEK_FROZEN` resolved `accept_local`), still unrefused, that lead the job away
/// from its place here: the planner's decision applies to them too (review R3-S5). Each is marked
/// refused with that decision's reason, sent, and audited (`planning.break_in.refused_with_kept_plan`,
/// actor: the planner who kept the plan). Returns how many were refused.
pub(crate) async fn refuse_break_ins_after_kept_plan(db: &DatabaseConnection) -> AppResult<i64> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sb.entity_sync_id AS bsid, c.entity_sync_id AS csid, k.id AS kept, k.resolved_by_id AS by_id, \
                    k.resolution_note AS note, k.inbound_payload_json AS kept_payload \
               FROM schedule_break_ins sb \
               JOIN schedule_commitments c ON c.id = sb.schedule_commitment_id \
               JOIN sync_conflicts k ON k.entity_type = 'schedule_commitments' AND k.entity_sync_id = c.entity_sync_id \
              WHERE sb.sync_refused_at IS NULL \
                AND k.conflict_type = ? AND k.status = 'resolved_local' AND k.resolution_action = 'accept_local' \
                AND k.resolved_at IS NOT NULL \
                AND EXISTS (SELECT 1 FROM sync_inbox i WHERE i.entity_type = 'schedule_break_ins' \
                              AND i.entity_sync_id = sb.entity_sync_id AND i.apply_status = 'applied' \
                              AND COALESCE(i.updated_at, i.created_at) >= k.resolved_at) \
              ORDER BY sb.id, k.id DESC",
            [WEEK_FROZEN.into()],
        ))
        .await?;
    let mut refused = 0;
    let mut done: Vec<String> = Vec::new();
    for r in rows {
        let break_in: String = r.try_get("", "bsid").map_err(sync_err)?;
        if done.contains(&break_in) {
            continue;
        }
        done.push(break_in.clone());
        let commitment: String = r.try_get("", "csid").map_err(sync_err)?;
        let kept: i64 = r.try_get("", "kept").map_err(sync_err)?;
        let planner: Option<i64> = r.try_get("", "by_id").map_err(sync_err)?;
        let note: Option<String> = r.try_get("", "note").map_err(sync_err)?;
        let kept_payload: Option<String> = r.try_get("", "kept_payload").map_err(sync_err)?;
        let (Some(job), Some(move_)) = (
            local_wire(db, "schedule_commitments", &commitment).await?,
            local_wire(db, "schedule_break_ins", &break_in).await?,
        ) else {
            continue;
        };
        let Some(at) = Place::of(&job) else {
            continue;
        };
        let m = BreakInMove {
            sid: break_in.clone(),
            payload: move_,
            inbox_id: None,
            refused: false,
            by_sync: true,
        };
        // Only the refused move itself, come late: from where the job is here to the slot and crew
        // the kept conflict refused (review F4-S1); a later break-in of another move is never touched.
        let Some(Ok(Value::Object(kept_version))) = kept_payload.as_deref().map(serde_json::from_str::<Value>) else {
            continue;
        };
        let Some(refused_place) = Place::of(&overlay(Some(&job), &kept_version)) else {
            continue;
        };
        if m.no_op() || !chains(&at, &refused_place, &[], std::slice::from_ref(&m), true).is_ok_and(|c| !c.is_empty()) {
            continue;
        }
        let reason = note.unwrap_or_default();
        let tx = db.begin().await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE schedule_break_ins SET sync_refused_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
                    sync_refused_reason = ?, row_version = row_version + 1 \
              WHERE entity_sync_id = ? AND sync_refused_at IS NULL",
            [refusal_text(WEEK_FROZEN, &reason).into(), break_in.clone().into()],
        ))
        .await?;
        audit_break_in_refused(&tx, kept, &break_in, &commitment, planner, &reason).await?;
        tx.commit().await?;
        refused += 1;
    }
    Ok(refused)
}

/// Applies a conflicted inbox row again through every check, in one transaction. Applied: its
/// conflict is closed (`resolved_remote`, `action`, `note`) and `true` is returned. Waiting: left
/// as it was, tried again at the next synchronisation. Refused again: the conflict stays open with
/// the new reason.
pub(crate) async fn reapply_conflicted(
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
/// here (a commitment), the crews a break-in names (else its commitment's crew here), the crew
/// itself (its site change).
async fn conflict_crews(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    sid: &str,
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
        // A crew's site change: the crew itself (its site here).
        "teams" => push(&mut crews, local_id(conn, "teams", sid).await?),
        _ => {}
    }
    Ok(crews)
}

/// "Keep this PC's plan" on a planning four-eyes conflict, before the generic keep
/// (`sync::conflicts::keep_local_version`, which then sends this PC's version):
/// - the person holds plan.confirm at the site of every crew involved (a week: its site here);
/// - the received version is still refused by this PC's rules: one let in now (its break-in or
///   decision arrived, its week open here since) is not kept, its conflict closes by itself
///   (`GATE_SYNC_KEEP_PLAN_NOT_REFUSED`, review R2-S2);
/// - a commitment refused while new here is stored here cancelled ("refusé (sync) : semaine gelée
///   — reason", by this person, now), a break-in refused while new here is stored here marked
///   refused: that version is what is sent, so every PC ends with the same row (never a delete);
/// - a row held here is sent again (a week stays frozen; its unfreeze event stays history); the
///   other PC lets it in as the plan put back (`judge_commitment`, review R2-B1);
/// - the break-ins that brought a refused commitment are refused with it and sent
///   ([`refuse_break_ins_of`]), so the PC they came from never counts them again.
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
        let crews = conflict_crews(conn, entity_type, sid, &received, local.as_ref()).await?;
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
    // Let in by this PC's rules now (its break-ins or its unfreeze decision arrived, its rights
    // arrived, or its week is open here since): keeping would send a stale plan back, and the other PC
    // would take it (review R2-S2). Every planning conflict is judged again by the retries at the next
    // synchronisation and closes then, audited (`retry_planning_conflicts`, review R3-S4).
    let still_refused = match entity_type {
        "schedule_commitments" => inbound_commitment_check(conn, sid, &received).await?.is_some(),
        "schedule_break_ins" => local.is_some() || inbound_break_in_check(conn, sid, &received).await?.is_some(),
        "weekly_schedules" => week_still_refused(conn, sid, &received).await?,
        "teams" => inbound_crew_site_check(conn, sid, &received).await?.is_some(),
        _ => true,
    };
    if !still_refused {
        return Err(keep_refused(
            "GATE_SYNC_KEEP_PLAN_NOT_REFUSED: This PC's rules accept the received version now (its break-in or its \
             unfreeze decision has arrived, its approver's right has arrived or been granted, or its week is open \
             here): there is no plan to keep; the conflict closes by itself at the next synchronisation."
                .to_string(),
        ));
    }
    if entity_type == "weekly_schedules" {
        return Ok(());
    }

    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let refusal = refusal_text(&conflict.conflict_type, reason);
    if local.is_some() {
        // The break-ins that brought the refused version, marked refused and sent with it.
        if entity_type == "schedule_commitments" {
            refuse_break_ins_of(
                conn,
                conflict.id,
                sid,
                &received,
                local.as_ref(),
                user_id,
                &refusal,
                &now,
                reason,
            )
            .await?;
        }
        return Ok(());
    }

    // Absent here: stored here in its refused form, then sent as this PC's version.
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
    store_refused(conn, entity_type, sid, version, &refused).await?;
    // Its break-ins waited for it: stored refused too, now that it is here.
    if entity_type == "schedule_commitments" {
        refuse_break_ins_of(
            conn,
            conflict.id,
            sid,
            &received,
            None,
            user_id,
            &refusal_text(&conflict.conflict_type, reason),
            &now,
            reason,
        )
        .await?;
    }
    Ok(())
}

/// The planner-facing words of a refusal (review N-g): "refusé (sync) : <what> — <reason>".
fn refusal_text(conflict_type: &str, reason: &str) -> String {
    let what = match conflict_type {
        WEEK_FROZEN => "semaine gelée",
        BREAK_IN_APPROVAL_REQUIRED => "break-in sans approbateur",
        BREAK_IN_SELF_APPROVAL => "break-in approuvé par son demandeur",
        BREAK_IN_APPROVER_UNAUTHORISED => "approbateur sans droit sur le site",
        BREAK_IN_UNSIGNED => "break-in non signé",
        _ => "règle de planification",
    };
    format!("refusé (sync) : {what} — {reason}")
}

/// Stores a refused version received from another PC (a commitment cancelled, a break-in marked).
async fn store_refused(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    sid: &str,
    version: i64,
    refused: &Payload,
) -> AppResult<()> {
    match crate::sync::materialize::apply_inbound_payload(
        conn,
        entity_type,
        sid,
        "upsert",
        version,
        &Value::Object(refused.clone()).to_string(),
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

/// Audit action of a break-in refused with its commitment's kept plan.
pub(crate) const BREAK_IN_REFUSED_WITH_PLAN_EVENT: &str = "planning.break_in.refused_with_kept_plan";

/// "Keep this PC's plan" on a commitment refused as `PLANNING_WEEK_FROZEN`: the break-ins that brought
/// its refused version are refused with it (CMMS Advisor decisions 2-3, review R2-B1), so the PC
/// they came from takes the marks with the kept version and never counts them again:
/// - a job held here: only the break-ins received from another PC that lie on a path from its place
///   here to the received place (review R3-S3); none when the received version did not move it, and
///   never one made on this PC;
/// - a job new here (stored cancelled): every break-in of it received and not applied, its whole
///   refused history.
///
/// Received ones not held here are stored marked refused (their open conflict closed with this
/// decision); held ones are marked; all are sent. Each is audited
/// (`planning.break_in.refused_with_kept_plan`, review N3-h).
#[allow(clippy::too_many_arguments)]
async fn refuse_break_ins_of(
    conn: &impl ConnectionTrait,
    kept_conflict: i64,
    sid: &str,
    received: &Payload,
    local: Option<&Payload>,
    user_id: i64,
    refusal: &str,
    now: &str,
    reason: &str,
) -> AppResult<()> {
    let (here, inbox) = break_in_moves(conn, sid, true).await?;
    let mut chosen: Vec<BreakInMove> = Vec::new();
    match (local.and_then(Place::of), Place::of(&overlay(local, received))) {
        (Some(from), Some(to)) => {
            if !from.same(&to) {
                for chain in chains(&from, &to, &here, &inbox, true).unwrap_or_default() {
                    for m in chain {
                        if m.by_sync && !m.refused && !chosen.iter().any(|c| c.sid == m.sid) {
                            chosen.push(m);
                        }
                    }
                }
            }
        }
        _ => chosen.extend(inbox.into_iter().filter(|m| !m.refused)),
    }
    for m in &chosen {
        match m.inbox_id {
            // Received, not held here: stored marked refused, sent on top of the server's version.
            Some(inbox_id) => {
                let Some(row) = conn
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT row_version, payload_json FROM sync_inbox WHERE id = ?",
                        [inbox_id.into()],
                    ))
                    .await?
                else {
                    continue;
                };
                let version: i64 = row.try_get("", "row_version").map_err(sync_err)?;
                let json: String = row.try_get("", "payload_json").map_err(sync_err)?;
                let mut marked = m.payload.clone();
                marked.insert("sync_refused_at".into(), now.into());
                marked.insert("sync_refused_reason".into(), refusal.into());
                store_refused(conn, "schedule_break_ins", &m.sid, version, &marked).await?;
                crate::sync::queries::overwrite_synced_state(
                    conn,
                    "schedule_break_ins",
                    &m.sid,
                    version,
                    &crate::sync::queries::hash_payload(&json),
                    "conflict_keep_local",
                )
                .await?;
                conn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE sync_inbox SET apply_status = 'superseded', \
                            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                      WHERE entity_type = 'schedule_break_ins' AND entity_sync_id = ? \
                        AND apply_status IN ('pending', 'deferred', 'conflict')",
                    [m.sid.clone().into()],
                ))
                .await?;
                crate::sync::capture::restage_current_state(conn, "schedule_break_ins", &m.sid, &[Some(&json)]).await?;
            }
            // Held here (it arrived before its job): marked; the change is captured and sent.
            None => {
                conn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE schedule_break_ins SET sync_refused_at = ?, sync_refused_reason = ?, \
                            row_version = row_version + 1 \
                      WHERE entity_sync_id = ? AND sync_refused_at IS NULL",
                    [now.into(), refusal.into(), m.sid.clone().into()],
                ))
                .await?;
            }
        }
        audit_break_in_refused(conn, kept_conflict, &m.sid, sid, Some(user_id), reason).await?;
    }
    Ok(())
}

/// Audits a break-in refused with a kept plan; an open conflict of its own is closed with that
/// decision (target: that conflict; otherwise the kept one).
async fn audit_break_in_refused(
    conn: &impl ConnectionTrait,
    kept_conflict: i64,
    break_in: &str,
    commitment: &str,
    user_id: Option<i64>,
    reason: &str,
) -> AppResult<()> {
    let open = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, conflict_type FROM sync_conflicts WHERE entity_type = 'schedule_break_ins' \
                AND entity_sync_id = ? AND status IN ('new', 'triaged', 'escalated')",
            [break_in.into()],
        ))
        .await?;
    let mut own: Vec<(i64, String)> = Vec::new();
    for c in open {
        own.push((
            c.try_get("", "id").map_err(sync_err)?,
            c.try_get("", "conflict_type").map_err(sync_err)?,
        ));
    }
    for (conflict_id, _) in &own {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sync_conflicts SET status = 'resolved_local', resolution_action = 'accept_local', \
                    resolution_note = ?, resolved_by_id = ?, \
                    resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1, \
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
              WHERE id = ?",
            [reason.into(), user_id.into(), (*conflict_id).into()],
        ))
        .await?;
    }
    let (target, conflict_type) = own
        .first()
        .map_or((kept_conflict, WEEK_FROZEN.to_string()), |(id, t)| (*id, t.clone()));
    crate::audit::writer::write_audit_event_in(
        conn,
        crate::audit::writer::AuditEventInput {
            action_code: BREAK_IN_REFUSED_WITH_PLAN_EVENT.to_string(),
            target_type: Some("sync_conflict".to_string()),
            target_id: Some(target.to_string()),
            actor_id: user_id,
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "operations".to_string(),
            details_json: Some(json!({
                "conflict_type": conflict_type,
                "kept_conflict_id": kept_conflict,
                "entity_type": "schedule_break_ins",
                "entity_sync_id": break_in,
                "commitment_sync_id": commitment,
                "reason": reason,
            })),
        },
    )
    .await?;
    Ok(())
}

// ── The times a frozen-week conflict card shows (CMMS Advisor decision 4, review N-l) ─────────

/// The two times a `PLANNING_WEEK_FROZEN` conflict card shows next to each other: when the other PC
/// changed the commitment (that PC's clock, as received) and when the week froze here (or was due
/// to). Shown only; neither ever lets the work in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlanningConflictTimes {
    pub changed_at: Option<String>,
    pub frozen_at: Option<String>,
}

/// The [`PlanningConflictTimes`] of a `PLANNING_WEEK_FROZEN` conflict on a commitment, read from its
/// received version and the week as it stands here; `None` for any other conflict.
pub(crate) async fn planning_conflict_times(
    conn: &impl ConnectionTrait,
    conflict_type: &str,
    entity_type: &str,
    entity_sync_id: &str,
    inbound_payload_json: Option<&str>,
) -> AppResult<Option<PlanningConflictTimes>> {
    if conflict_type != WEEK_FROZEN || entity_type != "schedule_commitments" {
        return Ok(None);
    }
    let Some(Ok(Value::Object(received))) = inbound_payload_json.map(serde_json::from_str::<Value>) else {
        return Ok(None);
    };
    let changed_at = field(&received, "updated_at")
        .or_else(|| field(&received, "created_at"))
        .map(str::to_string);
    let frozen_at = match commitment_context(conn, entity_sync_id, &received).await? {
        Ok(ctx) => {
            let week = match ctx.team {
                Some(team) => {
                    super::weekly_schedule::frozen_week_for_slot(
                        conn,
                        team,
                        &ctx.to.start,
                        &ctx.to.end,
                        crate::clock::now(),
                    )
                    .await?
                }
                None => None,
            };
            week.and_then(|w| w.frozen_at).or_else(|| {
                ctx.local
                    .as_ref()
                    .and_then(|l| field(l, "frozen_at"))
                    .map(str::to_string)
            })
        }
        Err(_) => None,
    };
    Ok(Some(PlanningConflictTimes { changed_at, frozen_at }))
}
