//! The workers of a permit and their personal padlocks received by synchronisation
//! (`sync::apply_hooks`; L4-05, L4-06, L4-18, migration 294), re-checked on the state received,
//! as the isolation locks are (`permit::sync_check`, migration 249) — never stricter than the
//! local rules (`permit::workers`):
//! - a padlock (`inbound_personal_lock_check`): placed by someone, at a readable time not in the
//!   future, by the worker of its line (their card is linked to the account that placed it) and not
//!   by a worker with a forced removal they were not told of; taken off by its owner, or by a forced
//!   removal recorded as for an isolation lock (an authoriser holding `ptw.close` at the permit's
//!   site here, a named remover who is neither the owner nor the authoriser, a reason, the
//!   attestation that the worker is absent, the contact attempts) and the worker told of it as for
//!   an isolation lock; once held here, its placement, its removal and the worker told are never
//!   rewritten (`PERMIT_PERSONAL_LOCK_RECORD_CHANGED`);
//! - a worker line (`inbound_worker_check`): its identity, induction and withdrawal are never
//!   rewritten once held here (`PERMIT_WORKER_RECORD_CHANGED`); a withdrawal while that worker's
//!   padlock is on the box here is refused (`PERMIT_WORKER_WITHDRAWN_LOCK_ON`), or waits when the
//!   padlock's removal is waiting in the inbox; a worker added to a permit already issued here is
//!   judged on the rules of the issue on the day they were added (induction of external staff,
//!   qualifications of the permit's type: `PERMIT_WORKER_ADDED_*`);
//! - the group lockout ([`open_padlocks_verdict`]): an isolation lock taken off, or a permit closed
//!   or handed back, while a personal padlock is still on its box here is refused
//!   (`PERMIT_ISOLATION_PERSONAL_LOCKS_ON`, `PERMIT_CLOSED_PERSONAL_LOCKS_ON`), or waits when every
//!   such padlock's removal is waiting in the inbox.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::AppResult;
use crate::permit::sync_check::{
    close_right_at_permit, conflict, field, flag, future_bound, group_previous_status, local_id, local_user,
    local_wire, overlay, removal_record_breach, time, wait, Payload,
};
use crate::settings::site::InboundVerdict;

pub(crate) const PERSONAL_LOCK_RECORD_CHANGED: &str = "PERMIT_PERSONAL_LOCK_RECORD_CHANGED";
pub(crate) const PERSONAL_LOCK_PLACEMENT_INVALID: &str = "PERMIT_PERSONAL_LOCK_PLACEMENT_INVALID";
pub(crate) const PERSONAL_LOCK_NOT_OWNER: &str = "PERMIT_PERSONAL_LOCK_NOT_OWNER";
pub(crate) const WORKER_RECORD_CHANGED: &str = "PERMIT_WORKER_RECORD_CHANGED";
pub(crate) const WORKER_WITHDRAWN_LOCK_ON: &str = "PERMIT_WORKER_WITHDRAWN_LOCK_ON";
pub(crate) const ISOLATION_PERSONAL_LOCKS_ON: &str = "PERMIT_ISOLATION_PERSONAL_LOCKS_ON";
pub(crate) const CLOSED_PERSONAL_LOCKS_ON: &str = "PERMIT_CLOSED_PERSONAL_LOCKS_ON";

/// What a padlock keeps from its placement.
const LOCK_PLACEMENT_KEYS: &[&str] = &[
    "permit_sync_id",
    "worker_sync_id",
    "lock_number",
    "placed_by_sync_id",
    "placed_at",
];
/// The record of a padlock taken off (with its forced-removal record).
const LOCK_REMOVAL_KEYS: &[&str] = &[
    "removed_by_sync_id",
    "removed_at",
    "forced_authorized_by_sync_id",
    "forced_reason",
    "forced_contact_attempts_json",
];
/// The record of the owner told of a forced removal.
const LOCK_INFORMED_KEYS: &[&str] = &["informed_by_sync_id", "informed_at", "informed_method"];
/// What a worker line keeps from its creation.
const WORKER_IDENTITY_KEYS: &[&str] = &[
    "permit_sync_id",
    "personnel_sync_id",
    "external_name",
    "external_company_sync_id",
    "added_by_sync_id",
    "added_at",
];
const WORKER_INDUCTION_KEYS: &[&str] = &["induction_at", "induction_by_sync_id"];
const WORKER_WITHDRAWAL_KEYS: &[&str] = &["withdrawn_at", "withdrawn_by_sync_id"];
/// Permit statuses reached through the issue (`sync_check::ISSUED_ON`, the permit protects or
/// protected work): a worker added then is judged on the issue's rules.
const ISSUED: &[&str] = &["issued", "active", "suspended", "revalidation_required"];

fn lock_label(lock: &Payload, sync_id: &str) -> String {
    format!("padlock '{}'", field(lock, "lock_number").unwrap_or(sync_id))
}

fn differs(held: &Payload, got: &Payload, keys: &[&str]) -> bool {
    keys.iter().any(|k| field(held, k) != field(got, k))
}

/// The rules a state of a padlock breaks on the row alone: placed by someone at a readable time not
/// in the future; taken off (not before it was placed) by its owner, or by a forced removal and its
/// worker told as for an isolation lock (`sync_check::removal_record_breach`, migration 249).
pub(crate) fn personal_lock_breach(lock: &Payload, sync_id: &str) -> Option<(&'static str, String)> {
    let label = lock_label(lock, sync_id);
    let placed = time(lock, "placed_at");
    if field(lock, "placed_by_sync_id").is_none() || placed.is_none_or(|t| t > future_bound()) {
        return Some((
            PERSONAL_LOCK_PLACEMENT_INVALID,
            format!("{label}: placed without who or when, or at a time unreadable or in the future"),
        ));
    }
    if let (Some(placed), Some(removed)) = (placed, time(lock, "removed_at")) {
        if removed < placed {
            return Some((
                "PERMIT_LOCK_REMOVAL_INVALID",
                format!("{label}: taken off before it was placed"),
            ));
        }
    }
    removal_record_breach(lock, &label, "placed_by_sync_id", "placed_at")
}

/// Once held here, a padlock's placement, its removal (with its forced-removal record) and the
/// worker told are never rewritten nor taken back by a received row.
fn lock_record_kept(local: Option<&Payload>, lock: &Payload, label: &str) -> Option<InboundVerdict> {
    let held = local?;
    let placement = differs(held, lock, LOCK_PLACEMENT_KEYS);
    let removal = field(held, "removed_at").is_some()
        && (differs(held, lock, LOCK_REMOVAL_KEYS)
            || flag(held, "forced_removal") != flag(lock, "forced_removal")
            || flag(held, "forced_worker_absent") != flag(lock, "forced_worker_absent"));
    let informed = field(held, "informed_at").is_some()
        && (differs(held, lock, LOCK_INFORMED_KEYS) || flag(held, "informed_in_app") != flag(lock, "informed_in_app"));
    (placement || removal || informed).then(|| InboundVerdict::Conflict {
        code: PERSONAL_LOCK_RECORD_CHANGED,
        message: format!(
            "{label}: its placement, its removal or the record of its owner told, held here, cannot change"
        ),
    })
}

/// Re-check of a personal padlock received by synchronisation (see the module documentation).
pub(crate) async fn inbound_personal_lock_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "permit_personal_locks", entity_sync_id).await?;
    let lock = overlay(local.as_ref(), received);
    let label = lock_label(&lock, entity_sync_id);
    if let Some(verdict) = lock_record_kept(local.as_ref(), &lock, &label) {
        return Ok(Some(verdict));
    }
    if let Some((c, m)) = personal_lock_breach(&lock, entity_sync_id) {
        return conflict(c, m);
    }
    // The permit and the worker line are applied before their padlocks; not here yet, the apply
    // waits for them.
    let Some(permit_sync) = field(&lock, "permit_sync_id") else {
        return Ok(None);
    };
    if local.is_none() {
        if let Some(verdict) = placement_check(conn, &lock, entity_sync_id, &label).await? {
            return Ok(Some(verdict));
        }
    }
    let new_here = |key: &str| field(&lock, key).is_some() && local.as_ref().is_none_or(|l| field(l, key).is_none());
    if flag(&lock, "forced_removal") && new_here("removed_at") {
        if let Some(authoriser) = field(&lock, "forced_authorized_by_sync_id") {
            if let Some(verdict) = close_right_at_permit(
                conn,
                authoriser,
                permit_sync,
                "PERMIT_FORCED_REMOVAL_UNAUTHORISED",
                format!("{label}: forced removal authorised"),
            )
            .await?
            {
                return Ok(Some(verdict));
            }
        }
    }
    if new_here("informed_at") && !flag(&lock, "informed_in_app") {
        if let Some(informer) = field(&lock, "informed_by_sync_id") {
            if let Some(verdict) = close_right_at_permit(
                conn,
                informer,
                permit_sync,
                "PERMIT_WORKER_INFORMED_UNAUTHORISED",
                format!("{label}: owner told of the forced removal, recorded"),
            )
            .await?
            {
                return Ok(Some(verdict));
            }
        }
    }
    Ok(None)
}

/// A padlock new here: placed by the worker of its line (the account linked to that worker's card),
/// who has no forced removal they were not told of, taken off before this placement
/// (`PERMIT_WORKER_NOT_INFORMED`, as `permit::queries::open_forced_removal_of` locally).
async fn placement_check(
    conn: &impl ConnectionTrait,
    lock: &Payload,
    entity_sync_id: &str,
    label: &str,
) -> AppResult<Option<InboundVerdict>> {
    let (Some(worker_sync), Some(placer_sync)) = (field(lock, "worker_sync_id"), field(lock, "placed_by_sync_id"))
    else {
        return Ok(None);
    };
    let Some(worker) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM permit_workers WHERE entity_sync_id = ?",
            [worker_sync.into()],
        ))
        .await?
    else {
        return wait(format!("{label}: its worker line {worker_sync} is not applied yet"));
    };
    let personnel: Option<i64> = worker.try_get("", "personnel_id")?;
    let Some(placer) = local_user(conn, placer_sync).await? else {
        return wait(format!("{label}: the person who placed it is not applied yet"));
    };
    let card = crate::qualification::gate::personnel_for_user(conn, placer).await?;
    if personnel.is_none() || card != personnel {
        return conflict(
            PERSONAL_LOCK_NOT_OWNER,
            format!("{label}: placed by someone who is not the worker of its line"),
        );
    }
    let placed = time(lock, "placed_at");
    let open = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 'permit_isolations' AS t, pi.entity_sync_id AS sid, pi.removed_at AS removed_at FROM permit_isolations pi WHERE pi.applied_by_id = ? AND pi.forced_removal = 1 AND pi.informed_at IS NULL UNION ALL SELECT 'permit_personal_locks' AS t, pl.entity_sync_id AS sid, pl.removed_at AS removed_at FROM permit_personal_locks pl WHERE pl.placed_by_id = ? AND pl.forced_removal = 1 AND pl.informed_at IS NULL AND pl.entity_sync_id IS NOT ?",
            [placer.into(), placer.into(), entity_sync_id.into()],
        ))
        .await?;
    for r in open {
        let table: String = r.try_get("", "t")?;
        let sid: Option<String> = r.try_get("", "sid")?;
        let removed: Option<String> = r.try_get("", "removed_at")?;
        let before = match (removed.as_deref().and_then(crate::settings::site::instant), placed) {
            (Some(removed), Some(placed)) => removed <= placed,
            _ => true,
        };
        if !before {
            continue;
        }
        if let Some(sid) = sid {
            if removal_waiting(conn, &table, &sid, "informed_at").await? {
                return wait(format!(
                    "{label}: the owner's acknowledgement of an earlier forced removal is still arriving"
                ));
            }
        }
        return conflict(
            "PERMIT_WORKER_NOT_INFORMED",
            format!("{label}: placed by a worker not yet told of the forced removal of one of their locks"),
        );
    }
    Ok(None)
}

/// A waiting inbox row of `table` for `sync_id` that sets `key`.
async fn removal_waiting(conn: &impl ConnectionTrait, table: &str, sync_id: &str, key: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS w FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ? AND apply_status IN ('pending', 'deferred') AND json_extract(payload_json, '$.{key}') IS NOT NULL LIMIT 1"
            ),
            [table.into(), sync_id.into()],
        ))
        .await?
        .is_some())
}

/// Which padlocks [`open_padlocks_verdict`] looks at.
pub(crate) enum PadlocksOf<'a> {
    /// The padlocks on the group lock box of the permit with this sync id.
    Permit(&'a str),
    /// The padlock of the worker line with this sync id.
    Worker(&'a str),
}

/// The group lockout on a received row (L4-18): padlocks still on the box here. `Ok(None)` when none;
/// a wait when every one has its removal waiting in the inbox (applied first, then this row is
/// retried); otherwise the conflict `code` naming them.
pub(crate) async fn open_padlocks_verdict(
    conn: &impl ConnectionTrait,
    of: PadlocksOf<'_>,
    code: &'static str,
    what: &str,
) -> AppResult<Option<InboundVerdict>> {
    let (sql, sync_id) = match of {
        PadlocksOf::Permit(s) => (
            "SELECT pl.entity_sync_id AS sid, pl.lock_number AS n FROM permit_personal_locks pl JOIN work_permits wp ON wp.id = pl.permit_id WHERE wp.entity_sync_id = ? AND pl.removed_at IS NULL",
            s,
        ),
        PadlocksOf::Worker(s) => (
            "SELECT pl.entity_sync_id AS sid, pl.lock_number AS n FROM permit_personal_locks pl JOIN permit_workers pw ON pw.id = pl.worker_id WHERE pw.entity_sync_id = ? AND pl.removed_at IS NULL",
            s,
        ),
    };
    let rows = conn
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [sync_id.into()]))
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut on = Vec::new();
    let mut waiting = false;
    for r in rows {
        let sid: String = r.try_get("", "sid")?;
        let number: String = r.try_get("", "n")?;
        if removal_waiting(conn, "permit_personal_locks", &sid, "removed_at").await? {
            waiting = true;
        } else {
            on.push(number);
        }
    }
    if on.is_empty() && waiting {
        return wait(format!("{what}: the removal of a personal padlock is still arriving"));
    }
    conflict(
        code,
        format!(
            "{what} while personal padlock(s) {} are still on the group lock box here",
            on.join(", ")
        ),
    )
}

/// Re-check of a worker line received by synchronisation (see the module documentation).
pub(crate) async fn inbound_worker_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "permit_workers", entity_sync_id).await?;
    let w = overlay(local.as_ref(), received);
    let label = field(&w, "external_name").unwrap_or(entity_sync_id).to_string();
    if let Some(held) = &local {
        let rewritten = differs(held, &w, WORKER_IDENTITY_KEYS)
            || (field(held, "induction_at").is_some() && differs(held, &w, WORKER_INDUCTION_KEYS))
            || (field(held, "withdrawn_at").is_some() && differs(held, &w, WORKER_WITHDRAWAL_KEYS));
        if rewritten {
            return conflict(
                WORKER_RECORD_CHANGED,
                format!("worker {label}: who they are, their induction or their withdrawal, held here, cannot change"),
            );
        }
    }
    let Some(permit_sync) = field(&w, "permit_sync_id") else {
        return Ok(None);
    };
    // Withdrawn while their padlock is on the box here: the group lockout.
    if field(&w, "withdrawn_at").is_some() && local.as_ref().is_some_and(|l| field(l, "withdrawn_at").is_none()) {
        return open_padlocks_verdict(
            conn,
            PadlocksOf::Worker(entity_sync_id),
            WORKER_WITHDRAWN_LOCK_ON,
            &format!("worker {label} withdrawn"),
        )
        .await;
    }
    if local.is_some() || field(&w, "withdrawn_at").is_some() {
        return Ok(None);
    }
    // A worker new here on a permit already issued here (not one judged with its issue, which
    // `sync_check::issue_check` judges with its workers).
    let Some(permit) = local_wire(conn, "work_permits", permit_sync).await? else {
        return Ok(None);
    };
    let held_status = match group_previous_status(permit_sync) {
        Some(previous) => previous,
        None => field(&permit, "status").map(str::to_string),
    };
    if !held_status.as_deref().is_some_and(|s| ISSUED.contains(&s)) {
        return Ok(None);
    }
    let code = field(&permit, "code").unwrap_or(permit_sync).to_string();
    if let Some(card) = field(&w, "personnel_sync_id") {
        if local_id(conn, "personnel", card).await?.is_none() {
            return wait(format!(
                "worker {label}: their personnel record {card} is not applied yet"
            ));
        }
    }
    let Some(type_sync) = field(&permit, "permit_type_sync_id") else {
        return Ok(None);
    };
    let Some(type_id) = local_id(conn, "permit_types", type_sync).await? else {
        return wait(format!("permit {code}: its permit type is not applied yet"));
    };
    let wo_id = match field(&permit, "linked_work_order_sync_id") {
        Some(wo) => local_id(conn, "work_orders", wo).await?,
        None => None,
    };
    let Some(added) = time(&w, "added_at").filter(|t| *t <= future_bound()) else {
        return conflict(
            WORKER_RECORD_CHANGED,
            format!("worker {label} of permit {code}: added at a time unreadable or in the future"),
        );
    };
    match crate::permit::workers::received_workers_breach(conn, &code, type_id, wo_id, added.date_naive(), &[&w], true)
        .await?
    {
        Some((c, m)) => conflict(c, m),
        None => Ok(None),
    }
}
