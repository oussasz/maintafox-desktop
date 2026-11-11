//! What a work permit says before it is issued, its HSE approval and its gas tests (L4-03, L4-04,
//! migration 290).
//!
//! - **Content** (`work_permits.work_description`, `risk_assessment`, `preventive_measures`,
//!   `ppe_required`): written while the permit is a draft or waiting for review. A change while it
//!   waits for review sends it back to draft (the review judged the former content), and any HSE
//!   approval of the former content is cleared. The PPE are free text: no PPE list exists in
//!   Données de référence.
//! - **HSE approval** (`hse_approved_by_id`, `hse_approved_at`, `hse_approval_note`): recorded by
//!   the session user, on a permit waiting for review or approved, once its content is complete.
//! - **Gas tests** (`permit_gas_tests`): O2 %, LEL %, H2S ppm and optionally CO ppm, measured at a
//!   time, by the session user, with an instrument. Append-only. The acceptance limits are settings
//!   of the catalog (`permit.gas_*`, shipped with the usual confined-space limits), read when the
//!   readings are judged: never constants.
//!
//! **Issue gate** ([`issue_blockers`], [`issue_ready`]): `approved → issued` (the only transition
//! into `issued`, `permit::queries::set_work_permit_status`) is refused while the content is
//! incomplete, while a type requiring HSE approval has none, while a permit needing a gas test
//! (type flagged "test atmosphérique", or the shipped confined-space type) has no valid reading, or
//! while a worker lacks an induction or a qualification (`permit::workers`, L4-05 / L4-06). A
//! permit issued on another PC is judged on the same rules on arrival (`permit::sync_check::
//! issue_check`). The screen shows the same list before the click.
//!
//! **Gas test validity** (CMMS Advisor 2026-11-11, review of L4-04): a reading counts only when it
//! was measured after the permit was raised and after the last reset of its safety record
//! (`safety_content_since`: a return to draft, a change of equipment or site). The latest counting
//! reading (by its instant) must be within the limits and younger than the re-test interval
//! (`permit.gas_test_validity_hours`, shipped 4 h) at the issue, at the activation, at the
//! reinstatement of a suspended permit and at each start or resume of the linked work order
//! (`gas_ready`, called by `permit::queries::activation_ready`): a break longer than the interval
//! needs a new test.
//!
//! **HSE approval** (CMMS Advisor 2026-11-11): recorded with `ptw.hse_approve` by a person other than
//! the permit's author and submitter (the four-eyes rule of the approval,
//! `permit::queries::approver_is_second_person`); on a site that opted out of four-eyes the author
//! may record it, flagged `hse_self_approved`. It may be the person who approved the permit.

use std::collections::BTreeMap;

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::{AppError, AppResult, AppValidationIssue};

/// Settings of the catalog holding the gas-test acceptance limits (`settings::catalog`).
pub const SETTING_GAS_O2_MIN: &str = "permit.gas_o2_min_percent";
pub const SETTING_GAS_O2_MAX: &str = "permit.gas_o2_max_percent";
pub const SETTING_GAS_LEL_MAX: &str = "permit.gas_lel_max_percent";
pub const SETTING_GAS_H2S_MAX: &str = "permit.gas_h2s_max_ppm";
pub const SETTING_GAS_CO_MAX: &str = "permit.gas_co_max_ppm";
/// How long a gas test stays valid (hours): the re-test interval.
pub const SETTING_GAS_VALIDITY_HOURS: &str = "permit.gas_test_validity_hours";

/// Longest text of one content field.
pub const MAX_CONTENT_CHARS: usize = 4000;
/// Longest HSE approval comment.
pub const MAX_HSE_NOTE_CHARS: usize = 1000;
/// Longest instrument reference of a gas test.
pub const MAX_INSTRUMENT_CHARS: usize = 200;

/// Sync identity of the shipped confined-space permit type (migration 67): its permits always need
/// a gas test, whatever its "test atmosphérique" flag says.
const SHIPPED_CONFINED_SPACE_TYPE_SYNC_ID: &str = "33333333-3333-4333-8333-333333333333";
const SHIPPED_CONFINED_SPACE_TYPE_CODE: &str = "confined_space";

/// Statuses in which the content may be written (a change in `pending_review` returns to draft).
pub const CONTENT_EDIT_STATUSES: &[&str] = &["draft", "pending_review"];
/// Statuses in which the HSE approval may be recorded.
pub const HSE_APPROVAL_STATUSES: &[&str] = &["pending_review", "approved"];
/// Statuses in which a gas test may be recorded: before the issue, and while the permit is in use
/// (periodic re-tests).
pub const GAS_TEST_STATUSES: &[&str] = &[
    "draft",
    "pending_review",
    "approved",
    "issued",
    "active",
    "suspended",
    "revalidation_required",
];

pub const CODE_WORK_DESCRIPTION_MISSING: &str = "GATE_PERMIT_WORK_DESCRIPTION_MISSING";
pub const CODE_RISK_ASSESSMENT_MISSING: &str = "GATE_PERMIT_RISK_ASSESSMENT_MISSING";
pub const CODE_PREVENTIVE_MEASURES_MISSING: &str = "GATE_PERMIT_PREVENTIVE_MEASURES_MISSING";
pub const CODE_PPE_MISSING: &str = "GATE_PERMIT_PPE_MISSING";
pub const CODE_HSE_APPROVAL_MISSING: &str = "GATE_PERMIT_HSE_APPROVAL_MISSING";
pub const CODE_GAS_TEST_MISSING: &str = "GATE_PERMIT_GAS_TEST_MISSING";
pub const CODE_GAS_O2_OUT_OF_LIMITS: &str = "GATE_PERMIT_GAS_O2_OUT_OF_LIMITS";
pub const CODE_GAS_LEL_TOO_HIGH: &str = "GATE_PERMIT_GAS_LEL_TOO_HIGH";
pub const CODE_GAS_H2S_TOO_HIGH: &str = "GATE_PERMIT_GAS_H2S_TOO_HIGH";
pub const CODE_GAS_CO_TOO_HIGH: &str = "GATE_PERMIT_GAS_CO_TOO_HIGH";
pub const CODE_GAS_TEST_EXPIRED: &str = "GATE_PERMIT_GAS_TEST_EXPIRED";
pub const CODE_GAS_TEST_BEFORE_PERMIT: &str = "GATE_PERMIT_GAS_TEST_BEFORE_PERMIT";

/// The acceptance limits of a gas test, read from the settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GasLimits {
    /// Inclusive bounds of the oxygen reading (%).
    pub o2_min_percent: f64,
    pub o2_max_percent: f64,
    /// A flammable-gas reading (% LEL) must stay below it.
    pub lel_max_percent: f64,
    /// An H2S reading (ppm) must stay below it.
    pub h2s_max_ppm: f64,
    /// A CO reading (ppm), when measured, must stay below it.
    pub co_max_ppm: f64,
}

/// One refusal of the issue gate: a `GATE_` code and its parameters (the screen translates it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueBlocker {
    pub code: String,
    pub params: BTreeMap<String, String>,
}

impl IssueBlocker {
    pub(crate) fn new(code: &str, params: &[(&str, String)]) -> Self {
        Self {
            code: code.to_string(),
            params: params.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitGasTest {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub o2_percent: f64,
    pub lel_percent: f64,
    pub h2s_ppm: f64,
    pub co_ppm: Option<f64>,
    pub measured_at: String,
    pub measured_by_id: i64,
    pub measured_by_name: Option<String>,
    pub instrument: String,
    pub row_version: i64,
    /// Judged against the current limits (never stored with the reading).
    pub within_limits: bool,
    /// Measured after the permit was raised and after the last reset of its safety record: only
    /// such a reading counts for the gates.
    pub counts: bool,
}

/// The safety content of a permit as the permit screen shows it, with what still blocks its issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitSafetyContent {
    pub permit_id: i64,
    pub permit_status: String,
    pub permit_row_version: i64,
    pub work_description: Option<String>,
    pub risk_assessment: Option<String>,
    pub preventive_measures: Option<String>,
    pub ppe_required: Option<String>,
    /// The permit's type requires an HSE approval ("Approbation HSE").
    pub requires_hse_approval: bool,
    /// The permit needs a gas test ("Test atmosphérique", or the confined-space type).
    pub requires_gas_test: bool,
    pub hse_approved_by_id: Option<i64>,
    pub hse_approved_by_name: Option<String>,
    pub hse_approved_at: Option<String>,
    pub hse_approval_note: Option<String>,
    /// The HSE approval was recorded by the permit's author or submitter on a site that opted out of
    /// four-eyes. `None`: no approval, or recorded before this was kept.
    pub hse_self_approved: Option<bool>,
    pub gas_limits: GasLimits,
    /// The re-test interval of the settings (hours).
    pub gas_validity_hours: f64,
    /// Latest first.
    pub gas_tests: Vec<PermitGasTest>,
    /// Why the permit cannot be issued now (empty: nothing in its content blocks it).
    pub issue_blockers: Vec<IssueBlocker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitSafetyContentInput {
    pub permit_id: i64,
    pub expected_row_version: i64,
    pub work_description: String,
    pub risk_assessment: String,
    pub preventive_measures: String,
    pub ppe_required: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitHseApprovalInput {
    pub permit_id: i64,
    pub expected_row_version: i64,
    #[serde(default)]
    pub note: Option<String>,
}

/// A gas test; the person who measured is the session user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitGasTestInput {
    pub permit_id: i64,
    pub o2_percent: f64,
    pub lel_percent: f64,
    pub h2s_ppm: f64,
    #[serde(default)]
    pub co_ppm: Option<f64>,
    /// RFC 3339; not in the future (beyond the tolerated clock difference).
    pub measured_at: String,
    pub instrument: String,
}

fn refused(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(code, message, crate::errors::issue_params(params))
}

fn changed_elsewhere() -> AppError {
    AppError::ValidationFailed(vec![
        "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on work_permits.".into(),
    ])
}

fn now_ts() -> String {
    crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn non_blank(v: Option<&str>) -> bool {
    v.is_some_and(|s| !s.trim().is_empty())
}

/// Whether permits of this type need a gas test before their issue: the type's "test
/// atmosphérique" flag, or the shipped confined-space type (by its sync identity or its code, which
/// never changes).
pub(crate) fn needs_gas_test(flag: bool, type_sync_id: &str, type_code: &str) -> bool {
    flag || type_sync_id == SHIPPED_CONFINED_SPACE_TYPE_SYNC_ID
        || type_code.trim().eq_ignore_ascii_case(SHIPPED_CONFINED_SPACE_TYPE_CODE)
}

/// The gas-test limits of the settings. A missing or out-of-bounds setting is an error (nothing is
/// judged on a guessed limit); limits that leave no oxygen reading acceptable are refused.
pub async fn gas_limits(db: &impl ConnectionTrait) -> AppResult<GasLimits> {
    use crate::settings::catalog::decimal_setting;
    let limits = GasLimits {
        o2_min_percent: decimal_setting(db, SETTING_GAS_O2_MIN).await?,
        o2_max_percent: decimal_setting(db, SETTING_GAS_O2_MAX).await?,
        lel_max_percent: decimal_setting(db, SETTING_GAS_LEL_MAX).await?,
        h2s_max_ppm: decimal_setting(db, SETTING_GAS_H2S_MAX).await?,
        co_max_ppm: decimal_setting(db, SETTING_GAS_CO_MAX).await?,
    };
    if limits.o2_min_percent >= limits.o2_max_percent {
        return Err(refused(
            "GATE_PERMIT_GAS_LIMITS_INVALID",
            "The oxygen limits of the settings leave no reading acceptable.",
            &[
                ("min", limits.o2_min_percent.to_string()),
                ("max", limits.o2_max_percent.to_string()),
            ],
        ));
    }
    Ok(limits)
}

/// The limits a reading breaks (empty: within limits). O2 within its bounds (inclusive); LEL, H2S
/// and CO (when measured) below their limit.
pub fn reading_breaches(
    limits: &GasLimits,
    o2: f64,
    lel: f64,
    h2s: f64,
    co: Option<f64>,
    permit_code: &str,
) -> Vec<IssueBlocker> {
    let p = || ("permit", permit_code.to_string());
    let mut out = Vec::new();
    if !(limits.o2_min_percent..=limits.o2_max_percent).contains(&o2) {
        out.push(IssueBlocker::new(
            CODE_GAS_O2_OUT_OF_LIMITS,
            &[
                p(),
                ("value", o2.to_string()),
                ("min", limits.o2_min_percent.to_string()),
                ("max", limits.o2_max_percent.to_string()),
            ],
        ));
    }
    if lel.is_nan() || lel >= limits.lel_max_percent {
        out.push(IssueBlocker::new(
            CODE_GAS_LEL_TOO_HIGH,
            &[
                p(),
                ("value", lel.to_string()),
                ("max", limits.lel_max_percent.to_string()),
            ],
        ));
    }
    if h2s.is_nan() || h2s >= limits.h2s_max_ppm {
        out.push(IssueBlocker::new(
            CODE_GAS_H2S_TOO_HIGH,
            &[p(), ("value", h2s.to_string()), ("max", limits.h2s_max_ppm.to_string())],
        ));
    }
    if let Some(co) = co.filter(|c| c.is_nan() || *c >= limits.co_max_ppm) {
        out.push(IssueBlocker::new(
            CODE_GAS_CO_TOO_HIGH,
            &[p(), ("value", co.to_string()), ("max", limits.co_max_ppm.to_string())],
        ));
    }
    out
}

/// The content blockers of a permit (each field not blank).
pub fn content_blockers(
    permit_code: &str,
    work_description: Option<&str>,
    risk_assessment: Option<&str>,
    preventive_measures: Option<&str>,
    ppe_required: Option<&str>,
) -> Vec<IssueBlocker> {
    [
        (work_description, CODE_WORK_DESCRIPTION_MISSING),
        (risk_assessment, CODE_RISK_ASSESSMENT_MISSING),
        (preventive_measures, CODE_PREVENTIVE_MEASURES_MISSING),
        (ppe_required, CODE_PPE_MISSING),
    ]
    .into_iter()
    .filter(|(v, _)| !non_blank(*v))
    .map(|(_, code)| IssueBlocker::new(code, &[("permit", permit_code.to_string())]))
    .collect()
}

/// The permit's row as the gate reads it.
struct PermitRow {
    id: i64,
    code: String,
    status: String,
    row_version: i64,
    entity_id: i64,
    created_by_id: Option<i64>,
    submitted_by_id: Option<i64>,
    requested_at: Option<String>,
    safety_reset_at: Option<String>,
    hse_self_approved: Option<bool>,
    work_description: Option<String>,
    risk_assessment: Option<String>,
    preventive_measures: Option<String>,
    ppe_required: Option<String>,
    hse_approved_by_id: Option<i64>,
    hse_approved_by_name: Option<String>,
    hse_approved_at: Option<String>,
    hse_approval_note: Option<String>,
    requires_hse_approval: bool,
    requires_gas_test: bool,
}

async fn permit_row(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<PermitRow> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wp.id, wp.code, wp.status, wp.row_version, wp.entity_id, wp.created_by_id, wp.submitted_by_id, wp.requested_at, wp.safety_reset_at, wp.hse_self_approved, wp.work_description, wp.risk_assessment, wp.preventive_measures, wp.ppe_required, wp.hse_approved_by_id, COALESCE(u.display_name, u.username) AS hse_approved_by_name, wp.hse_approved_at, wp.hse_approval_note, COALESCE(pt.requires_hse_approval, 0) AS requires_hse_approval, COALESCE(pt.requires_atmospheric_test, 0) AS requires_atmospheric_test, COALESCE(pt.entity_sync_id, '') AS type_sync_id, COALESCE(pt.code, '') AS type_code FROM work_permits wp LEFT JOIN permit_types pt ON pt.id = wp.permit_type_id LEFT JOIN user_accounts u ON u.id = wp.hse_approved_by_id WHERE wp.id = ?",
            [permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: permit_id.to_string(),
        })?;
    let atmospheric: i64 = row.try_get("", "requires_atmospheric_test")?;
    let type_sync_id: String = row.try_get("", "type_sync_id")?;
    let type_code: String = row.try_get("", "type_code")?;
    Ok(PermitRow {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        status: row.try_get("", "status")?,
        row_version: row.try_get("", "row_version")?,
        entity_id: row.try_get("", "entity_id")?,
        created_by_id: row.try_get("", "created_by_id")?,
        submitted_by_id: row.try_get("", "submitted_by_id")?,
        requested_at: row.try_get("", "requested_at")?,
        safety_reset_at: row.try_get("", "safety_reset_at")?,
        hse_self_approved: row.try_get::<Option<i64>>("", "hse_self_approved")?.map(|v| v != 0),
        work_description: row.try_get("", "work_description")?,
        risk_assessment: row.try_get("", "risk_assessment")?,
        preventive_measures: row.try_get("", "preventive_measures")?,
        ppe_required: row.try_get("", "ppe_required")?,
        hse_approved_by_id: row.try_get("", "hse_approved_by_id")?,
        hse_approved_by_name: row.try_get("", "hse_approved_by_name")?,
        hse_approved_at: row.try_get("", "hse_approved_at")?,
        hse_approval_note: row.try_get("", "hse_approval_note")?,
        requires_hse_approval: row.try_get::<i64>("", "requires_hse_approval")? != 0,
        requires_gas_test: needs_gas_test(atmospheric != 0, &type_sync_id, &type_code),
    })
}

/// From when a gas test counts: the later of the permit's request and the last reset of its safety
/// record. `None` when neither is readable (every reading counts, as before they were recorded).
pub(crate) fn safety_content_since(
    requested_at: Option<&str>,
    safety_reset_at: Option<&str>,
) -> Option<chrono::DateTime<Utc>> {
    let read = |v: Option<&str>| v.and_then(crate::settings::site::instant);
    match (read(requested_at), read(safety_reset_at)) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// The re-test interval of the settings (hours); missing or out of bounds is an error.
pub async fn gas_validity_hours(db: &impl ConnectionTrait) -> AppResult<f64> {
    crate::settings::catalog::decimal_setting(db, SETTING_GAS_VALIDITY_HOURS).await
}

/// The longest age of a valid gas test.
#[allow(clippy::cast_possible_truncation)] // bounded by the setting (at most 24 h)
pub(crate) fn validity_duration(validity_hours: f64) -> chrono::Duration {
    chrono::Duration::seconds((validity_hours * 3600.0).round() as i64)
}

/// The gas part of the gates, at `at`: the latest counting reading exists, is younger than the
/// re-test interval and within the limits. `tests`: latest first.
pub(crate) fn gas_blockers(
    permit_code: &str,
    limits: &GasLimits,
    validity_hours: f64,
    tests: &[PermitGasTest],
    at: chrono::DateTime<Utc>,
) -> Vec<IssueBlocker> {
    let Some(t) = tests.iter().find(|t| t.counts) else {
        return vec![IssueBlocker::new(
            CODE_GAS_TEST_MISSING,
            &[("permit", permit_code.to_string())],
        )];
    };
    let measured = crate::settings::site::instant(&t.measured_at);
    if measured.is_none_or(|m| at - m > validity_duration(validity_hours)) {
        return vec![IssueBlocker::new(
            CODE_GAS_TEST_EXPIRED,
            &[
                ("permit", permit_code.to_string()),
                ("measured_at", t.measured_at.clone()),
                ("hours", validity_hours.to_string()),
            ],
        )];
    }
    reading_breaches(limits, t.o2_percent, t.lel_percent, t.h2s_ppm, t.co_ppm, permit_code)
}

/// The gas tests of a permit, latest first (by instant), judged against `limits`; `since`: from
/// when a reading counts ([`safety_content_since`]).
pub async fn list_permit_gas_tests(
    db: &impl ConnectionTrait,
    permit_id: i64,
    limits: &GasLimits,
    since: Option<chrono::DateTime<Utc>>,
) -> AppResult<Vec<PermitGasTest>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT g.id, g.entity_sync_id, g.permit_id, g.o2_percent, g.lel_percent, g.h2s_ppm, g.co_ppm, g.measured_at, g.measured_by_id, COALESCE(u.display_name, u.username) AS measured_by_name, g.instrument, g.row_version FROM permit_gas_tests g LEFT JOIN user_accounts u ON u.id = g.measured_by_id WHERE g.permit_id = ? ORDER BY julianday(g.measured_at) DESC, g.id DESC",
            [permit_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let o2: f64 = r.try_get("", "o2_percent")?;
            let lel: f64 = r.try_get("", "lel_percent")?;
            let h2s: f64 = r.try_get("", "h2s_ppm")?;
            let co: Option<f64> = r.try_get("", "co_ppm")?;
            let measured_at: String = r.try_get("", "measured_at")?;
            let counts = match since {
                None => true,
                Some(s) => crate::settings::site::instant(&measured_at).is_some_and(|m| m >= s),
            };
            Ok(PermitGasTest {
                id: r.try_get("", "id")?,
                entity_sync_id: r.try_get("", "entity_sync_id")?,
                permit_id: r.try_get("", "permit_id")?,
                o2_percent: o2,
                lel_percent: lel,
                h2s_ppm: h2s,
                co_ppm: co,
                measured_at,
                measured_by_id: r.try_get("", "measured_by_id")?,
                measured_by_name: r.try_get("", "measured_by_name")?,
                instrument: r.try_get("", "instrument")?,
                row_version: r.try_get("", "row_version")?,
                within_limits: reading_breaches(limits, o2, lel, h2s, co, "").is_empty(),
                counts,
            })
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()
        .map_err(AppError::from)
}

impl PermitRow {
    fn since(&self) -> Option<chrono::DateTime<Utc>> {
        safety_content_since(self.requested_at.as_deref(), self.safety_reset_at.as_deref())
    }
}

fn blockers_of(row: &PermitRow, limits: &GasLimits, validity_hours: f64, tests: &[PermitGasTest]) -> Vec<IssueBlocker> {
    let mut out = content_blockers(
        &row.code,
        row.work_description.as_deref(),
        row.risk_assessment.as_deref(),
        row.preventive_measures.as_deref(),
        row.ppe_required.as_deref(),
    );
    if row.requires_hse_approval && (row.hse_approved_at.is_none() || row.hse_approved_by_id.is_none()) {
        out.push(IssueBlocker::new(
            CODE_HSE_APPROVAL_MISSING,
            &[("permit", row.code.clone())],
        ));
    }
    if row.requires_gas_test {
        // Latest counting reading first: the one that describes the atmosphere now.
        out.extend(gas_blockers(
            &row.code,
            limits,
            validity_hours,
            tests,
            crate::clock::now(),
        ));
    }
    out
}

/// What blocks the issue of the permit now (empty: its content, HSE approval, gas test and workers
/// allow it).
pub async fn issue_blockers(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<IssueBlocker>> {
    let row = permit_row(db, permit_id).await?;
    let limits = gas_limits(db).await?;
    let hours = gas_validity_hours(db).await?;
    let tests = list_permit_gas_tests(db, permit_id, &limits, row.since()).await?;
    let mut out = blockers_of(&row, &limits, hours, &tests);
    out.extend(crate::permit::workers::issue_blockers(db, permit_id).await?);
    Ok(out)
}

/// The gas part of the gates outside the issue (activation, reinstatement, start or resume of the
/// linked work order): a permit needing a gas test has a counting reading younger than the re-test
/// interval and within the limits, now.
pub(crate) async fn gas_ready(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<()> {
    let row = permit_row(db, permit_id).await?;
    if !row.requires_gas_test {
        return Ok(());
    }
    let limits = gas_limits(db).await?;
    let hours = gas_validity_hours(db).await?;
    let tests = list_permit_gas_tests(db, permit_id, &limits, row.since()).await?;
    let blockers = gas_blockers(&row.code, &limits, hours, &tests, crate::clock::now());
    if blockers.is_empty() {
        return Ok(());
    }
    Err(crate::permit::workers::blockers_error(blockers, "Permit not usable"))
}

/// The issue gate (`approved → issued`): every blocker is refused at once, each with its code.
pub(crate) async fn issue_ready(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<()> {
    let blockers = issue_blockers(db, permit_id).await?;
    if blockers.is_empty() {
        return Ok(());
    }
    Err(crate::errors::org_validation_failed_issues(
        blockers
            .into_iter()
            .map(|b| {
                AppValidationIssue::error(
                    b.code.clone(),
                    format!("Permit cannot be issued: {}", b.code),
                    b.params.into_iter().collect(),
                )
            })
            .collect(),
    ))
}

pub async fn get_permit_safety_content(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<PermitSafetyContent> {
    let row = permit_row(db, permit_id).await?;
    let limits = gas_limits(db).await?;
    let hours = gas_validity_hours(db).await?;
    let tests = list_permit_gas_tests(db, permit_id, &limits, row.since()).await?;
    let mut issue_blockers = blockers_of(&row, &limits, hours, &tests);
    issue_blockers.extend(crate::permit::workers::issue_blockers(db, permit_id).await?);
    Ok(PermitSafetyContent {
        permit_id: row.id,
        permit_status: row.status,
        permit_row_version: row.row_version,
        work_description: row.work_description,
        risk_assessment: row.risk_assessment,
        preventive_measures: row.preventive_measures,
        ppe_required: row.ppe_required,
        requires_hse_approval: row.requires_hse_approval,
        requires_gas_test: row.requires_gas_test,
        hse_approved_by_id: row.hse_approved_by_id,
        hse_approved_by_name: row.hse_approved_by_name,
        hse_approved_at: row.hse_approved_at,
        hse_approval_note: row.hse_approval_note,
        hse_self_approved: row.hse_self_approved,
        gas_limits: limits,
        gas_validity_hours: hours,
        gas_tests: tests,
        issue_blockers,
    })
}

fn checked_text(field: &str, value: &str, max: usize) -> AppResult<Option<String>> {
    let v = value.trim();
    if v.chars().count() > max {
        return Err(refused(
            "GATE_PERMIT_CONTENT_TOO_LONG",
            format!("{field} is longer than {max} characters."),
            &[("max", max.to_string())],
        ));
    }
    Ok((!v.is_empty()).then(|| v.to_string()))
}

/// Writes the content of a permit (draft or waiting for review). A change while it waits for
/// review sends it back to draft with its review record cleared; a change of content clears an HSE
/// approval of the former content. `actor_id` is the session user (audited when the review resets).
pub async fn save_permit_safety_content(
    db: &DatabaseConnection,
    input: PermitSafetyContentInput,
    actor_id: i64,
) -> AppResult<PermitSafetyContent> {
    let txn = db.begin().await?;
    let out = save_permit_safety_content_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn save_permit_safety_content_in(
    db: &impl ConnectionTrait,
    input: PermitSafetyContentInput,
    actor_id: i64,
) -> AppResult<PermitSafetyContent> {
    let current = permit_row(db, input.permit_id).await?;
    if input.expected_row_version != current.row_version {
        return Err(changed_elsewhere());
    }
    if !CONTENT_EDIT_STATUSES.contains(&current.status.as_str()) {
        return Err(refused(
            "GATE_PERMIT_CONTENT_LOCKED",
            format!("Permit content cannot change in status '{}'.", current.status),
            &[("status", current.status.clone())],
        ));
    }
    let work = checked_text("work_description", &input.work_description, MAX_CONTENT_CHARS)?;
    let risk = checked_text("risk_assessment", &input.risk_assessment, MAX_CONTENT_CHARS)?;
    let measures = checked_text("preventive_measures", &input.preventive_measures, MAX_CONTENT_CHARS)?;
    let ppe = checked_text("ppe_required", &input.ppe_required, MAX_CONTENT_CHARS)?;
    let unchanged = work == current.work_description
        && risk == current.risk_assessment
        && measures == current.preventive_measures
        && ppe == current.ppe_required;
    if unchanged {
        return get_permit_safety_content(db, input.permit_id).await;
    }
    let back_to_draft = current.status == "pending_review";
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_permits SET work_description = ?, risk_assessment = ?, preventive_measures = ?, ppe_required = ?, hse_approved_by_id = NULL, hse_approved_at = NULL, hse_approval_note = NULL, status = ?, row_version = row_version + 1 WHERE id = ? AND row_version = ?",
            [
                work.into(),
                risk.into(),
                measures.into(),
                ppe.into(),
                (if back_to_draft { "draft" } else { current.status.as_str() }).into(),
                input.permit_id.into(),
                current.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(changed_elsewhere());
    }
    if back_to_draft {
        let before = crate::permit::queries::get_work_permit(db, input.permit_id).await?;
        crate::permit::queries::clear_review_record(db, input.permit_id).await?;
        if let Some(before) = before {
            audit(
                db,
                "permit.review_reset",
                input.permit_id,
                actor_id,
                serde_json::json!({
                    "code": before.code,
                    "reason": "content_changed",
                    "cleared_submitted_by_id": before.submitted_by_id,
                    "cleared_submitted_at": before.submitted_at,
                    "status_before": "pending_review",
                    "status_after": "draft",
                }),
            )
            .await?;
        }
    }
    get_permit_safety_content(db, input.permit_id).await
}

/// Records the HSE approval of a permit by the session user (waiting for review or approved, its
/// content complete).
pub async fn record_permit_hse_approval(
    db: &DatabaseConnection,
    input: PermitHseApprovalInput,
    actor_id: i64,
) -> AppResult<PermitSafetyContent> {
    let txn = db.begin().await?;
    let out = record_permit_hse_approval_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn record_permit_hse_approval_in(
    db: &impl ConnectionTrait,
    input: PermitHseApprovalInput,
    actor_id: i64,
) -> AppResult<PermitSafetyContent> {
    let current = permit_row(db, input.permit_id).await?;
    if input.expected_row_version != current.row_version {
        return Err(changed_elsewhere());
    }
    if !HSE_APPROVAL_STATUSES.contains(&current.status.as_str()) {
        return Err(refused(
            "GATE_PERMIT_HSE_APPROVAL_WRONG_STATUS",
            format!("HSE approval cannot be recorded in status '{}'.", current.status),
            &[("status", current.status.clone())],
        ));
    }
    if current.hse_approved_at.is_some() {
        return Err(refused(
            "GATE_PERMIT_HSE_ALREADY_APPROVED",
            "The HSE approval of this permit is already recorded.",
            &[("permit", current.code.clone())],
        ));
    }
    if !content_blockers(
        &current.code,
        current.work_description.as_deref(),
        current.risk_assessment.as_deref(),
        current.preventive_measures.as_deref(),
        current.ppe_required.as_deref(),
    )
    .is_empty()
    {
        return Err(refused(
            "GATE_PERMIT_HSE_CONTENT_INCOMPLETE",
            "The HSE approval needs the work description, risk assessment, preventive measures and PPE.",
            &[("permit", current.code.clone())],
        ));
    }
    let self_approved = hse_four_eyes(db, &current, actor_id).await?;
    let note = match input.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.chars().count() > MAX_HSE_NOTE_CHARS => {
            return Err(refused(
                "GATE_PERMIT_HSE_NOTE_TOO_LONG",
                format!("HSE approval comment longer than {MAX_HSE_NOTE_CHARS} characters."),
                &[("max", MAX_HSE_NOTE_CHARS.to_string())],
            ))
        }
        other => other.map(str::to_string),
    };
    let at = now_ts();
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_permits SET hse_approved_by_id = ?, hse_approved_at = ?, hse_approval_note = ?, hse_self_approved = ?, row_version = row_version + 1 WHERE id = ? AND row_version = ? AND hse_approved_at IS NULL",
            [
                actor_id.into(),
                at.clone().into(),
                note.clone().into(),
                i64::from(self_approved).into(),
                input.permit_id.into(),
                current.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(changed_elsewhere());
    }
    audit(
        db,
        "permit.hse_approved",
        input.permit_id,
        actor_id,
        serde_json::json!({ "code": current.code, "approved_at": at, "note": note, "hse_self_approved": self_approved }),
    )
    .await?;
    get_permit_safety_content(db, input.permit_id).await
}

/// Four-eyes on the HSE approval (CMMS Advisor 2026-11-11): the HSE approver is neither the permit's
/// author nor its submitter (`approver_is_second_person`, the rule of the approval itself). Returns
/// whether it is a self-approval, allowed only where the permit's site opted out of four-eyes;
/// elsewhere refused, saying why. The person who approved the permit may record it.
async fn hse_four_eyes(db: &impl ConnectionTrait, permit: &PermitRow, actor_id: i64) -> AppResult<bool> {
    use crate::reliability::study::decisions::SelfApprovalRule;
    let creator = permit.created_by_id;
    let submitter = permit.submitted_by_id;
    if crate::permit::queries::approver_is_second_person(creator.as_ref(), submitter.as_ref(), &actor_id) {
        return Ok(false);
    }
    if crate::permit::queries::permit_four_eyes_rule(db, permit.entity_id).await? == SelfApprovalRule::Allowed {
        return Ok(true);
    }
    let code = permit.code.clone();
    Err(if creator == Some(actor_id) {
        refused(
            "GATE_PERMIT_HSE_APPROVER_IS_AUTHOR",
            format!("Permit {code} was written by you: another person records its HSE approval."),
            &[("permit", code.clone())],
        )
    } else if submitter == Some(actor_id) {
        refused(
            "GATE_PERMIT_HSE_APPROVER_IS_SUBMITTER",
            format!("Permit {code} was submitted for review by you: another person records its HSE approval."),
            &[("permit", code.clone())],
        )
    } else {
        refused(
            "GATE_PERMIT_HSE_AUTHOR_UNKNOWN",
            format!(
                "Permit {code}: its author or submitter is not recorded, so the HSE approval cannot be shown to come from another person."
            ),
            &[("permit", code.clone())],
        )
    })
}

fn checked_reading(field: &str, value: f64, max: f64) -> AppResult<f64> {
    if value.is_finite() && (0.0..=max).contains(&value) {
        Ok(value)
    } else {
        Err(refused(
            "GATE_PERMIT_GAS_VALUE_INVALID",
            format!("{field} must be a number between 0 and {max}."),
            &[("field", field.to_string()), ("max", max.to_string())],
        ))
    }
}

/// Records a gas test on a permit, measured by the session user.
pub async fn record_permit_gas_test(
    db: &DatabaseConnection,
    input: PermitGasTestInput,
    actor_id: i64,
) -> AppResult<PermitSafetyContent> {
    let txn = db.begin().await?;
    let out = record_permit_gas_test_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

pub(crate) async fn record_permit_gas_test_in(
    db: &impl ConnectionTrait,
    input: PermitGasTestInput,
    actor_id: i64,
) -> AppResult<PermitSafetyContent> {
    let current = permit_row(db, input.permit_id).await?;
    if !GAS_TEST_STATUSES.contains(&current.status.as_str()) {
        return Err(refused(
            "GATE_PERMIT_GAS_TEST_WRONG_STATUS",
            format!("A gas test cannot be recorded in status '{}'.", current.status),
            &[("status", current.status.clone())],
        ));
    }
    // Physical bounds of the readings (percentages up to 100; ppm as an instrument reads them).
    let o2 = checked_reading("o2_percent", input.o2_percent, 100.0)?;
    let lel = checked_reading("lel_percent", input.lel_percent, 100.0)?;
    let h2s = checked_reading("h2s_ppm", input.h2s_ppm, 1_000_000.0)?;
    let co = input
        .co_ppm
        .map(|c| checked_reading("co_ppm", c, 1_000_000.0))
        .transpose()?;
    let instrument = input.instrument.trim().to_string();
    if instrument.is_empty() {
        return Err(refused(
            "GATE_PERMIT_GAS_INSTRUMENT_REQUIRED",
            "The instrument used for the gas test is required.",
            &[],
        ));
    }
    if instrument.chars().count() > MAX_INSTRUMENT_CHARS {
        return Err(refused(
            "GATE_PERMIT_CONTENT_TOO_LONG",
            format!("instrument is longer than {MAX_INSTRUMENT_CHARS} characters."),
            &[("max", MAX_INSTRUMENT_CHARS.to_string())],
        ));
    }
    let measured_at = chrono::DateTime::parse_from_rfc3339(input.measured_at.trim())
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .filter(|d| {
            *d <= crate::clock::now() + chrono::Duration::minutes(crate::settings::site::MAX_CLOCK_SKEW_MINUTES)
        })
        .ok_or_else(|| {
            refused(
                "GATE_PERMIT_GAS_TEST_TIME_INVALID",
                "The time of the gas test must be a readable date and time, not in the future.",
                &[],
            )
        })?
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    // A reading taken before the permit was raised (or before its last reset) describes another
    // atmosphere: refused (review of L4-04).
    if let Some(since) = current.since() {
        if crate::settings::site::instant(&measured_at).is_some_and(|m| m < since) {
            return Err(refused(
                CODE_GAS_TEST_BEFORE_PERMIT,
                "The gas test was measured before the permit was raised or last sent back to draft.",
                &[
                    ("permit", current.code.clone()),
                    ("since", since.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                ],
            ));
        }
    }
    let now = now_ts();
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_gas_tests (entity_sync_id, permit_id, o2_percent, lel_percent, h2s_ppm, co_ppm, measured_at, measured_by_id, instrument, row_version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            sync_id.into(),
            input.permit_id.into(),
            o2.into(),
            lel.into(),
            h2s.into(),
            co.into(),
            measured_at.clone().into(),
            actor_id.into(),
            instrument.clone().into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let limits = gas_limits(db).await?;
    audit(
        db,
        "permit.gas_test_recorded",
        input.permit_id,
        actor_id,
        serde_json::json!({
            "code": current.code,
            "measured_at": measured_at,
            "o2_percent": o2,
            "lel_percent": lel,
            "h2s_ppm": h2s,
            "co_ppm": co,
            "instrument": instrument,
            "within_limits": reading_breaches(&limits, o2, lel, h2s, co, "").is_empty(),
        }),
    )
    .await?;
    get_permit_safety_content(db, input.permit_id).await
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

/// Tests that move permits through `issued` give them what the issue gate asks for: the content,
/// the HSE approval when their type requires it, and a gas test within the limits when they need
/// one. Written directly (the row version is kept), as the people of the test would have.
#[cfg(test)]
pub(crate) async fn fill_issue_requirements_for_tests(db: &impl ConnectionTrait, permit_id: i64) {
    let row = permit_row(db, permit_id).await.expect("permit row");
    // A person other than the author and submitter when the test has one (HSE four-eyes).
    let approver: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts ORDER BY (id = COALESCE(?, -1) OR id = COALESCE(?, -1)), id LIMIT 1",
            [row.created_by_id.into(), row.submitted_by_id.into()],
        ))
        .await
        .expect("user")
        .expect("a user")
        .try_get("", "id")
        .expect("id");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET work_description = COALESCE(work_description, 'Remplacement du roulement côté accouplement'), risk_assessment = COALESCE(risk_assessment, 'Énergie résiduelle, pièces en mouvement'), preventive_measures = COALESCE(preventive_measures, 'Consignation, balisage de la zone'), ppe_required = COALESCE(ppe_required, 'Casque, gants, lunettes'), hse_approved_by_id = CASE WHEN ? THEN COALESCE(hse_approved_by_id, ?) ELSE hse_approved_by_id END, hse_approved_at = CASE WHEN ? THEN COALESCE(hse_approved_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')) ELSE hse_approved_at END WHERE id = ?",
        [
            row.requires_hse_approval.into(),
            approver.into(),
            row.requires_hse_approval.into(),
            permit_id.into(),
        ],
    ))
    .await
    .expect("fill permit content");
    if row.requires_gas_test {
        let limits = gas_limits(db).await.expect("limits");
        let o2 = (limits.o2_min_percent + limits.o2_max_percent) / 2.0;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO permit_gas_tests (entity_sync_id, permit_id, o2_percent, lel_percent, h2s_ppm, co_ppm, measured_at, measured_by_id, instrument, row_version, created_at, updated_at) VALUES (?, ?, ?, 0, 0, 0, ?, ?, 'Détecteur 4 gaz (essai)', 1, ?, ?)",
            [
                Uuid::new_v4().to_string().into(),
                permit_id.into(),
                o2.into(),
                now_ts().into(),
                approver.into(),
                now_ts().into(),
                now_ts().into(),
            ],
        ))
        .await
        .expect("gas test");
    }
}
