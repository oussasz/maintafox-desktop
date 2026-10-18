//! Safety rules re-checked on rows received by synchronisation (`sync::apply_hooks::before_upsert_checked`).
//!
//! A permit, an isolation or a work order changed on another device reaches this one as a row. The
//! rules enforced where the change was made are checked again here, on the state received, before
//! anything is written — never stricter than the local rules (a stricter rule would refuse work that
//! is valid where it was done):
//! - a permit becoming `active` has its activation time, an expiry later than its (re)activation, and,
//!   when its type requires energy isolation, at least one isolation, every isolation verified by a
//!   second person, and no lock removed before that (re)activation;
//! - a permit `closed` or `handed_back` has every lock removal verified; `handed_back` has a handover
//!   log; `suspended` / `revalidation_required` has an open suspension;
//! - a permit never leaves `closed`, `handed_back`, `cancelled` or `expired` except along the lifecycle
//!   (`closed` → `handed_back`, `handed_back` → `cancelled`);
//! - an isolation is verified by someone other than the person who applied it, after it was applied;
//!   its lock removal is verified, after verification, by someone other than the applier;
//! - a work order entering `in_progress` has its start time and passes the permit gate at that time
//!   (`permit::wo_gate`): a live permit when it requires one, every live permit active, not expired and
//!   with its isolations ready.
//!
//! Judged on the state, not the path (an offline device may send several steps as one row), and at the
//! times written in the rows, never with this device's clock. A row already held here in the same
//! status passes. Rows still waiting in the sync inbox count, laid over the local ones: a permit and
//! its isolations created on another device arrive together, the isolations after the permit they
//! name, so waiting for them would never end. A referenced row that is neither here nor waiting
//! (permit type, work-order status) makes the row wait; a rule that fails is a sync conflict for a
//! person to decide, with nothing written.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{Map, Value};

use crate::errors::{AppError, AppResult};
use crate::settings::site::{instant, InboundVerdict};

pub(crate) type Payload = Map<String, Value>;

/// Permit statuses after which a permit no longer protects work (`permit::queries`).
const TERMINAL: &[&str] = &["closed", "handed_back", "cancelled", "expired"];

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

/// Rows of `table` naming the parent `parent_sync_id` (local column `parent_col`, payload key
/// `parent_key`): the local ones, with the latest version of each still waiting in the inbox laid
/// over them (a row moved to another parent by that version no longer counts).
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
        let sid: String = r.try_get("", "sid").map_err(sync_err)?;
        if let Some(p) = local_wire(conn, table, &sid).await? {
            rows.insert(sid, p);
        }
    }
    let waiting = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT entity_sync_id AS sid, payload_json FROM sync_inbox \
                 WHERE entity_type = ? AND apply_status IN ('pending', 'deferred') \
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
        let Ok(Value::Object(received)) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        let merged = overlay(rows.get(&sid), &received);
        rows.insert(sid, merged);
    }
    Ok(rows
        .into_iter()
        .filter(|(_, p)| field(p, parent_key) == Some(parent_sync_id))
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

/// An isolation verified by a second person after it was applied (the two-person rule).
fn verified_by_second_person(iso: &Payload) -> bool {
    let applier = field(iso, "applied_by_sync_id");
    field(iso, "applied_at").is_some()
        && field(iso, "verified_at").is_some()
        && applier.is_some()
        && field(iso, "verified_by_sync_id").is_some_and(|v| Some(v) != applier)
}

/// A lock removal verified before `at` (an unreadable time counts as removed, as locally any removal
/// counts).
fn removed_before(iso: &Payload, at: DateTime<Utc>) -> bool {
    field(iso, "removal_verified_at").is_some_and(|t| instant(t).is_none_or(|t| t < at))
}

/// The isolations of a permit ready to protect work at `at` (`permit::queries::activation_ready`).
fn isolations_ready(
    permit_code: &str,
    needs_isolation: bool,
    isolations: &[Payload],
    at: DateTime<Utc>,
) -> Option<(&'static str, String)> {
    if needs_isolation && isolations.is_empty() {
        return Some((
            "LOTO_NO_ISOLATION",
            format!("permit {permit_code} requires energy isolation and has no isolation point"),
        ));
    }
    let unverified = isolations.iter().filter(|i| !verified_by_second_person(i)).count();
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

async fn permit_isolations(conn: &impl ConnectionTrait, permit_sync_id: &str) -> AppResult<Vec<Payload>> {
    Ok(children(
        conn,
        "permit_isolations",
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
    let suspensions = || {
        children(
            conn,
            "permit_suspensions",
            "work_permits",
            "permit_id",
            "permit_sync_id",
            entity_sync_id,
        )
    };
    match to {
        "active" => {
            let Some(activated) = field(&permit, "activated_at").and_then(instant) else {
                return conflict(
                    "SYNC_STATE_TIME_MISSING",
                    format!("permit {code} is active but its activation time is missing or unreadable"),
                );
            };
            // A revalidated permit protects the work again from its last reinstatement.
            let mut at = activated;
            for (_, s) in suspensions().await? {
                if let Some(t) = field(&s, "reinstated_at").and_then(instant) {
                    at = at.max(t);
                }
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
                    format!("permit {code} was activated at {at} but expires at {expiry}"),
                );
            }
            let Some(needs) = type_requires_isolation(conn, &permit).await? else {
                return wait(format!("permit {code}: its permit type is not applied yet"));
            };
            let isolations = permit_isolations(conn, entity_sync_id).await?;
            if let Some((c, m)) = isolations_ready(&code, needs, &isolations, at) {
                return conflict(c, m);
            }
        }
        "closed" | "handed_back" => {
            let pending = permit_isolations(conn, entity_sync_id)
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
            if to == "handed_back" {
                let logs = children(
                    conn,
                    "permit_handover_logs",
                    "work_permits",
                    "permit_id",
                    "permit_sync_id",
                    entity_sync_id,
                )
                .await?;
                if logs.is_empty() {
                    return conflict(
                        "PERMIT_HANDOVER_REQUIRED",
                        format!("permit {code} is handed back without a handover log"),
                    );
                }
            }
        }
        "suspended" | "revalidation_required" => {
            if !suspensions()
                .await?
                .iter()
                .any(|(_, s)| field(s, "reinstated_at").is_none())
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

/// The application / verification fields of an isolation.
fn isolation_actions(p: &Payload) -> [Option<&str>; 6] {
    [
        field(p, "applied_by_sync_id"),
        field(p, "applied_at"),
        field(p, "verified_by_sync_id"),
        field(p, "verified_at"),
        field(p, "removal_verified_by_sync_id"),
        field(p, "removal_verified_at"),
    ]
}

/// Re-check of an isolation received by synchronisation: the two-person rule on its verification
/// and on the verification of its lock removal (`permit::queries`).
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
    let applier = field(&iso, "applied_by_sync_id");
    if field(&iso, "verified_at").is_some() {
        if applier.is_none() || field(&iso, "applied_at").is_none() {
            return conflict(
                "PERMIT_ISOLATION_NOT_APPLIED",
                format!("isolation '{point}' is verified but was never applied"),
            );
        }
        if field(&iso, "verified_by_sync_id").is_none_or(|v| Some(v) == applier) {
            return conflict(
                "PERMIT_TWO_PERSON_VERIFY",
                format!("isolation '{point}' was verified by the person who applied it (or by nobody)"),
            );
        }
    }
    if field(&iso, "removal_verified_at").is_some() {
        if field(&iso, "verified_at").is_none() {
            return conflict(
                "PERMIT_REMOVAL_NOT_VERIFIABLE",
                format!("isolation '{point}': lock removal verified on an isolation never verified"),
            );
        }
        if field(&iso, "removal_verified_by_sync_id").is_none_or(|v| Some(v) == applier) {
            return conflict(
                "PERMIT_TWO_PERSON_REMOVAL",
                format!("isolation '{point}': lock removal verified by the person who applied it (or by nobody)"),
            );
        }
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

/// Re-check of a work order received by synchronisation as it enters `in_progress`: the permit gate
/// (`permit::wo_gate`) at the time the work started or resumed.
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
    let Some(started) = field(&wo, "actual_start").and_then(instant) else {
        return conflict(
            "SYNC_STATE_TIME_MISSING",
            format!("work order {code} is in progress but its start time is missing or unreadable"),
        );
    };
    // A resumed work order passed the gate again when its last wait ended.
    let mut at = started;
    let segments = children(
        conn,
        "work_order_delay_segments",
        "work_orders",
        "work_order_id",
        "work_order_sync_id",
        entity_sync_id,
    )
    .await?;
    for (_, s) in &segments {
        if let Some(t) = field(s, "ended_at").and_then(instant) {
            at = at.max(t);
        }
    }
    let permits = children(
        conn,
        "work_permits",
        "work_orders",
        "linked_work_order_id",
        "linked_work_order_sync_id",
        entity_sync_id,
    )
    .await?;
    let live: Vec<&(String, Payload)> = permits
        .iter()
        .filter(|(_, p)| !TERMINAL.contains(&field(p, "status").unwrap_or_default()))
        .collect();
    if live.is_empty() && flag(&wo, "requires_permit") {
        return conflict(
            "PERMIT_REQUIRED_TO_START",
            format!("work order {code} requires a work permit and was started without one"),
        );
    }
    for (permit_sync, p) in live {
        let pcode = field(p, "code").unwrap_or_default();
        let status = field(p, "status").unwrap_or_default();
        if status == "suspended" || status == "revalidation_required" {
            return conflict(
                "PERMIT_SUSPENDED",
                format!("work order {code} started while permit {pcode} was suspended"),
            );
        }
        if status != "active" {
            return conflict(
                "PERMIT_NOT_ACTIVE",
                format!("work order {code} started while permit {pcode} was '{status}'"),
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
        let isolations = permit_isolations(conn, permit_sync).await?;
        if let Some((c, m)) = isolations_ready(pcode, needs, &isolations, at) {
            return conflict(c, m);
        }
    }
    Ok(None)
}
