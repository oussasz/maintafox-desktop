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
//!   log; `suspended` / `revalidation_required` has an open suspension;
//! - a permit never leaves `closed`, `handed_back`, `cancelled` or `expired` except along the lifecycle
//!   (`closed` → `handed_back`, `handed_back` → `cancelled`);
//! - an isolation (`isolation_rule_breach`, shared with the counting of waiting isolations): verified
//!   after it was applied and by someone other than the person who applied it; its lock removal
//!   verified after verification and by someone other than the applier. It never moves to another
//!   permit; it is applied or verified only while its permit is being prepared or suspended (not after
//!   the permit started protecting the work), and its lock removal is verified only once the permit
//!   was active (`ISOLATION_SETUP_STATUSES`, `ISOLATION_REMOVAL_STATUSES` of `permit::queries`);
//! - a permit past draft, and an isolation or suspension of a permit still protecting work, are never
//!   deleted (this device never deletes them);
//! - a work order entering `in_progress` passes the permit gate (`permit::wo_gate`) at the time it
//!   started or resumed: a permit when it requires one, every permit live at that time active, not
//!   suspended, not expired, with its isolations ready.
//!
//! Judged on the state, not the path (an offline device may send several steps as one row), and at the
//! times written in the rows, never with this device's clock (it only bounds times in the future). A
//! row already held here in the same status passes. Rows still waiting in the sync inbox count, laid
//! over the local ones — a permit and its isolations created on another device arrive together, the
//! isolations after the permit they name — but only a waiting row that is newer than the local one,
//! can be applied (its references are here or waiting too) and, for an isolation, keeps the
//! isolation rules. A referenced row that is neither here nor waiting (permit type, work-order
//! status, the permits of a work order requiring one) makes the row wait; a rule that fails is a sync
//! conflict for a person to decide, with nothing written.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{Map, Value};

use crate::errors::{AppError, AppResult};
use crate::settings::site::{instant, InboundVerdict, MAX_CLOCK_SKEW_MINUTES};

pub(crate) type Payload = Map<String, Value>;

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

/// Clock difference tolerated between the devices that wrote two times being compared.
fn skew() -> Duration {
    Duration::minutes(MAX_CLOCK_SKEW_MINUTES)
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
/// removal). The received-isolation check and the counting of waiting isolations both use it.
pub(crate) fn isolation_rule_breach(iso: &Payload) -> Option<(&'static str, String)> {
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
    }
    None
}

/// The local `row_version` of a row (`None`: not here, or a table without one).
async fn local_row_version(conn: &impl ConnectionTrait, table: &str, sync_id: &str) -> AppResult<Option<i64>> {
    let layout = crate::sync::row_layout::layout(conn, table).await?;
    if !layout.has_row_version {
        return Ok(None);
    }
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT row_version AS v FROM \"{table}\" WHERE entity_sync_id = ?"),
        [sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "v").map_err(sync_err))
    .transpose()
}

/// Whether a waiting inbox row can be applied: each required reference is here, or waiting in the
/// inbox itself.
async fn references_available(conn: &impl ConnectionTrait, table: &str, received: &Payload) -> AppResult<bool> {
    let layout = crate::sync::row_layout::layout(conn, table).await?;
    let Some((entity_type, sync_id)) = crate::sync::row_codec::first_missing_ref(conn, &layout, received).await? else {
        return Ok(true);
    };
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS w FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ? \
              AND apply_status = 'pending' AND operation NOT IN ('delete', 'archive')",
            [entity_type.into(), sync_id.into()],
        ))
        .await?
        .is_some())
}

/// Rows of `table` naming the parent `parent_sync_id` (local column `parent_col`, payload key
/// `parent_key`): the local ones, with the latest waiting version of each laid over them. A waiting
/// row counts only when it is pending (a deferred row waits for something that may never come),
/// newer than the local row, applicable (`references_available`) and, for an isolation, within the
/// isolation rules. A row moved to another parent by its waiting version no longer counts.
async fn children(
    conn: &impl ConnectionTrait,
    table: &str,
    parent_table: &str,
    parent_col: &str,
    parent_key: &str,
    parent_sync_id: &str,
) -> AppResult<Vec<(String, Payload)>> {
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
    let waiting = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT entity_sync_id AS sid, payload_json, row_version FROM sync_inbox \
                 WHERE entity_type = ? AND apply_status = 'pending' \
                   AND operation NOT IN ('delete', 'archive') \
                   AND json_extract(payload_json, '$.{parent_key}') = ? \
                 ORDER BY row_version, id"
            ),
            [table.into(), parent_sync_id.into()],
        ))
        .await?;
    for r in waiting {
        let sid: String = r.try_get("", "sid").map_err(sync_err)?;
        let json: String = r.try_get("", "payload_json").map_err(sync_err)?;
        let version: i64 = r.try_get("", "row_version").map_err(sync_err)?;
        let Ok(Value::Object(received)) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        if local_row_version(conn, table, &sid)
            .await?
            .is_some_and(|v| v >= version)
        {
            continue;
        }
        if !references_available(conn, table, &received).await? {
            continue;
        }
        let merged = overlay(rows.get(&sid), &received);
        if table == "permit_isolations" && isolation_rule_breach(&merged).is_some() {
            continue;
        }
        rows.insert(sid, merged);
    }
    Ok(rows
        .into_iter()
        .filter(|(_, p)| field(p, parent_key) == Some(parent_sync_id))
        .collect())
}

async fn permit_children(conn: &impl ConnectionTrait, table: &str, permit_sync_id: &str) -> AppResult<Vec<Payload>> {
    Ok(children(
        conn,
        table,
        "work_permits",
        "permit_id",
        "permit_sync_id",
        permit_sync_id,
    )
    .await?
    .into_iter()
    .map(|(_, p)| p)
    .collect())
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

/// The moment a permit (last) started protecting the work: its activation, or its last
/// reinstatement when later. Reinstatements dated after the permit's own close or in the future are
/// ignored. `None` without a readable activation time.
async fn protection_start(
    conn: &impl ConnectionTrait,
    permit_sync_id: &str,
    permit: &Payload,
) -> AppResult<Option<DateTime<Utc>>> {
    let Some(activated) = time(permit, "activated_at") else {
        return Ok(None);
    };
    let bound = [time(permit, "closed_at"), time(permit, "handed_back_at")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or_else(|| Utc::now() + skew());
    let mut at = activated;
    for s in permit_children(conn, "permit_suspensions", permit_sync_id).await? {
        if let Some(t) = time(&s, "reinstated_at").filter(|t| *t <= bound) {
            at = at.max(t);
        }
    }
    Ok(Some(at))
}

/// A lock removal verified before `at` (an unreadable time counts as removed, as locally any removal
/// counts).
fn removed_before(iso: &Payload, at: DateTime<Utc>) -> bool {
    field(iso, "removal_verified_at").is_some_and(|t| instant(t).is_none_or(|t| t < at))
}

/// The isolations of a permit ready to protect work from `at` (`permit::queries::activation_ready`):
/// every isolation it holds counts (none can be added while it protects work). Waiting isolations
/// already passed `isolation_rule_breach`; local ones are checked again.
fn isolations_ready(
    permit_code: &str,
    needs_isolation: bool,
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
        .filter(|i| field(i, "verified_at").is_none() || isolation_rule_breach(i).is_some())
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

/// Re-check of a permit received by synchronisation (see the module documentation).
pub(crate) async fn inbound_permit_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "work_permits", entity_sync_id).await?;
    let permit = overlay(local.as_ref(), received);
    let to = field(&permit, "status").unwrap_or_default();
    let from = local.as_ref().and_then(|l| field(l, "status"));
    if from == Some(to) {
        return Ok(None);
    }
    let code = field(&permit, "code").unwrap_or(entity_sync_id).to_string();
    if let Some(from) = from {
        let along = matches!((from, to), ("closed", "handed_back") | ("handed_back", "cancelled"));
        if TERMINAL.contains(&from) && !along {
            return conflict(
                "PERMIT_STATUS_BACKWARDS",
                format!("permit {code} is '{from}' here; it cannot become '{to}'"),
            );
        }
    }
    match to {
        "active" => {
            let Some(at) = protection_start(conn, entity_sync_id, &permit).await? else {
                return conflict(
                    "SYNC_STATE_TIME_MISSING",
                    format!("permit {code} is active but its activation time is missing or unreadable"),
                );
            };
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
            let isolations = permit_children(conn, "permit_isolations", entity_sync_id).await?;
            if let Some((c, m)) = isolations_ready(&code, needs, &isolations.iter().collect::<Vec<_>>(), at) {
                return conflict(c, m);
            }
        }
        "closed" | "handed_back" => {
            let pending = permit_children(conn, "permit_isolations", entity_sync_id)
                .await?
                .iter()
                .filter(|i| field(i, "removal_verified_at").is_none())
                .count();
            if pending > 0 {
                return conflict(
                    "PERMIT_REMOVAL_PENDING",
                    format!("permit {code} is {to} with {pending} lock removal(s) not verified"),
                );
            }
            if to == "handed_back"
                && permit_children(conn, "permit_handover_logs", entity_sync_id)
                    .await?
                    .is_empty()
            {
                return conflict(
                    "PERMIT_HANDOVER_REQUIRED",
                    format!("permit {code} is handed back without a handover log"),
                );
            }
        }
        "suspended" | "revalidation_required" => {
            if !permit_children(conn, "permit_suspensions", entity_sync_id)
                .await?
                .iter()
                .any(|s| field(s, "reinstated_at").is_none())
            {
                return conflict(
                    "PERMIT_SUSPENSION_MISSING",
                    format!("permit {code} is {to} without an open suspension"),
                );
            }
        }
        _ => {}
    }
    Ok(None)
}

/// The application / verification fields of an isolation, and its permit.
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
    if let Some((c, m)) = isolation_rule_breach(&iso) {
        return conflict(c, m);
    }
    // The permit is applied before its isolations; not here yet, the apply waits for it.
    let Some(permit_sync) = field(&iso, "permit_sync_id") else {
        return Ok(None);
    };
    let Some(permit) = local_wire(conn, "work_permits", permit_sync).await? else {
        return Ok(None);
    };
    let status = field(&permit, "status").unwrap_or_default();
    let changed =
        |key: &str| field(&iso, key).is_some() && local.as_ref().is_none_or(|l| field(l, key) != field(&iso, key));
    let code = field(&permit, "code").unwrap_or(permit_sync).to_string();
    // Applied or verified only while the permit was being prepared or suspended: a permit that
    // protects (or protected) the work had it before it started, or before its last reinstatement.
    if (changed("applied_at") || changed("verified_at")) && !ISOLATION_SETUP.contains(&status) {
        if let Some(start) = protection_start(conn, permit_sync, &permit).await? {
            for key in ["applied_at", "verified_at"] {
                if changed(key) && time(&iso, key).is_none_or(|t| t > start + skew()) {
                    return conflict(
                        "PERMIT_ISOLATION_WRONG_STATUS",
                        format!("isolation '{point}' was changed after permit {code} started protecting the work"),
                    );
                }
            }
        }
    }
    // A lock removal is verified only once the permit was active (never while it is prepared or
    // suspended).
    if changed("removal_verified_at") && ISOLATION_SETUP.contains(&status) {
        return conflict(
            "PERMIT_ISOLATION_WRONG_STATUS",
            format!("isolation '{point}': lock removal verified while permit {code} is '{status}'"),
        );
    }
    Ok(None)
}

/// Re-check of a delete received by synchronisation: this device never deletes a permit past draft,
/// nor an isolation or suspension of a permit that still protects (or will protect) work.
pub(crate) async fn inbound_delete_check(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<InboundVerdict>> {
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
    let refused = if entity_type == "work_permits" {
        status != "draft"
    } else {
        !TERMINAL.contains(&status.as_str())
    };
    if refused {
        return conflict(
            "PERMIT_DELETE_REFUSED",
            format!("{entity_type} of permit {code} ('{status}') cannot be deleted by synchronisation"),
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

/// When a work order last entered `in_progress`: the latest of its start, the end of its last wait
/// (resume) and its last recorded change to in progress (reopen), never later than the row itself was
/// written.
async fn work_started_at(
    conn: &impl ConnectionTrait,
    wo_sync_id: &str,
    wo: &Payload,
    started: DateTime<Utc>,
) -> AppResult<DateTime<Utc>> {
    let mut at = started;
    for (_, s) in children(
        conn,
        "work_order_delay_segments",
        "work_orders",
        "work_order_id",
        "work_order_sync_id",
        wo_sync_id,
    )
    .await?
    {
        if let Some(t) = time(&s, "ended_at") {
            at = at.max(t);
        }
    }
    // Local changes to in progress (recorded rows may have no sync identity), then waiting ones.
    for r in conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.acted_at AS at FROM wo_state_transition_log t JOIN work_orders w ON w.id = t.wo_id \
             WHERE w.entity_sync_id = ? AND t.to_status = 'in_progress'",
            [wo_sync_id.into()],
        ))
        .await?
    {
        if let Some(acted) = r
            .try_get::<Option<String>>("", "at")
            .map_err(sync_err)?
            .as_deref()
            .and_then(instant)
        {
            at = at.max(acted);
        }
    }
    for (_, t) in children(
        conn,
        "wo_state_transition_log",
        "work_orders",
        "wo_id",
        "wo_sync_id",
        wo_sync_id,
    )
    .await?
    {
        if field(&t, "to_status") == Some("in_progress") {
            if let Some(acted) = time(&t, "acted_at") {
                at = at.max(acted);
            }
        }
    }
    if let Some(written) = time(wo, "updated_at") {
        at = at.min(written.max(started));
    }
    Ok(at)
}

/// Re-check of a work order received by synchronisation as it enters `in_progress`: the permit gate
/// (`permit::wo_gate`) at the time the work started or resumed. A permit is judged in the state it
/// had at that time (its activation, close, suspensions and expiry are dated), so a later version of
/// it (suspended or closed since) does not refuse a start that was valid.
pub(crate) async fn inbound_work_order_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "work_orders", entity_sync_id).await?;
    let wo = overlay(local.as_ref(), received);
    let Some(status_sync) = field(&wo, "status_sync_id") else {
        return Ok(None);
    };
    let Some(to) = status_code(conn, status_sync).await? else {
        return wait(format!("work-order status {status_sync} is not applied yet"));
    };
    if to != "in_progress" {
        return Ok(None);
    }
    if let Some(from_sync) = local.as_ref().and_then(|l| field(l, "status_sync_id")) {
        if status_code(conn, from_sync).await?.as_deref() == Some("in_progress") {
            return Ok(None);
        }
    }
    let code = field(&wo, "code").unwrap_or(entity_sync_id).to_string();
    let Some(started) = time(&wo, "actual_start") else {
        return conflict(
            "SYNC_STATE_TIME_MISSING",
            format!("work order {code} is in progress but its start time is missing or unreadable"),
        );
    };
    let at = work_started_at(conn, entity_sync_id, &wo, started).await?;
    let permits = children(
        conn,
        "work_permits",
        "work_orders",
        "linked_work_order_id",
        "linked_work_order_sync_id",
        entity_sync_id,
    )
    .await?;
    if permits.is_empty() && flag(&wo, "requires_permit") {
        return wait(format!(
            "work order {code} requires a work permit and none has been received yet"
        ));
    }
    let mut live_at_start = 0;
    for (permit_sync, p) in &permits {
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
        if time(p, "activated_at").is_none_or(|a| a > at + skew()) {
            return conflict(
                "PERMIT_NOT_ACTIVE",
                format!("work order {code} started at {at} while permit {pcode} was not active yet"),
            );
        }
        let suspensions = permit_children(conn, "permit_suspensions", permit_sync).await?;
        let suspended = suspensions.iter().any(|s| {
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
        }
        let Some(needs) = type_requires_isolation(conn, p).await? else {
            return wait(format!("permit {pcode}: its permit type is not applied yet"));
        };
        // The isolations the permit held at the start: applied by then (one applied later, while it
        // was suspended again, does not count against a start that was valid).
        let isolations = permit_children(conn, "permit_isolations", permit_sync).await?;
        let held: Vec<&Payload> = isolations
            .iter()
            .filter(|i| field(i, "applied_at").is_some_and(|t| instant(t).is_none_or(|t| t <= at + skew())))
            .collect();
        let late_verified = held
            .iter()
            .any(|i| time(i, "verified_at").is_some_and(|v| v > at + skew()));
        if late_verified {
            return conflict(
                "LOTO_NOT_VERIFIED",
                format!("work order {code} started at {at} before an isolation of permit {pcode} was verified"),
            );
        }
        if let Some((c, m)) = isolations_ready(pcode, needs, &held, at) {
            return conflict(c, m);
        }
    }
    if live_at_start == 0 && flag(&wo, "requires_permit") {
        return conflict(
            "PERMIT_REQUIRED_TO_START",
            format!("work order {code} requires a work permit and none was live when it started"),
        );
    }
    Ok(None)
}
