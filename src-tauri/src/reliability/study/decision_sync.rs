//! A reliability study decision received by synchronisation, approved or applied, is checked
//! again on arrival (audit finding AU-76b, CMMS Advisor answers 2026-10-26), after the four-eyes
//! check of `decisions::inbound_self_approval_check`:
//! - its status never goes backwards here (applied → approved, approved → proposed, decided →
//!   rejected or the reverse), and a refusal mark held here is never cleared
//!   ([`DECISION_STATUS_REGRESSION`]);
//! - its content never changes: a decision held here must arrive with the same content (what was
//!   proposed, by whom, the designated approver, and once decided who decided it, when and with
//!   which note); a decision new here is checked against its approval seal
//!   ([`DECISION_CONTENT_CHANGED`]); approved without a seal by an app version that seals, it is
//!   refused ([`DECISION_APPROVAL_UNSEALED`]); sent by an app version older than the seal (no seal
//!   key at all), it waits, shown in the sync centre as waiting for the sending PC's update
//!   ([`crate::sync::materialize::DEFER_SENDER_OUTDATED`]);
//! - the person who approved it is the designated approver when one was named
//!   ([`DECISION_NOT_DESIGNATED_APPROVER`]);
//! - the person who approved it holds `ram.manage` plant-wide here, now; it waits while that
//!   person or rights giving it are still arriving, and is a conflict otherwise
//!   ([`DECISION_APPROVER_UNAUTHORISED`]); an approval already accepted here is not judged again
//!   when the decision moves on to applied;
//! - the person who applied it holds the rights of applying here, now (`ram.manage`, plus `pm.edit`
//!   for a PM interval or `ot.create` for a work order), with the same waiting
//!   ([`DECISION_APPLIER_UNAUTHORISED`]).
//!
//! The seal (`approval_seal`, migration 264) proves that the content did not change after the
//! approval; it does not prove who approved: any PC can compute it (docs/KNOWN_LIMITS.md).
//!
//! These conflicts are resolved only by "keep this PC's decision" or "escalate"
//! ([`keep_this_pc_decision`], `sync::queries`): the decision is then marked refused
//! (`sync_refused_at` / `sync_refused_reason`), sent with the mark, and on every PC it can no longer
//! be applied, nor its PM version published. Nothing is deleted. A rights conflict closes by
//! itself, audited, when the rights arrive ([`retry_decision_conflicts`]).

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};
use crate::reliability::study::domain::{
    DECISION_PM_INTERVAL, STATUS_APPLIED, STATUS_APPROVED, STATUS_PROPOSED, STATUS_REJECTED,
};
use crate::sync::materialize::ApplyOutcome;

type Payload = Map<String, Value>;

const ENTITY: &str = "reliability_study_decisions";

/// The received decision went backwards here, or would clear a refusal mark held here.
pub const DECISION_STATUS_REGRESSION: &str = "DECISION_STATUS_REGRESSION";
/// Its content differs from the decision held here, or from its approval seal.
pub const DECISION_CONTENT_CHANGED: &str = "DECISION_CONTENT_CHANGED";
/// Approved or applied with an empty seal by an app version that seals.
pub const DECISION_APPROVAL_UNSEALED: &str = "DECISION_APPROVAL_UNSEALED";
/// Approved by someone else than the designated approver.
pub const DECISION_NOT_DESIGNATED_APPROVER: &str = "DECISION_NOT_DESIGNATED_APPROVER";
/// Approved by someone who does not hold `ram.manage` plant-wide here.
pub const DECISION_APPROVER_UNAUTHORISED: &str = "DECISION_APPROVER_UNAUTHORISED";
/// Applied by someone who does not hold the rights of applying it here.
pub const DECISION_APPLIER_UNAUTHORISED: &str = "DECISION_APPLIER_UNAUTHORISED";

/// The conflicts of a received decision (`sync::queries` accepts only "keep this PC's decision" and
/// "escalate" on them).
pub const RELIABILITY_DECISION_CONFLICTS: &[&str] = &[
    DECISION_STATUS_REGRESSION,
    DECISION_CONTENT_CHANGED,
    DECISION_APPROVAL_UNSEALED,
    DECISION_NOT_DESIGNATED_APPROVER,
    DECISION_APPROVER_UNAUTHORISED,
    DECISION_APPLIER_UNAUTHORISED,
];

/// The rights conflicts judged again at each synchronisation ([`retry_decision_conflicts`]).
const RIGHTS_CONFLICTS: &[&str] = &[DECISION_APPROVER_UNAUTHORISED, DECISION_APPLIER_UNAUTHORISED];

/// Audit action of a rights conflict closed by the rights that arrived.
pub const CONFLICT_CLOSED_EVENT: &str = "reliability.decision_conflict.closed_by_rights";
/// Audit action of a decision marked refused here ("keep this PC's decision").
pub const REFUSED_EVENT: &str = "reliability.decision.refused_at_sync";

// ── Approval seal ──────────────────────────────────────────────────────────────────────────────

/// Version prefix of the seal.
pub const SEAL_PREFIX: &str = "v1:";

/// What was proposed, fixed when the decision is proposed (payload keys: references as sync ids).
const PROPOSAL_KEYS: &[&str] = &[
    "study_sync_id",
    "title",
    "action_kind",
    "equipment_sync_id",
    "pm_plan_sync_id",
    "proposed_interval_hours",
    "proposed_trigger_definition_json",
    "wo_type_code",
    "justification",
    "evidence_json",
    "baseline_mtbf_hours",
    "baseline_failures",
    "baseline_exposure_hours",
    "fmeca_item_sync_id",
    "rcm_decision_sync_id",
    "proposed_by_sync_id",
    "proposed_at",
    "approver_sync_id",
];

/// What the decision adds, fixed when it is decided.
const DECISION_KEYS: &[&str] = &["decided_by_sync_id", "decided_at", "decision_note"];

/// What applying adds, fixed when it is applied.
const APPLY_KEYS: &[&str] = &["applied_by_sync_id", "applied_at"];

/// Text columns holding JSON: compared and sealed as their parsed value, so that a re-encoding
/// (key order, the meter reference rewritten by sync) never changes the seal.
const JSON_TEXT_KEYS: &[&str] = &["proposed_trigger_definition_json", "evidence_json"];

/// Significant digits a non-integral number is sealed and compared with.
const SEALED_DIGITS: usize = 9;

/// A number in one form whatever the PC that reads it. An integer stays an integer; a real number
/// (a REAL column, or a JSON number written with a fraction or an exponent, as sync writes every
/// real) is rounded to [`SEALED_DIGITS`] significant digits and kept as text. Sync carries reals as
/// JSON text, and the JSON parser can land one unit in the last place away from the value the
/// sending PC sealed (review B1), so the last digits never take part in the seal or the comparison.
fn canonical_number(n: &serde_json::Number) -> Value {
    if !n.is_f64() {
        return Value::Number(n.clone());
    }
    match n.as_f64() {
        Some(f) if f.is_finite() => Value::String(format!("{f:.prec$e}", prec = SEALED_DIGITS - 1)),
        _ => Value::Null,
    }
}

/// A value in its canonical form: numbers in one form, object keys sorted, nulls dropped from
/// objects.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Number(n) => canonical_number(n),
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), canonical(v)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        other => other.clone(),
    }
}

/// The canonical value of a content key of a payload (`None`: empty).
fn content_value(payload: &Payload, key: &str) -> Option<Value> {
    let v = payload.get(key)?;
    if v.is_null() {
        return None;
    }
    if JSON_TEXT_KEYS.contains(&key) {
        if let Some(text) = v.as_str() {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                return Some(canonical(&parsed));
            }
        }
    }
    Some(canonical(v))
}

/// The approval seal of a decision in its synchronised form: `v1:` + SHA-256 (hex) of the
/// canonical JSON of its proposal and decision content. The one function used at approval,
/// at the backfill of migration 264 and on arrival.
pub fn approval_seal(payload: &Payload) -> String {
    let content: BTreeMap<&str, Value> = PROPOSAL_KEYS
        .iter()
        .chain(DECISION_KEYS)
        .filter_map(|k| content_value(payload, k).map(|v| (*k, v)))
        .collect();
    let text = serde_json::to_string(&content).unwrap_or_default();
    format!("{SEAL_PREFIX}{}", hex::encode(Sha256::digest(text.as_bytes())))
}

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("reliability decision sync check: {e}"))
}

/// The content of a decision held here, in its synchronised form (references as sync ids, the
/// meter of its trigger as the meter's sync id), read without the sync layout so that migration
/// 264 can use it. `None`: no such decision.
pub(crate) async fn local_seal_content(conn: &impl ConnectionTrait, decision_id: i64) -> AppResult<Option<Payload>> {
    let Some(r) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT d.title, d.action_kind, CAST(d.proposed_interval_hours AS REAL) AS proposed_interval_hours, \
                    d.proposed_trigger_definition_json, d.wo_type_code, d.justification, d.evidence_json, \
                    CAST(d.baseline_mtbf_hours AS REAL) AS baseline_mtbf_hours, \
                    CAST(d.baseline_failures AS INTEGER) AS baseline_failures, \
                    CAST(d.baseline_exposure_hours AS REAL) AS baseline_exposure_hours, \
                    d.proposed_at, d.decided_at, d.decision_note, \
                    s.entity_sync_id AS study_sync_id, e.entity_sync_id AS equipment_sync_id, \
                    p.entity_sync_id AS pm_plan_sync_id, fi.entity_sync_id AS fmeca_item_sync_id, \
                    rd.entity_sync_id AS rcm_decision_sync_id, pb.entity_sync_id AS proposed_by_sync_id, \
                    ap.entity_sync_id AS approver_sync_id, db.entity_sync_id AS decided_by_sync_id \
               FROM reliability_study_decisions d \
               LEFT JOIN reliability_studies s ON s.id = d.study_id \
               LEFT JOIN equipment e ON e.id = d.equipment_id \
               LEFT JOIN pm_plans p ON p.id = d.pm_plan_id \
               LEFT JOIN fmeca_items fi ON fi.id = d.fmeca_item_id \
               LEFT JOIN rcm_decisions rd ON rd.id = d.rcm_decision_id \
               LEFT JOIN user_accounts pb ON pb.id = d.proposed_by_id \
               LEFT JOIN user_accounts ap ON ap.id = d.approver_id \
               LEFT JOIN user_accounts db ON db.id = d.decided_by_id \
              WHERE d.id = ?",
            [decision_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let mut content = Payload::new();
    for key in [
        "title",
        "action_kind",
        "wo_type_code",
        "justification",
        "evidence_json",
        "proposed_at",
        "decided_at",
        "decision_note",
        "study_sync_id",
        "equipment_sync_id",
        "pm_plan_sync_id",
        "fmeca_item_sync_id",
        "rcm_decision_sync_id",
        "proposed_by_sync_id",
        "approver_sync_id",
        "decided_by_sync_id",
    ] {
        let v: Option<String> = r.try_get("", key).map_err(sync_err)?;
        content.insert(key.into(), v.map_or(Value::Null, Value::String));
    }
    // An INTEGER column travels as a JSON integer, a REAL column as a JSON real.
    let failures: Option<i64> = r.try_get("", "baseline_failures").map_err(sync_err)?;
    content.insert("baseline_failures".into(), failures.map_or(Value::Null, Value::from));
    for key in [
        "proposed_interval_hours",
        "baseline_mtbf_hours",
        "baseline_exposure_hours",
    ] {
        let v: Option<f64> = r.try_get("", key).map_err(sync_err)?;
        content.insert(
            key.into(),
            v.and_then(serde_json::Number::from_f64)
                .map_or(Value::Null, Value::Number),
        );
    }
    // The meter of the trigger travels as its sync id (`sync::row_spec`, json_ref
    // `$.asset_meter_id`): the seal is computed on that form.
    let trigger: Option<String> = r.try_get("", "proposed_trigger_definition_json").map_err(sync_err)?;
    let trigger = match trigger {
        Some(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(mut t)) => {
                if let Some(meter) = t.get("asset_meter_id").and_then(Value::as_i64) {
                    let sync = conn
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT entity_sync_id FROM equipment_meters WHERE id = ?",
                            [meter.into()],
                        ))
                        .await?
                        .and_then(|m| m.try_get::<Option<String>>("", "entity_sync_id").ok().flatten());
                    t.insert("asset_meter_id".into(), sync.map_or(Value::Null, Value::String));
                }
                Value::String(Value::Object(t).to_string())
            }
            _ => Value::String(text),
        },
        None => Value::Null,
    };
    content.insert("proposed_trigger_definition_json".into(), trigger);
    Ok(Some(content))
}

/// The seal of a decision held here, from its own content ([`approval_seal`]).
pub(crate) async fn local_approval_seal(conn: &impl ConnectionTrait, decision_id: i64) -> AppResult<Option<String>> {
    Ok(local_seal_content(conn, decision_id)
        .await?
        .map(|content| approval_seal(&content)))
}

// ── Arrival check ──────────────────────────────────────────────────────────────────────────────

fn text<'a>(p: &'a Payload, key: &str) -> Option<&'a str> {
    crate::permit::sync_check::field(p, key)
}

fn decided(status: &str) -> bool {
    status == STATUS_APPROVED || status == STATUS_APPLIED
}

fn conflict(code: &str, message: String) -> AppResult<Option<ApplyOutcome>> {
    Ok(Some(ApplyOutcome::Conflict {
        code: code.into(),
        message,
    }))
}

/// Whether the status `to` received over `from` held here goes backwards.
fn goes_back(from: &str, to: &str) -> bool {
    let rank = |s: &str| match s {
        STATUS_PROPOSED => Some(0),
        STATUS_APPROVED | STATUS_REJECTED => Some(1),
        STATUS_APPLIED => Some(2),
        _ => None,
    };
    let crossed = (decided(from) && to == STATUS_REJECTED) || (from == STATUS_REJECTED && to != STATUS_REJECTED);
    crossed || matches!((rank(from), rank(to)), (Some(a), Some(b)) if b < a)
}

/// The content keys whose received value differs from the one held here (keys the payload leaves
/// out keep the held value and are not compared). The decision keys count once the held decision
/// is decided, the apply keys once it is applied.
fn changed_keys(held: &Payload, received: &Payload) -> Vec<&'static str> {
    let held_status = text(held, "status").unwrap_or_default();
    let received_status = text(received, "status").unwrap_or(held_status);
    let decided_or_rejected = |s: &str| decided(s) || s == STATUS_REJECTED;
    let mut keys: Vec<&'static str> = PROPOSAL_KEYS.to_vec();
    // Compared once both sides record the decision (a version from before it is judged by its
    // status: going back is refused on its own).
    if decided_or_rejected(held_status) && decided_or_rejected(received_status) {
        keys.extend_from_slice(DECISION_KEYS);
    }
    if held_status == STATUS_APPLIED && received_status == STATUS_APPLIED {
        keys.extend_from_slice(APPLY_KEYS);
    }
    keys.into_iter()
        .filter(|k| received.contains_key(*k) && content_value(received, k) != content_value(held, k))
        .collect()
}

/// Whether a decision carries a refusal mark: any value but null, an empty text included, as the
/// gates read the column (`sync_refused_at IS NOT NULL`).
fn marked(p: &Payload) -> bool {
    p.get("sync_refused_at").is_some_and(|v| !v.is_null())
}

/// The statuses a decision can have; anything else received is refused (review S1).
const STATUSES: [&str; 4] = [STATUS_PROPOSED, STATUS_APPROVED, STATUS_REJECTED, STATUS_APPLIED];

/// The local id of a user named by sync id (`None`: not here).
async fn user_here(conn: &impl ConnectionTrait, user_sync: &str) -> AppResult<Option<i64>> {
    crate::permit::sync_check::local_id(conn, "user_accounts", user_sync).await
}

/// Whether `user_sync` holds every right of `rights` here, now: `Ok(true)`; else a wait while the
/// person or rights giving them are still arriving, or `Ok(false)`.
async fn holds_rights(
    conn: &impl ConnectionTrait,
    user_sync: &str,
    rights: &[&str],
    who: &str,
) -> AppResult<Result<bool, ApplyOutcome>> {
    let Some(user) = user_here(conn, user_sync).await? else {
        return Ok(Err(ApplyOutcome::Deferred {
            code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
            message: format!("{who} {user_sync} is not applied yet"),
            waits_for: None,
        }));
    };
    for right in rights {
        if crate::settings::site::holds_tenant_right(conn, user, right).await? {
            continue;
        }
        if crate::permit::sync_check::rights_may_still_arrive(conn, user_sync, right).await? {
            return Ok(Err(ApplyOutcome::Deferred {
                code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                message: format!("the rights of {who} {user_sync} are still arriving"),
                waits_for: None,
            }));
        }
        return Ok(Ok(false));
    }
    Ok(Ok(true))
}

/// See the module documentation. Runs before `decisions::inbound_self_approval_check` for the
/// refusal marks, after it for the rest (`sync::apply_hooks::before_upsert_checked`).
pub(crate) async fn inbound_refusal_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<RefusalVerdict> {
    let held = crate::permit::sync_check::local_wire(conn, ENTITY, entity_sync_id).await?;
    let title = text(received, "title").unwrap_or_default().to_string();
    match (held.as_ref(), marked(received)) {
        // Refused on another PC and new here: kept as history, never applied or published, so not
        // judged.
        (None, true) => Ok(RefusalVerdict::Accept),
        // Refused there, held here: the marks are taken when the content is the one held here (the
        // status and links held here are kept, `keep_held_progress`).
        (Some(held), true) => {
            let changed = changed_keys(held, received);
            if changed.is_empty() {
                Ok(RefusalVerdict::Accept)
            } else {
                Ok(RefusalVerdict::Outcome(ApplyOutcome::Conflict {
                    code: DECISION_CONTENT_CHANGED.into(),
                    message: format!(
                        "decision '{title}' refused on another PC arrives with a content that differs from the one \
                         held here ({})",
                        changed.join(", ")
                    ),
                }))
            }
        }
        // A refusal mark held here is never cleared by synchronisation.
        (Some(held), false) if marked(held) => Ok(RefusalVerdict::Outcome(ApplyOutcome::Conflict {
            code: DECISION_STATUS_REGRESSION.into(),
            message: format!(
                "decision '{title}' was refused here at synchronisation; the received version would clear that \
                 refusal"
            ),
        })),
        _ => Ok(RefusalVerdict::Judge),
    }
}

/// What [`inbound_refusal_check`] decided.
#[derive(Debug)]
pub(crate) enum RefusalVerdict {
    /// A refused decision (history, or marks on the decision held here): applied without judging.
    Accept,
    /// Refused or waiting.
    Outcome(ApplyOutcome),
    /// Not refused anywhere: judged by the four-eyes and approval checks.
    Judge,
}

/// See the module documentation (after the four-eyes check).
pub(crate) async fn inbound_approval_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    let held = crate::permit::sync_check::local_wire(conn, ENTITY, entity_sync_id).await?;
    let row = crate::permit::sync_check::overlay(held.as_ref(), received);
    let title = text(&row, "title").unwrap_or_default().to_string();
    let status = text(&row, "status").unwrap_or_default().to_string();
    if let Some(held) = held.as_ref() {
        let from = text(held, "status").unwrap_or_default();
        if goes_back(from, &status) {
            return conflict(
                DECISION_STATUS_REGRESSION,
                format!("decision '{title}' is {from} here; the received version moves it back to {status}"),
            );
        }
        let changed = changed_keys(held, received);
        if !changed.is_empty() {
            return conflict(
                DECISION_CONTENT_CHANGED,
                format!(
                    "decision '{title}' arrives with a content that differs from the one held here ({}); a decision \
                     never changes after it is proposed",
                    changed.join(", ")
                ),
            );
        }
    }
    if !decided(&status) {
        return Ok(None);
    }
    match received.get("approval_seal") {
        // Sent by an app version older than the seal.
        None => {
            if held.is_none() {
                return Ok(Some(ApplyOutcome::Deferred {
                    code: crate::sync::materialize::DEFER_SENDER_OUTDATED.into(),
                    message: format!(
                        "decision '{title}' was approved on a PC whose app version does not seal approvals: waiting \
                         for that PC to be updated"
                    ),
                    waits_for: None,
                }));
            }
        }
        Some(Value::String(seal)) if !seal.trim().is_empty() => {
            if *seal != approval_seal(&row) {
                return conflict(
                    DECISION_CONTENT_CHANGED,
                    format!("decision '{title}' does not match its approval seal: its content changed after approval"),
                );
            }
        }
        Some(_) => {
            return conflict(
                DECISION_APPROVAL_UNSEALED,
                format!("decision '{title}' is {status} without an approval seal"),
            );
        }
    }
    let decider = text(&row, "decided_by_sync_id").unwrap_or_default().to_string();
    if let Some(designated) = text(&row, "approver_sync_id") {
        if designated != decider {
            return conflict(
                DECISION_NOT_DESIGNATED_APPROVER,
                format!("decision '{title}' was approved by someone else than the approver designated for it"),
            );
        }
    }
    let held_status = held.as_ref().and_then(|h| text(h, "status")).unwrap_or_default();
    // An approval accepted here already (its content and decider are the ones held, checked
    // above) is not judged again when the decision moves on.
    if !decided(held_status) {
        match holds_rights(conn, &decider, &[crate::rbac::permissions::RAM_MANAGE], "approver").await? {
            Err(waiting) => return Ok(Some(waiting)),
            Ok(false) => {
                return conflict(
                    DECISION_APPROVER_UNAUTHORISED,
                    format!(
                        "decision '{title}' was approved by someone who does not hold the right to approve reliability \
                         decisions (ram.manage, plant-wide) here"
                    ),
                )
            }
            Ok(true) => {}
        }
    }
    if status == STATUS_APPLIED && held_status != STATUS_APPLIED {
        let Some(applier) = text(&row, "applied_by_sync_id").map(str::to_string) else {
            return conflict(
                DECISION_APPLIER_UNAUTHORISED,
                format!("decision '{title}' is applied but names nobody who applied it"),
            );
        };
        let second = if text(&row, "action_kind") == Some(DECISION_PM_INTERVAL) {
            crate::rbac::permissions::PM_EDIT
        } else {
            crate::rbac::permissions::OT_CREATE
        };
        match holds_rights(
            conn,
            &applier,
            &[crate::rbac::permissions::RAM_MANAGE, second],
            "applier",
        )
        .await?
        {
            Err(waiting) => return Ok(Some(waiting)),
            Ok(false) => {
                return conflict(
                    DECISION_APPLIER_UNAUTHORISED,
                    format!(
                        "decision '{title}' was applied by someone who does not hold the rights to apply it here \
                         (ram.manage and {second}, plant-wide)"
                    ),
                )
            }
            Ok(true) => {}
        }
    }
    Ok(None)
}

/// A refused decision received over the decision held here: the status, apply data and links held
/// here are kept (only the marks are taken), and a refusal held here keeps its marks. Run before
/// the write (`sync::apply_hooks::before_write`).
pub(crate) async fn keep_held_progress(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    payload: &mut Payload,
) -> AppResult<()> {
    let Some(held) = crate::permit::sync_check::local_wire(conn, ENTITY, entity_sync_id).await? else {
        return Ok(());
    };
    if !marked(payload) && !marked(&held) {
        return Ok(());
    }
    let mut keys = vec![
        "status",
        "applied_by_sync_id",
        "applied_at",
        "linked_work_order_sync_id",
        "linked_pm_plan_version_sync_id",
        "linked_pm_plan_sync_id",
        "approval_seal",
    ];
    if marked(&held) {
        keys.extend(["sync_refused_at", "sync_refused_reason"]);
    }
    for key in keys {
        match held.get(key) {
            Some(v) => {
                payload.insert(key.into(), v.clone());
            }
            None => {
                payload.remove(key);
            }
        }
    }
    Ok(())
}

/// Whether the decision held here (`sync_id`) carries no refusal mark (`None`: not held here).
pub(crate) async fn held_unrefused(conn: &impl ConnectionTrait, sync_id: &str) -> AppResult<Option<bool>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT sync_refused_at IS NULL AS open FROM reliability_study_decisions WHERE entity_sync_id = ?",
        [sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<bool>("", "open"))
    .transpose()
    .map_err(sync_err)
}

/// When the decision `sync_id` is refused here and a work order created from it is held here, or a
/// PM version published from it is in force here: the work order is kept, the version keeps
/// running, and the planners and the decision's proposer are told (CMMS Advisor Q6, S4 option A).
pub(crate) async fn after_refusal_received(conn: &impl ConnectionTrait, sync_id: &str) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT d.id, d.linked_work_order_id, \
                    (SELECT v.id FROM pm_plan_versions v \
                      WHERE v.id = d.linked_pm_plan_version_id AND v.status = 'published') AS published \
               FROM reliability_study_decisions d \
              WHERE d.entity_sync_id = ? AND d.sync_refused_at IS NOT NULL",
            [sync_id.into()],
        ))
        .await?;
    if let Some(r) = row {
        let id: i64 = r.try_get("", "id").map_err(sync_err)?;
        let work_order: Option<i64> = r.try_get("", "linked_work_order_id").map_err(sync_err)?;
        let published: Option<i64> = r.try_get("", "published").map_err(sync_err)?;
        if work_order.is_some() || published.is_some() {
            crate::settings::received_notice::announce_refused_decision(conn, id, work_order, published).await;
        }
    }
    Ok(())
}

/// After a received decision was written (`sync::apply_hooks::after_upsert`):
/// - approved or applied, not refused, and without a seal here (an approval from a PC that does not
///   seal yet, accepted over the proposal held here by comparison): sealed now from the content
///   judged here, as the backfill of migration 264 does, so this PC never sends it unsealed
///   (review S2); nothing is sent for it;
/// - refused, new here or newly refused here: the work order or PM version it gave is told of
///   ([`after_refusal_received`], review S3).
pub(crate) async fn after_decision_written(
    conn: &impl ConnectionTrait,
    sync_id: &str,
    inserted: bool,
    was_unrefused: Option<bool>,
) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, status, approval_seal IS NULL AS unsealed, sync_refused_at IS NOT NULL AS refused \
               FROM reliability_study_decisions WHERE entity_sync_id = ?",
            [sync_id.into()],
        ))
        .await?;
    let Some(r) = row else {
        return Ok(());
    };
    let id: i64 = r.try_get("", "id").map_err(sync_err)?;
    let status: String = r.try_get("", "status").map_err(sync_err)?;
    let unsealed: bool = r.try_get("", "unsealed").map_err(sync_err)?;
    let refused: bool = r.try_get("", "refused").map_err(sync_err)?;
    if decided(&status) && unsealed && !refused {
        if let Some(seal) = local_approval_seal(conn, id).await? {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE reliability_study_decisions SET approval_seal = ? WHERE id = ? AND approval_seal IS NULL",
                [seal.into(), id.into()],
            ))
            .await?;
        }
    }
    // An unsealed version of this decision waiting here for its sender's update is superseded by the
    // version just written (the updated sender re-sent it sealed, migration 264): it is closed as
    // an older version, never retried into a stale-version conflict (review S7). Only a version
    // older than the newest other received version of it: a newer unsealed state is never closed
    // as older (review R2-N2).
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE sync_inbox SET apply_status = 'applied', rejection_code = ?, \
                rejection_message = 'superseded by a later version of the decision applied here', \
                waits_for_entity_type = NULL, waits_for_sync_id = NULL, \
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE entity_type = ? AND entity_sync_id = ? AND apply_status = 'deferred' AND rejection_code = ? \
            AND row_version < (SELECT MAX(o.row_version) FROM sync_inbox o \
                                WHERE o.entity_type = sync_inbox.entity_type \
                                  AND o.entity_sync_id = sync_inbox.entity_sync_id \
                                  AND NOT (o.apply_status = 'deferred' AND COALESCE(o.rejection_code, '') = ?))",
        [
            crate::sync::materialize::SKIPPED_OLDER_VERSION.into(),
            ENTITY.into(),
            sync_id.into(),
            crate::sync::materialize::DEFER_SENDER_OUTDATED.into(),
            crate::sync::materialize::DEFER_SENDER_OUTDATED.into(),
        ],
    ))
    .await?;
    if refused && (inserted || was_unrefused == Some(true)) {
        after_refusal_received(conn, sync_id).await?;
    }
    Ok(())
}

// ── Re-send of the decisions sealed by the upgrade (migration 264) ────────────────────────────

/// `system_config` key (local to this PC): the decisions sealed by migration 264, to be sent once
/// more, sealed, after the first complete pull that follows the upgrade (JSON array of sync ids).
pub const SEAL_RESEND_KEY: &str = "reliability.decisions.seal_resend_pending";

/// After a complete pull (`sync::transport::worker`, all received rows applied, the outbox drained):
/// each decision listed by migration 264 is sent once more, sealed, by this PC only when the version
/// the server holds is still the one this PC sent (`sync_entity_state.source = 'outbox_ack'`: any
/// newer version received since moves it to `inbound_apply`), or when the server never received it
/// (no known server version), and no newer version of it waits in the inbox here. Another PC's newer
/// version (a decision applied elsewhere since this PC approved it) is never overwritten: that PC
/// re-sends, or the decision is already sealed everywhere (review R2-S8, R3-N1). A decision with a
/// change not sent yet at the upgrade is not listed: the migration journaled its sealed row on top
/// of that change (review R3-B1). The list is cleared once judged. Returns how many were staged
/// (drained into the outbox here).
pub(crate) async fn resend_sealed_after_complete_pull(conn: &impl ConnectionTrait) -> AppResult<i64> {
    let held: Option<String> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = ?",
            [SEAL_RESEND_KEY.into()],
        ))
        .await?
        .map(|r| r.try_get::<String>("", "value"))
        .transpose()
        .map_err(sync_err)?;
    let Some(held) = held else {
        return Ok(0);
    };
    let mut staged = 0;
    let listed = match serde_json::from_str::<Vec<String>>(&held) {
        Ok(listed) => listed,
        Err(err) => {
            // Never silent (review R3-N2): the decisions it named are not re-sent; their receivers
            // keep their visible wait.
            tracing::error!(target: "maintafox::sync", error = %err, list = %held, "unreadable list of decisions to re-send sealed: discarded");
            Vec::new()
        }
    };
    for sync_id in listed {
        // A newer version of it received here and not settled yet (pending, waiting or refused): the
        // version the server holds is not this PC's to re-send (review R3-N1).
        let row = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT (SELECT s.source FROM sync_entity_state s \
                          WHERE s.entity_type = ? AND s.entity_sync_id = d.entity_sync_id) AS source, \
                        d.approval_seal IS NOT NULL AS sealed \
                   FROM reliability_study_decisions d WHERE d.entity_sync_id = ? \
                    AND NOT EXISTS (SELECT 1 FROM sync_inbox i WHERE i.entity_type = ? \
                                     AND i.entity_sync_id = d.entity_sync_id \
                                     AND i.apply_status IN ('pending', 'deferred', 'conflict') \
                                     AND i.row_version > COALESCE((SELECT s2.synced_row_version \
                                           FROM sync_entity_state s2 WHERE s2.entity_type = i.entity_type \
                                            AND s2.entity_sync_id = i.entity_sync_id), 0))",
                [ENTITY.into(), sync_id.as_str().into(), ENTITY.into()],
            ))
            .await?;
        let Some(row) = row else {
            continue;
        };
        let source: Option<String> = row.try_get("", "source").map_err(sync_err)?;
        let sealed: bool = row.try_get("", "sealed").map_err(sync_err)?;
        let ours = matches!(source.as_deref(), None | Some("outbox_ack"));
        if sealed && ours {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, natural_key_json, operation, \
                   owner_user_sync_id) VALUES (?, ?, NULL, 'upsert', NULL)",
                [ENTITY.into(), sync_id.as_str().into()],
            ))
            .await?;
            staged += 1;
        }
    }
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM system_config WHERE key = ?",
        [SEAL_RESEND_KEY.into()],
    ))
    .await?;
    if staged > 0 {
        crate::sync::capture::drain_capture_journal(conn).await?;
    }
    Ok(staged)
}

/// After a received PM version was written (`sync::apply_hooks::after_upsert`): published, from a
/// decision already refused here (its refusal arrived before the version): the version keeps
/// running and its planners and the decision's proposer are told, as when the refusal arrives
/// after the version (review R2-N4).
pub(crate) async fn after_version_written(conn: &impl ConnectionTrait, version_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT d.id FROM pm_plan_versions v JOIN reliability_study_decisions d ON d.id = v.source_id \
              WHERE v.id = ? AND v.status = 'published' AND v.source_type = 'reliability_study_decision' \
                AND d.sync_refused_at IS NOT NULL",
            [version_id.into()],
        ))
        .await?;
    if let Some(r) = row {
        let decision: i64 = r.try_get("", "id").map_err(sync_err)?;
        crate::settings::received_notice::announce_refused_decision(conn, decision, None, Some(version_id)).await;
    }
    Ok(())
}

// ── A seal-only re-send crossing this PC's unsent change (review R3-S9) ──────────────────────

/// Audit action of a crossing settled without a person.
pub const SEAL_RESEND_SETTLED_EVENT: &str = "reliability.decision.seal_resend_settled";

/// Keys migration 264 added to a decision's payload: a re-send by an upgraded PC carries them, the
/// version it re-sends did not.
const MIGRATION_264_KEYS: [&str; 3] = ["approval_seal", "sync_refused_at", "sync_refused_reason"];

/// The version this PC based its unsent change on (the server version it knows), as it was
/// received or sent: its payload is kept in the inbox (received) or the outbox (sent from here).
async fn base_payload(conn: &impl ConnectionTrait, sync_id: &str) -> AppResult<Option<Payload>> {
    let Some(state) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT synced_row_version AS v, synced_payload_hash AS h FROM sync_entity_state \
              WHERE entity_type = ? AND entity_sync_id = ?",
            [ENTITY.into(), sync_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let version: i64 = state.try_get("", "v").map_err(sync_err)?;
    let hash: String = state.try_get("", "h").map_err(sync_err)?;
    let text = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT payload_json FROM ( \
                SELECT payload_json, 0 AS o, id FROM sync_inbox \
                 WHERE entity_type = ? AND entity_sync_id = ? AND row_version = ? AND payload_hash = ? \
                UNION ALL \
                SELECT payload_json, 1 AS o, id FROM sync_outbox \
                 WHERE entity_type = ? AND entity_sync_id = ? AND row_version = ? AND payload_hash = ?) \
             ORDER BY o, id DESC LIMIT 1",
            [
                ENTITY.into(),
                sync_id.into(),
                version.into(),
                hash.clone().into(),
                ENTITY.into(),
                sync_id.into(),
                version.into(),
                hash.into(),
            ],
        ))
        .await?
        .map(|r| r.try_get::<String>("", "payload_json"))
        .transpose()
        .map_err(sync_err)?;
    Ok(text.and_then(|t| serde_json::from_str::<Payload>(&t).ok()))
}

/// Whether `other` (a version the server holds, or one received) is only a seal-only re-send of the
/// version this PC based its unsent change on: every value the same as that base version (numbers
/// compared as the seal compares them), apart from the keys migration 264 added (the seal, and the
/// refusal marks when empty), and a seal equal to the one this PC computes for the decision. No
/// person changed anything in it: an upgraded PC sent its known version again, sealed.
pub(crate) async fn is_seal_only_resend(
    conn: &impl ConnectionTrait,
    sync_id: &str,
    other: &Payload,
) -> AppResult<bool> {
    let Some(id) = crate::permit::sync_check::local_id(conn, ENTITY, sync_id).await? else {
        return Ok(false);
    };
    let Some(seal) = other.get("approval_seal").and_then(Value::as_str) else {
        return Ok(false);
    };
    // The refusal marks are left out of the comparison below only when empty: a refusal reason
    // without its date is not empty either (review R4-N5).
    let reason_set = other.get("sync_refused_reason").is_some_and(|v| !v.is_null());
    if marked(other) || reason_set || local_approval_seal(conn, id).await?.as_deref() != Some(seal) {
        return Ok(false);
    }
    let Some(base) = base_payload(conn, sync_id).await? else {
        return Ok(false);
    };
    if base.contains_key("approval_seal") {
        // The base already carried a seal: not a re-send of a version from before the upgrade.
        return Ok(false);
    }
    let keys: std::collections::BTreeSet<String> = base.keys().chain(other.keys()).cloned().collect();
    let same = keys
        .iter()
        .filter(|k| !MIGRATION_264_KEYS.contains(&k.as_str()))
        .all(|k| base.get(k).map(canonical) == other.get(k).map(canonical));
    Ok(same)
}

/// Settles a seal-only re-send crossing this PC's unsent change: the server holds `version`
/// (`hash`) now, sealed, of the content this PC's change was based on. This PC's unsent items are
/// superseded and its current row (its change, sealed) is staged again on top of that version, so
/// it reaches the server as the next version; nothing is lost and no person is asked. Audited.
async fn settle_seal_resend(
    conn: &impl ConnectionTrait,
    sync_id: &str,
    version: i64,
    hash: &str,
    path: &str,
) -> AppResult<()> {
    crate::sync::queries::overwrite_synced_state(conn, ENTITY, sync_id, version, hash, "seal_resend_settled").await?;
    // A server conflict still open for one of this PC's items about to be superseded (the pull path
    // can meet one): closed with the settlement, so no stale item waits for a person (review R4-N3).
    // Its outbox item is superseded below and the change staged again, so nothing is lost; the
    // closure is noted on the conflict and counted in the audit. Only items in `conflict`: a
    // rejection stays for a person.
    let closed_conflicts = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sync_conflicts \
                SET status = 'resolved_remote', resolution_action = 'seal_resend_settled', resolution_note = ?, \
                    resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1, \
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
              WHERE status IN ('new', 'triaged', 'escalated') \
                AND linked_outbox_id IN (SELECT id FROM sync_outbox \
                     WHERE entity_type = ? AND entity_sync_id = ? AND status = 'conflict')",
            [
                format!(
                    "The decision was re-sent sealed by an upgraded PC (server version {version}) with the content this \
                     change was based on: this change is staged again on top of it, nobody asked."
                )
                .into(),
                ENTITY.into(),
                sync_id.into(),
            ],
        ))
        .await?
        .rows_affected();
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE sync_outbox SET status = 'superseded', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE entity_type = ? AND entity_sync_id = ? \
            AND status IN ('pending', 'in_flight', 'conflict', 'rejected', 'dead_letter')",
        [ENTITY.into(), sync_id.into()],
    ))
    .await?;
    crate::sync::capture::restage_current_state(conn, ENTITY, sync_id, &[]).await?;
    crate::audit::writer::write_audit_event_in(
        conn,
        crate::audit::writer::AuditEventInput {
            action_code: SEAL_RESEND_SETTLED_EVENT.to_string(),
            target_type: Some("reliability_study_decisions".to_string()),
            target_id: Some(sync_id.to_string()),
            actor_id: None,
            auth_context: "system".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "This PC's unsent change of the decision was crossed by the same decision re-sent sealed by an upgraded PC: staged again on top of it, nobody asked",
                "decision_sync_id": sync_id,
                "server_row_version": version,
                "path": path,
                "conflicts_closed": closed_conflicts,
            })),
        },
    )
    .await?;
    tracing::info!(
        target: "maintafox::sync",
        decision = sync_id,
        version,
        path,
        "seal-only re-send crossing this PC's change settled"
    );
    Ok(())
}

/// A server conflict on this PC's push of a decision (`sync::queries`, beside the stock count and
/// crossed-hold settlements): when the server's version is a seal-only re-send of the version this
/// change was based on ([`is_seal_only_resend`]), it is settled with no person. `true` when settled.
pub(crate) async fn settle_seal_resend_server_conflict(
    conn: &impl ConnectionTrait,
    conflict: &crate::sync::domain::SyncServerConflictInput,
) -> AppResult<bool> {
    if conflict.entity_type != ENTITY || conflict.server_deleted {
        return Ok(false);
    }
    let Some(json) = conflict.server_payload_json.as_deref() else {
        return Ok(false);
    };
    let Ok(server) = serde_json::from_str::<Payload>(json) else {
        return Ok(false);
    };
    if !is_seal_only_resend(conn, &conflict.entity_sync_id, &server).await? {
        return Ok(false);
    }
    let hash = conflict
        .server_payload_hash
        .clone()
        .unwrap_or_else(|| crate::sync::queries::hash_payload(json));
    settle_seal_resend(
        conn,
        &conflict.entity_sync_id,
        conflict.server_row_version,
        &hash,
        "push",
    )
    .await?;
    Ok(true)
}

/// A received decision while this PC has an unsent change of it (`sync::materialize`, before the
/// concurrency check): a seal-only re-send of the version that change was based on is settled with
/// no person and not written over this PC's row (its content is the same, sealed alike). `Some`:
/// settled.
pub(crate) async fn settle_seal_resend_inbound(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    row_version: i64,
    payload_hash: &str,
    payload_json: &str,
) -> AppResult<Option<ApplyOutcome>> {
    if entity_type != ENTITY {
        return Ok(None);
    }
    // Only a version newer than the one this PC knows the server holds: this runs before the
    // concurrency check, so an older or equal version must never move the known version back
    // (review R4-N2). It is left to that check (a person conflict, or unchanged).
    match crate::sync::queries::synced_row_version(conn, ENTITY, entity_sync_id).await? {
        Some((known, _)) if row_version > known => {}
        _ => return Ok(None),
    }
    let unsent = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS u FROM sync_outbox WHERE entity_type = ? AND entity_sync_id = ? \
               AND status IN ('pending', 'in_flight', 'conflict') LIMIT 1",
            [ENTITY.into(), entity_sync_id.into()],
        ))
        .await?
        .is_some();
    if !unsent {
        return Ok(None);
    }
    let Ok(received) = serde_json::from_str::<Payload>(payload_json) else {
        return Ok(None);
    };
    if !is_seal_only_resend(conn, entity_sync_id, &received).await? {
        return Ok(None);
    }
    settle_seal_resend(conn, entity_sync_id, row_version, payload_hash, "pull").await?;
    Ok(Some(ApplyOutcome::Unchanged))
}

// ── Self-closing rights conflicts ──────────────────────────────────────────────────────────────

/// Decisions refused for rights (`DECISION_APPROVER_UNAUTHORISED` / `DECISION_APPLIER_UNAUTHORISED`)
/// that this PC's rules let in now (the approver's or applier's account or right arrived or was
/// granted here): each is applied again through every check; applied, its conflict is closed
/// (`resolved_remote`, `judged_valid`) and audited ([`CONFLICT_CLOSED_EVENT`]). Returns how many
/// applied. Wired next to the planning retries (`sync::materialize`).
pub(crate) async fn retry_decision_conflicts(db: &DatabaseConnection) -> AppResult<i64> {
    let mut applied = 0;
    for (inbox_id, conflict_id, sid, _received) in
        crate::planning::sync_check::open_planning_conflicts(db, ENTITY, RIGHTS_CONFLICTS).await?
    {
        let closed = crate::planning::sync_check::reapply_conflicted(
            db,
            inbox_id,
            conflict_id,
            "judged_valid",
            "The decision is judged valid here now (the right of the person who approved or applied it has arrived \
             or been granted): it is applied.",
        )
        .await?;
        if closed {
            applied += 1;
            // Target: the decision's sync id, the same on every PC (`sync::row_spec`, audit targets).
            crate::audit::emit(
                db,
                crate::audit::AuditEvent {
                    event_type: CONFLICT_CLOSED_EVENT,
                    actor_id: None,
                    entity_type: Some("reliability_study_decisions"),
                    entity_id: Some(&sid),
                    summary: "Reliability decision conflict closed: the rights of its approver or applier arrived",
                    detail_json: Some(
                        serde_json::json!({ "sync_conflict_id": conflict_id, "decision_sync_id": sid }).to_string(),
                    ),
                    ..Default::default()
                },
            )
            .await;
        }
    }
    Ok(applied)
}

// ── "Keep this PC's decision" ──────────────────────────────────────────────────────────────────

fn keep_refused(message: &str) -> AppError {
    AppError::ValidationFailed(vec![message.to_string()])
}

/// The person-facing words of a refusal: "refusé (sync) : <what> — <reason>".
fn refusal_text(conflict_type: &str, reason: &str) -> String {
    let what = match conflict_type {
        DECISION_STATUS_REGRESSION => "statut revenu en arrière",
        DECISION_CONTENT_CHANGED => "contenu modifié après approbation",
        DECISION_APPROVAL_UNSEALED => "approbation sans sceau",
        DECISION_NOT_DESIGNATED_APPROVER => "approuvée par une autre personne que l'approbateur désigné",
        DECISION_APPROVER_UNAUTHORISED => "approbateur sans le droit d'approuver",
        DECISION_APPLIER_UNAUTHORISED => "appliquée par une personne sans les droits",
        _ => "règle de décision",
    };
    format!("refusé (sync) : {what} — {reason}")
}

/// "Keep this PC's decision" on a [`RELIABILITY_DECISION_CONFLICTS`] conflict, before the generic
/// keep (`sync::conflicts::keep_local_version`, which then sends this PC's version):
/// - the person holds `ram.manage` plant-wide (the right that approves decisions);
/// - the received version is still refused by this PC's rules (a wait counts as refused); one let in
///   now closes by itself at the next synchronisation;
/// - a decision held here is marked refused (`sync_refused_at`, the reason), a decision new here is
///   stored in its received form marked refused, without links: that version is what is sent, so
///   every PC ends with the same refused decision, which none applies or publishes (never a delete);
/// - a work order created here from the decision is kept, its planner and the decision's proposer
///   told (CMMS Advisor Q6); the refusal is audited ([`REFUSED_EVENT`]).
pub(crate) async fn keep_this_pc_decision(
    conn: &impl ConnectionTrait,
    conflict: &crate::sync::domain::SyncConflictRecord,
    user_id: i64,
    reason: &str,
) -> AppResult<()> {
    if !crate::settings::site::holds_tenant_right(conn, user_id, crate::rbac::permissions::RAM_MANAGE).await? {
        return Err(keep_refused(
            "GATE_SYNC_KEEP_DECISION_NOT_ALLOWED: Keeping this PC's decision needs the right to approve reliability \
             decisions (ram.manage, plant-wide).",
        ));
    }
    let received: Payload = conflict
        .inbound_payload_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<Value>(j).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    let sid = conflict.entity_sync_id.as_str();
    if inbound_decision_check(conn, sid, &received).await?.is_none() {
        return Err(keep_refused(
            "GATE_SYNC_KEEP_DECISION_NOT_REFUSED: This PC's rules accept the received decision now (the right of the \
             person who approved or applied it has arrived or been granted): there is no decision to keep; the \
             conflict closes by itself at the next synchronisation.",
        ));
    }
    let now = crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let refusal = refusal_text(&conflict.conflict_type, reason);
    let held_id = crate::permit::sync_check::local_id(conn, ENTITY, sid).await?;
    if let Some(id) = held_id {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reliability_study_decisions SET sync_refused_at = ?, sync_refused_reason = ?, \
                    row_version = row_version + 1, updated_at = ? \
              WHERE id = ? AND sync_refused_at IS NULL",
            [
                now.clone().into(),
                refusal.clone().into(),
                now.clone().into(),
                id.into(),
            ],
        ))
        .await?;
    } else {
        // A status that is none of a decision's cannot be recorded here, now or later: escalate
        // (review R2-N3).
        if received
            .get("status")
            .is_some_and(|s| !s.as_str().is_some_and(|s| STATUSES.contains(&s)))
        {
            return Err(keep_refused(
                "GATE_SYNC_KEEP_DECISION_UNKNOWN_STATUS: The received decision has a status that is not a status of a \
                 decision: it can never be recorded here. Escalate the conflict.",
            ));
        }
        // What it gave that is held here, named before its links are cleared: a work order, or a
        // PM version published here (review S3).
        let work_order = match text(&received, "linked_work_order_sync_id") {
            Some(w) => crate::permit::sync_check::local_id(conn, "work_orders", w).await?,
            None => None,
        };
        let published = match text(&received, "linked_pm_plan_version_sync_id") {
            Some(v) => conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM pm_plan_versions WHERE entity_sync_id = ? AND status = 'published'",
                    [v.into()],
                ))
                .await?
                .map(|r| r.try_get::<i64>("", "id"))
                .transpose()
                .map_err(sync_err)?,
            None => None,
        };
        let mut refused = received.clone();
        refused.insert("sync_refused_at".into(), now.clone().into());
        refused.insert("sync_refused_reason".into(), refusal.clone().into());
        for key in [
            "linked_work_order_sync_id",
            "linked_pm_plan_version_sync_id",
            "linked_pm_plan_sync_id",
        ] {
            refused.insert(key.into(), Value::Null);
        }
        match crate::sync::materialize::apply_inbound_payload(
            conn,
            ENTITY,
            sid,
            "upsert",
            conflict.inbound_row_version.unwrap_or(1),
            &Value::Object(refused).to_string(),
        )
        .await?
        {
            ApplyOutcome::Applied | ApplyOutcome::Unchanged => {}
            ApplyOutcome::Deferred { message, .. } | ApplyOutcome::Conflict { message, .. } => {
                return Err(keep_refused(&format!(
                    "GATE_SYNC_KEEP_DECISION_NOT_YET: The refused decision cannot be recorded here yet: {message}"
                )));
            }
        }
        if work_order.is_some() || published.is_some() {
            if let Some(id) = crate::permit::sync_check::local_id(conn, ENTITY, sid).await? {
                crate::settings::received_notice::announce_refused_decision(conn, id, work_order, published).await;
            }
        }
    }
    let user_sync: Option<String> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<String>>("", "entity_sync_id").ok().flatten());
    crate::audit::writer::write_audit_event_in(
        conn,
        crate::audit::writer::AuditEventInput {
            action_code: REFUSED_EVENT.to_string(),
            target_type: Some("reliability_study_decisions".to_string()),
            target_id: Some(sid.to_string()),
            actor_id: Some(user_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Reliability decision refused at synchronisation (this PC's decision kept)",
                "decision_sync_id": sid,
                "conflict_type": conflict.conflict_type,
                "refused_by_sync_id": user_sync,
                "reason": reason,
            })),
        },
    )
    .await?;
    if held_id.is_some() {
        after_refusal_received(conn, sid).await?;
    }
    Ok(())
}

/// Every arrival check of a received decision, in order: refusal marks, four-eyes
/// (`decisions::inbound_self_approval_check`), then the approval checks ([`inbound_approval_check`]).
pub(crate) async fn inbound_decision_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    payload: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    use crate::reliability::study::decisions::{inbound_self_approval_check, InboundDecisionVerdict};
    // Only the four statuses of a decision; anything else (another spelling, a blank, a number)
    // would pass every check below and overwrite the status held here (review S1).
    if let Some(status) = payload.get("status") {
        if !status.as_str().is_some_and(|s| STATUSES.contains(&s)) {
            let title = text(payload, "title").unwrap_or_default();
            return Ok(Some(ApplyOutcome::Conflict {
                code: DECISION_STATUS_REGRESSION.into(),
                message: format!(
                    "decision '{title}' arrives with the status {status}, which is not a status of a decision \
                     (proposed, approved, rejected, applied)"
                ),
            }));
        }
    }
    match inbound_refusal_check(conn, entity_sync_id, payload).await? {
        RefusalVerdict::Accept => return Ok(None),
        RefusalVerdict::Outcome(outcome) => return Ok(Some(outcome)),
        RefusalVerdict::Judge => {}
    }
    if let Some(verdict) = inbound_self_approval_check(conn, entity_sync_id, payload).await? {
        return Ok(Some(match verdict {
            InboundDecisionVerdict::Defer(message) => ApplyOutcome::Deferred {
                code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                message,
                waits_for: None,
            },
            InboundDecisionVerdict::Conflict(message) => ApplyOutcome::Conflict {
                code: "DECISION_SELF_APPROVAL".into(),
                message,
            },
            // A rule re-checked on the received row (the screens list it as a refused value).
            InboundDecisionVerdict::LinkedWhileUnapproved(message) => ApplyOutcome::Conflict {
                code: "INVALID_VALUE".into(),
                message,
            },
        }));
    }
    inbound_approval_check(conn, entity_sync_id, payload).await
}
