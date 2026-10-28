//! Safety rules re-checked on rows received by synchronisation (`sync::apply_hooks`).
//!
//! A permit, an isolation or a work order changed on another device reaches this one as a row. The
//! rules enforced where the change was made are checked again here, on the state received, before
//! anything is written — never stricter than the local rules (a stricter rule would refuse work that
//! is valid where it was done):
//! - a permit becoming `active` has its activation time, an expiry later than the moment it protects
//!   the work (activation or last reinstatement), and, when its type requires energy isolation, at
//!   least one isolation; every isolation verified by a second person and no lock removed before that
//!   moment;
//! - a permit `closed` or `handed_back` has every lock removal verified; `handed_back` has a handover
//!   log that keeps the hand-over rule; `suspended` / `revalidation_required` has an open suspension;
//! - a handover log (`handover_rule_breach`): the person receiving the permit is not the person who
//!   signed the hand-over. A log without a signer or receiver passes (devices before signers were
//!   recorded send none); a log naming a person not applied here yet waits for that person (the
//!   generic apply);
//! - a permit never changes its permit type (as no local edit does);
//! - four-eyes on its approval (`four_eyes_check`, migration 244): the approver is neither the
//!   author nor the submitter written on the row, unless it is flagged a self-approval and the
//!   permit's site opted out of four-eyes here; a permit past review names who approved it; the
//!   author (or its absence), and once past review here the submission and approval, are never
//!   removed or rewritten;
//! - a permit's equipment, linked work order or site changes only while the copy held here is a
//!   draft or waiting for review (a change of equipment or work order there needs the versions of a
//!   return to draft), with no lock fitted under it here without a verified removal and no
//!   isolation point planned on it here for a change of equipment or work order; its linked work
//!   order is never removed, its work order is on its equipment, and its node is the one derived
//!   from them or a level above it, by the local rule (`equipment_change_check`);
//! - a permit never leaves `closed`, `handed_back`, `cancelled` or `expired` except along the lifecycle
//!   (`closed` → `handed_back`, `handed_back` → `cancelled`);
//! - an isolation (`isolation_rule_breach`): verified after it was applied and by someone other than
//!   the person who applied it; its lock removal verified after verification and by someone other
//!   than the applier (and, when the permit's type requires the three-person chain, than the
//!   isolation verifier). It never moves to another permit; it is added, applied or verified (and its
//!   people never change) only while its permit is prepared or suspended — when that change reaches
//!   a permit protecting the work, it must date from before the permit started protecting it; its
//!   lock removal is verified only once the permit was active (`ISOLATION_SETUP_STATUSES` and
//!   `ISOLATION_REMOVAL_STATUSES` of `permit::queries`);
//! - a lock taken off (migration 249, `lock_removal_breach`, `lock_record_check`): a normal removal
//!   is recorded by the lock's applier, with who and when; a forced removal (applier absent) names an
//!   authoriser who is neither the applier nor the remover, a remover who is not the applier, a
//!   reason of at least 20 characters, the attestation that the worker is not on site and at least
//!   one well-formed contact attempt, and its authoriser holds `ptw.close` at the permit's site
//!   here; the removal is verified only after the lock was taken off, never by the remover; the
//!   worker is told of a forced removal by their own in-app acknowledgement or by a person holding
//!   `ptw.close` at the site here, after the removal; a lock taken off happens only once the permit
//!   was active; once recorded here, the removal and the telling are never rewritten; and a worker
//!   with a forced removal they were not told of before fits no new lock (`forced_removal_block`);
//! - an isolation line cancelled (migration 243, `cancellation_check`): only a line never applied
//!   (not here, not in the row), with who and when, on a permit held here as a draft or waiting for
//!   review (as before the change when it arrives with its permit's change); a line cancelled here
//!   is kept as it was (never un-cancelled, applied or edited). A cancelled line never applied is
//!   no isolation of its permit: not counted by the activation, close and equipment-change checks;
//! - a work order entering `in_progress` passes the permit gate (`permit::wo_gate`) at the time it
//!   started, resumed or was reopened: a permit when it requires one (of the type a plant rule named,
//!   when a rule flagged it), every permit live at that time active, not suspended, not expired, not
//!   expiring before the end of the work (the planned end; without one, that time plus the
//!   expected duration), with its isolations ready; the same gate judges a work order arriving
//!   `completed` or `closed` that this PC never held in execution (new here, or draft, planning or
//!   ready here: its start was made elsewhere), except plant history received by a snapshot;
//! - a permit type whose three-person lock-removal setting changes (or a new type with it on), or
//!   whose energy-isolation rule is switched off, or whose record of such a change changes, says who
//!   changed it, when (later than the change recorded here) and why, and that person holds the
//!   settings and permit-issue rights here; the shipped LOTO type's energy-isolation rule is never
//!   switched off, and a type's code never changes (`inbound_permit_type_check`);
//! - deletes (this device never deletes them): a permit, its isolations and suspensions only while
//!   the permit is a draft (the LOTO history of any other permit is kept); a permit type or a
//!   qualification requirement never (they are deactivated or edited).
//!
//! Judged on the state, not the path (an offline device may send several steps as one row), and at the
//! times written in the rows, compared exactly. This device's clock is used only to refuse a time
//! in the future, beyond the tolerated difference between device clocks (`MAX_CLOCK_SKEW_MINUTES`).
//! A permit already held here in the same status (and of the same type) passes.
//!
//! Rows that arrive together are applied together. A permit's change is judged with its isolations,
//! suspensions and handover logs still waiting in the inbox, and a work order's start with its
//! waiting permits, waits and status changes — the children arrive after the row they name, so
//! waiting for them would never end. Such a waiting child counts only when it is pending (or set
//! aside only for its missing parent), passes the checks before an apply (not older than the version
//! applied here, no unsent local change of it, no code clash) and everything it refers to (its
//! parent excepted) is applied here; otherwise the parent waits (shown as deferred, naming the row it
//! waits for; it waits as long as that row does not come). The counted children are then applied in
//! the same transaction as the parent, through the same inbox apply as any received row
//! (`sync::materialize::apply_inbox_row_in`: checks, apply, inbox status, synced version), each with
//! its own full checks (`apply_group`): if any is refused, nothing of the group is written, the parent
//! is a sync conflict and the children stay waiting, to be judged on their own. A rule that fails is
//! a sync conflict for a person to decide, with nothing written.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{Map, Value};

use crate::errors::{AppError, AppResult};
use crate::permit::wo_gate::Hours;
use crate::settings::site::{instant, InboundVerdict, MAX_CLOCK_SKEW_MINUTES};

pub(crate) type Payload = Map<String, Value>;

/// Work-order statuses past "ready": the work started (`wo::workflow::state_machine`).
const PAST_READY: &[&str] = &["in_progress", "on_hold", "completed", "closed"];

/// Permit statuses after which a permit no longer protects work (`permit::queries`).
const TERMINAL: &[&str] = &["closed", "handed_back", "cancelled", "expired"];
/// Statuses in which isolations are applied and verified (`permit::queries::ISOLATION_SETUP_STATUSES`).
const ISOLATION_SETUP: &[&str] = &[
    "draft",
    "pending_review",
    "approved",
    "issued",
    "suspended",
    "revalidation_required",
];
/// Permit statuses whose arrival is judged (and applied with the children it was judged on).
const JUDGED_STATUSES: &[&str] = &["active", "closed", "handed_back", "suspended", "revalidation_required"];

tokio::task_local! {
    /// While the children counted for a permit's change are applied with it (`apply_group`): that
    /// permit and its status before the change (`None`: new here).
    static GROUP_PARENT: (String, Option<String>, bool);
}

/// A non-empty text field of a payload.
pub(crate) fn field<'a>(p: &'a Payload, key: &str) -> Option<&'a str> {
    p.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn flag(p: &Payload, key: &str) -> bool {
    match p.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

fn time(p: &Payload, key: &str) -> Option<DateTime<Utc>> {
    field(p, key).and_then(instant)
}

/// A number of hours in a payload, read as this PC reads the stored column (`permit::wo_gate::Hours`):
/// absent when missing, null or blank; a JSON number, or a number sent as text (SQLite's REAL
/// affinity stores numeric text as that number); anything else is unreadable (refused when it is
/// judged, as on this PC), never taken as "no value".
fn hours(p: &Payload, key: &str) -> Hours {
    match p.get(key) {
        None | Some(Value::Null) => Hours::Absent,
        Some(Value::Number(n)) => n
            .as_f64()
            .map_or_else(|| Hours::Unreadable(n.to_string()), Hours::Value),
        Some(Value::String(s)) => Hours::from_text(Some(s)),
        Some(other) => Hours::Unreadable(other.to_string()),
    }
}

/// Latest time a received row may carry: this device's clock plus the tolerated difference.
fn future_bound() -> DateTime<Utc> {
    Utc::now() + Duration::minutes(MAX_CLOCK_SKEW_MINUTES)
}

pub(crate) fn conflict(code: &'static str, message: String) -> AppResult<Option<InboundVerdict>> {
    Ok(Some(InboundVerdict::Conflict { code, message }))
}

pub(crate) fn wait(message: String) -> AppResult<Option<InboundVerdict>> {
    Ok(Some(InboundVerdict::Defer(message)))
}

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("safety re-check of a received row: {e}"))
}

/// The local row with this sync id, as this device would send it.
pub(crate) async fn local_wire(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    sync_id: &str,
) -> AppResult<Option<Payload>> {
    let layout = crate::sync::row_layout::layout(conn, entity_type).await?;
    Ok(crate::sync::row_codec::load_row_payload(
        conn,
        &layout,
        &crate::sync::row_codec::RowLocator::Identity(sync_id.to_string()),
    )
    .await?
    .map(|r| r.payload))
}

/// The received row completed by the local one (a field left out keeps its local value).
pub(crate) fn overlay(local: Option<&Payload>, received: &Payload) -> Payload {
    let mut merged = local.cloned().unwrap_or_default();
    for (k, v) in received {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// The isolation rule a state of an isolation breaks, if any (`permit::queries`: verify and verify
/// removal). `three_person`: the permit's type requires the three-person chain (migration 236), so the
/// lock removal is verified by someone other than the isolation verifier too.
pub(crate) fn isolation_rule_breach(iso: &Payload, three_person: bool) -> Option<(&'static str, String)> {
    let point = field(iso, "isolation_point").unwrap_or("?");
    let applier = field(iso, "applied_by_sync_id");
    if field(iso, "cancelled_at").is_some() && ISOLATION_ACTION_KEYS.iter().any(|k| field(iso, k).is_some()) {
        return Some((
            "PERMIT_ISOLATION_CANCEL_APPLIED",
            format!("isolation '{point}' is cancelled but was applied, verified or removed"),
        ));
    }
    if field(iso, "verified_at").is_some() {
        if applier.is_none() || field(iso, "applied_at").is_none() {
            return Some((
                "PERMIT_ISOLATION_NOT_APPLIED",
                format!("isolation '{point}' is verified but was never applied"),
            ));
        }
        if field(iso, "verified_by_sync_id").is_none_or(|v| Some(v) == applier) {
            return Some((
                "PERMIT_TWO_PERSON_VERIFY",
                format!("isolation '{point}' was verified by the person who applied it (or by nobody)"),
            ));
        }
    }
    if field(iso, "removal_verified_at").is_some() {
        if field(iso, "verified_at").is_none() {
            return Some((
                "PERMIT_REMOVAL_NOT_VERIFIABLE",
                format!("isolation '{point}': lock removal verified on an isolation never verified"),
            ));
        }
        if field(iso, "removal_verified_by_sync_id").is_none_or(|v| Some(v) == applier) {
            return Some((
                "PERMIT_TWO_PERSON_REMOVAL",
                format!("isolation '{point}': lock removal verified by the person who applied it (or by nobody)"),
            ));
        }
        if three_person && field(iso, "removal_verified_by_sync_id") == field(iso, "verified_by_sync_id") {
            return Some((
                "PERMIT_THREE_PERSON_REMOVAL",
                format!(
                    "isolation '{point}': lock removal verified by the person who verified it (the permit type \
                     requires a third person)"
                ),
            ));
        }
    }
    lock_removal_breach(iso, point)
}

/// The lock-removal rules a state of an isolation breaks, if any (migration 249,
/// `permit::queries::record_permit_isolation_removed`, `force_permit_isolation_removal`,
/// `acknowledge_forced_removal`, `record_worker_informed`). Judged on the row alone.
fn lock_removal_breach(iso: &Payload, point: &str) -> Option<(&'static str, String)> {
    let applier = field(iso, "applied_by_sync_id");
    let remover = field(iso, "removed_by_sync_id");
    let removed_at = field(iso, "removed_at");
    let forced = flag(iso, "forced_removal");
    let invalid = |code: &'static str, what: &str| Some((code, format!("isolation '{point}': {what}")));
    if remover.is_some() != removed_at.is_some() || removed_at.is_some_and(|t| instant(t).is_none()) {
        return invalid(
            "PERMIT_LOCK_REMOVAL_INVALID",
            "lock taken off without who or when, or at an unreadable time",
        );
    }
    let removed = removed_at.and_then(instant);
    if removed.is_some() && (applier.is_none() || field(iso, "applied_at").is_none()) {
        return invalid("PERMIT_ISOLATION_NOT_APPLIED", "lock taken off but never applied");
    }
    if removed.is_some_and(|t| t > future_bound()) {
        return invalid("PERMIT_LOCK_REMOVAL_INVALID", "lock taken off at a time in the future");
    }
    let authoriser = field(iso, "forced_authorized_by_sync_id");
    if forced {
        let reason_ok = field(iso, "forced_reason")
            .is_some_and(|r| r.chars().count() >= crate::permit::domain::MIN_FORCED_REMOVAL_REASON_CHARS);
        if removed.is_none()
            || authoriser.is_none()
            || !reason_ok
            || !flag(iso, "forced_worker_absent")
            || !contact_attempts_valid(iso)
        {
            return invalid(
                "PERMIT_FORCED_REMOVAL_INVALID",
                "forced removal without its remover, time, authoriser, reason (20 characters at least), \
                 attestation that the worker is absent or contact attempts",
            );
        }
        if authoriser == applier {
            return invalid(
                "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_APPLIER",
                "forced removal authorised by the lock's own applier",
            );
        }
        if authoriser == remover {
            return invalid(
                "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_REMOVER",
                "forced removal authorised by the person who took the lock off",
            );
        }
        if remover == applier {
            return invalid(
                "PERMIT_FORCED_REMOVAL_REMOVER_IS_APPLIER",
                "forced removal whose remover is the absent applier",
            );
        }
    } else {
        if authoriser.is_some()
            || field(iso, "forced_reason").is_some()
            || field(iso, "forced_contact_attempts_json").is_some()
            || flag(iso, "forced_worker_absent")
        {
            return invalid(
                "PERMIT_FORCED_REMOVAL_INVALID",
                "a forced-removal record on a lock removal not flagged forced",
            );
        }
        if remover.is_some() && remover != applier {
            return invalid(
                "PERMIT_LOCK_REMOVAL_NOT_APPLIER",
                "lock removal recorded by someone other than its applier",
            );
        }
    }
    if field(iso, "removal_verified_at").is_some() {
        let verified = time(iso, "removal_verified_at");
        // A removal verified with no trace of the lock taken off at all: verified on a PC before
        // the update (migration 249) and received here (CMMS Advisor, S2 option a: kept refused,
        // a person checks on site, then resolves the conflict; the lock stays fitted here).
        if remover.is_none() && removed_at.is_none() {
            return invalid(
                "PERMIT_REMOVAL_PRE_UPGRADE",
                "lock removal verified before the update, with no trace of the lock taken off by its applier \
                 (retrait vérifié avant la mise à jour, sans trace du retrait par le poseur) — check on site, then \
                 resolve",
            );
        }
        if removed.is_none() || verified.is_none_or(|v| removed.is_some_and(|r| v < r)) {
            return invalid(
                "PERMIT_REMOVAL_LOCK_NOT_REMOVED",
                "lock removal verified before the lock was taken off",
            );
        }
        if field(iso, "removal_verified_by_sync_id") == remover {
            return invalid(
                "PERMIT_REMOVAL_VERIFIER_IS_REMOVER",
                "lock removal verified by the person who took it off",
            );
        }
    }
    let informer = field(iso, "informed_by_sync_id");
    let informed_at = field(iso, "informed_at");
    let in_app = flag(iso, "informed_in_app");
    let method = field(iso, "informed_method");
    if informer.is_none() && informed_at.is_none() {
        if in_app || method.is_some() {
            return invalid("PERMIT_WORKER_INFORMED_INVALID", "worker told without who or when");
        }
        return None;
    }
    let told = informed_at.and_then(instant);
    if !forced
        || informer.is_none()
        || told.is_none_or(|t| t > future_bound() || removed.is_some_and(|r| t < r))
        || (in_app && method.is_some())
        || (!in_app && (method.is_none() || informer == applier))
    {
        return invalid(
            "PERMIT_WORKER_INFORMED_INVALID",
            "worker told of a forced removal without who, when (after the removal) or how, or of a removal \
             not forced",
        );
    }
    if in_app && informer != applier {
        return invalid(
            "PERMIT_WORKER_ACK_NOT_YOURS",
            "forced removal acknowledged in the app by someone other than the worker",
        );
    }
    None
}

/// The contact attempts of a forced removal, as the save checks them: a JSON list of 1 to
/// [`crate::permit::domain::MAX_CONTACT_ATTEMPTS`] attempts, each at a readable time not in the
/// future, with a method and a result of at most [`crate::permit::domain::MAX_CONTACT_RESULT_CHARS`]
/// characters. The method is not checked against the plant's list here (the list may reach this PC
/// after the row).
fn contact_attempts_valid(iso: &Payload) -> bool {
    use crate::permit::domain::{ContactAttempt, MAX_CONTACT_ATTEMPTS, MAX_CONTACT_RESULT_CHARS};
    let Some(raw) = field(iso, "forced_contact_attempts_json") else {
        return false;
    };
    let Ok(attempts) = serde_json::from_str::<Vec<ContactAttempt>>(raw) else {
        return false;
    };
    !attempts.is_empty()
        && attempts.len() <= MAX_CONTACT_ATTEMPTS
        && attempts.iter().all(|a| {
            instant(&a.at).is_some_and(|t| t <= future_bound())
                && !a.method.trim().is_empty()
                && !a.result.trim().is_empty()
                && a.result.trim().chars().count() <= MAX_CONTACT_RESULT_CHARS
        })
}

/// The keys of an isolation's actions (who fitted, verified and verified the removal, and when).
const ISOLATION_ACTION_KEYS: &[&str] = &[
    "applied_by_sync_id",
    "applied_at",
    "verified_by_sync_id",
    "verified_at",
    "removal_verified_by_sync_id",
    "removal_verified_at",
    "removed_by_sync_id",
    "removed_at",
];

/// The record of a lock taken off (migration 249): who and when, and the forced-removal record.
const LOCK_REMOVAL_KEYS: &[&str] = &[
    "removed_by_sync_id",
    "removed_at",
    "forced_authorized_by_sync_id",
    "forced_reason",
    "forced_contact_attempts_json",
];
/// The record of the worker told of a forced removal.
const WORKER_INFORMED_KEYS: &[&str] = &["informed_by_sync_id", "informed_at", "informed_method"];

/// A cancelled line never applied (migration 243): no isolation of its permit, never a lock to fit
/// (`permit::queries::COUNTED_ISOLATION_SQL`). A line that would hold both still counts.
fn cancelled_line(iso: &Payload) -> bool {
    field(iso, "cancelled_at").is_some() && field(iso, "applied_at").is_none()
}

/// The lines that count as isolations of their permit (cancelled planned lines left out).
fn counted_isolations<'a>(isolations: &[&'a Payload]) -> Vec<&'a Payload> {
    isolations.iter().copied().filter(|i| !cancelled_line(i)).collect()
}

/// The hand-over rule a state of a handover log breaks, if any (`permit::queries`: append a
/// handover log): the person receiving the permit is not the person who signed the hand-over. A log
/// without a signer or a receiver keeps the rule (nobody is invented for older logs).
pub(crate) fn handover_rule_breach(log: &Payload) -> Option<(&'static str, String)> {
    let signer = field(log, "signed_by_sync_id")?;
    if field(log, "received_by_sync_id") != Some(signer) {
        return None;
    }
    let roles = format!(
        "{} -> {}",
        field(log, "handed_from_role").unwrap_or("?"),
        field(log, "handed_to_role").unwrap_or("?")
    );
    Some((
        "PERMIT_HANDOVER_RECEIVER_IS_SIGNER",
        format!("hand-over '{roles}' is received by the person who signed it"),
    ))
}

/// Re-check of a handover log received by synchronisation: refused when its receiver is its
/// signer. A log already held here is never rewritten (append-only), so it is not judged again.
pub(crate) async fn inbound_handover_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    if local_wire(conn, "permit_handover_logs", entity_sync_id)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    match handover_rule_breach(received) {
        Some((c, m)) => conflict(c, m),
        None => Ok(None),
    }
}

/// A waiting inbox row applied together with the row it belongs to.
#[derive(Debug, Clone)]
pub(crate) struct GroupRow {
    entity_type: &'static str,
    inbox_id: i64,
}

impl GroupRow {
    /// The waiting inbox row `inbox_id` of `entity_type`, applied in a group (a count's reversal
    /// applied with its held adjustment, `inventory::count_four_eyes::count_movement_group`).
    pub(crate) fn new(entity_type: &'static str, inbox_id: i64) -> Self {
        Self { entity_type, inbox_id }
    }

    /// The inbox row applied in the group.
    pub(crate) fn inbox_id(&self) -> i64 {
        self.inbox_id
    }
}

/// Rows applied in the same transaction as a received permit or work order (`apply_group`).
#[derive(Debug, Clone)]
pub(crate) struct Group {
    parent_sync_id: String,
    previous_status: Option<String>,
    rows: Vec<GroupRow>,
    /// The parent's received change is judged (`false`: a work order new here received by the
    /// snapshot of this PC's first join, the plant's history; its children are applied with it, not
    /// judged again).
    judged: bool,
    /// A received work-order status change (or a work order new here): its close-out children are
    /// in the group, and a completion or close is judged on the written state after it
    /// (`wo::closeout_sync::judge_closeout`). `None` for permit and start-only groups.
    pub(crate) closeout: Option<crate::wo::closeout_sync::CloseoutEntry>,
}

impl Group {
    /// A group of waiting rows applied with `parent_sync_id`, judged against `previous_status`
    /// (the parent's status here before the change; `None`: new here).
    pub(crate) fn new(
        parent_sync_id: String,
        previous_status: Option<String>,
        rows: Vec<GroupRow>,
        judged: bool,
        closeout: Option<crate::wo::closeout_sync::CloseoutEntry>,
    ) -> Self {
        Self {
            parent_sync_id,
            previous_status,
            rows,
            judged,
            closeout,
        }
    }

    /// The rows of a work-order start group followed by those of its close-out group (the same
    /// received work order): one group, judged against the close-out group's previous status.
    pub(crate) fn followed_by(self, closeout: Group) -> Group {
        let mut rows = self.rows;
        for row in closeout.rows {
            if !rows.iter().any(|r| r.inbox_id == row.inbox_id) {
                rows.push(row);
            }
        }
        Group { rows, ..closeout }
    }
}

/// While the children of a group are applied (`apply_group`): the status `parent_sync_id` had here
/// before the change (`Some(None)`: new here); `None` outside that parent's group.
pub(crate) fn group_previous_status(parent_sync_id: &str) -> Option<Option<String>> {
    GROUP_PARENT
        .try_with(|(parent, previous, _)| (parent == parent_sync_id).then(|| previous.clone()))
        .ok()
        .flatten()
}

/// While the children of a group are applied: whether `parent_sync_id`'s received change is judged
/// (`Some(false)`: snapshot history); `None` outside that parent's group.
pub(crate) fn group_parent_judged(parent_sync_id: &str) -> Option<bool> {
    GROUP_PARENT
        .try_with(|(parent, _, judged)| (parent == parent_sync_id).then_some(*judged))
        .ok()
        .flatten()
}

/// The rows of one table naming a parent: the local ones with the counted waiting versions laid over
/// them, the counted waiting rows themselves, and why a waiting row cannot be counted yet.
pub(crate) struct Children {
    pub(crate) rows: Vec<(String, Payload)>,
    pub(crate) counted: Vec<GroupRow>,
    pub(crate) blocked: Option<String>,
}

/// Rows of `table` naming the parent `parent_sync_id` (a row of `parent_table`; local column
/// `parent_col`, payload key `parent_key`). A waiting inbox row counts when it is pending, or deferred
/// only for a missing parent (it arrived before its parent); when it passes the checks before an apply
/// (`sync::materialize::inbox_row_admissible`: not stale, no unsent local change, no code clash);
/// when everything it refers to other than this parent is applied here (otherwise: `blocked`); and,
/// for an isolation or a handover log, when it keeps the isolation or hand-over rules (otherwise it
/// is left to be refused on its own). A row moved to another parent by its waiting version no longer
/// counts.
pub(crate) async fn children(
    conn: &impl ConnectionTrait,
    table: &'static str,
    parent_table: &str,
    parent_col: &str,
    parent_key: &str,
    parent_sync_id: &str,
) -> AppResult<Children> {
    children_with(conn, table, parent_table, parent_col, parent_key, parent_sync_id, &[]).await
}

/// [`children`], where a reference to one of `applied_with` (rows counted in the same group,
/// applied before these) does not block a waiting row.
async fn children_with(
    conn: &impl ConnectionTrait,
    table: &'static str,
    parent_table: &str,
    parent_col: &str,
    parent_key: &str,
    parent_sync_id: &str,
    applied_with: &[(&str, String)],
) -> AppResult<Children> {
    let three_person = table == "permit_isolations" && permit_three_person(conn, parent_sync_id).await?;
    let mut rows: BTreeMap<String, Payload> = BTreeMap::new();
    let local = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT c.entity_sync_id AS sid FROM \"{table}\" c JOIN \"{parent_table}\" p ON p.id = c.\"{parent_col}\" \
                 WHERE p.entity_sync_id = ?"
            ),
            [parent_sync_id.into()],
        ))
        .await?;
    for r in local {
        // Rows recorded before they had a sync identity are read by their callers directly.
        let Some(sid) = r.try_get::<Option<String>>("", "sid").map_err(sync_err)? else {
            continue;
        };
        if let Some(p) = local_wire(conn, table, &sid).await? {
            rows.insert(sid, p);
        }
    }
    let layout = crate::sync::row_layout::layout(conn, table).await?;
    let waiting = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT id FROM sync_inbox \
                 WHERE entity_type = ? \
                   AND (apply_status = 'pending' OR (apply_status = 'deferred' AND rejection_code = ?)) \
                   AND operation NOT IN ('delete', 'archive') \
                   AND json_extract(payload_json, '$.{parent_key}') = ? \
                 ORDER BY row_version, id"
            ),
            [
                table.into(),
                crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                parent_sync_id.into(),
            ],
        ))
        .await?;
    let mut counted = Vec::new();
    let mut blocked = None;
    for r in waiting {
        let inbox_id: i64 = r.try_get("", "id").map_err(sync_err)?;
        let Some(item) = crate::sync::materialize::load_inbox_row(conn, inbox_id).await? else {
            continue;
        };
        let sid = item.entity_sync_id.clone();
        let Ok(Value::Object(received)) = serde_json::from_str::<Value>(&item.payload_json) else {
            continue;
        };
        if !crate::sync::materialize::inbox_row_admissible(conn, &item).await? {
            continue;
        }
        let missing: Vec<(&'static str, String)> = crate::sync::row_codec::missing_refs(conn, &layout, &received)
            .await?
            .into_iter()
            .filter(|(entity, id)| !(*entity == parent_table && id == parent_sync_id))
            .filter(|(entity, id)| !applied_with.iter().any(|(e, i)| e == entity && i == id))
            .collect();
        if let Some((entity, id)) = missing.first() {
            blocked = Some(format!("{table} {sid} waits for {entity} {id}"));
            continue;
        }
        let merged = overlay(rows.get(&sid), &received);
        if table == "permit_isolations" && isolation_rule_breach(&merged, three_person).is_some() {
            continue;
        }
        if table == "permit_handover_logs" && handover_rule_breach(&merged).is_some() {
            continue;
        }
        rows.insert(sid.clone(), merged);
        counted.push(GroupRow {
            entity_type: table,
            inbox_id,
        });
    }
    Ok(Children {
        rows: rows
            .into_iter()
            .filter(|(_, p)| field(p, parent_key) == Some(parent_sync_id))
            .collect(),
        counted,
        blocked,
    })
}

pub(crate) async fn permit_children(
    conn: &impl ConnectionTrait,
    table: &'static str,
    permit_sync_id: &str,
) -> AppResult<Children> {
    children(
        conn,
        table,
        "work_permits",
        "permit_id",
        "permit_sync_id",
        permit_sync_id,
    )
    .await
}

pub(crate) fn payloads(c: &Children) -> Vec<&Payload> {
    c.rows.iter().map(|(_, p)| p).collect()
}

/// Whether permits of the type named by the payload need energy isolation; `None` when the type is
/// not here yet. Read from this device's type, as the local activation does.
async fn type_requires_isolation(conn: &impl ConnectionTrait, permit: &Payload) -> AppResult<Option<bool>> {
    let Some(type_sync) = field(permit, "permit_type_sync_id") else {
        return Ok(None);
    };
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(requires_energy_isolation, 0) AS r FROM permit_types WHERE entity_sync_id = ?",
            [type_sync.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "r").map_err(sync_err))
        .transpose()?
        .map(|v| v != 0))
}

/// Whether the type of the permit named by the payload requires the three-person lock-removal chain
/// (read from this device's type, as the local removal check does; a type not here: no).
async fn type_three_person(conn: &impl ConnectionTrait, permit: &Payload) -> AppResult<bool> {
    let Some(type_sync) = field(permit, "permit_type_sync_id") else {
        return Ok(false);
    };
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(requires_three_person_removal, 0) AS r FROM permit_types WHERE entity_sync_id = ?",
            [type_sync.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "r").map_err(sync_err))
        .transpose()?
        .is_some_and(|v| v != 0))
}

/// The three-person setting of a permit's type, for the permit here or still waiting in the inbox.
async fn permit_three_person(conn: &impl ConnectionTrait, permit_sync_id: &str) -> AppResult<bool> {
    if let Some(permit) = local_wire(conn, "work_permits", permit_sync_id).await? {
        return type_three_person(conn, &permit).await;
    }
    let waiting = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT payload_json FROM sync_inbox WHERE entity_type = 'work_permits' AND entity_sync_id = ? \
               AND apply_status IN ('pending', 'deferred') ORDER BY row_version DESC, id DESC LIMIT 1",
            [permit_sync_id.into()],
        ))
        .await?;
    let Some(row) = waiting else {
        return Ok(false);
    };
    let json: String = row.try_get("", "payload_json").map_err(sync_err)?;
    match serde_json::from_str::<Value>(&json) {
        Ok(Value::Object(permit)) => type_three_person(conn, &permit).await,
        _ => Ok(false),
    }
}

/// The moment a permit (last) started protecting the work: its activation, or its last
/// reinstatement when later. Reinstatements dated after the permit's own close or in the future are
/// ignored. `None` without a readable activation time.
fn protection_start(permit: &Payload, suspensions: &[&Payload]) -> Option<DateTime<Utc>> {
    let activated = time(permit, "activated_at")?;
    let bound = [time(permit, "closed_at"), time(permit, "handed_back_at")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or_else(future_bound)
        .min(future_bound());
    let mut at = activated;
    for s in suspensions {
        if let Some(t) = time(s, "reinstated_at").filter(|t| *t <= bound) {
            at = at.max(t);
        }
    }
    Some(at)
}

/// A lock removal verified before `at` (an unreadable time counts as removed, as locally any removal
/// counts).
fn removed_before(iso: &Payload, at: DateTime<Utc>) -> bool {
    // A lock taken off (migration 249) protects nothing, its removal verified or not.
    ["removal_verified_at", "removed_at"]
        .iter()
        .any(|k| field(iso, k).is_some_and(|t| instant(t).is_none_or(|t| t < at)))
}

/// The isolations of a permit ready to protect work from `at` (`permit::queries::activation_ready`).
fn isolations_ready(
    permit_code: &str,
    needs_isolation: bool,
    three_person: bool,
    isolations: &[&Payload],
    at: DateTime<Utc>,
) -> Option<(&'static str, String)> {
    if needs_isolation && isolations.is_empty() {
        return Some((
            "LOTO_NO_ISOLATION",
            format!("permit {permit_code} requires energy isolation and has no isolation point"),
        ));
    }
    let unverified = isolations
        .iter()
        .filter(|i| field(i, "verified_at").is_none() || isolation_rule_breach(i, three_person).is_some())
        .count();
    if unverified > 0 {
        return Some((
            "LOTO_NOT_VERIFIED",
            format!(
                "permit {permit_code}: {unverified} of {} isolation points are not verified by a second person",
                isolations.len()
            ),
        ));
    }
    if isolations.iter().any(|i| removed_before(i, at)) {
        return Some((
            "LOTO_ISOLATION_REMOVED",
            format!("permit {permit_code}: a lock was removed before the permit protected the work"),
        ));
    }
    None
}

/// The local status of a permit and the received one, when the received row changes it into a
/// status that is judged.
async fn permit_transition(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<(Option<Payload>, Payload)> {
    let local = local_wire(conn, "work_permits", entity_sync_id).await?;
    let permit = overlay(local.as_ref(), received);
    Ok((local, permit))
}

/// The local id of a row of `table` (fixed names, never received text) found by its sync identity.
pub(crate) async fn local_id(conn: &impl ConnectionTrait, table: &str, sync_id: &str) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT id FROM \"{table}\" WHERE entity_sync_id = ?"),
        [sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "id"))
    .transpose()
    .map_err(sync_err)
}

/// Whether the site a permit holds here (`entity_sync_id` of the copy held here) is an org node
/// that exists here and is deleted, and the received site (`node`) is the node that replaced it
/// when a new organisation structure was published (`org::validation::node_replaces_deleted_node`:
/// same code and node type, in another structure). Never any other live node, such as the plant
/// above it: the publish moves a permit only to the copy of its own node. A held node missing here,
/// or no node, is not deleted: nothing tells it apart from a site change. `None`: the held node is
/// deleted here and the received one is not applied yet, or its structure model is still a draft
/// here (`org::validation::node_copy_awaits_activation`): the row waits.
async fn site_moved_to_replacing_node(
    conn: &impl ConnectionTrait,
    held_node: Option<&str>,
    node: Option<&str>,
) -> AppResult<Option<bool>> {
    let Some(held_node) = held_node else {
        return Ok(Some(false));
    };
    let Some(held_id) = local_id(conn, "org_nodes", held_node).await? else {
        return Ok(Some(false));
    };
    if crate::permit::queries::live_org_node(conn, held_id).await? {
        return Ok(Some(false));
    }
    let Some(node) = node else {
        return Ok(Some(false));
    };
    let Some(node_id) = local_id(conn, "org_nodes", node).await? else {
        return Ok(None);
    };
    if crate::org::validation::node_replaces_deleted_node(conn, held_id, node_id).await? {
        return Ok(Some(true));
    }
    // The copy's structure is not activated here yet (the publish arrived in part): wait.
    if crate::org::validation::node_copy_awaits_activation(conn, Some(held_id), node_id).await? {
        return Ok(None);
    }
    Ok(Some(false))
}

/// A received permit whose equipment, linked work order or site differs from the copy held here
/// (`permit::queries::update_work_permit_checked`, CMMS Advisor decision A), judged as an edit on
/// this PC is:
/// - a linked work order is never removed (no edit on this PC removes it);
/// - the copy held here is a draft or waiting for review (a permit approved here was approved for
///   its equipment at its site: the sending PC held that version too, or its change is older and is
///   not applied here), except a permit whose site alone moved off a node deleted here (a new
///   organisation structure was published: its former nodes are deleted and every permit moved to
///   the node that replaces its own), judged on the rules below whatever its status;
/// - held here waiting for review, a change of equipment, work order or site sent the permit back to
///   draft on the other PC; it may arrive already submitted or approved again, since the journal
///   folds every save made there before a sync into one row whose version says nothing of how many
///   saves it carries. It is accepted on the rules below; a sending PC that skipped the return to
///   draft is not detected on arrival (left to step 6 stage 4, who sent the row and their rights;
///   `docs/KNOWN_LIMITS.md`);
/// - for a change of equipment or work order, no lock fitted under it here without a verified
///   removal and no isolation point planned on it here (they name the former equipment's points).
///   Isolation rows received just before the permit row (planned on the other PC for the new
///   equipment) count too: nothing on the rows tells which PC made them or for which version of the
///   permit, so a person decides (`docs/KNOWN_LIMITS.md`);
/// - the work order's equipment is the permit's;
/// - its node is the one derived from its equipment and work order or a level above it
///   (`permit::queries::derived_permit_node` and `node_on_permit_chain`, the local rule itself): a
///   work order recording no usable node is refused, never replaced by the equipment's node.
///
/// Equipment, a work order or a node not applied here yet: the row waits.
async fn equipment_change_check(
    conn: &impl ConnectionTrait,
    permit_sync_id: &str,
    local: &Payload,
    permit: &Payload,
    code: &str,
) -> AppResult<Option<InboundVerdict>> {
    let (asset, wo) = (
        field(permit, "asset_sync_id"),
        field(permit, "linked_work_order_sync_id"),
    );
    let (held_asset, held_wo) = (field(local, "asset_sync_id"), field(local, "linked_work_order_sync_id"));
    let (node, held_node) = (field(permit, "entity_sync_id"), field(local, "entity_sync_id"));
    if let (Some(held_wo), None) = (held_wo, wo) {
        return conflict(
            "PERMIT_WO_UNLINKED",
            format!("permit {code}: its work order {held_wo} was removed from it (no edit removes it)"),
        );
    }
    let equipment_changes = asset != held_asset || wo != held_wo;
    if !equipment_changes && node == held_node {
        return Ok(None);
    }
    let held = field(local, "status").unwrap_or_default();
    // Its site alone moved off a node deleted here to the node replacing it: the publish of a new
    // organisation structure moves every permit off the former nodes to their copies
    // (`org::validation::publish_model_with_remap`). Whatever its status, it is judged on the rules
    // below (its site on its equipment's or work order's chain).
    let site_reanchored = if equipment_changes {
        false
    } else {
        match site_moved_to_replacing_node(conn, held_node, node).await? {
            Some(reanchored) => reanchored,
            None => {
                return wait(format!(
                    "permit {code}: its site {} is not applied yet, or its new structure is not active here yet",
                    node.unwrap_or("none")
                ))
            }
        }
    };
    if !site_reanchored && !crate::permit::queries::WP_EQUIPMENT_CHANGE_STATUSES.contains(&held) {
        return if equipment_changes {
            conflict(
                "PERMIT_EQUIPMENT_LOCKED",
                format!(
                    "permit {code} is '{held}' here: it was approved for its equipment, which cannot change \
                     (here {} / work order {}, received {} / work order {})",
                    held_asset.unwrap_or("none"),
                    held_wo.unwrap_or("none"),
                    asset.unwrap_or("none"),
                    wo.unwrap_or("none")
                ),
            )
        } else {
            conflict(
                "PERMIT_SITE_LOCKED",
                format!(
                    "permit {code} is '{held}' here: it was approved at its site, which cannot change \
                     (here {}, received {})",
                    held_node.unwrap_or("none"),
                    node.unwrap_or("none")
                ),
            )
        };
    }
    if equipment_changes {
        let count = |condition: &str| {
            format!(
                "SELECT COUNT(*) AS c FROM permit_isolations pi JOIN work_permits wp ON wp.id = pi.permit_id \
                 WHERE wp.entity_sync_id = ? AND {condition}"
            )
        };
        for (condition, refusal, what) in [
            (
                "pi.applied_at IS NOT NULL AND pi.removal_verified_at IS NULL",
                "PERMIT_EQUIPMENT_LOCK_FITTED",
                "lock(s) fitted here without a verified removal",
            ),
            (
                // A cancelled line no longer names a point (CMMS Advisor decision).
                "pi.applied_at IS NULL AND pi.cancelled_at IS NULL",
                "PERMIT_EQUIPMENT_ISOLATIONS_PLANNED",
                "isolation point(s) planned here for its former equipment",
            ),
        ] {
            let n: i64 = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    count(condition),
                    [permit_sync_id.into()],
                ))
                .await?
                .map(|r| r.try_get::<i64>("", "c"))
                .transpose()
                .map_err(sync_err)?
                .unwrap_or(0);
            if n > 0 {
                return conflict(
                    refusal,
                    format!("permit {code}: its equipment or work order changed while it has {n} {what}"),
                );
            }
        }
    }
    let Some(asset) = asset else {
        // No equipment: malformed, refused by the generic apply (the column is required).
        return Ok(None);
    };
    let Some(asset_id) = local_id(conn, "equipment", asset).await? else {
        return wait(format!("permit {code}: its equipment {asset} is not applied yet"));
    };
    let mut wo_id = None;
    if let Some(wo) = wo {
        let Some(id) = local_id(conn, "work_orders", wo).await? else {
            return wait(format!("permit {code}: its work order {wo} is not applied yet"));
        };
        let wo_equipment = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT equipment_id AS v FROM work_orders WHERE id = ?",
                [id.into()],
            ))
            .await?
            .map(|r| r.try_get::<Option<i64>>("", "v"))
            .transpose()
            .map_err(sync_err)?
            .flatten();
        if wo_equipment != Some(asset_id) {
            return conflict(
                "PERMIT_WO_EQUIPMENT_MISMATCH",
                format!("permit {code} is on equipment {asset} but its work order {wo} is on other equipment"),
            );
        }
        wo_id = Some(id);
    }
    let derived = crate::permit::queries::derived_permit_node(conn, asset_id, wo_id).await?;
    let node_id = match node {
        Some(n) => match local_id(conn, "org_nodes", n).await? {
            Some(id) => Some(id),
            None => return wait(format!("permit {code}: its site {n} is not applied yet")),
        },
        None => None,
    };
    let on_chain = match (derived, node_id) {
        (Some(d), Some(n)) => crate::permit::queries::node_on_permit_chain(conn, d, n).await?,
        _ => false,
    };
    if !on_chain {
        return conflict(
            "PERMIT_SITE_NOT_EQUIPMENT_SITE",
            format!(
                "permit {code}: its site must be the one of its equipment or work order ({}) or a level above \
                 it, received {}",
                derived.map_or_else(|| "none usable".to_string(), |d| format!("node {d}")),
                node.unwrap_or("none")
            ),
        );
    }
    Ok(None)
}

/// Permit statuses reached only through an approval (`permit::queries`: pending_review → approved
/// → issued → active …). A permit cancelled or expired may never have been approved.
const APPROVED_ON: &[&str] = &[
    "approved",
    "issued",
    "active",
    "suspended",
    "revalidation_required",
    "closed",
    "handed_back",
];

/// Who submitted a permit for review and when (migration 244).
fn submission(p: &Payload) -> [Option<&str>; 2] {
    [field(p, "submitted_by_sync_id"), field(p, "submitted_at")]
}

/// The flag of a self-approval: `None` when absent, null or unreadable (never taken as "no").
fn self_approved_flag(p: &Payload) -> Option<bool> {
    match p.get("self_approved") {
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// Who approved a permit, when, and whether it was a self-approval (migration 244).
fn approval(p: &Payload) -> (Option<&str>, Option<&str>, Option<bool>) {
    (
        field(p, "approved_by_sync_id"),
        field(p, "approved_at"),
        self_approved_flag(p),
    )
}

fn approval_recorded(p: &Payload) -> bool {
    let (by, at, flag) = approval(p);
    by.is_some() || at.is_some() || flag.is_some()
}

/// Whether a change of the four-eyes opt-out of `site_id` is still waiting in the inbox here:
/// `relaxing` a rule switching it on (`value_json` true), else one switching it off. A self-approval
/// waits for the opt-out it was made under (four-eyes meanwhile), and one accepted under the opt-out
/// held here waits while a return to four-eyes is on its way; nothing is written meanwhile.
pub(crate) async fn site_rule_change_waiting(
    conn: &impl ConnectionTrait,
    site_id: i64,
    relaxing: bool,
) -> AppResult<bool> {
    let Some(site_sync) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id AS s FROM org_nodes WHERE id = ?",
            [site_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "s"))
        .transpose()
        .map_err(sync_err)?
        .flatten()
    else {
        return Ok(false);
    };
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS w FROM sync_inbox WHERE entity_type = 'site_settings' \
               AND apply_status IN ('pending', 'deferred') AND operation NOT IN ('delete', 'archive') \
               AND json_extract(payload_json, '$.site_node_sync_id') = ? \
               AND json_extract(payload_json, '$.setting_key') = ? \
               AND (TRIM(COALESCE(json_extract(payload_json, '$.value_json'), '')) = 'true') = ? \
             LIMIT 1",
            [
                site_sync.into(),
                crate::reliability::study::decisions::SELF_APPROVAL_SETTING.into(),
                relaxing.into(),
            ],
        ))
        .await?
        .is_some())
}

/// Four-eyes on a permit received by synchronisation (owner decision 2026-10-17, migration 244;
/// the local rule is `permit::queries::set_work_permit_status`):
/// - the record held here is kept: the permit's author (or its absence) is never removed, changed or
///   added; once the copy held here is past review (approved, issued, … or cancelled), who
///   submitted it and who approved it are never removed, changed or added. Held here as a draft or waiting for review, a new
///   submission may arrive (the permit went back to draft and was submitted again there);
/// - the record received is whole: a draft names no submission or approval, a permit waiting for
///   review no approval; who and when come together; an approval is not dated in the future nor
///   before its submission;
/// - a permit received past review (`APPROVED_ON`) whose approval is new here names who approved
///   it. The approver is judged against the author and the submitter written on the row, by sync
///   identity (a person not applied here yet does not change the verdict; the row then waits for
///   that person in the generic apply): a second person (both recorded, the approver neither) is a
///   four-eyes approval, never flagged as a self-approval; anything else is a self-approval, which
///   must be flagged as one and is accepted only when the permit's site opted out of four-eyes on
///   this PC (`permit::queries::permit_four_eyes_rule`). Its site not applied here yet, or a change
///   of the site's opt-out still waiting in the inbox (either way), makes the row wait. A conflict
///   names when it was approved and the site's rule on this PC now.
async fn four_eyes_check(
    conn: &impl ConnectionTrait,
    local: Option<&Payload>,
    permit: &Payload,
    code: &str,
) -> AppResult<Option<InboundVerdict>> {
    let status = field(permit, "status").unwrap_or_default();
    let held = local.and_then(|l| field(l, "status"));
    if let Some(l) = local {
        // The author is set once, at creation: a copy held here keeps the one it has, none included
        // (a permit written before authors were recorded is never given one by a received row).
        if field(l, "created_by_sync_id") != field(permit, "created_by_sync_id") {
            return conflict(
                "PERMIT_APPROVAL_RECORD_CHANGED",
                format!("permit {code}: the author recorded here (or its absence) cannot be removed, changed or added"),
            );
        }
        let past_review_here = held.is_some_and(|h| !crate::permit::queries::WP_EQUIPMENT_CHANGE_STATUSES.contains(&h));
        if past_review_here {
            if submission(l) != submission(permit) || approval(l) != approval(permit) {
                return conflict(
                    "PERMIT_APPROVAL_RECORD_CHANGED",
                    format!(
                        "permit {code} is '{}' here: who submitted and who approved it cannot be removed, changed or \
                         added",
                        held.unwrap_or_default()
                    ),
                );
            }
            // Unchanged: judged when it arrived here.
            return Ok(None);
        }
    }
    let invalid = |why: &str| {
        conflict(
            "PERMIT_APPROVAL_RECORD_INVALID",
            format!("permit {code} ({status}): {why}"),
        )
    };
    let [submitter, submitted_at] = submission(permit);
    let (approver, approved_at, flag) = approval(permit);
    if submitter.is_some() != submitted_at.is_some() {
        return invalid("its submission says who without when, or when without who");
    }
    if status == "draft" && (submitter.is_some() || approval_recorded(permit)) {
        return invalid("a draft carries a submission or an approval (a draft is submitted again)");
    }
    if status == "pending_review" && approval_recorded(permit) {
        return invalid("a permit waiting for review carries an approval");
    }
    if !approval_recorded(permit) {
        if APPROVED_ON.contains(&status) {
            return conflict(
                "PERMIT_APPROVER_MISSING",
                format!("permit {code} is '{status}' but does not say who approved it (four-eyes)"),
            );
        }
        return Ok(None);
    }
    let (Some(approver), Some(approved_at)) = (approver, approved_at) else {
        return invalid("its approval says who without when, or when without who");
    };
    let Some(approved) = instant(approved_at) else {
        return invalid("its approval time cannot be read");
    };
    if approved > future_bound() {
        return conflict(
            "SYNC_TIME_INCONSISTENT",
            format!("permit {code}: its approval is dated in the future ({approved_at})"),
        );
    }
    if let Some(at) = submitted_at {
        match instant(at) {
            None => return invalid("its submission time cannot be read"),
            Some(submitted) if approved < submitted => {
                return invalid("it was approved before it was submitted for review");
            }
            Some(_) => {}
        }
    }
    let Some(flag) = flag else {
        return invalid("its approval does not say whether it was a self-approval");
    };
    let second_person = crate::permit::queries::approver_is_second_person(
        field(permit, "created_by_sync_id").as_ref(),
        submitter.as_ref(),
        &approver,
    );
    if second_person {
        if flag {
            return invalid("approved by a second person but flagged as a self-approval");
        }
        return Ok(None);
    }
    let who = if submitter == Some(approver) {
        "the person who submitted it"
    } else if field(permit, "created_by_sync_id") == Some(approver) {
        "the person who wrote it"
    } else {
        "a person not shown to differ from its author and submitter (not recorded)"
    };
    // The conflict says when it was approved and the site's rule on this PC now (CMMS Advisor
    // decision A: a self-approval arriving after its site went back to four-eyes is a conflict).
    if !flag {
        return conflict(
            "PERMIT_SELF_APPROVAL",
            format!("permit {code} was approved by {who} on {approved_at} without being recorded as a self-approval"),
        );
    }
    // A self-approval: only where the permit's site opted out of four-eyes, on this PC.
    let Some(node) = field(permit, "entity_sync_id") else {
        return conflict(
            "PERMIT_SELF_APPROVAL",
            format!("permit {code} was approved by {who} on {approved_at}; it names no site, so four-eyes applies"),
        );
    };
    let Some(node_id) = local_id(conn, "org_nodes", node).await? else {
        return wait(format!("permit {code}: its site {node} is not applied yet"));
    };
    let rule = crate::permit::queries::permit_four_eyes_rule(conn, node_id).await?;
    let site = crate::planning::weekly_schedule::planning_site_of_node(conn, node_id).await?;
    match (rule, site) {
        (crate::reliability::study::decisions::SelfApprovalRule::Allowed, Some(site)) => {
            if site_rule_change_waiting(conn, site, false).await? {
                return wait(format!(
                    "permit {code}: self-approved; its site's return to four-eyes is still arriving"
                ));
            }
            Ok(None)
        }
        (crate::reliability::study::decisions::SelfApprovalRule::FourEyes, Some(site)) => {
            if site_rule_change_waiting(conn, site, true).await? {
                return wait(format!(
                    "permit {code}: self-approved; its site's four-eyes opt-out is still arriving"
                ));
            }
            conflict(
                "PERMIT_SELF_APPROVAL",
                format!(
                    "permit {code} was approved by {who} on {approved_at}; its site's setting on this PC now: \
                     four-eyes (self-approval not allowed)"
                ),
            )
        }
        _ => conflict(
            "PERMIT_SELF_APPROVAL",
            format!(
                "permit {code} was approved by {who} on {approved_at}; its site is not a planning site on this PC, \
                 so four-eyes applies"
            ),
        ),
    }
}

/// Re-check of a permit received by synchronisation (see the module documentation).
pub(crate) async fn inbound_permit_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let (local, permit) = permit_transition(conn, entity_sync_id, received).await?;
    let to = field(&permit, "status").unwrap_or_default();
    let from = local.as_ref().and_then(|l| field(l, "status"));
    let code = field(&permit, "code").unwrap_or(entity_sync_id).to_string();
    // A permit keeps the type it was created with (no local edit changes it): its isolation and
    // lock-removal rules come from that type.
    if let Some(l) = &local {
        let (held, got) = (field(l, "permit_type_sync_id"), field(&permit, "permit_type_sync_id"));
        if held != got {
            return conflict(
                "PERMIT_TYPE_CHANGED",
                format!(
                    "permit {code}: its permit type cannot change (here {}, received {})",
                    held.unwrap_or("none"),
                    got.unwrap_or("none")
                ),
            );
        }
        if let Some(verdict) = equipment_change_check(conn, entity_sync_id, l, &permit, &code).await? {
            return Ok(Some(verdict));
        }
    }
    // Before the "same status" pass: a row in the status held here may still rewrite who approved it.
    if let Some(verdict) = four_eyes_check(conn, local.as_ref(), &permit, &code).await? {
        return Ok(Some(verdict));
    }
    if from == Some(to) {
        return Ok(None);
    }
    if let Some(from) = from {
        let along = matches!((from, to), ("closed", "handed_back") | ("handed_back", "cancelled"));
        if TERMINAL.contains(&from) && !along {
            return conflict(
                "PERMIT_STATUS_BACKWARDS",
                format!("permit {code} is '{from}' here; it cannot become '{to}'"),
            );
        }
    }
    if !JUDGED_STATUSES.contains(&to) {
        return Ok(None);
    }
    let isolations = permit_children(conn, "permit_isolations", entity_sync_id).await?;
    let suspensions = permit_children(conn, "permit_suspensions", entity_sync_id).await?;
    let handovers = permit_children(conn, "permit_handover_logs", entity_sync_id).await?;
    if let Some(why) = [&isolations, &suspensions, &handovers]
        .iter()
        .find_map(|c| c.blocked.clone())
    {
        return wait(format!("permit {code}: {why}"));
    }
    match to {
        "active" => {
            let Some(at) = protection_start(&permit, &payloads(&suspensions)) else {
                return conflict(
                    "SYNC_STATE_TIME_MISSING",
                    format!("permit {code} is active but its activation time is missing or unreadable"),
                );
            };
            if at > future_bound() {
                return conflict(
                    "SYNC_TIME_INCONSISTENT",
                    format!("permit {code} is dated in the future ({at})"),
                );
            }
            let Some(expiry) = field(&permit, "expires_at") else {
                return conflict(
                    "PERMIT_EXPIRY_MISSING",
                    format!("permit {code} is active without an expiry"),
                );
            };
            if instant(expiry).is_none_or(|e| e <= at) {
                return conflict(
                    "PERMIT_EXPIRED",
                    format!("permit {code} protects the work from {at} but expires at {expiry}"),
                );
            }
            let Some(needs) = type_requires_isolation(conn, &permit).await? else {
                return wait(format!("permit {code}: its permit type is not applied yet"));
            };
            let three_person = type_three_person(conn, &permit).await?;
            let counted = counted_isolations(&payloads(&isolations));
            if let Some((c, m)) = isolations_ready(&code, needs, three_person, &counted, at) {
                return conflict(c, m);
            }
        }
        "closed" | "handed_back" => {
            let pending = payloads(&isolations)
                .iter()
                .filter(|i| field(i, "removal_verified_at").is_none() && !cancelled_line(i))
                .count();
            if pending > 0 {
                return conflict(
                    "PERMIT_REMOVAL_PENDING",
                    format!("permit {code} is {to} with {pending} lock removal(s) not verified"),
                );
            }
            // A log received by the person who signed it does not count.
            if to == "handed_back" && payloads(&handovers).iter().all(|h| handover_rule_breach(h).is_some()) {
                return conflict(
                    "PERMIT_HANDOVER_REQUIRED",
                    format!("permit {code} is handed back without a valid handover log"),
                );
            }
        }
        _ => {
            if !payloads(&suspensions)
                .iter()
                .any(|s| field(s, "reinstated_at").is_none())
            {
                return conflict(
                    "PERMIT_SUSPENSION_MISSING",
                    format!("permit {code} is {to} without an open suspension"),
                );
            }
        }
    }
    Ok(None)
}

/// The waiting children a received permit's change was judged on, to apply with it.
pub(crate) async fn permit_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<Group>> {
    let (local, permit) = permit_transition(conn, entity_sync_id, received).await?;
    let to = field(&permit, "status").unwrap_or_default();
    let from = local.as_ref().and_then(|l| field(l, "status")).map(str::to_string);
    if from.as_deref() == Some(to) || !JUDGED_STATUSES.contains(&to) {
        return Ok(None);
    }
    let mut rows = Vec::new();
    for table in ["permit_isolations", "permit_suspensions", "permit_handover_logs"] {
        rows.extend(permit_children(conn, table, entity_sync_id).await?.counted);
    }
    Ok((!rows.is_empty()).then(|| Group {
        parent_sync_id: entity_sync_id.to_string(),
        previous_status: from,
        judged: true,
        rows,
        closeout: None,
    }))
}

/// Applies the children a row was judged on, after the row itself, in its transaction, each through
/// the same inbox apply as any received row (`sync::materialize::apply_inbox_row_in`: checks, its own
/// safety checks, apply, inbox status, synced version). A child already applied (by the group of a
/// permit applied just before) is skipped. If one is not applied, the result is the conflict the
/// caller records after rolling its transaction back: nothing of the group is kept, and the children
/// stay waiting, to be judged on their own.
pub(crate) async fn apply_group(
    conn: &impl ConnectionTrait,
    group: Group,
) -> AppResult<Option<crate::sync::materialize::ApplyOutcome>> {
    use crate::sync::materialize::ApplyOutcome;
    for row in group.rows {
        let Some(item) = crate::sync::materialize::load_inbox_row(conn, row.inbox_id).await? else {
            continue;
        };
        let waiting = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS w FROM sync_inbox WHERE id = ? AND apply_status IN ('pending', 'deferred')",
                [row.inbox_id.into()],
            ))
            .await?
            .is_some();
        if !waiting {
            continue;
        }
        let outcome = GROUP_PARENT
            .scope(
                (
                    group.parent_sync_id.clone(),
                    group.previous_status.clone(),
                    group.judged,
                ),
                Box::pin(crate::sync::materialize::apply_inbox_row_in(conn, &item)),
            )
            .await?;
        match outcome {
            ApplyOutcome::Applied | ApplyOutcome::Unchanged => {}
            // A child that waits (a row it needs has not arrived) makes the whole group wait: rolled
            // back, retried with the next rows applied.
            ApplyOutcome::Deferred {
                code,
                message,
                waits_for,
            } => {
                return Ok(Some(ApplyOutcome::Deferred {
                    code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                    message: format!(
                        "{} {} received with it waits ({code}: {message})",
                        row.entity_type, item.entity_sync_id
                    ),
                    waits_for,
                }));
            }
            ApplyOutcome::Conflict { code, message } => {
                return Ok(Some(ApplyOutcome::Conflict {
                    code: "SYNC_GROUP_REFUSED".into(),
                    message: format!(
                        "{} {} received with it was not applied ({code}: {message})",
                        row.entity_type, item.entity_sync_id
                    ),
                }));
            }
        }
    }
    Ok(None)
}

/// The people and times of an isolation's actions, and its permit.
fn isolation_actions(p: &Payload) -> [Option<&str>; 7] {
    [
        field(p, "permit_sync_id"),
        field(p, "applied_by_sync_id"),
        field(p, "applied_at"),
        field(p, "verified_by_sync_id"),
        field(p, "verified_at"),
        field(p, "removal_verified_by_sync_id"),
        field(p, "removal_verified_at"),
    ]
}

/// What a line cancelled here keeps as it was: its permit, content (the energy and method texts and
/// references, the mapping review mark), actions and cancellation.
const CANCELLED_LINE_KEPT: &[&str] = &[
    "removed_by_sync_id",
    "removed_at",
    "permit_sync_id",
    "isolation_point",
    "energy_type",
    "isolation_method",
    "energy_type_sync_id",
    "isolation_method_sync_id",
    "lock_number",
    "applied_by_sync_id",
    "applied_at",
    "verified_by_sync_id",
    "verified_at",
    "removal_verified_by_sync_id",
    "removal_verified_at",
    "cancelled_at",
    "cancelled_by_sync_id",
];

/// The cancellation rules of a received isolation line (`permit::queries::cancel_permit_isolation`,
/// CMMS Advisor decision), judged on the copies held here:
/// - a line cancelled here is kept as it was: a row un-cancelling it, applying it or changing it is
///   refused (`PERMIT_ISOLATION_CANCELLED`);
/// - a line received cancelled was never applied, neither in the row nor here
///   (`PERMIT_ISOLATION_CANCEL_APPLIED`), says who cancelled it and when, at a readable time not in
///   the future (`PERMIT_ISOLATION_CANCEL_INVALID`), and its permit is held here as a draft or waiting
///   for review (`PERMIT_ISOLATION_CANCEL_WRONG_STATUS`) — when it arrives with its permit's change,
///   as the permit was here before that change. A permit not here yet: the apply waits for it.
async fn cancellation_check(
    conn: &impl ConnectionTrait,
    local: Option<&Payload>,
    iso: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let point = field(iso, "isolation_point").unwrap_or("?").to_string();
    if let Some(held) = local.filter(|l| field(l, "cancelled_at").is_some()) {
        if CANCELLED_LINE_KEPT.iter().any(|k| field(held, k) != field(iso, k))
            || flag(held, "mapping_review_required") != flag(iso, "mapping_review_required")
        {
            return conflict(
                "PERMIT_ISOLATION_CANCELLED",
                format!(
                    "isolation '{point}' is cancelled here: it is kept as it was (never un-cancelled, applied or changed)"
                ),
            );
        }
        return Ok(None);
    }
    if field(iso, "cancelled_at").is_none() {
        return Ok(None);
    }
    let acted = |p: &Payload| ISOLATION_ACTION_KEYS.iter().any(|k| field(p, k).is_some());
    if acted(iso) || local.is_some_and(acted) {
        return conflict(
            "PERMIT_ISOLATION_CANCEL_APPLIED",
            format!("isolation '{point}' was cancelled but it was applied (here or on the other PC): an applied lock is removed with a verified removal"),
        );
    }
    if field(iso, "cancelled_by_sync_id").is_none() || time(iso, "cancelled_at").is_none_or(|t| t > future_bound()) {
        return conflict(
            "PERMIT_ISOLATION_CANCEL_INVALID",
            format!("isolation '{point}' was cancelled without who or when, or at a time unreadable or in the future"),
        );
    }
    let Some(permit_sync) = field(iso, "permit_sync_id") else {
        return Ok(None);
    };
    let Some(permit) = local_wire(conn, "work_permits", permit_sync).await? else {
        return Ok(None);
    };
    let current = field(&permit, "status").unwrap_or_default().to_string();
    let held = GROUP_PARENT
        .try_with(|(parent, previous, _)| (parent == permit_sync).then(|| previous.clone()))
        .ok()
        .flatten()
        .map_or(current, |previous| previous.unwrap_or_else(|| "draft".into()));
    if !crate::permit::queries::ISOLATION_CANCEL_STATUSES.contains(&held.as_str()) {
        let code = field(&permit, "code").unwrap_or(permit_sync);
        return conflict(
            "PERMIT_ISOLATION_CANCEL_WRONG_STATUS",
            format!(
                "isolation '{point}' was cancelled while permit {code} is '{held}' here: a planned line is cancelled \
                 only while its permit is a draft or waiting for review"
            ),
        );
    }
    Ok(None)
}

/// Re-check of an isolation received by synchronisation (see the module documentation).
pub(crate) async fn inbound_isolation_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "permit_isolations", entity_sync_id).await?;
    let iso = overlay(local.as_ref(), received);
    if let Some(verdict) = cancellation_check(conn, local.as_ref(), &iso).await? {
        return Ok(Some(verdict));
    }
    if let Some(verdict) = lock_record_kept(local.as_ref(), &iso) {
        return Ok(Some(verdict));
    }
    if local
        .as_ref()
        .is_some_and(|l| isolation_actions(l) == isolation_actions(&iso) && lock_record(l) == lock_record(&iso))
    {
        return Ok(None);
    }
    let point = field(&iso, "isolation_point").unwrap_or(entity_sync_id).to_string();
    if let Some(l) = &local {
        if field(l, "permit_sync_id") != field(&iso, "permit_sync_id") {
            return conflict(
                "PERMIT_ISOLATION_MOVED",
                format!("isolation '{point}' cannot move to another permit"),
            );
        }
    }
    let three_person = match field(&iso, "permit_sync_id") {
        Some(permit_sync) => permit_three_person(conn, permit_sync).await?,
        None => false,
    };
    if let Some((c, m)) = isolation_rule_breach(&iso, three_person) {
        return conflict(c, m);
    }
    // The permit is applied before its isolations; not here yet, the apply waits for it.
    let Some(permit_sync) = field(&iso, "permit_sync_id") else {
        return Ok(None);
    };
    let Some(permit) = local_wire(conn, "work_permits", permit_sync).await? else {
        return Ok(None);
    };
    let current = field(&permit, "status").unwrap_or_default().to_string();
    let code = field(&permit, "code").unwrap_or(permit_sync).to_string();
    // Applied with its permit's change: judged against the permit's new status when isolations may
    // change in it (suspended, revalidation), otherwise as it was before that change.
    let judged = if ISOLATION_SETUP.contains(&current.as_str()) {
        current.clone()
    } else {
        GROUP_PARENT
            .try_with(|(parent, previous, _)| (parent == permit_sync).then(|| previous.clone()))
            .ok()
            .flatten()
            .map_or_else(
                || current.clone(),
                |previous| previous.unwrap_or_else(|| "draft".into()),
            )
    };
    let wrong = |what: &str| {
        conflict(
            "PERMIT_ISOLATION_WRONG_STATUS",
            format!("isolation '{point}': {what} while permit {code} protects the work"),
        )
    };
    if local.is_none() && !ISOLATION_SETUP.contains(&judged.as_str()) {
        return wrong("added");
    }
    let swapped = |key: &str| {
        local
            .as_ref()
            .is_some_and(|l| field(l, key).is_some() && field(l, key) != field(&iso, key))
    };
    if (swapped("applied_by_sync_id") || swapped("verified_by_sync_id")) && !ISOLATION_SETUP.contains(&judged.as_str())
    {
        return wrong("its fitter or verifier changed");
    }
    if swapped("removal_verified_by_sync_id") {
        return wrong("its removal verifier changed");
    }
    let changed =
        |key: &str| field(&iso, key).is_some() && local.as_ref().is_none_or(|l| field(l, key) != field(&iso, key));
    // Fitted or verified only while the permit was prepared or suspended: when that reaches a permit
    // that protects (or protected) the work, it dates from before the permit started protecting it.
    if (changed("applied_at") || changed("verified_at")) && !ISOLATION_SETUP.contains(&current.as_str()) {
        let suspensions = permit_children(conn, "permit_suspensions", permit_sync).await?;
        if let Some(start) = protection_start(&permit, &payloads(&suspensions)) {
            for key in ["applied_at", "verified_at"] {
                if changed(key) && time(&iso, key).is_none_or(|t| t > start) {
                    return wrong("fitted or verified after the permit started protecting the work");
                }
            }
        }
    }
    // A lock removal is verified only once the permit was active.
    if changed("removal_verified_at") && ISOLATION_SETUP.contains(&current.as_str()) {
        return conflict(
            "PERMIT_ISOLATION_WRONG_STATUS",
            format!("isolation '{point}': lock removal verified while permit {code} is '{current}'"),
        );
    }
    // A lock is taken off only once the permit was active (migration 249).
    if changed("removed_at") && ISOLATION_SETUP.contains(&current.as_str()) {
        return conflict(
            "PERMIT_ISOLATION_WRONG_STATUS",
            format!("isolation '{point}': lock taken off while permit {code} is '{current}'"),
        );
    }
    lock_record_check(conn, local.as_ref(), &iso, entity_sync_id, permit_sync, &point).await
}

/// The removal and worker-informed values of an isolation, compared as this PC stores them.
fn lock_record(p: &Payload) -> (Vec<Option<&str>>, Vec<Option<&str>>, [bool; 3]) {
    (
        LOCK_REMOVAL_KEYS.iter().map(|k| field(p, k)).collect(),
        WORKER_INFORMED_KEYS.iter().map(|k| field(p, k)).collect(),
        [
            flag(p, "forced_removal"),
            flag(p, "forced_worker_absent"),
            flag(p, "informed_in_app"),
        ],
    )
}

/// Once recorded here, a lock removal (with its forced-removal record) and the worker told are
/// never rewritten nor taken back by a received row (`PERMIT_LOCK_REMOVAL_RECORD_CHANGED`).
fn lock_record_kept(local: Option<&Payload>, iso: &Payload) -> Option<InboundVerdict> {
    let held = local?;
    let point = field(iso, "isolation_point").unwrap_or("?");
    let (held_removal, held_informed, held_flags) = lock_record(held);
    let (removal, informed, flags) = lock_record(iso);
    let removal_changed = field(held, "removed_at").is_some()
        && (held_removal != removal || held_flags[0] != flags[0] || held_flags[1] != flags[1]);
    let informed_changed =
        field(held, "informed_at").is_some() && (held_informed != informed || held_flags[2] != flags[2]);
    (removal_changed || informed_changed).then(|| InboundVerdict::Conflict {
        code: "PERMIT_LOCK_REMOVAL_RECORD_CHANGED",
        message: format!(
            "isolation '{point}': the record of its lock taken off (or of the worker told) held here cannot change"
        ),
    })
}

/// The local id of the user with this sync id.
pub(crate) async fn local_user(conn: &impl ConnectionTrait, user_sync_id: &str) -> AppResult<Option<i64>> {
    local_id(conn, "user_accounts", user_sync_id).await
}

/// `ptw.close` of `user_sync` at the site of the permit `permit_sync` here (the rule of
/// `permit::access::may_receive_handover`, the local saves' check): `Ok(None)` when held, else the
/// verdict (waiting while the person or their rights may still arrive, else `code`).
async fn close_right_at_permit(
    conn: &impl ConnectionTrait,
    user_sync: &str,
    permit_sync: &str,
    code: &'static str,
    message: String,
) -> AppResult<Option<InboundVerdict>> {
    let Some(user) = local_user(conn, user_sync).await? else {
        return wait(format!("{message}: that person is not applied here yet"));
    };
    let Some(permit) = local_id(conn, "work_permits", permit_sync).await? else {
        return Ok(None);
    };
    if crate::permit::access::may_receive_handover(conn, permit, user).await? {
        return Ok(None);
    }
    if rights_may_still_arrive(conn, user_sync, crate::rbac::permissions::PTW_CLOSE).await? {
        return wait(format!("{message}: their rights are still arriving"));
    }
    conflict(
        code,
        format!("{message} without the permit close right at the permit's site here"),
    )
}

/// The checks of a received lock record that need this PC (migration 249): the authoriser of a
/// forced removal new here and the person recording that the worker was told hold `ptw.close` at
/// the permit's site here; a lock newly applied by a worker with a forced removal they were not told
/// of before it is refused ([`forced_removal_block`]).
async fn lock_record_check(
    conn: &impl ConnectionTrait,
    local: Option<&Payload>,
    iso: &Payload,
    entity_sync_id: &str,
    permit_sync: &str,
    point: &str,
) -> AppResult<Option<InboundVerdict>> {
    let new_here = |key: &str| field(iso, key).is_some() && local.is_none_or(|l| field(l, key).is_none());
    if flag(iso, "forced_removal") && new_here("removed_at") {
        if let Some(authoriser) = field(iso, "forced_authorized_by_sync_id") {
            if let Some(verdict) = close_right_at_permit(
                conn,
                authoriser,
                permit_sync,
                "PERMIT_FORCED_REMOVAL_UNAUTHORISED",
                format!("isolation '{point}': forced removal authorised"),
            )
            .await?
            {
                return Ok(Some(verdict));
            }
        }
    }
    if new_here("informed_at") && !flag(iso, "informed_in_app") {
        if let Some(informer) = field(iso, "informed_by_sync_id") {
            if let Some(verdict) = close_right_at_permit(
                conn,
                informer,
                permit_sync,
                "PERMIT_WORKER_INFORMED_UNAUTHORISED",
                format!("isolation '{point}': worker told of the forced removal, recorded"),
            )
            .await?
            {
                return Ok(Some(verdict));
            }
        }
    }
    let applied_changed = ["applied_by_sync_id", "applied_at"]
        .iter()
        .any(|k| field(iso, k).is_some() && local.is_none_or(|l| field(l, k) != field(iso, k)));
    if applied_changed {
        return forced_removal_block(conn, iso, entity_sync_id, point).await;
    }
    Ok(None)
}

/// A lock applied by a worker who has, here, a forced removal they were not told of, taken off
/// before this lock was applied: refused (`PERMIT_WORKER_NOT_INFORMED`), as the local apply is. When
/// the record of the worker told of it is waiting in the inbox, the row waits for it.
async fn forced_removal_block(
    conn: &impl ConnectionTrait,
    iso: &Payload,
    entity_sync_id: &str,
    point: &str,
) -> AppResult<Option<InboundVerdict>> {
    let (Some(worker), Some(applied_at)) = (field(iso, "applied_by_sync_id"), field(iso, "applied_at")) else {
        return Ok(None);
    };
    let applied = instant(applied_at);
    let open = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pi.entity_sync_id AS sid, pi.removed_at AS removed_at FROM permit_isolations pi \
               JOIN user_accounts u ON u.id = pi.applied_by_id \
              WHERE u.entity_sync_id = ? AND pi.forced_removal = 1 AND pi.informed_at IS NULL \
                AND pi.entity_sync_id IS NOT ?",
            [worker.into(), entity_sync_id.into()],
        ))
        .await?;
    for r in open {
        let sid: Option<String> = r.try_get("", "sid").map_err(sync_err)?;
        let removed: Option<String> = r.try_get("", "removed_at").map_err(sync_err)?;
        // Taken off after this lock was applied: the worker could not have been told before.
        let before = match (removed.as_deref().and_then(instant), applied) {
            (Some(removed), Some(applied)) => removed <= applied,
            _ => true,
        };
        if !before {
            continue;
        }
        let told_waiting = match &sid {
            Some(sid) => conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS w FROM sync_inbox WHERE entity_type = 'permit_isolations' \
                       AND entity_sync_id = ? AND apply_status IN ('pending', 'deferred') \
                       AND json_extract(payload_json, '$.informed_at') IS NOT NULL LIMIT 1",
                    [sid.as_str().into()],
                ))
                .await?
                .is_some(),
            None => false,
        };
        if told_waiting {
            return wait(format!(
                "isolation '{point}': the worker's acknowledgement of an earlier forced removal is still arriving"
            ));
        }
        return conflict(
            "PERMIT_WORKER_NOT_INFORMED",
            format!(
                "isolation '{point}': applied by a worker not yet told of the forced removal of their lock \
                 (taken off before)"
            ),
        );
    }
    Ok(None)
}

/// The value of an on/off column of a received row, read as SQLite stores it in an INTEGER column:
/// `true`/`false`, the numbers 1 and 0 (1.0 and 0.0 too) and the texts "1" and "0" (also "1.0",
/// "0.0") give on or off; a missing key gives off (the column default). Anything else (another
/// number, other text, text padded with spaces, null) is `None`: it is refused rather than read one
/// way and stored another.
fn on_off(p: &Payload, key: &str) -> Option<bool> {
    let number = match p.get(key) {
        None => return Some(false),
        Some(Value::Bool(b)) => return Some(*b),
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        Some(_) => None,
    }?;
    if number == 1.0 {
        Some(true)
    } else if number == 0.0 {
        Some(false)
    } else {
        None
    }
}

/// A safety setting of a permit type whose change is an administrator action recorded on the row as
/// who changed it, when and why, and re-checked on arrival ([`judge_setting_record`]).
struct SettingRecord {
    /// The setting's column (on / off).
    setting: &'static str,
    /// Wire fields of the record: author (user sync id), time, reason.
    by: &'static str,
    at: &'static str,
    why: &'static str,
    /// Columns of the record here (the copy check).
    by_column: &'static str,
    at_column: &'static str,
    why_column: &'static str,
    /// The setting's name in the messages.
    name: &'static str,
    unaudited: &'static str,
    stale: &'static str,
    unauthorised: &'static str,
    /// Only switching it off is an administrator action (switching it on is a plain edit); otherwise
    /// a change either way, or a new type with it on, is.
    off_only: bool,
}

/// The three-person lock-removal setting (migrations 236, 238; `permit::queries::
/// guard_three_person_change`).
const THREE_PERSON_SETTING: SettingRecord = SettingRecord {
    setting: "requires_three_person_removal",
    by: "three_person_changed_by_sync_id",
    at: "three_person_changed_at",
    why: "three_person_change_reason",
    by_column: "three_person_changed_by_id",
    at_column: "three_person_changed_at",
    why_column: "three_person_change_reason",
    name: "three-person lock-removal rule",
    unaudited: "PERMIT_TYPE_THREE_PERSON_UNAUDITED",
    stale: "PERMIT_TYPE_THREE_PERSON_STALE",
    unauthorised: "PERMIT_TYPE_THREE_PERSON_UNAUTHORISED",
    off_only: false,
};

/// The energy-isolation setting: switching it off (migration 240; `permit::queries::
/// guard_isolation_relax`).
const ISOLATION_SETTING: SettingRecord = SettingRecord {
    setting: "requires_energy_isolation",
    by: "isolation_changed_by_sync_id",
    at: "isolation_changed_at",
    why: "isolation_change_reason",
    by_column: "isolation_changed_by_id",
    at_column: "isolation_changed_at",
    why_column: "isolation_change_reason",
    name: "energy-isolation rule",
    unaudited: "PERMIT_TYPE_ISOLATION_UNAUDITED",
    stale: "PERMIT_TYPE_ISOLATION_STALE",
    unauthorised: "PERMIT_TYPE_ISOLATION_UNAUTHORISED",
    off_only: true,
};

/// Re-check of a permit type received by synchronisation. Its code never changes once the type exists
/// (`PERMIT_TYPE_CODE_LOCKED`, as the local save refuses it). The shipped LOTO type's energy-isolation
/// rule is never switched off (`PERMIT_TYPE_LOTO_ISOLATION_LOCKED`, as the local save refuses it).
/// Its two safety settings — the three-person lock-removal chain (a change either way, or a new type
/// with it on) and the energy-isolation rule (switching it off) — are administrator actions with
/// step-up and a reason, recorded on the row as who changed them, when and why (migrations 238, 240).
/// Each record is judged whenever the received row makes such a change or rewrites the record
/// itself (a row that leaves both alone passes; switching the isolation rule on is a plain edit). It
/// is applied only when:
/// - the setting is a value this version reads one way and stores the same way ([`on_off`]);
/// - the reason has at least [`crate::settings::catalog::MIN_REASON_CHARS`] characters, the time is
///   readable, not in the future and strictly later than the change already recorded here for this
///   type (an older record of this type is not replayed: `..._STALE`; a time recorded here that
///   cannot be read refuses every received change of the setting);
/// - the record is not one already held here for another type (copied from that type);
/// - the person named holds the settings right (`adm.settings`) and the permit-issue right
///   (`ptw.issue`) plant-wide here, judged on arrival (the rights the local save needs).
///
/// A type new here is applied once its record passes the reason and time checks, without judging
/// who changed it, when it carries the setting every type may have without an administrator: the
/// three-person chain off, or any energy-isolation value (a new type is created with the rule off
/// by a plain edit). A PC paired later is so never stuck on a past change by an administrator who
/// has since lost the right.
///
/// A person not applied here yet, or whose rights are still waiting in the inbox, makes it wait;
/// otherwise a sync conflict, nothing written. The step-up itself cannot be re-verified here, and a
/// record made up by the sending device cannot be told from a real one (docs/KNOWN_LIMITS.md).
pub(crate) async fn inbound_permit_type_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "permit_types", entity_sync_id).await?;
    // A type's code is fixed once created (`permit::queries::CODE_PERMIT_TYPE_CODE_LOCKED`): plant
    // rules, permits and work orders name the type by it. Compared exactly, as stored: a received
    // code that is not text (a number or true/false, which would be stored as text, or null) is a
    // change too.
    if let (Some(held), Some(sent)) = (
        local.as_ref().and_then(|l| l.get("code")).and_then(Value::as_str),
        received.get("code"),
    ) {
        if sent.as_str() != Some(held) {
            return conflict(
                crate::permit::queries::CODE_PERMIT_TYPE_CODE_LOCKED,
                format!("permit type {held}: its code cannot be changed (received: {sent})"),
            );
        }
    }
    let ty = overlay(local.as_ref(), received);
    let code = field(&ty, "code").unwrap_or(entity_sync_id).to_string();
    // The shipped LOTO type always requires an energy isolation (`permit::queries::
    // guard_isolation_relax`): a received switch-off is refused, whoever made it.
    let isolation_held = local.as_ref().and_then(|l| on_off(l, ISOLATION_SETTING.setting));
    if crate::permit::queries::is_shipped_loto_type(entity_sync_id, &code)
        && on_off(&ty, ISOLATION_SETTING.setting) == Some(false)
        && isolation_held != Some(false)
    {
        return conflict(
            "PERMIT_TYPE_LOTO_ISOLATION_LOCKED",
            format!("permit type {code}: the LOTO permit type always requires an energy isolation"),
        );
    }
    for setting in [&THREE_PERSON_SETTING, &ISOLATION_SETTING] {
        if let Some(verdict) = judge_setting_record(conn, entity_sync_id, &code, &ty, local.as_ref(), setting).await? {
            return Ok(Some(verdict));
        }
    }
    Ok(None)
}

/// One setting's record re-checked on arrival (`inbound_permit_type_check`).
async fn judge_setting_record(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    code: &str,
    ty: &Payload,
    local: Option<&Payload>,
    s: &SettingRecord,
) -> AppResult<Option<InboundVerdict>> {
    let (Some(wanted), Some(held)) = (
        on_off(ty, s.setting),
        local.map_or(Some(None), |l| on_off(l, s.setting).map(Some)),
    ) else {
        return conflict(
            "INVALID_VALUE",
            format!(
                "permit type {code}: its {} must be on or off, got {}",
                s.name,
                ty.get(s.setting).unwrap_or(&Value::Null)
            ),
        );
    };
    // The update's own restore of the shipped LOTO type's isolation rule (migration 241: nobody, a
    // fixed reason, made by each PC at its own upgrade, at a readable time not in the future, with
    // the same tolerance as every other received time) is no change by anyone: read as no record,
    // so the rows of PCs that needed it and of PCs that did not are accepted both ways. Only for
    // that type and that rule; switching it off stays refused above. A record with an unreadable or
    // future time is not the update's: judged as any record (and refused for its time).
    let restore =
        s.setting == ISOLATION_SETTING.setting && crate::permit::queries::is_shipped_loto_type(entity_sync_id, code);
    let record = |p: &Payload| {
        let r = [s.by, s.at, s.why].map(|k| field(p, k).map(str::to_string));
        let timely = r[1].as_deref().and_then(instant).is_some_and(|at| at <= future_bound());
        if restore
            && timely
            && crate::permit::queries::is_loto_isolation_restore_record(r[0].as_deref(), r[2].as_deref())
        {
            [None, None, None]
        } else {
            r
        }
    };
    let held_record = local.map_or([None, None, None], record);
    let changes = if s.off_only {
        held == Some(true) && !wanted
    } else {
        held.map_or(wanted, |h| h != wanted)
    };
    if !changes && record(ty) == held_record {
        return Ok(None);
    }
    let unaudited = |why: String| {
        conflict(
            s.unaudited,
            format!("permit type {code}: its {} was changed {why}", s.name),
        )
    };
    let min = crate::settings::catalog::MIN_REASON_CHARS;
    let Some(reason) = field(ty, s.why).filter(|r| r.chars().count() >= min) else {
        return unaudited(format!("without a reason of at least {min} characters"));
    };
    let Some(changed_at) = time(ty, s.at) else {
        return unaudited("without a readable time of the change".into());
    };
    if changed_at > future_bound() {
        return unaudited("at a time in the future".into());
    }
    // Each change is later than the one before it: a record not later than the one held here is
    // an earlier change of this type (or this one again), not a new one. A time held here that
    // cannot be read cannot be compared: refused (fails closed).
    if let Some(held_at) = local.and_then(|l| field(l, s.at)) {
        let stale = |why: String| {
            conflict(
                s.stale,
                format!("permit type {code}: its {} was received {why}", s.name),
            )
        };
        let Some(last) = instant(held_at) else {
            return stale(format!(
                "while the change recorded here has an unreadable time ({held_at})"
            ));
        };
        if changed_at <= last {
            return stale(format!(
                "with a record dated {changed_at}, not later than the change already recorded here ({last})"
            ));
        }
    }
    // New here with a value any type may have without an administrator: who changed it is not
    // judged (a PC paired later is never stuck on a past change of a since-demoted administrator).
    if local.is_none() && (s.off_only || !wanted) {
        return Ok(None);
    }
    let unauthorised = |why: &str| {
        conflict(
            s.unauthorised,
            format!("permit type {code}: its {} was changed {why}", s.name),
        )
    };
    let Some(author_sync) = field(ty, s.by) else {
        return unauthorised("without saying who changed it");
    };
    let Some(author) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
            [author_sync.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id").map_err(sync_err))
        .transpose()?
    else {
        return wait(format!(
            "permit type {code}: the person who changed its {} is not applied yet",
            s.name
        ));
    };
    // A change is made on one type at a time: the same person, time and reason as the record held
    // here for another type is that type's record copied.
    let others = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {} AS at, {} AS reason FROM permit_types WHERE {} = ? AND entity_sync_id <> ?",
                s.at_column, s.why_column, s.by_column
            ),
            [author.into(), entity_sync_id.into()],
        ))
        .await?;
    for other in others {
        let at: Option<String> = other.try_get("", "at").map_err(sync_err)?;
        let why: Option<String> = other.try_get("", "reason").map_err(sync_err)?;
        let same_time = at.as_deref().and_then(instant) == Some(changed_at);
        if same_time && why.as_deref().map(str::trim) == Some(reason) {
            return unaudited("with the record of a change of another permit type".into());
        }
    }
    // The rights the local save needs (`commands::permit::upsert_permit_type`). Each is held before
    // the change is made, so its rows come before this one: while some are still waiting here, wait;
    // otherwise refuse (never applied without the right).
    for (right, label) in [
        (crate::rbac::permissions::ADM_SETTINGS, "the settings right"),
        (crate::rbac::permissions::PTW_ISSUE, "the permit-issue right"),
    ] {
        if !crate::settings::site::holds_tenant_right(conn, author, right).await? {
            if rights_may_still_arrive(conn, author_sync, right).await? {
                return wait(format!(
                    "permit type {code}: the rights of the person who changed its {} are still arriving",
                    s.name
                ));
            }
            return unauthorised(&format!("by someone without {label} here"));
        }
    }
    Ok(None)
}

/// Re-check of a delete received by synchronisation. This device never deletes these rows: a
/// permit never (no screen deletes one, a draft included: it is cancelled; its lines and
/// suspensions would not go with it, `permit_isolations.permit_id` has no foreign key); its
/// isolations and suspensions only while the permit is a draft (the LOTO history of any other
/// permit is kept); a cancelled isolation line never, whatever its permit's status and whether its
/// permit is held here or not (it is kept, greyed, as the trace of its cancellation); a permit type
/// and a qualification requirement never (they are edited or deactivated, which keeps their
/// history).
pub(crate) async fn inbound_delete_check(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<InboundVerdict>> {
    if matches!(
        entity_type,
        "qualification_requirements" | "permit_types" | "work_permits"
    ) {
        let held = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT 1 AS h FROM \"{entity_type}\" WHERE entity_sync_id = ?"),
                [entity_sync_id.into()],
            ))
            .await?
            .is_some();
        return if !held {
            Ok(None)
        } else if entity_type == "work_permits" {
            conflict(
                "PERMIT_DELETE_REFUSED",
                format!(
                    "permit {entity_sync_id} cannot be deleted by synchronisation: a permit is cancelled, never \
                     deleted (its isolation lines and suspensions would be left without it)"
                ),
            )
        } else if entity_type == "permit_types" {
            conflict(
                "PERMIT_DELETE_REFUSED",
                "a permit type is edited, never deleted, by synchronisation".into(),
            )
        } else {
            conflict(
                "QUALIFICATION_REQUIREMENT_DELETE_REFUSED",
                "a qualification requirement is deactivated, never deleted, by synchronisation".into(),
            )
        };
    }
    let sql = match entity_type {
        // LEFT JOIN: a cancelled line is refused even when its permit is not held here.
        "permit_isolations" => {
            "SELECT p.status, p.code, c.cancelled_at FROM permit_isolations c \
             LEFT JOIN work_permits p ON p.id = c.permit_id WHERE c.entity_sync_id = ?"
        }
        "permit_suspensions" => {
            "SELECT p.status, p.code, NULL AS cancelled_at FROM permit_suspensions c \
             JOIN work_permits p ON p.id = c.permit_id WHERE c.entity_sync_id = ?"
        }
        _ => return Ok(None),
    };
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [entity_sync_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let status: Option<String> = row.try_get("", "status").map_err(sync_err)?;
    let code: Option<String> = row.try_get("", "code").map_err(sync_err)?;
    let cancelled_at: Option<String> = row.try_get("", "cancelled_at").map_err(sync_err)?;
    if cancelled_at.is_some() {
        return conflict(
            "PERMIT_DELETE_REFUSED",
            format!(
                "a cancelled isolation line of permit {} ('{}') cannot be deleted by synchronisation: it is \
                 kept as the trace of its cancellation",
                code.as_deref().unwrap_or("not held here"),
                status.as_deref().unwrap_or("unknown")
            ),
        );
    }
    // A planned line whose permit is not held here (rows left from before permit deletes were
    // refused): nothing left to protect, as before.
    let (Some(status), Some(code)) = (status, code) else {
        return Ok(None);
    };
    if status != "draft" {
        return conflict(
            "PERMIT_DELETE_REFUSED",
            format!(
                "{entity_type} of permit {code} ('{status}') cannot be deleted by synchronisation: only a draft \
                 permit, its isolations and suspensions (the LOTO history of any other permit is kept)"
            ),
        );
    }
    Ok(None)
}

/// Code of a work-order status named by its sync id; `None` when not here.
pub(crate) async fn status_code(conn: &impl ConnectionTrait, status_sync_id: &str) -> AppResult<Option<String>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT code FROM work_order_statuses WHERE entity_sync_id = ?",
        [status_sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<String>("", "code").map_err(sync_err))
    .transpose()
}

/// The received work order, when its start is judged here (otherwise `None`), with its local copy
/// overlaid: when it enters `in_progress` here; or when it arrives `completed` or `closed` while
/// this PC never held it in execution (new here, or draft / planning / ready here), so the start
/// made elsewhere on the way is judged too (AU-99 review N4) — except plant history received as a
/// work order new here by the snapshot of this PC's first join (`sync::plant_join::
/// first_join_snapshot_row`: activated, no complete pull yet, nothing of it held here). A snapshot
/// received once this PC has joined, or on a PC with no activation, is judged.
async fn work_order_start(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Result<Option<Payload>, String>> {
    let local = local_wire(conn, "work_orders", entity_sync_id).await?;
    let wo = overlay(local.as_ref(), received);
    let Some(status_sync) = field(&wo, "status_sync_id") else {
        return Ok(Ok(None));
    };
    let Some(to) = status_code(conn, status_sync).await? else {
        return Ok(Err(format!("work-order status {status_sync} is not applied yet")));
    };
    let from = match local.as_ref().and_then(|l| field(l, "status_sync_id")) {
        Some(from_sync) => status_code(conn, from_sync).await?,
        None => None,
    };
    let judged = match to.as_str() {
        "in_progress" => from.as_deref() != Some("in_progress"),
        "completed" | "closed" => {
            !matches!(
                from.as_deref(),
                Some("in_progress" | "on_hold" | "completed" | "closed")
            ) && (local.is_some()
                || !crate::sync::plant_join::first_join_snapshot_row(conn, "work_orders", entity_sync_id).await?)
        }
        _ => false,
    };
    Ok(Ok(judged.then_some(wo)))
}

pub(crate) async fn wo_children(
    conn: &impl ConnectionTrait,
    table: &'static str,
    key: &str,
    col: &str,
    wo_sync: &str,
) -> AppResult<Children> {
    children(conn, table, "work_orders", col, key, wo_sync).await
}

/// [`wo_children`], where a reference to one of `applied_with` (rows of the same group applied
/// before these) does not block a waiting row.
pub(crate) async fn wo_children_with(
    conn: &impl ConnectionTrait,
    table: &'static str,
    key: &str,
    col: &str,
    wo_sync: &str,
    applied_with: &[(&str, String)],
) -> AppResult<Children> {
    children_with(conn, table, "work_orders", col, key, wo_sync, applied_with).await
}

/// Re-check of a protected permit requirement of a work order (`wo::workflow::actions::
/// permit_requirement`): the flag a plant rule set (`permit_rule_code`), or the requirement carried
/// from a request flagged "sécurité" (`permit_from_safety_di`, migration 255). A received work order
/// whose protected requirement is cleared (`requires_permit` 0) says who cleared it, when and why (10
/// characters or more), before its execution started, and that person holds the permit-issue right
/// here (plant-wide for a rule, at the work order's site for a safety request:
/// `permit_requirement::may_clear_protected_requirement`). A person not applied here yet makes it
/// wait. A state already held here (same clearing) passes. A different complete clearing received
/// over one recorded here is judged the same way; a removed or partial one is refused.
///
/// A plant rule is recorded when the work order is created, or later on a work order not started yet
/// that a rule matches (`permit::requirement_rules::stamp_matching_rule`: each PC derives the same
/// from the same work order and rule list). A received row that carries no rule does not speak for
/// the rule (audit finding AU-04 R2-B1):
/// - with no rule held here (its sender has not applied the rule list that matches it here yet, owner
///   decision D2 (a), audit finding AU-04 B2), it is applied: a work order not started gets the rule
///   after the apply, and one already started is recorded as a safety finding
///   (`sync::apply_hooks::after_received_work_order`);
/// - over a rule held here, it is judged with that rule: a clearing it carries is judged as a
///   clearing of that rule (the permit-issue right plant-wide, who, when and why), and one without a
///   clearing (a PC that never had the rule) is applied with the rule, its permit type, the
///   requirement and a clearing held here kept ([`keep_held_rule_trace`]), a started work order
///   recorded once as a safety finding.
///
/// A received row that names another rule, or another permit type for the rule, is refused once the
/// work order has started, here or in the row; before (two PCs that derived different rules), it
/// is applied and each PC keeps the rule its own list gives (`requirement_rules::
/// converge_received_rule`, audit findings AU-04 R2-S1, R3-B1).
/// The safety origin is never turned off by a received row. A work order new here sent by a PC
/// without migration 255 (no `permit_from_safety_di` in the row) is judged with the safety flag of
/// its source request here, and gets that origin when it is applied (`sync::apply_hooks::
/// after_upsert`).
pub(crate) async fn inbound_permit_rule_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    use crate::wo::workflow::actions::permit_requirement::{self as requirement, ProtectedOrigin};
    let local = local_wire(conn, "work_orders", entity_sync_id).await?;
    let mut wo = overlay(local.as_ref(), received);
    let code = field(&wo, "code").unwrap_or(entity_sync_id).to_string();
    // A row without a rule over a rule held here is judged with the rule held here (R2-B1).
    let held_rule = local.as_ref().filter(|l| field(l, "permit_rule_code").is_some());
    let blank_over_held = held_rule.is_some() && field(&wo, "permit_rule_code").is_none();
    if let Some(l) = held_rule.filter(|_| blank_over_held) {
        for key in RULE_TRACE_KEYS {
            wo.insert(key.to_string(), l.get(key).cloned().unwrap_or(Value::Null));
        }
    }
    // The rule trace is written once (`wo::queries::create_work_order_in`, `di::conversion`,
    // `requirement_rules::stamp_matching_rule`): a received row that names another rule or permit
    // type is refused once the work order has started. Before, it comes from a PC that derived its
    // rule from another version of the list: applied, and both PCs keep the same rule (see above).
    if let Some(l) = held_rule {
        let trace = |p: &Payload| {
            (
                field(p, "permit_rule_code").map(str::to_string),
                field(p, "permit_rule_permit_type").map(str::to_string),
            )
        };
        if trace(l) != trace(&wo) {
            let changed = || {
                conflict(
                    "WO_PERMIT_RULE_CHANGED",
                    format!("work order {code}: the plant rule that requires its permit cannot be removed or changed"),
                )
            };
            // Started on either side (here, or in the row): refused. A row that moves the status
            // back to "ready" does not make a started work order not started (R3-B1).
            for side in [l, &wo] {
                let Some(status_sync) = field(side, "status_sync_id") else {
                    return changed();
                };
                let Some(status) = status_code(conn, status_sync).await? else {
                    return wait(format!("work-order status {status_sync} is not applied yet"));
                };
                if !requirement::NOT_STARTED_STATUS_CODES.contains(&status.as_str()) {
                    return changed();
                }
            }
        }
    }
    // A request flagged "sécurité" here protects its work orders that have not started: each PC
    // derives that protection itself from the request (`permit_requirement::
    // raise_work_orders_of_safety_request`, after the apply here). A received row without that origin
    // for such a work order comes from a PC that has not received the flag yet: it is applied and
    // protected here in the same transaction, never stored unprotected, and that PC raises it too
    // when the request reaches it. Except over a clearing recorded here (it is not replaced by a row
    // that never saw it), and a row that turns the origin off without a flagged request (the origin
    // is written once, at conversion or create). A work order stored here with origin 0 while its
    // request is flagged here (received started, as sent) is judged with that origin on every later
    // row too, so a later untick still needs who, when and why.
    let local_origin = local.as_ref().is_some_and(|l| flag(l, SAFETY_ORIGIN));
    let mut safety_origin = flag(&wo, SAFETY_ORIGIN);
    if !safety_origin {
        let request_flagged = match source_request_is_safety(conn, &wo).await? {
            Some(flagged) => flagged,
            None if local.is_none() => {
                return wait(format!(
                    "work order {code}: the request it was converted from is not applied yet"
                ))
            }
            None => false,
        };
        let origin_off = || {
            conflict(
                "WO_PERMIT_SAFETY_ORIGIN_CHANGED",
                format!(
                    "work order {code}: the safety request its permit requirement comes from cannot be removed by \
                     synchronisation"
                ),
            )
        };
        if request_flagged {
            let Some(status_sync) = field(&wo, "status_sync_id") else {
                return origin_off();
            };
            let Some(status) = status_code(conn, status_sync).await? else {
                return wait(format!("work-order status {status_sync} is not applied yet"));
            };
            let not_started = crate::wo::workflow::actions::permit_requirement::NOT_STARTED_STATUS_CODES
                .contains(&status.as_str())
                && time(&wo, "actual_start").is_none();
            let cleared_here = local
                .as_ref()
                .is_some_and(|l| field(l, "permit_rule_cleared_at").is_some());
            if not_started && !cleared_here {
                return Ok(None);
            }
            if local_origin {
                return origin_off();
            }
            // New here and started: judged with that origin (a clearing says who, when and why).
            safety_origin = true;
        } else if local_origin {
            return origin_off();
        }
    }
    // Over a rule held here, a row without a rule and without a clearing comes from a PC that never
    // had the rule: applied, the rule's trace, requirement and clearing held here kept
    // ([`keep_held_rule_trace`]).
    if blank_over_held && CLEARING_KEYS.iter().all(|k| field(received, k).is_none()) {
        return Ok(None);
    }
    let origin = match field(&wo, "permit_rule_code") {
        Some(rule) => ProtectedOrigin::Rule(rule.to_string()),
        None if safety_origin => ProtectedOrigin::SafetyRequest,
        None => return Ok(None),
    };
    let required_by = match origin {
        ProtectedOrigin::Rule(_) => "a plant rule",
        ProtectedOrigin::SafetyRequest => "a request flagged as a safety concern",
    };
    let clearing = |p: &Payload| {
        [
            field(p, "permit_rule_cleared_by_sync_id").map(str::to_string),
            field(p, "permit_rule_cleared_at").map(str::to_string),
            field(p, "permit_rule_clear_reason").map(str::to_string),
        ]
    };
    // The whole clearing (who, when, why) is the audit trace: any change of it is judged.
    let none: [Option<String>; 3] = [None, None, None];
    let clearing_changed = local
        .as_ref()
        .map_or(clearing(&wo) != none, |l| clearing(l) != clearing(&wo));
    // A clearing recorded here is never removed nor partly rewritten (who, when or why missing). A
    // different complete clearing (another PC that cleared the same order offline) is judged in full
    // below, like a new one, so both PCs converge on it when it is valid.
    // A clearing withdrawn as a whole with the requirement in force again (no who, when or why): what
    // a PC does when the requirement is required again (`permit_requirement::
    // withdraw_clearing_on_retick`) or when a safety requirement changes site
    // (`void_clearing_on_site_change`): the protected state, accepted. Removing a clearing without
    // requiring the permit, or a part of it, is refused below.
    if clearing_changed && clearing(&wo) == none && flag(&wo, "requires_permit") {
        return Ok(None);
    }
    // A safety requirement cleared at one site does not stay cleared at another: a received move of
    // a work order whose clearing is kept is refused (the PC that moved it withdraws the clearing in
    // the same change). A work order that had no site here was cleared with the plant-wide right,
    // which holds at any site.
    if origin == ProtectedOrigin::SafetyRequest && field(&wo, "permit_rule_cleared_at").is_some() {
        if let Some(l) = local.as_ref() {
            let (held_site, new_site) = (field(l, "entity_sync_id"), field(&wo, "entity_sync_id"));
            if held_site.is_some() && held_site != new_site && clearing(l) == clearing(&wo) {
                return conflict(
                    "WO_PERMIT_RULE_CLEAR_INVALID",
                    format!(
                        "work order {code}: its permit requirement from a safety request was cleared at its site \
                         here and the received work order moves it to another site with that clearing kept"
                    ),
                );
            }
        }
    }
    if clearing_changed
        && local
            .as_ref()
            .is_some_and(|l| field(l, "permit_rule_cleared_at").is_some())
        && clearing(&wo).iter().any(Option::is_none)
    {
        return conflict(
            "WO_PERMIT_RULE_CLEAR_INVALID",
            format!(
                "work order {code}: the clearing of its permit requirement (required by {required_by}) recorded \
                 here cannot be removed or partly rewritten"
            ),
        );
    }
    if !clearing_changed {
        // Still required: the requirement is in force (the start gate judges it).
        if flag(&wo, "requires_permit") {
            return Ok(None);
        }
        // The cleared state held here, unchanged: judged again only when the received row moves the
        // work's start before the clearing (a clearing is valid only before execution started).
        if let Some(l) = local.as_ref().filter(|l| !flag(l, "requires_permit")) {
            let start_moved = field(l, "actual_start") != field(&wo, "actual_start");
            let cleared = time(&wo, "permit_rule_cleared_at");
            let started = time(&wo, "actual_start");
            if start_moved && cleared.zip(started).is_some_and(|(c, s)| c > s) {
                return conflict(
                    "WO_PERMIT_RULE_CLEAR_INVALID",
                    format!(
                        "work order {code}: the permit required by {required_by} was cleared after its execution \
                         started (the received start is before the clearing recorded here)"
                    ),
                );
            }
            return Ok(None);
        }
    }
    let invalid = |why: &str| {
        conflict(
            "WO_PERMIT_RULE_CLEAR_INVALID",
            format!("work order {code}: the permit required by {required_by} was cleared {why}"),
        )
    };
    let reason_ok = field(&wo, "permit_rule_clear_reason")
        .is_some_and(|r| r.chars().count() >= requirement::MIN_CLEAR_REASON_CHARS);
    let Some(cleared_at) = time(&wo, "permit_rule_cleared_at") else {
        return invalid("without the time it was cleared");
    };
    if !reason_ok {
        return invalid("without a reason of at least 10 characters");
    }
    if cleared_at > future_bound() {
        return invalid("at a time in the future");
    }
    if time(&wo, "actual_start").is_some_and(|started| cleared_at > started) {
        return invalid("after its execution started");
    }
    // Past "ready" without a start time: it started at a time not known here, so a clearing cannot
    // be shown to come before it (the clear action itself refuses a work order past "ready").
    if time(&wo, "actual_start").is_none() {
        if let Some(status_sync) = field(&wo, "status_sync_id") {
            let Some(status) = status_code(conn, status_sync).await? else {
                return wait(format!("work-order status {status_sync} is not applied yet"));
            };
            if PAST_READY.contains(&status.as_str()) {
                return invalid(&format!(
                    "while it is '{status}' with no start time (it counts as started)"
                ));
            }
        }
    }
    let Some(clearer_sync) = field(&wo, "permit_rule_cleared_by_sync_id") else {
        return invalid("without saying who cleared it");
    };
    let Some(clearer) = local_user(conn, clearer_sync).await? else {
        return wait(format!(
            "work order {code}: the person who cleared its permit requirement is not applied yet"
        ));
    };
    // The work order's site here (its org node, `entity_id`, sent as `entity_sync_id`).
    let site_sync = field(&wo, "entity_sync_id");
    let site = match site_sync {
        Some(node) => local_id(conn, "org_nodes", node).await?,
        None => None,
    };
    if !requirement::may_clear_protected_requirement(conn, clearer, &origin, site).await? {
        if origin == ProtectedOrigin::SafetyRequest && site.is_none() {
            if let Some(node) = site_sync {
                return wait(format!("work order {code}: its site {node} is not applied yet"));
            }
        }
        // The right is held on the PC that cleared it before the clearing, so its rows come before
        // this one: while some are still waiting here, wait; otherwise refuse (never applied without
        // the right).
        if rights_may_still_arrive(conn, clearer_sync, crate::rbac::permissions::PTW_ISSUE).await? {
            return wait(format!(
                "work order {code}: the rights of the person who cleared its permit requirement are still arriving"
            ));
        }
        let right_where = match origin {
            ProtectedOrigin::Rule(_) => "the permit-issue right here",
            ProtectedOrigin::SafetyRequest => "the permit-issue right at its site here",
        };
        return conflict(
            "WO_PERMIT_RULE_CLEAR_UNAUTHORISED",
            format!(
                "work order {code}: the permit required by {required_by} was cleared by someone without {right_where}"
            ),
        );
    }
    Ok(None)
}

/// The column that records a permit requirement carried from a request flagged "sécurité".
pub(crate) const SAFETY_ORIGIN: &str = "permit_from_safety_di";

/// The keys of a work order's plant rule trace (the rule and the permit type it calls for).
const RULE_TRACE_KEYS: [&str; 2] = ["permit_rule_code", "permit_rule_permit_type"];

/// The keys of the clearing of a protected permit requirement (who, when, why).
const CLEARING_KEYS: [&str; 3] = [
    "permit_rule_cleared_by_sync_id",
    "permit_rule_cleared_at",
    "permit_rule_clear_reason",
];

/// What the apply of a received work order does with the plant rule held here, decided before its
/// row is written ([`keep_held_rule_trace`]) and finished after (`sync::apply_hooks::
/// after_received_work_order`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HeldRule {
    /// The row carried no rule over the rule held here: the rule, its permit type and, when the row
    /// carries no clearing, the requirement and the clearing held here are kept. `received_requires`:
    /// whether the row required the permit (recorded in the finding of a started work order).
    Kept { received_requires: bool },
    /// The row named another rule (or permit type) on a work order not started: applied, then each
    /// PC keeps the rule its own list gives (`requirement_rules::converge_received_rule`). `held`:
    /// the rule and the clearing held here before the write.
    Differs {
        held: crate::permit::requirement_rules::HeldTrace,
    },
    /// The row named a rule while none is held here (a work order new here, or not stamped yet):
    /// judged the same way after the write, never adopted as sent when the list here asks for
    /// another rule or permit type (audit finding AU-04 R4-B1).
    Received,
}

/// Before a received work order is written (it passed [`inbound_permit_rule_check`]), audit
/// findings AU-04 R2-B1 / R2-S1. A row that carries no rule (the key sent empty) over a rule held
/// here does not speak for the rule: the rule and its permit type held here are written instead of
/// the empty ones; when the row carries no clearing either (a PC that never had the rule), the
/// clearing held here is kept too and the permit stays required if either side requires it (a
/// clearing held here then withdrawn as a re-tick, `permit_requirement::
/// withdraw_received_clearing_of_required`, recorded). A row with a clearing, judged as a clearing of
/// the rule held here, writes that clearing. The apply runs with capture suppressed: nothing of this
/// is sent, and every PC keeps its rule the same way. A row that names another rule is noted, to
/// converge after the write. A row without the rule keys (an older sender) leaves them as they are.
pub(crate) async fn keep_held_rule_trace(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    payload: &mut Payload,
) -> AppResult<Option<HeldRule>> {
    if !payload.contains_key("permit_rule_code") {
        return Ok(None);
    }
    let local = local_wire(conn, "work_orders", entity_sync_id).await?;
    let held = local
        .as_ref()
        .and_then(|l| field(l, "permit_rule_code").map(str::to_string));
    let (Some(local), Some(held)) = (local, held) else {
        // Nothing held here: a row that names a rule is judged after the write (R4-B1).
        return Ok(field(payload, "permit_rule_code").map(|_| HeldRule::Received));
    };
    let held_type = field(&local, "permit_rule_permit_type").map(str::to_string);
    match field(payload, "permit_rule_code").map(str::to_string) {
        Some(received) => {
            let received_type = match payload.get("permit_rule_permit_type") {
                Some(_) => field(payload, "permit_rule_permit_type").map(str::to_string),
                None => held_type.clone(),
            };
            if received == held && received_type == held_type {
                return Ok(None);
            }
            let text = |k: &str| field(&local, k).map(str::to_string);
            Ok(Some(HeldRule::Differs {
                held: crate::permit::requirement_rules::HeldTrace {
                    code: held,
                    permit_type: held_type,
                    cleared_by_sync_id: text("permit_rule_cleared_by_sync_id"),
                    cleared_at: text("permit_rule_cleared_at"),
                    clear_reason: text("permit_rule_clear_reason"),
                },
            }))
        }
        None => {
            let received_requires = flag(payload, "requires_permit");
            for key in RULE_TRACE_KEYS {
                payload.insert(key.to_string(), local.get(key).cloned().unwrap_or(Value::Null));
            }
            if CLEARING_KEYS.iter().all(|k| field(payload, k).is_none()) {
                for key in CLEARING_KEYS {
                    payload.insert(key.to_string(), local.get(key).cloned().unwrap_or(Value::Null));
                }
                let requires = received_requires || flag(&local, "requires_permit");
                payload.insert("requires_permit".into(), Value::from(i64::from(requires)));
            }
            Ok(Some(HeldRule::Kept { received_requires }))
        }
    }
}

/// Whether the source request of a received work order (`source_di_sync_id`) is flagged "sécurité"
/// here: `Some(false)` when it has none, `None` when it is not applied here yet.
async fn source_request_is_safety(conn: &impl ConnectionTrait, wo: &Payload) -> AppResult<Option<bool>> {
    let Some(di_sync) = field(wo, "source_di_sync_id") else {
        return Ok(Some(false));
    };
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT safety_flag FROM intervention_requests WHERE entity_sync_id = ?",
        [di_sync.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "safety_flag").map(|f| f != 0).map_err(sync_err))
    .transpose()
}

/// Whether rows that could give `user_sync_id` the right `permission` are still waiting in the inbox
/// here: a waiting plant-wide, non-emergency assignment of that person to a role that carries the
/// permission (here), or a waiting permission row for `permission` of a role that person holds
/// plant-wide (here, or by such a waiting assignment). Rows of other people, other roles, other
/// permissions, site-scoped or emergency assignments never hold the check. A right is held before it
/// is used, so its rows reach this PC before (or with) the rows that use it; while they wait, the
/// check waits too (fail-closed: nothing is applied without the right).
pub(crate) async fn rights_may_still_arrive(
    conn: &impl ConnectionTrait,
    user_sync_id: &str,
    permission: &str,
) -> AppResult<bool> {
    const WAITING: &str = "apply_status IN ('pending', 'deferred') AND operation NOT IN ('delete', 'archive')";
    // The roles that person holds plant-wide: here, and by waiting assignments.
    let mut roles: Vec<String> = Vec::new();
    for r in conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ro.entity_sync_id AS role FROM user_scope_assignments usa \
               JOIN roles ro ON ro.id = usa.role_id JOIN user_accounts ua ON ua.id = usa.user_id \
              WHERE ua.entity_sync_id = ? AND usa.deleted_at IS NULL AND usa.scope_type = 'tenant' \
                AND COALESCE(usa.is_emergency, 0) = 0",
            [user_sync_id.into()],
        ))
        .await?
    {
        if let Some(role) = r.try_get::<Option<String>>("", "role").map_err(sync_err)? {
            roles.push(role);
        }
    }
    let mut waiting_roles: Vec<String> = Vec::new();
    for r in conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT json_extract(payload_json, '$.role_sync_id') AS role FROM sync_inbox \
                 WHERE entity_type = 'user_scope_assignments' AND {WAITING} \
                   AND json_extract(payload_json, '$.user_sync_id') = ? \
                   AND json_extract(payload_json, '$.scope_type') = 'tenant' \
                   AND COALESCE(json_extract(payload_json, '$.is_emergency'), 0) = 0"
            ),
            [user_sync_id.into()],
        ))
        .await?
    {
        if let Some(role) = r.try_get::<Option<String>>("", "role").map_err(sync_err)? {
            waiting_roles.push(role);
        }
    }
    // A waiting assignment to a role that carries the permission here.
    for role in &waiting_roles {
        let carries = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS c FROM roles ro JOIN role_permissions rp ON rp.role_id = ro.id \
                   JOIN permissions p ON p.id = rp.permission_id \
                  WHERE ro.entity_sync_id = ? AND p.name = ?",
                [role.as_str().into(), permission.into()],
            ))
            .await?
            .is_some();
        if carries {
            return Ok(true);
        }
    }
    roles.extend(waiting_roles);
    if roles.is_empty() {
        return Ok(false);
    }
    // A waiting permission row for that permission, of a role that person holds.
    let list = serde_json::to_string(&roles)?;
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS w FROM sync_inbox WHERE entity_type = 'role_permissions' AND {WAITING} \
                   AND json_extract(payload_json, '$.role_sync_id') IN (SELECT value FROM json_each(?)) \
                   AND json_extract(payload_json, '$.permission_key') = ? \
                 LIMIT 1"
            ),
            [list.into(), permission.into()],
        ))
        .await?
        .is_some())
}

/// Re-check of a work order received by synchronisation as it enters `in_progress`: the permit gate
/// (`permit::wo_gate`) at the time the work started, resumed or was reopened. A permit is judged in
/// the state it had at that time (its activation, close, suspensions and expiry are dated), so a later
/// version of it (suspended or closed since) does not refuse a start that was valid.
pub(crate) async fn inbound_work_order_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let wo = match work_order_start(conn, entity_sync_id, received).await? {
        Err(why) => return wait(why),
        Ok(None) => return Ok(None),
        Ok(Some(wo)) => wo,
    };
    let code = field(&wo, "code").unwrap_or(entity_sync_id).to_string();
    let Some(started) = time(&wo, "actual_start") else {
        return conflict(
            "SYNC_STATE_TIME_MISSING",
            format!("work order {code} is in progress but its start time is missing or unreadable"),
        );
    };
    let segments = wo_children(
        conn,
        "work_order_delay_segments",
        "work_order_sync_id",
        "work_order_id",
        entity_sync_id,
    )
    .await?;
    let transitions = wo_children(conn, "wo_state_transition_log", "wo_sync_id", "wo_id", entity_sync_id).await?;
    let permits = wo_children(
        conn,
        "work_permits",
        "linked_work_order_sync_id",
        "linked_work_order_id",
        entity_sync_id,
    )
    .await?;
    if let Some(why) = [&segments, &transitions, &permits]
        .iter()
        .find_map(|c| c.blocked.clone())
    {
        return wait(format!("work order {code}: {why}"));
    }
    // When it last entered in progress: its start, the end of its last wait (resume), its last
    // recorded change to in progress (reopen; local rows may have no sync identity).
    let mut moments = vec![started];
    moments.extend(payloads(&segments).iter().filter_map(|s| time(s, "ended_at")));
    // Its recorded entries into in progress: when, by which action, and the estimated remaining time
    // entered on a resume or reopening (migration 242).
    let mut entries: Vec<(DateTime<Utc>, Option<String>, Hours)> = payloads(&transitions)
        .iter()
        .filter(|t| field(t, "to_status") == Some("in_progress"))
        .filter_map(|t| {
            time(t, "acted_at").map(|at| {
                (
                    at,
                    field(t, "action").map(str::to_string),
                    hours(t, "estimated_remaining_hours"),
                )
            })
        })
        .collect();
    for r in conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.acted_at AS at, t.action AS action, \
             CASE WHEN typeof(t.estimated_remaining_hours) IN ('real', 'integer') \
             THEN t.estimated_remaining_hours END AS rem_number, \
             CASE WHEN typeof(t.estimated_remaining_hours) = 'text' THEN t.estimated_remaining_hours END AS rem_text \
             FROM wo_state_transition_log t JOIN work_orders w ON w.id = t.wo_id \
             WHERE w.entity_sync_id = ? AND t.to_status = 'in_progress'",
            [entity_sync_id.into()],
        ))
        .await?
    {
        if let Some(acted) = r
            .try_get::<Option<String>>("", "at")
            .map_err(sync_err)?
            .as_deref()
            .and_then(instant)
        {
            let entered = match r.try_get::<Option<f64>>("", "rem_number").map_err(sync_err)? {
                Some(h) => Hours::Value(h),
                None => Hours::from_text(
                    r.try_get::<Option<String>>("", "rem_text")
                        .map_err(sync_err)?
                        .as_deref(),
                ),
            };
            let action = r.try_get::<Option<String>>("", "action").map_err(sync_err)?;
            entries.push((acted, action, entered));
        }
    }
    moments.extend(entries.iter().map(|(at, _, _)| *at));
    // The row was written when it last entered in progress: a wait or status change dated after it,
    // or any time in the future, is not consistent.
    let written = time(&wo, "updated_at");
    if let Some(bad) = moments
        .iter()
        .enumerate()
        .find(|(n, t)| **t > future_bound() || (*n > 0 && written.is_some_and(|w| **t > w)))
        .map(|(_, t)| *t)
    {
        return conflict(
            "SYNC_TIME_INCONSISTENT",
            format!("work order {code}: a start or resume time ({bad}) is later than the row itself or in the future"),
        );
    }
    let at = moments.into_iter().max().unwrap_or(started);
    // The estimated remaining time entered on the resume or reopening at `at`, used instead of the
    // expected duration as on the PC that recorded it (`permit::wo_gate`): only when every recorded
    // entry at that moment is a resume or a reopening carrying one (the largest); otherwise (none
    // recorded, a start, a missing value) the full expected duration. A value that is not a number of
    // hours greater than 0 is never recorded by a PC: refused.
    let mut remaining: Option<f64> = None;
    let mut full_duration = false;
    for (_, action, entered) in entries.iter().filter(|(when, _, _)| *when == at) {
        if *entered == Hours::Absent {
            full_duration = true;
            continue;
        }
        let Some(h) = entered.usable() else {
            let value = match entered {
                Hours::Value(h) => h.to_string(),
                Hours::Unreadable(raw) => raw.clone(),
                Hours::Absent => String::new(),
            };
            return conflict(
                crate::permit::wo_gate::CODE_REMAINING_TIME_INVALID,
                format!(
                    "work order {code}: the estimated remaining time recorded when it entered in progress at {at} ({value}) is not a number of hours greater than 0"
                ),
            );
        };
        if matches!(action.as_deref(), Some("resume" | "reopen")) {
            remaining = Some(remaining.map_or(h, |r: f64| r.max(h)));
        } else {
            full_duration = true;
        }
    }
    let remaining = remaining.filter(|_| !full_duration);
    if permits.rows.is_empty() && flag(&wo, "requires_permit") {
        return wait(format!(
            "work order {code} requires a work permit and none has been received yet"
        ));
    }
    let mut live_at_start = 0;
    let mut live_types: Vec<String> = Vec::new();
    for (permit_sync, p) in &permits.rows {
        let pcode = field(p, "code").unwrap_or_default();
        let status = field(p, "status").unwrap_or_default();
        // Cancelled or expired: not live (as locally). Closed or handed back before the start: no
        // longer live then.
        if status == "cancelled" || status == "expired" {
            continue;
        }
        let ended = [time(p, "closed_at"), time(p, "handed_back_at")]
            .into_iter()
            .flatten()
            .min();
        if ended.is_some_and(|e| e <= at) {
            continue;
        }
        live_at_start += 1;
        if let Some(type_sync) = field(p, "permit_type_sync_id") {
            if let Some(r) = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT code FROM permit_types WHERE entity_sync_id = ?",
                    [type_sync.into()],
                ))
                .await?
            {
                live_types.push(r.try_get::<String>("", "code").map_err(sync_err)?);
            }
        }
        if time(p, "activated_at").is_none_or(|a| a > at) {
            return conflict(
                "PERMIT_NOT_ACTIVE",
                format!("work order {code} started at {at} while permit {pcode} was not active yet"),
            );
        }
        let isolations = permit_children(conn, "permit_isolations", permit_sync).await?;
        let suspensions = permit_children(conn, "permit_suspensions", permit_sync).await?;
        if let Some(why) = [&isolations, &suspensions].iter().find_map(|c| c.blocked.clone()) {
            return wait(format!("work order {code}: permit {pcode}: {why}"));
        }
        let suspended = payloads(&suspensions).iter().any(|s| {
            time(s, "suspended_at").is_some_and(|t| t <= at) && time(s, "reinstated_at").is_none_or(|r| r > at)
        });
        if suspended {
            return conflict(
                "PERMIT_SUSPENDED",
                format!("work order {code} started at {at} while permit {pcode} was suspended"),
            );
        }
        if let Some(expiry) = field(p, "expires_at") {
            if instant(expiry).is_none_or(|e| e <= at) {
                return conflict(
                    "PERMIT_EXPIRED",
                    format!("work order {code} started at {at} after permit {pcode} expired ({expiry})"),
                );
            }
            // The work would outlive its protection (`permit::wo_gate`), judged as of the start or
            // resume the row records (`at`, never this PC's clock): against the later of the planned
            // end (fixed once the order is past ready) and `at` plus the hours of work (the remaining
            // time entered on that resume or reopening, else the expected duration); a planned end
            // already past with no hours, or an unreadable planned end or duration, is refused; with
            // neither an end nor hours, not judged. As locally.
            if let Some(expires) = instant(expiry) {
                use crate::permit::wo_gate::{expiry_refusal, shown_hours, stored, work_end, WorkEnd};
                let end = work_end(
                    field(&wo, "planned_end"),
                    &hours(&wo, "expected_duration_hours"),
                    remaining,
                    at,
                );
                if let Some(c) = expiry_refusal(expires, &end) {
                    let why = match &end {
                        WorkEnd::Planned(e) => format!("expires ({expiry}) before its planned end ({})", stored(*e)),
                        WorkEnd::Estimated { end: e, hours, remaining } => format!(
                            "expires ({expiry}) before its estimated end ({}: start or resume + {} h, {}; later than any planned end)",
                            stored(*e),
                            shown_hours(*hours),
                            if *remaining {
                                "the estimated remaining time entered"
                            } else {
                                "the expected duration"
                            }
                        ),
                        WorkEnd::Unreadable(raw) => {
                            format!("is judged against a planned end ({raw}) that is not a readable date and time")
                        }
                        WorkEnd::PlannedPassed(p) => format!(
                            "expires ({expiry}) and its planned end ({}) had passed with no expected duration or remaining time: the end of the work is unknown",
                            stored(*p)
                        ),
                        WorkEnd::DurationUnreadable(raw) => {
                            format!("is judged against an expected duration ({raw}) that is not a number of hours")
                        }
                        WorkEnd::Unknown => String::new(),
                    };
                    return conflict(
                        c,
                        format!("work order {code} started at {at} while permit {pcode} {why}"),
                    );
                }
            }
        }
        let Some(needs) = type_requires_isolation(conn, p).await? else {
            return wait(format!("permit {pcode}: its permit type is not applied yet"));
        };
        // The isolations the permit held at the start: fitted by then (one fitted later, while it
        // was suspended again, does not count against a start that was valid); each verified by then.
        let all = payloads(&isolations);
        let held: Vec<&Payload> = all
            .iter()
            .copied()
            .filter(|i| field(i, "applied_at").is_some_and(|t| instant(t).is_none_or(|t| t <= at)))
            .collect();
        if held.iter().any(|i| time(i, "verified_at").is_some_and(|v| v > at)) {
            return conflict(
                "LOTO_NOT_VERIFIED",
                format!("work order {code} started at {at} before an isolation of permit {pcode} was verified"),
            );
        }
        let three_person = type_three_person(conn, p).await?;
        if let Some((c, m)) = isolations_ready(pcode, needs, three_person, &held, at) {
            return conflict(c, m);
        }
    }
    if live_at_start == 0 && flag(&wo, "requires_permit") {
        return conflict(
            "PERMIT_REQUIRED_TO_START",
            format!("work order {code} requires a work permit and none was live when it started"),
        );
    }
    // A plant rule that flagged it (flag not cleared) names the permit type it needs
    // (`permit::wo_gate`): one live permit is of that type.
    if flag(&wo, "requires_permit") && field(&wo, "permit_rule_cleared_at").is_none() {
        if let Some(rule_type) = field(&wo, "permit_rule_permit_type") {
            if !live_types.iter().any(|t| t == rule_type) {
                return conflict(
                    "PERMIT_TYPE_REQUIRED_TO_START",
                    format!(
                        "work order {code} started without a live permit of type {rule_type}, required by a plant rule"
                    ),
                );
            }
        }
    }
    Ok(None)
}

/// The waiting permits, waits and status changes a received work-order start was judged on, to
/// apply with it (each permit then applies its own children with itself).
pub(crate) async fn work_order_group(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<Group>> {
    if !matches!(work_order_start(conn, entity_sync_id, received).await?, Ok(Some(_))) {
        return Ok(None);
    }
    let mut rows = Vec::new();
    let permits = wo_children(
        conn,
        "work_permits",
        "linked_work_order_sync_id",
        "linked_work_order_id",
        entity_sync_id,
    )
    .await?;
    rows.extend(permits.counted);
    // The isolations and suspensions the start was judged on: a permit applied in this group
    // applies them itself first (they are then skipped here); those of a permit already here are
    // applied here.
    for (permit_sync, _) in &permits.rows {
        for table in ["permit_isolations", "permit_suspensions"] {
            rows.extend(permit_children(conn, table, permit_sync).await?.counted);
        }
    }
    for (table, key, col) in [
        ("work_order_delay_segments", "work_order_sync_id", "work_order_id"),
        ("wo_state_transition_log", "wo_sync_id", "wo_id"),
    ] {
        rows.extend(wo_children(conn, table, key, col, entity_sync_id).await?.counted);
    }
    Ok((!rows.is_empty()).then(|| Group {
        parent_sync_id: entity_sync_id.to_string(),
        previous_status: None,
        judged: true,
        rows,
        closeout: None,
    }))
}
