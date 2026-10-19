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
//! - a work order entering `in_progress` passes the permit gate (`permit::wo_gate`) at the time it
//!   started, resumed or was reopened: a permit when it requires one (of the type a plant rule named,
//!   when a rule flagged it), every permit live at that time active, not suspended, not expired, not
//!   expiring before the work order's planned end, with its isolations ready;
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
    static GROUP_PARENT: (String, Option<String>);
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
    None
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

/// Rows applied in the same transaction as a received permit or work order (`apply_group`).
#[derive(Debug, Clone)]
pub(crate) struct Group {
    parent_sync_id: String,
    previous_status: Option<String>,
    rows: Vec<GroupRow>,
}

/// The rows of one table naming a parent: the local ones with the counted waiting versions laid over
/// them, the counted waiting rows themselves, and why a waiting row cannot be counted yet.
struct Children {
    rows: Vec<(String, Payload)>,
    counted: Vec<GroupRow>,
    blocked: Option<String>,
}

/// Rows of `table` naming the parent `parent_sync_id` (a row of `parent_table`; local column
/// `parent_col`, payload key `parent_key`). A waiting inbox row counts when it is pending, or deferred
/// only for a missing parent (it arrived before its parent); when it passes the checks before an apply
/// (`sync::materialize::inbox_row_admissible`: not stale, no unsent local change, no code clash);
/// when everything it refers to other than this parent is applied here (otherwise: `blocked`); and,
/// for an isolation or a handover log, when it keeps the isolation or hand-over rules (otherwise it
/// is left to be refused on its own). A row moved to another parent by its waiting version no longer
/// counts.
async fn children(
    conn: &impl ConnectionTrait,
    table: &'static str,
    parent_table: &str,
    parent_col: &str,
    parent_key: &str,
    parent_sync_id: &str,
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

async fn permit_children(
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

fn payloads(c: &Children) -> Vec<&Payload> {
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
    field(iso, "removal_verified_at").is_some_and(|t| instant(t).is_none_or(|t| t < at))
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
            if let Some((c, m)) = isolations_ready(&code, needs, three_person, &payloads(&isolations), at) {
                return conflict(c, m);
            }
        }
        "closed" | "handed_back" => {
            let pending = payloads(&isolations)
                .iter()
                .filter(|i| field(i, "removal_verified_at").is_none())
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
        rows,
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
                (group.parent_sync_id.clone(), group.previous_status.clone()),
                Box::pin(crate::sync::materialize::apply_inbox_row_in(conn, &item)),
            )
            .await?;
        match outcome {
            ApplyOutcome::Applied | ApplyOutcome::Unchanged => {}
            // A child that waits (a row it needs has not arrived) makes the whole group wait: rolled
            // back, retried with the next rows applied.
            ApplyOutcome::Deferred { code, message } => {
                return Ok(Some(ApplyOutcome::Deferred {
                    code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                    message: format!(
                        "{} {} received with it waits ({code}: {message})",
                        row.entity_type, item.entity_sync_id
                    ),
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

/// Re-check of an isolation received by synchronisation (see the module documentation).
pub(crate) async fn inbound_isolation_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "permit_isolations", entity_sync_id).await?;
    let iso = overlay(local.as_ref(), received);
    if local
        .as_ref()
        .is_some_and(|l| isolation_actions(l) == isolation_actions(&iso))
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
            .try_with(|(parent, previous)| (parent == permit_sync).then(|| previous.clone()))
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
    // rules, permits and work orders name the type by it. Compared exactly, as stored.
    if let (Some(held), Some(Value::String(sent))) = (
        local.as_ref().and_then(|l| l.get("code")).and_then(Value::as_str),
        received.get("code"),
    ) {
        if held != sent {
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
    let record = |p: &Payload| [s.by, s.at, s.why].map(|k| field(p, k).map(str::to_string));
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
/// permit, its isolations and suspensions may go only while the permit is a draft (the LOTO history
/// of any other permit is kept); a permit type and a qualification requirement never (they are
/// edited or deactivated, which keeps their history).
pub(crate) async fn inbound_delete_check(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<InboundVerdict>> {
    if entity_type == "qualification_requirements" || entity_type == "permit_types" {
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
        "work_permits" => "SELECT status, code FROM work_permits WHERE entity_sync_id = ?",
        "permit_isolations" => {
            "SELECT p.status, p.code FROM permit_isolations c JOIN work_permits p ON p.id = c.permit_id \
             WHERE c.entity_sync_id = ?"
        }
        "permit_suspensions" => {
            "SELECT p.status, p.code FROM permit_suspensions c JOIN work_permits p ON p.id = c.permit_id \
             WHERE c.entity_sync_id = ?"
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
    let status: String = row.try_get("", "status").map_err(sync_err)?;
    let code: String = row.try_get("", "code").map_err(sync_err)?;
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
async fn status_code(conn: &impl ConnectionTrait, status_sync_id: &str) -> AppResult<Option<String>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT code FROM work_order_statuses WHERE entity_sync_id = ?",
        [status_sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<String>("", "code").map_err(sync_err))
    .transpose()
}

/// The received work order, when it enters `in_progress` here (otherwise `None`), with its local copy.
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
    if to != "in_progress" {
        return Ok(Ok(None));
    }
    if let Some(from_sync) = local.as_ref().and_then(|l| field(l, "status_sync_id")) {
        if status_code(conn, from_sync).await?.as_deref() == Some("in_progress") {
            return Ok(Ok(None));
        }
    }
    Ok(Ok(Some(wo)))
}

async fn wo_children(
    conn: &impl ConnectionTrait,
    table: &'static str,
    key: &str,
    col: &str,
    wo_sync: &str,
) -> AppResult<Children> {
    children(conn, table, "work_orders", col, key, wo_sync).await
}

/// Re-check of the permit flag a plant rule set on a work order (`wo::workflow::actions::
/// permit_requirement`): a received work order whose rule-set flag is cleared (`requires_permit` 0,
/// `permit_rule_code` set) says who cleared it, when and why (10 characters or more), before its
/// execution started, and that person holds the permit-issue right here. A person not applied here
/// yet makes it wait. A state already held here (same clearing) passes. A different complete clearing
/// received over one recorded here is judged the same way; a removed or partial one is refused.
pub(crate) async fn inbound_permit_rule_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "work_orders", entity_sync_id).await?;
    let wo = overlay(local.as_ref(), received);
    let code = field(&wo, "code").unwrap_or(entity_sync_id).to_string();
    // The rule trace is written once, at conversion, and never changes (`di::conversion`): a received
    // row that blanks or changes it is refused, and the local trace is the one judged.
    if let Some(l) = local.as_ref().filter(|l| field(l, "permit_rule_code").is_some()) {
        let trace = |p: &Payload| {
            (
                field(p, "permit_rule_code").map(str::to_string),
                field(p, "permit_rule_permit_type").map(str::to_string),
            )
        };
        if trace(l) != trace(&wo) {
            return conflict(
                "WO_PERMIT_RULE_CHANGED",
                format!("work order {code}: the plant rule that requires its permit cannot be removed or changed"),
            );
        }
    }
    if field(&wo, "permit_rule_code").is_none() {
        return Ok(None);
    }
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
    if clearing_changed
        && local
            .as_ref()
            .is_some_and(|l| field(l, "permit_rule_cleared_at").is_some())
        && clearing(&wo).iter().any(Option::is_none)
    {
        return conflict(
            "WO_PERMIT_RULE_CLEAR_INVALID",
            format!("work order {code}: the clearing of its rule permit flag recorded here cannot be removed or partly rewritten"),
        );
    }
    if !clearing_changed {
        // Still flagged: the flag is in force (the start gate judges it).
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
                        "work order {code}: the permit required by a plant rule was cleared after its execution \
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
            format!("work order {code}: the permit required by a plant rule was cleared {why}"),
        )
    };
    let reason_ok = field(&wo, "permit_rule_clear_reason")
        .is_some_and(|r| r.chars().count() >= crate::wo::workflow::actions::permit_requirement::MIN_CLEAR_REASON_CHARS);
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
    let Some(clearer) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
            [clearer_sync.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id").map_err(sync_err))
        .transpose()?
    else {
        return wait(format!(
            "work order {code}: the person who cleared its permit flag is not applied yet"
        ));
    };
    if !crate::settings::site::holds_tenant_right(conn, clearer, crate::rbac::permissions::PTW_ISSUE).await? {
        // The right is held on the PC that cleared it before the clearing, so its rows come before
        // this one: while some are still waiting here, wait; otherwise refuse (never applied without
        // the right).
        if rights_may_still_arrive(conn, clearer_sync, crate::rbac::permissions::PTW_ISSUE).await? {
            return wait(format!(
                "work order {code}: the rights of the person who cleared its permit flag are still arriving"
            ));
        }
        return conflict(
            "WO_PERMIT_RULE_CLEAR_UNAUTHORISED",
            format!("work order {code}: the permit required by a plant rule was cleared by someone without the permit-issue right here"),
        );
    }
    Ok(None)
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
    moments.extend(
        payloads(&transitions)
            .iter()
            .filter(|t| field(t, "to_status") == Some("in_progress"))
            .filter_map(|t| time(t, "acted_at")),
    );
    for r in conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.acted_at AS at FROM wo_state_transition_log t JOIN work_orders w ON w.id = t.wo_id \
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
            moments.push(acted);
        }
    }
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
            // The work would outlive its protection (`permit::wo_gate`): a permit expiring before the
            // planned end refuses the start (the planned end is fixed once the order is past ready).
            // An unreadable planned end is refused, as locally.
            if let (Some(end), Some(expires)) = (field(&wo, "planned_end"), instant(expiry)) {
                if instant(end).is_none_or(|e| expires < e) {
                    return conflict(
                        crate::permit::wo_gate::CODE_EXPIRES_BEFORE_PLANNED_END,
                        format!(
                            "work order {code} started at {at} while permit {pcode} expires ({expiry}) before its planned end ({end})"
                        ),
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
        rows,
    }))
}
