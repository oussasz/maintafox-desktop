//! The people working under a work permit, their induction and qualifications, and their personal
//! padlocks on the permit's group lock box (L4-05, L4-06, L4-18, migration 291).
//!
//! - **Workers** (`permit_workers`): a personnel record (internal staff, or contractor staff with a
//!   card), or an external worker without a card: a name and a company of the contractor list
//!   (`external_companies`). Each has a role on the permit (free text). External staff (a worker
//!   without a card, or a card whose employment origin is `external`) need an induction recorded
//!   (when, by whom: the session user) before the permit is issued; one added to a permit already
//!   issued is added with it. A worker is withdrawn, never deleted, and not while their padlock is on.
//! - **Qualifications** (L4-06): every worker with a card must hold, valid on the issue date, the
//!   personal (`each`) qualifications the permit's type requires (`qualification::gate::
//!   check_person_for_permit_type`; overrides recorded on the linked work order are honoured). An
//!   external worker without a card cannot be checked: refused when the type requires anything
//!   (fails closed, as the qualification gate does for a login without a card). Checked when a
//!   worker is added, when they place their padlock, and at the issue.
//! - **Personal padlocks** (`permit_personal_locks`, L4-18): a worker holding `ptw.lock_own` places
//!   and removes their own padlock on the group lock box of a permit they are listed on, and nobody
//!   else's. Placed while the permit is issued or active; removed by its owner while the permit is in
//!   use or ended. When the owner is absent, a `ptw.close` holder (after a step-up) removes it by
//!   force with a reason and the attestation that the worker is not on site. While a padlock is on
//!   the box, no isolation lock comes off and the permit is not closed (the group lockout rule).

use chrono::{NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult, AppValidationIssue};
use crate::permit::safety_content::IssueBlocker;

/// Statuses in which workers are added, withdrawn and inducted (every status before the permit ends).
pub const WORKER_EDIT_STATUSES: &[&str] = &[
    "draft",
    "pending_review",
    "approved",
    "issued",
    "active",
    "suspended",
    "revalidation_required",
];
/// Statuses before the issue: an external worker added then is inducted before the issue.
const PRE_ISSUE_STATUSES: &[&str] = &["draft", "pending_review", "approved"];
/// Statuses in which a personal padlock is placed on the group lock box.
pub const LOCK_PLACE_STATUSES: &[&str] = &["issued", "active"];
/// Statuses in which a personal padlock comes off: while the permit is in use (the worker leaves),
/// and once it ended without being closed (cancelled, expired: the lock must come off).
pub const LOCK_REMOVE_STATUSES: &[&str] = &[
    "issued",
    "active",
    "suspended",
    "revalidation_required",
    "cancelled",
    "expired",
];

/// Longest name, company-free text or role of a worker.
pub const MAX_WORKER_TEXT_CHARS: usize = 200;
/// Longest padlock number.
pub const MAX_LOCK_NUMBER_CHARS: usize = 50;

pub const CODE_INDUCTION_MISSING: &str = "GATE_PERMIT_WORKER_INDUCTION_MISSING";
pub const CODE_QUALIFICATION_EXPIRED: &str = "GATE_PERMIT_WORKER_QUALIFICATION_EXPIRED";
pub const CODE_QUALIFICATION_MISSING: &str = "GATE_PERMIT_WORKER_QUALIFICATION_MISSING";
pub const CODE_QUALIFICATION_UNVERIFIABLE: &str = "GATE_PERMIT_WORKER_QUALIFICATION_UNVERIFIABLE";
pub const CODE_PERSONAL_LOCKS_ON: &str = "GATE_PERMIT_PERSONAL_LOCKS_ON";
pub const CODE_LOCK_NOT_YOURS: &str = "GATE_PERMIT_PERSONAL_LOCK_NOT_YOURS";
pub const CODE_LOCK_NOT_A_WORKER: &str = "GATE_PERMIT_PERSONAL_LOCK_NOT_A_WORKER";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitPersonalLock {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub worker_id: i64,
    pub lock_number: String,
    pub placed_by_id: i64,
    pub placed_by_name: Option<String>,
    pub placed_at: String,
    pub removed_by_id: Option<i64>,
    pub removed_by_name: Option<String>,
    pub removed_at: Option<String>,
    /// Removed by a `ptw.close` holder while its owner was absent.
    pub forced_removal: bool,
    pub forced_reason: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitWorker {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub personnel_id: Option<i64>,
    pub personnel_name: Option<String>,
    pub employee_code: Option<String>,
    pub external_name: Option<String>,
    pub external_company_id: Option<i64>,
    /// The worker's company: the one named on the line, or the company of their card.
    pub company_name: Option<String>,
    /// External staff (no card, or a card of external origin): an induction is required.
    pub is_external: bool,
    pub role_on_permit: String,
    pub induction_at: Option<String>,
    pub induction_by_id: Option<i64>,
    pub induction_by_name: Option<String>,
    pub added_by_id: i64,
    pub added_by_name: Option<String>,
    pub added_at: String,
    pub withdrawn_at: Option<String>,
    pub row_version: i64,
    /// The session user is this worker (their card is linked to the login).
    pub is_me: bool,
    /// The worker's padlock on the group lock box, when on.
    pub open_lock: Option<PermitPersonalLock>,
    /// What this worker lacks for the permit today (empty: nothing). Withdrawn workers: empty.
    pub blockers: Vec<IssueBlocker>,
}

/// The workers of a permit and the padlocks of its group lock box, as the permit screen shows them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitWorkersView {
    pub permit_id: i64,
    pub permit_status: String,
    /// Listed first, withdrawn last.
    pub workers: Vec<PermitWorker>,
    /// Every padlock ever placed, latest first.
    pub locks: Vec<PermitPersonalLock>,
    /// Padlocks still on the box.
    pub open_lock_count: i64,
}

/// Adds a worker. Exactly one of `personnel_id` or (`external_name` and `external_company_id`).
/// `induction_at` (RFC 3339, not in the future) records the induction of an external worker given
/// by the session user; required for an external worker added to a permit already issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitWorkerAddInput {
    pub permit_id: i64,
    #[serde(default)]
    pub personnel_id: Option<i64>,
    #[serde(default)]
    pub external_name: Option<String>,
    #[serde(default)]
    pub external_company_id: Option<i64>,
    pub role_on_permit: String,
    #[serde(default)]
    pub induction_at: Option<String>,
}

/// The induction of an external worker, given by the session user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitWorkerInductionInput {
    pub worker_id: i64,
    pub expected_row_version: i64,
    /// RFC 3339, not in the future; now when omitted.
    #[serde(default)]
    pub induction_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitWorkerWithdrawInput {
    pub worker_id: i64,
    pub expected_row_version: i64,
}

/// The session user's own padlock on the group lock box of `permit_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalLockPlaceInput {
    pub permit_id: i64,
    pub lock_number: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalLockRemoveInput {
    pub lock_id: i64,
    pub expected_row_version: i64,
}

/// A padlock whose owner is absent, removed by a `ptw.close` holder (the session user, step-up).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalLockForceRemoveInput {
    pub lock_id: i64,
    pub expected_row_version: i64,
    /// At least `MIN_FORCED_REMOVAL_REASON_CHARS` characters once trimmed.
    pub reason: String,
    /// "Le travailleur n'est pas sur le site": must be true.
    pub worker_absent_attested: bool,
}

fn refused(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(code, message, crate::errors::issue_params(params))
}

fn changed_elsewhere(table: &str) -> AppError {
    AppError::ValidationFailed(vec![format!(
        "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on {table}."
    )])
}

fn now_ts() -> String {
    crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn today() -> NaiveDate {
    crate::clock::now().date_naive()
}

/// Blockers as one refusal, each with its code.
pub(crate) fn blockers_error(blockers: Vec<IssueBlocker>, what: &str) -> AppError {
    crate::errors::org_validation_failed_issues(
        blockers
            .into_iter()
            .map(|b| {
                AppValidationIssue::error(
                    b.code.clone(),
                    format!("{what}: {}", b.code),
                    b.params.into_iter().collect(),
                )
            })
            .collect(),
    )
}

/// What the gates read of the permit.
struct PermitFacts {
    id: i64,
    code: String,
    status: String,
    permit_type_id: i64,
    linked_wo_id: Option<i64>,
}

async fn permit_facts(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<PermitFacts> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, status, permit_type_id, linked_work_order_id FROM work_permits WHERE id = ?",
            [permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: permit_id.to_string(),
        })?;
    Ok(PermitFacts {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        status: row.try_get("", "status")?,
        permit_type_id: row.try_get("", "permit_type_id")?,
        linked_wo_id: row.try_get("", "linked_work_order_id")?,
    })
}

fn ensure_status(permit: &PermitFacts, allowed: &[&str], code: &str, what: &str) -> AppResult<()> {
    if allowed.contains(&permit.status.as_str()) {
        Ok(())
    } else {
        Err(refused(
            code,
            format!(
                "{what} is not allowed on permit {} in status '{}'.",
                permit.code, permit.status
            ),
            &[("permit", permit.code.clone()), ("status", permit.status.clone())],
        ))
    }
}

const WORKER_SELECT: &str = "SELECT w.id, w.entity_sync_id, w.permit_id, w.personnel_id, p.full_name AS personnel_name, p.employee_code, p.employment_origin, w.external_name, w.external_company_id, COALESCE(ec.name, pec.name) AS company_name, w.role_on_permit, w.induction_at, w.induction_by_id, COALESCE(ui.display_name, ui.username) AS induction_by_name, w.added_by_id, COALESCE(ua.display_name, ua.username) AS added_by_name, w.added_at, w.withdrawn_at, w.row_version FROM permit_workers w LEFT JOIN personnel p ON p.id = w.personnel_id LEFT JOIN external_companies ec ON ec.id = w.external_company_id LEFT JOIN external_companies pec ON pec.id = p.external_company_id LEFT JOIN user_accounts ui ON ui.id = w.induction_by_id LEFT JOIN user_accounts ua ON ua.id = w.added_by_id";

const LOCK_SELECT: &str = "SELECT l.id, l.entity_sync_id, l.permit_id, l.worker_id, l.lock_number, l.placed_by_id, COALESCE(up.display_name, up.username) AS placed_by_name, l.placed_at, l.removed_by_id, COALESCE(ur.display_name, ur.username) AS removed_by_name, l.removed_at, l.forced_removal, l.forced_reason, l.row_version FROM permit_personal_locks l LEFT JOIN user_accounts up ON up.id = l.placed_by_id LEFT JOIN user_accounts ur ON ur.id = l.removed_by_id";

fn map_worker(r: &sea_orm::QueryResult) -> AppResult<PermitWorker> {
    let personnel_id: Option<i64> = r.try_get("", "personnel_id")?;
    let origin: Option<String> = r.try_get("", "employment_origin")?;
    Ok(PermitWorker {
        id: r.try_get("", "id")?,
        entity_sync_id: r.try_get("", "entity_sync_id")?,
        permit_id: r.try_get("", "permit_id")?,
        personnel_id,
        personnel_name: r.try_get("", "personnel_name")?,
        employee_code: r.try_get("", "employee_code")?,
        external_name: r.try_get("", "external_name")?,
        external_company_id: r.try_get("", "external_company_id")?,
        company_name: r.try_get("", "company_name")?,
        is_external: personnel_id.is_none() || origin.as_deref() == Some("external"),
        role_on_permit: r.try_get("", "role_on_permit")?,
        induction_at: r.try_get("", "induction_at")?,
        induction_by_id: r.try_get("", "induction_by_id")?,
        induction_by_name: r.try_get("", "induction_by_name")?,
        added_by_id: r.try_get("", "added_by_id")?,
        added_by_name: r.try_get("", "added_by_name")?,
        added_at: r.try_get("", "added_at")?,
        withdrawn_at: r.try_get("", "withdrawn_at")?,
        row_version: r.try_get("", "row_version")?,
        is_me: false,
        open_lock: None,
        blockers: Vec::new(),
    })
}

fn map_lock(r: &sea_orm::QueryResult) -> AppResult<PermitPersonalLock> {
    Ok(PermitPersonalLock {
        id: r.try_get("", "id")?,
        entity_sync_id: r.try_get("", "entity_sync_id")?,
        permit_id: r.try_get("", "permit_id")?,
        worker_id: r.try_get("", "worker_id")?,
        lock_number: r.try_get("", "lock_number")?,
        placed_by_id: r.try_get("", "placed_by_id")?,
        placed_by_name: r.try_get("", "placed_by_name")?,
        placed_at: r.try_get("", "placed_at")?,
        removed_by_id: r.try_get("", "removed_by_id")?,
        removed_by_name: r.try_get("", "removed_by_name")?,
        removed_at: r.try_get("", "removed_at")?,
        forced_removal: r.try_get::<i64>("", "forced_removal")? != 0,
        forced_reason: r.try_get("", "forced_reason")?,
        row_version: r.try_get("", "row_version")?,
    })
}

async fn worker_rows(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<PermitWorker>> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("{WORKER_SELECT} WHERE w.permit_id = ? ORDER BY (w.withdrawn_at IS NOT NULL), w.added_at, w.id"),
        [permit_id.into()],
    ))
    .await?
    .iter()
    .map(map_worker)
    .collect()
}

async fn worker_row(db: &impl ConnectionTrait, worker_id: i64) -> AppResult<PermitWorker> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{WORKER_SELECT} WHERE w.id = ?"),
            [worker_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitWorker".into(),
            id: worker_id.to_string(),
        })?;
    map_worker(&row)
}

async fn lock_rows(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<PermitPersonalLock>> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("{LOCK_SELECT} WHERE l.permit_id = ? ORDER BY l.placed_at DESC, l.id DESC"),
        [permit_id.into()],
    ))
    .await?
    .iter()
    .map(map_lock)
    .collect()
}

async fn lock_row(db: &impl ConnectionTrait, lock_id: i64) -> AppResult<PermitPersonalLock> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LOCK_SELECT} WHERE l.id = ?"),
            [lock_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitPersonalLock".into(),
            id: lock_id.to_string(),
        })?;
    map_lock(&row)
}

/// The permit a padlock is on (the commands check the rights at that permit's site).
pub async fn lock_permit_id(db: &impl ConnectionTrait, lock_id: i64) -> AppResult<i64> {
    Ok(lock_row(db, lock_id).await?.permit_id)
}

/// The permit a worker line is on.
pub async fn worker_permit_id(db: &impl ConnectionTrait, worker_id: i64) -> AppResult<i64> {
    Ok(worker_row(db, worker_id).await?.permit_id)
}

/// The name a refusal gives a worker: their card's name, or their name and company.
fn worker_label(w: &PermitWorker) -> String {
    match (&w.personnel_name, &w.external_name) {
        (Some(name), _) => name.clone(),
        (None, Some(name)) => match &w.company_name {
            Some(company) => format!("{name} ({company})"),
            None => name.clone(),
        },
        (None, None) => w.id.to_string(),
    }
}

/// What a listed worker lacks for the permit on `day`: the induction of external staff, and the
/// personal qualifications of the permit's type (a gap overridden on the linked work order is
/// covered).
async fn worker_blockers(
    db: &impl ConnectionTrait,
    permit: &PermitFacts,
    w: &PermitWorker,
    day: NaiveDate,
) -> AppResult<Vec<IssueBlocker>> {
    use crate::qualification::gate;
    let label = worker_label(w);
    let mut out = Vec::new();
    if w.is_external && w.induction_at.is_none() {
        out.push(IssueBlocker::new(
            CODE_INDUCTION_MISSING,
            &[("permit", permit.code.clone()), ("worker", label.clone())],
        ));
    }
    match w.personnel_id {
        Some(pid) => {
            let check =
                gate::check_person_for_permit_type(db, pid, permit.permit_type_id, permit.linked_wo_id, day).await?;
            if !check.qualified {
                for g in &check.gaps {
                    let expired = matches!(g.status.as_str(), "expired" | "expires_before_work");
                    out.push(IssueBlocker::new(
                        if expired {
                            CODE_QUALIFICATION_EXPIRED
                        } else {
                            CODE_QUALIFICATION_MISSING
                        },
                        &[
                            ("permit", permit.code.clone()),
                            ("worker", label.clone()),
                            ("qualification", g.label.clone()),
                            ("expires_at", g.expires_at.clone().unwrap_or_default()),
                        ],
                    ));
                }
            }
        }
        None => {
            let personal = gate::permit_type_requirements(db, permit.permit_type_id)
                .await?
                .iter()
                .any(|r| r.coverage == "each");
            if personal {
                out.push(IssueBlocker::new(
                    CODE_QUALIFICATION_UNVERIFIABLE,
                    &[
                        ("permit", permit.code.clone()),
                        ("worker", label.clone()),
                        ("company", w.company_name.clone().unwrap_or_default()),
                    ],
                ));
            }
        }
    }
    Ok(out)
}

/// What the workers of a permit lack for its issue today (L4-05, L4-06): empty when nothing.
pub async fn issue_blockers(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<IssueBlocker>> {
    let permit = permit_facts(db, permit_id).await?;
    let day = today();
    let mut out = Vec::new();
    for w in worker_rows(db, permit_id)
        .await?
        .iter()
        .filter(|w| w.withdrawn_at.is_none())
    {
        out.extend(worker_blockers(db, &permit, w, day).await?);
    }
    Ok(out)
}

/// The group lockout rule: no isolation lock comes off and the permit is not closed while a
/// personal padlock is still on its group lock box.
pub(crate) async fn ensure_no_personal_lock_on(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n, (SELECT code FROM work_permits WHERE id = ?) AS code FROM permit_personal_locks WHERE permit_id = ? AND removed_at IS NULL",
            [permit_id.into(), permit_id.into()],
        ))
        .await?;
    let (n, code) = match row {
        Some(r) => (
            r.try_get::<i64>("", "n")?,
            r.try_get::<Option<String>>("", "code")?.unwrap_or_default(),
        ),
        None => (0, String::new()),
    };
    if n == 0 {
        return Ok(());
    }
    Err(refused(
        CODE_PERSONAL_LOCKS_ON,
        format!(
            "Permit {code}: {n} personal padlock(s) still on the group lock box; every worker removes theirs first."
        ),
        &[("permit", code), ("count", n.to_string())],
    ))
}

/// The workers and padlocks of a permit, for `viewer_id` (the session user: `is_me`).
pub async fn list_permit_workers(
    db: &impl ConnectionTrait,
    permit_id: i64,
    viewer_id: i64,
) -> AppResult<PermitWorkersView> {
    let permit = permit_facts(db, permit_id).await?;
    let me = crate::qualification::gate::personnel_for_user(db, viewer_id).await?;
    let locks = lock_rows(db, permit_id).await?;
    let mut workers = worker_rows(db, permit_id).await?;
    let day = today();
    for w in &mut workers {
        w.is_me = me.is_some() && w.personnel_id == me;
        w.open_lock = locks
            .iter()
            .find(|l| l.worker_id == w.id && l.removed_at.is_none())
            .cloned();
        if w.withdrawn_at.is_none() {
            w.blockers = worker_blockers(db, &permit, w, day).await?;
        }
    }
    let open_lock_count = i64::try_from(locks.iter().filter(|l| l.removed_at.is_none()).count()).unwrap_or(i64::MAX);
    Ok(PermitWorkersView {
        permit_id,
        permit_status: permit.status,
        workers,
        locks,
        open_lock_count,
    })
}

fn checked_text(field: &str, value: &str, required_code: &str, max: usize) -> AppResult<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(refused(required_code, format!("{field} is required."), &[]));
    }
    if v.chars().count() > max {
        return Err(refused(
            "GATE_PERMIT_CONTENT_TOO_LONG",
            format!("{field} is longer than {max} characters."),
            &[("max", max.to_string())],
        ));
    }
    Ok(v.to_string())
}

/// An induction time: readable, not in the future (beyond the tolerated clock difference).
fn checked_induction_time(value: Option<&str>) -> AppResult<String> {
    let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(now_ts());
    };
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .filter(|d| {
            *d <= crate::clock::now() + chrono::Duration::minutes(crate::settings::site::MAX_CLOCK_SKEW_MINUTES)
        })
        .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .ok_or_else(|| {
            refused(
                "GATE_PERMIT_WORKER_INDUCTION_TIME_INVALID",
                "The time of the induction must be a readable date and time, not in the future.",
                &[],
            )
        })
}

async fn audit(
    db: &impl ConnectionTrait,
    action_code: &str,
    permit_id: i64,
    actor_id: i64,
    details: serde_json::Value,
) -> AppResult<()> {
    crate::audit::writer::write_audit_event_in(
        db,
        crate::audit::writer::AuditEventInput {
            action_code: action_code.to_string(),
            target_type: Some("work_permit".to_string()),
            target_id: Some(permit_id.to_string()),
            actor_id: Some(actor_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(details),
        },
    )
    .await?;
    Ok(())
}

/// Adds a worker to a permit (the session user adds them). Refused when the worker lacks a
/// qualification the permit's type requires (L4-06), and, on a permit already issued, when an
/// external worker comes without their induction.
pub async fn add_permit_worker(
    db: &DatabaseConnection,
    input: PermitWorkerAddInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let txn = db.begin().await?;
    let out = add_permit_worker_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn add_permit_worker_in(
    db: &impl ConnectionTrait,
    input: PermitWorkerAddInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let permit = permit_facts(db, input.permit_id).await?;
    ensure_status(
        &permit,
        WORKER_EDIT_STATUSES,
        "GATE_PERMIT_WORKER_WRONG_STATUS",
        "Changing the workers",
    )?;
    let role = checked_text(
        "role_on_permit",
        &input.role_on_permit,
        "GATE_PERMIT_WORKER_ROLE_REQUIRED",
        MAX_WORKER_TEXT_CHARS,
    )?;
    let external_name = input.external_name.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let (personnel_id, external_name, company_id) = match (input.personnel_id, external_name, input.external_company_id)
    {
        (Some(pid), None, None) => (Some(pid), None, None),
        (None, Some(name), Some(company)) => (
            None,
            Some(checked_text(
                "external_name",
                name,
                "GATE_PERMIT_WORKER_IDENTITY_REQUIRED",
                MAX_WORKER_TEXT_CHARS,
            )?),
            Some(company),
        ),
        _ => {
            return Err(refused(
                "GATE_PERMIT_WORKER_IDENTITY_REQUIRED",
                "Choose a person from the personnel list, or give the external worker's name and company.",
                &[],
            ))
        }
    };
    if let Some(pid) = personnel_id {
        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT full_name FROM personnel WHERE id = ?",
                [pid.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "Personnel".into(),
                id: pid.to_string(),
            })?;
        let name: String = exists.try_get("", "full_name")?;
        let listed = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS x FROM permit_workers WHERE permit_id = ? AND personnel_id = ? AND withdrawn_at IS NULL",
                [permit.id.into(), pid.into()],
            ))
            .await?
            .is_some();
        if listed {
            return Err(refused(
                "GATE_PERMIT_WORKER_ALREADY_LISTED",
                format!("{name} is already a worker of permit {}.", permit.code),
                &[("permit", permit.code.clone()), ("worker", name)],
            ));
        }
    }
    if let Some(company) = company_id {
        let active = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS x FROM external_companies WHERE id = ? AND is_active = 1",
                [company.into()],
            ))
            .await?
            .is_some();
        if !active {
            return Err(refused(
                "GATE_PERMIT_WORKER_COMPANY_UNKNOWN",
                "The company is not an active company of the contractor list.",
                &[],
            ));
        }
    }
    let induction_at = match input.induction_at.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(raw) => Some(checked_induction_time(Some(raw))?),
        None => None,
    };
    let now = now_ts();
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_workers (entity_sync_id, permit_id, personnel_id, external_name, external_company_id, role_on_permit, induction_at, induction_by_id, added_by_id, added_at, row_version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            sync_id.clone().into(),
            permit.id.into(),
            personnel_id.into(),
            external_name.clone().into(),
            company_id.into(),
            role.clone().into(),
            induction_at.clone().into(),
            induction_at.as_ref().map(|_| actor_id).into(),
            actor_id.into(),
            now.clone().into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let worker_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM permit_workers WHERE entity_sync_id = ?",
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("permit worker not found after insert")))?
        .try_get("", "id")?;
    let added = worker_row(db, worker_id).await?;
    // L4-06: refused now when a qualification is missing or expired; the induction of external
    // staff is required now only on a permit already issued (before, the issue asks for it).
    let blockers: Vec<IssueBlocker> = worker_blockers(db, &permit, &added, today())
        .await?
        .into_iter()
        .filter(|b| b.code != CODE_INDUCTION_MISSING || !PRE_ISSUE_STATUSES.contains(&permit.status.as_str()))
        .collect();
    if !blockers.is_empty() {
        return Err(blockers_error(blockers, "Worker refused"));
    }
    audit(
        db,
        "permit.worker_added",
        permit.id,
        actor_id,
        serde_json::json!({
            "code": permit.code,
            "worker": worker_label(&added),
            "personnel_id": personnel_id,
            "external_company_id": company_id,
            "role": role,
            "induction_at": induction_at,
        }),
    )
    .await?;
    list_permit_workers(db, permit.id, actor_id).await
}

/// Records the induction of an external worker, given by the session user.
pub async fn record_permit_worker_induction(
    db: &DatabaseConnection,
    input: PermitWorkerInductionInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let txn = db.begin().await?;
    let out = record_permit_worker_induction_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn record_permit_worker_induction_in(
    db: &impl ConnectionTrait,
    input: PermitWorkerInductionInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let w = worker_row(db, input.worker_id).await?;
    if w.row_version != input.expected_row_version {
        return Err(changed_elsewhere("permit_workers"));
    }
    let permit = permit_facts(db, w.permit_id).await?;
    ensure_status(
        &permit,
        WORKER_EDIT_STATUSES,
        "GATE_PERMIT_WORKER_WRONG_STATUS",
        "Recording an induction",
    )?;
    let label = worker_label(&w);
    if w.withdrawn_at.is_some() {
        return Err(refused(
            "GATE_PERMIT_WORKER_WITHDRAWN",
            format!("{label} is no longer a worker of permit {}.", permit.code),
            &[("permit", permit.code.clone()), ("worker", label)],
        ));
    }
    if !w.is_external {
        return Err(refused(
            "GATE_PERMIT_WORKER_INDUCTION_NOT_EXTERNAL",
            format!("{label} is internal staff: no induction is recorded on the permit."),
            &[("worker", label)],
        ));
    }
    if w.induction_at.is_some() {
        return Err(refused(
            "GATE_PERMIT_WORKER_INDUCTION_ALREADY_RECORDED",
            format!("The induction of {label} is already recorded."),
            &[("worker", label)],
        ));
    }
    let at = checked_induction_time(input.induction_at.as_deref())?;
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_workers SET induction_at = ?, induction_by_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ? AND induction_at IS NULL",
            [
                at.clone().into(),
                actor_id.into(),
                now_ts().into(),
                w.id.into(),
                w.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(changed_elsewhere("permit_workers"));
    }
    audit(
        db,
        "permit.worker_inducted",
        permit.id,
        actor_id,
        serde_json::json!({ "code": permit.code, "worker": label, "induction_at": at }),
    )
    .await?;
    list_permit_workers(db, permit.id, actor_id).await
}

/// Withdraws a worker from a permit (kept as history), not while their padlock is on the box.
pub async fn withdraw_permit_worker(
    db: &DatabaseConnection,
    input: PermitWorkerWithdrawInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let txn = db.begin().await?;
    let out = withdraw_permit_worker_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn withdraw_permit_worker_in(
    db: &impl ConnectionTrait,
    input: PermitWorkerWithdrawInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let w = worker_row(db, input.worker_id).await?;
    if w.row_version != input.expected_row_version {
        return Err(changed_elsewhere("permit_workers"));
    }
    let permit = permit_facts(db, w.permit_id).await?;
    ensure_status(
        &permit,
        WORKER_EDIT_STATUSES,
        "GATE_PERMIT_WORKER_WRONG_STATUS",
        "Changing the workers",
    )?;
    let label = worker_label(&w);
    if w.withdrawn_at.is_some() {
        return Err(refused(
            "GATE_PERMIT_WORKER_WITHDRAWN",
            format!("{label} is no longer a worker of permit {}.", permit.code),
            &[("permit", permit.code.clone()), ("worker", label)],
        ));
    }
    let lock_on = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM permit_personal_locks WHERE worker_id = ? AND removed_at IS NULL",
            [w.id.into()],
        ))
        .await?
        .is_some();
    if lock_on {
        return Err(refused(
            "GATE_PERMIT_WORKER_LOCK_ON",
            format!(
                "{label}'s padlock is still on the group lock box of permit {}.",
                permit.code
            ),
            &[("permit", permit.code.clone()), ("worker", label)],
        ));
    }
    let now = now_ts();
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_workers SET withdrawn_at = ?, withdrawn_by_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ? AND withdrawn_at IS NULL",
            [
                now.clone().into(),
                actor_id.into(),
                now.into(),
                w.id.into(),
                w.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(changed_elsewhere("permit_workers"));
    }
    audit(
        db,
        "permit.worker_withdrawn",
        permit.id,
        actor_id,
        serde_json::json!({ "code": permit.code, "worker": label }),
    )
    .await?;
    list_permit_workers(db, permit.id, actor_id).await
}

/// The session user's worker line on the permit (their card, listed, not withdrawn).
async fn my_worker(db: &impl ConnectionTrait, permit: &PermitFacts, actor_id: i64) -> AppResult<PermitWorker> {
    let me = crate::qualification::gate::personnel_for_user(db, actor_id).await?;
    let not_a_worker = || {
        refused(
            CODE_LOCK_NOT_A_WORKER,
            format!("You are not listed as a worker of permit {}.", permit.code),
            &[("permit", permit.code.clone())],
        )
    };
    let Some(pid) = me else {
        return Err(not_a_worker());
    };
    worker_rows(db, permit.id)
        .await?
        .into_iter()
        .find(|w| w.personnel_id == Some(pid) && w.withdrawn_at.is_none())
        .ok_or_else(not_a_worker)
}

/// "Je pose mon cadenas" (L4-18): the session user places their own padlock on the group lock box
/// of a permit they are listed on, issued or active, while they still hold what the permit
/// requires (L4-06) and their induction when they are external staff.
pub async fn place_my_personal_lock(
    db: &DatabaseConnection,
    input: PersonalLockPlaceInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let txn = db.begin().await?;
    let out = place_my_personal_lock_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn place_my_personal_lock_in(
    db: &impl ConnectionTrait,
    input: PersonalLockPlaceInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let permit = permit_facts(db, input.permit_id).await?;
    let worker = my_worker(db, &permit, actor_id).await?;
    ensure_status(
        &permit,
        LOCK_PLACE_STATUSES,
        "GATE_PERMIT_PERSONAL_LOCK_WRONG_STATUS",
        "Placing a padlock",
    )?;
    let blockers = worker_blockers(db, &permit, &worker, today()).await?;
    if !blockers.is_empty() {
        return Err(blockers_error(blockers, "Padlock refused"));
    }
    let lock_number = checked_text(
        "lock_number",
        &input.lock_number,
        "GATE_PERMIT_PERSONAL_LOCK_NUMBER_REQUIRED",
        MAX_LOCK_NUMBER_CHARS,
    )?;
    let already = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lock_number FROM permit_personal_locks WHERE worker_id = ? AND removed_at IS NULL",
            [worker.id.into()],
        ))
        .await?;
    if let Some(r) = already {
        let number: String = r.try_get("", "lock_number")?;
        return Err(refused(
            "GATE_PERMIT_PERSONAL_LOCK_ALREADY_ON",
            format!(
                "Your padlock {number} is already on the group lock box of permit {}.",
                permit.code
            ),
            &[("permit", permit.code.clone()), ("lock", number)],
        ));
    }
    let now = now_ts();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_personal_locks (entity_sync_id, permit_id, worker_id, lock_number, placed_by_id, placed_at, forced_removal, row_version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, 1, ?, ?)",
        [
            Uuid::new_v4().to_string().into(),
            permit.id.into(),
            worker.id.into(),
            lock_number.clone().into(),
            actor_id.into(),
            now.clone().into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    audit(
        db,
        "permit.personal_lock_placed",
        permit.id,
        actor_id,
        serde_json::json!({ "code": permit.code, "worker": worker_label(&worker), "lock_number": lock_number }),
    )
    .await?;
    list_permit_workers(db, permit.id, actor_id).await
}

/// "J'ai retiré mon cadenas" (L4-18): only the padlock's owner, listed on the permit, takes it off,
/// once, while the permit is in use or ended.
pub async fn remove_my_personal_lock(
    db: &DatabaseConnection,
    input: PersonalLockRemoveInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let txn = db.begin().await?;
    let out = remove_my_personal_lock_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn remove_my_personal_lock_in(
    db: &impl ConnectionTrait,
    input: PersonalLockRemoveInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let lock = lock_row(db, input.lock_id).await?;
    let permit = permit_facts(db, lock.permit_id).await?;
    let worker = worker_row(db, lock.worker_id).await?;
    let me = crate::qualification::gate::personnel_for_user(db, actor_id).await?;
    // The owner: the worker of the padlock is the session user's card, and they placed it.
    if me.is_none() || worker.personnel_id != me || lock.placed_by_id != actor_id {
        return Err(refused(
            CODE_LOCK_NOT_YOURS,
            format!(
                "Padlock {} is not yours: only its owner removes it (or a forced removal when the owner is absent).",
                lock.lock_number
            ),
            &[("permit", permit.code.clone()), ("lock", lock.lock_number.clone())],
        ));
    }
    if lock.row_version != input.expected_row_version {
        return Err(changed_elsewhere("permit_personal_locks"));
    }
    if lock.removed_at.is_some() {
        return Err(refused(
            "GATE_PERMIT_PERSONAL_LOCK_ALREADY_REMOVED",
            format!("Padlock {} is already off the group lock box.", lock.lock_number),
            &[("lock", lock.lock_number.clone())],
        ));
    }
    ensure_status(
        &permit,
        LOCK_REMOVE_STATUSES,
        "GATE_PERMIT_PERSONAL_LOCK_WRONG_STATUS",
        "Removing a padlock",
    )?;
    let now = now_ts();
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_personal_locks SET removed_by_id = ?, removed_at = ?, row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ? AND removed_at IS NULL",
            [
                actor_id.into(),
                now.clone().into(),
                now.into(),
                lock.id.into(),
                lock.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(changed_elsewhere("permit_personal_locks"));
    }
    audit(
        db,
        "permit.personal_lock_removed",
        permit.id,
        actor_id,
        serde_json::json!({ "code": permit.code, "worker": worker_label(&worker), "lock_number": lock.lock_number }),
    )
    .await?;
    list_permit_workers(db, permit.id, actor_id).await
}

/// A padlock whose owner is absent, removed on the authority of a `ptw.close` holder (the session
/// user, checked with a step-up by the command): never the owner themselves, with a reason and the
/// attestation that the worker is not on site.
pub async fn force_remove_personal_lock(
    db: &DatabaseConnection,
    input: PersonalLockForceRemoveInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    let txn = db.begin().await?;
    let out = force_remove_personal_lock_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn force_remove_personal_lock_in(
    db: &impl ConnectionTrait,
    input: PersonalLockForceRemoveInput,
    actor_id: i64,
) -> AppResult<PermitWorkersView> {
    use crate::permit::domain::MIN_FORCED_REMOVAL_REASON_CHARS;
    let lock = lock_row(db, input.lock_id).await?;
    if lock.row_version != input.expected_row_version {
        return Err(changed_elsewhere("permit_personal_locks"));
    }
    let permit = permit_facts(db, lock.permit_id).await?;
    let worker = worker_row(db, lock.worker_id).await?;
    if lock.removed_at.is_some() {
        return Err(refused(
            "GATE_PERMIT_PERSONAL_LOCK_ALREADY_REMOVED",
            format!("Padlock {} is already off the group lock box.", lock.lock_number),
            &[("lock", lock.lock_number.clone())],
        ));
    }
    let me = crate::qualification::gate::personnel_for_user(db, actor_id).await?;
    if lock.placed_by_id == actor_id || (me.is_some() && worker.personnel_id == me) {
        return Err(refused(
            "GATE_PERMIT_PERSONAL_LOCK_FORCE_OWN",
            "This padlock is yours: remove it yourself.",
            &[("lock", lock.lock_number.clone())],
        ));
    }
    let reason = input.reason.trim().to_string();
    if reason.chars().count() < MIN_FORCED_REMOVAL_REASON_CHARS {
        return Err(refused(
            "GATE_PERMIT_PERSONAL_LOCK_FORCE_REASON_TOO_SHORT",
            format!("Explain the forced removal (at least {MIN_FORCED_REMOVAL_REASON_CHARS} characters)."),
            &[("min", MIN_FORCED_REMOVAL_REASON_CHARS.to_string())],
        ));
    }
    if !input.worker_absent_attested {
        return Err(refused(
            "GATE_PERMIT_PERSONAL_LOCK_FORCE_ATTESTATION_REQUIRED",
            "Attest that the worker is not on site.",
            &[],
        ));
    }
    ensure_status(
        &permit,
        LOCK_REMOVE_STATUSES,
        "GATE_PERMIT_PERSONAL_LOCK_WRONG_STATUS",
        "Removing a padlock",
    )?;
    let now = now_ts();
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_personal_locks SET removed_by_id = ?, removed_at = ?, forced_removal = 1, forced_reason = ?, row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ? AND removed_at IS NULL",
            [
                actor_id.into(),
                now.clone().into(),
                reason.clone().into(),
                now.into(),
                lock.id.into(),
                lock.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(changed_elsewhere("permit_personal_locks"));
    }
    audit(
        db,
        "permit.personal_lock_forced_removal",
        permit.id,
        actor_id,
        serde_json::json!({
            "code": permit.code,
            "worker": worker_label(&worker),
            "lock_number": lock.lock_number,
            "reason": reason,
            "worker_absent_attested": true,
        }),
    )
    .await?;
    list_permit_workers(db, permit.id, actor_id).await
}

/// Issue checks of a permit issued on another PC (`permit::sync_check::issue_check`): its workers as
/// received (`workers`: payloads of `permit_workers`), judged on the issue day. Returns the first
/// breach as `(conflict code, message)`; `Ok(None)` when they keep the rules, `Err` only on a read
/// failure. A worker naming a card or company not here yet makes the caller wait (`missing`).
pub(crate) async fn received_workers_breach(
    conn: &impl ConnectionTrait,
    code: &str,
    permit_type_id: i64,
    linked_wo_id: Option<i64>,
    issued_on: NaiveDate,
    workers: &[&serde_json::Map<String, serde_json::Value>],
) -> AppResult<Option<(&'static str, String)>> {
    use crate::permit::sync_check::field;
    use crate::qualification::gate;
    let mut personal: Option<bool> = None;
    for w in workers.iter().filter(|w| field(w, "withdrawn_at").is_none()) {
        let name = field(w, "external_name").map(str::to_string);
        let personnel = match field(w, "personnel_sync_id") {
            Some(sid) => conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, full_name, employment_origin FROM personnel WHERE entity_sync_id = ?",
                    [sid.into()],
                ))
                .await?
                .map(|r| -> AppResult<(i64, String, String)> {
                    Ok((
                        r.try_get("", "id")?,
                        r.try_get("", "full_name")?,
                        r.try_get::<Option<String>>("", "employment_origin")?
                            .unwrap_or_default(),
                    ))
                })
                .transpose()?,
            None => None,
        };
        let label = personnel
            .as_ref()
            .map(|(_, n, _)| n.clone())
            .or(name)
            .unwrap_or_else(|| "?".to_string());
        let external = personnel.as_ref().is_none_or(|(_, _, origin)| origin == "external");
        if external && field(w, "induction_at").is_none() {
            return Ok(Some((
                "PERMIT_ISSUE_WORKER_INDUCTION_MISSING",
                format!("permit {code} was issued with external worker {label} not inducted"),
            )));
        }
        match personnel {
            Some((pid, _, _)) => {
                let check =
                    gate::check_person_for_permit_type(conn, pid, permit_type_id, linked_wo_id, issued_on).await?;
                if !check.qualified {
                    let what: Vec<String> = check
                        .gaps
                        .iter()
                        .map(|g| format!("{} ({})", g.label, g.status))
                        .collect();
                    return Ok(Some((
                        "PERMIT_ISSUE_WORKER_QUALIFICATION",
                        format!(
                            "permit {code} was issued with worker {label} lacking, on {issued_on}: {}",
                            what.join(", ")
                        ),
                    )));
                }
            }
            None => {
                let needs = match personal {
                    Some(v) => v,
                    None => {
                        let v = gate::permit_type_requirements(conn, permit_type_id)
                            .await?
                            .iter()
                            .any(|r| r.coverage == "each");
                        personal = Some(v);
                        v
                    }
                };
                if needs {
                    return Ok(Some((
                        "PERMIT_ISSUE_WORKER_QUALIFICATION",
                        format!(
                            "permit {code} was issued with external worker {label} whose qualifications cannot be checked"
                        ),
                    )));
                }
            }
        }
    }
    Ok(None)
}
