//! DI SLA engine — rule resolution, deadline computation, and breach detection.
//!
//! Phase 2 - Sub-phase 04 - File 03 - Sprint S1 / Phase 1 completion.
//!
//! The target times of a request come from the shared DI / OT urgency scale
//! (`reference::urgency_scale`, `WORK.URGENCY`): each level carries the plant's target
//! response and completion times and its escalation threshold. `di_sla_rules` holds the
//! exceptions only: a rule for one urgency level and an origin and/or an asset criticality
//! overrides the level's targets. The engine resolves the most specific matching rule, falls
//! back to the level's targets, and computes breach flags from DI timestamps.

use crate::assets::identity::normalize_criticality_to_canon;
use crate::errors::{AppError, AppResult};
use chrono::{Duration, NaiveDateTime};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use super::domain::InterventionRequest;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Row from `di_sla_rules`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiSlaRule {
    pub id: i64,
    pub name: String,
    pub urgency_level: String,
    pub origin_type: Option<String>,
    pub asset_criticality_class: Option<String>,
    pub target_response_hours: i64,
    pub target_resolution_hours: i64,
    pub escalation_threshold_hours: i64,
    pub is_active: bool,
}

/// Lifecycle status codes for a computed SLA evaluation.
/// Display labels: On Track / At Risk / Breached / Completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiSlaLifecycleStatus {
    OnTrack,
    AtRisk,
    Breached,
    Completed,
    /// Waiting on the requester or deferred: the clock is stopped.
    Paused,
}

impl DiSlaLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnTrack => "on_track",
            Self::AtRisk => "at_risk",
            Self::Breached => "breached",
            Self::Completed => "completed",
            Self::Paused => "paused",
        }
    }
}

/// Computed SLA status for a single DI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiSlaStatus {
    pub rule_id: Option<i64>,
    pub target_response_hours: Option<i64>,
    pub target_resolution_hours: Option<i64>,
    /// Response deadline (`submitted_at + target_response_hours`).
    pub sla_deadline: Option<String>,
    /// Resolution deadline (`submitted_at + target_resolution_hours`).
    pub resolution_deadline: Option<String>,
    pub response_elapsed_hours: Option<f64>,
    pub resolution_elapsed_hours: Option<f64>,
    pub response_remaining_hours: Option<f64>,
    pub resolution_remaining_hours: Option<f64>,
    pub is_response_breached: bool,
    pub is_resolution_breached: bool,
    /// Hours the clock was paused (waiting on the requester, deferred); deadlines include them.
    pub paused_hours: f64,
    /// `on_track` | `at_risk` | `breached` | `completed` | `paused`, or null when no rule.
    pub status: Option<DiSlaLifecycleStatus>,
}

/// Historical SLA outcome for dashboard aggregation (includes late-but-completed clocks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlaHistoricalOutcome {
    /// Terminal DI with no late response/resolution clocks.
    Met,
    /// Currently breached, or terminal with a late clock.
    Breached,
    /// Open DI still On Track or At Risk (excluded from met/breached KPI).
    InFlight,
    /// No matching rule.
    None,
}

/// Admin input for updating an SLA rule.
#[derive(Debug, Clone, Deserialize)]
pub struct SlaRuleUpdateInput {
    pub id: i64,
    pub name: String,
    pub urgency_level: String,
    pub origin_type: Option<String>,
    pub asset_criticality_class: Option<String>,
    pub target_response_hours: i64,
    pub target_resolution_hours: i64,
    pub escalation_threshold_hours: i64,
    pub is_active: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "di_sla_rules row decode failed for column '{column}': {e}"
    ))
}

fn map_sla_rule(row: &QueryResult) -> AppResult<DiSlaRule> {
    Ok(DiSlaRule {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        name: row.try_get::<String>("", "name").map_err(|e| decode_err("name", e))?,
        urgency_level: row
            .try_get::<String>("", "urgency_level")
            .map_err(|e| decode_err("urgency_level", e))?,
        origin_type: row
            .try_get::<Option<String>>("", "origin_type")
            .map_err(|e| decode_err("origin_type", e))?,
        asset_criticality_class: row
            .try_get::<Option<String>>("", "asset_criticality_class")
            .map_err(|e| decode_err("asset_criticality_class", e))?,
        target_response_hours: row
            .try_get::<i64>("", "target_response_hours")
            .map_err(|e| decode_err("target_response_hours", e))?,
        target_resolution_hours: row
            .try_get::<i64>("", "target_resolution_hours")
            .map_err(|e| decode_err("target_resolution_hours", e))?,
        escalation_threshold_hours: row
            .try_get::<i64>("", "escalation_threshold_hours")
            .map_err(|e| decode_err("escalation_threshold_hours", e))?,
        is_active: row
            .try_get::<i64>("", "is_active")
            .map_err(|e| decode_err("is_active", e))?
            != 0,
    })
}

fn empty_status() -> DiSlaStatus {
    DiSlaStatus {
        rule_id: None,
        target_response_hours: None,
        target_resolution_hours: None,
        sla_deadline: None,
        resolution_deadline: None,
        response_elapsed_hours: None,
        resolution_elapsed_hours: None,
        response_remaining_hours: None,
        resolution_remaining_hours: None,
        is_response_breached: false,
        is_resolution_breached: false,
        paused_hours: 0.0,
        status: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse an ISO 8601 datetime string (`YYYY-MM-DDTHH:MM:SSZ`) into a `NaiveDateTime`.
fn parse_iso(s: &str) -> AppResult<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to parse datetime '{s}': {e}")))
}

/// Hours elapsed between two datetimes as f64.
fn hours_between(from: &NaiveDateTime, to: &NaiveDateTime) -> f64 {
    let diff = *to - *from;
    diff.num_minutes() as f64 / 60.0
}

fn criticality_codes_match(rule_class: &str, asset_class: &str) -> bool {
    let rule_n = normalize_criticality_to_canon(rule_class).unwrap_or_else(|_| rule_class.trim().to_ascii_uppercase());
    let asset_n =
        normalize_criticality_to_canon(asset_class).unwrap_or_else(|_| asset_class.trim().to_ascii_uppercase());
    rule_n == asset_n
}

/// Resolve asset criticality from production `equipment` + reference/lookup JOINs.
pub async fn lookup_asset_criticality(db: &impl ConnectionTrait, equipment_id: i64) -> AppResult<Option<String>> {
    let raw: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(rs_crit.code, lv.code) AS criticality_code \
             FROM equipment e \
             LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id \
             LEFT JOIN reference_values rs_crit ON rs_crit.id = e.equipment_criticality_ref_id \
             WHERE e.id = ? AND e.deleted_at IS NULL",
            [equipment_id.into()],
        ))
        .await?
        .and_then(|row| row.try_get::<Option<String>>("", "criticality_code").ok().flatten());

    Ok(raw.map(|code| normalize_criticality_to_canon(&code).unwrap_or_else(|_| code.trim().to_ascii_uppercase())))
}

fn is_sla_terminal(di: &InterventionRequest) -> bool {
    if di.converted_at.is_some() {
        return true;
    }
    matches!(di.status.as_str(), "closed")
}

/// Statuses where the request waits on someone else than maintenance: the requester
/// (`returned_for_clarification`) or a later date (`deferred`). The SLA clock is paused there
/// (ITIL "pending"); the time spent extends the deadlines (`di_paused_hours_sql!` in `domain`).
pub const SLA_PAUSED_STATUSES: &[&str] = &["returned_for_clarification", "deferred"];

fn is_sla_paused(di: &InterventionRequest) -> bool {
    SLA_PAUSED_STATUSES.contains(&di.status.as_str())
}

fn format_iso(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn shift_hours(iso: &str, hours: f64) -> AppResult<String> {
    let dt = parse_iso(iso)?;
    Ok(format_iso(dt + Duration::seconds((hours * 3600.0).round() as i64)))
}

/// Targets of one request (frozen on the request, or resolved from the rules for legacy rows).
struct ClockTargets {
    rule_id: Option<i64>,
    response_hours: i64,
    resolution_hours: i64,
    escalation_hours: i64,
    /// Deadlines before any pause.
    response_deadline: String,
    resolution_deadline: String,
}

/// The SLA clock of a request. Elapsed time runs from submission to screening (response) and to
/// the work order (resolution); a request closed without them stops its clocks at `closed_at`.
/// Time spent paused is not counted and extends the deadlines. A paused request is `Paused`,
/// never at risk or breached.
fn evaluate_clock(di: &InterventionRequest, t: &ClockTargets) -> AppResult<DiSlaStatus> {
    let submitted_at = parse_iso(&di.submitted_at)?;
    let now = crate::clock::now().naive_utc();
    let closed_at = match (&di.closed_at, di.status.as_str()) {
        (Some(c), "closed") => Some(parse_iso(c)?),
        _ => None,
    };
    let response_end = match &di.screened_at {
        Some(s) => parse_iso(s)?,
        None => closed_at.unwrap_or(now),
    };
    let resolution_end = match &di.converted_at {
        Some(s) => parse_iso(s)?,
        None => closed_at.unwrap_or(now),
    };
    let paused_response = di.sla_paused_response_hours.max(0.0);
    let paused_total = di.sla_paused_hours.max(0.0);
    let response_elapsed = (hours_between(&submitted_at, &response_end) - paused_response).max(0.0);
    let resolution_elapsed = (hours_between(&submitted_at, &resolution_end) - paused_total).max(0.0);

    let terminal = is_sla_terminal(di);
    let paused = !terminal && is_sla_paused(di);
    let response_running = di.screened_at.is_none() && !terminal;
    let resolution_running = di.converted_at.is_none() && !terminal;

    // Late = the clock went past its target; for a finished request, at the time it finished.
    let response_late = response_elapsed > t.response_hours as f64;
    let resolution_late = resolution_elapsed > t.resolution_hours as f64;
    // Breach flags drive the breach notifications: only while the clock runs.
    let is_response_breached = response_running && !paused && response_late;
    let is_resolution_breached = resolution_running && !paused && resolution_late;

    let status = if terminal {
        DiSlaLifecycleStatus::Completed
    } else if paused {
        DiSlaLifecycleStatus::Paused
    } else if is_response_breached || is_resolution_breached {
        DiSlaLifecycleStatus::Breached
    } else {
        let escalation = t.escalation_hours as f64;
        let at_risk = (response_running && response_elapsed >= escalation)
            || (resolution_running && resolution_elapsed >= escalation);
        if at_risk {
            DiSlaLifecycleStatus::AtRisk
        } else {
            DiSlaLifecycleStatus::OnTrack
        }
    };

    Ok(DiSlaStatus {
        rule_id: t.rule_id,
        target_response_hours: Some(t.response_hours),
        target_resolution_hours: Some(t.resolution_hours),
        sla_deadline: Some(shift_hours(&t.response_deadline, paused_response)?),
        resolution_deadline: Some(shift_hours(&t.resolution_deadline, paused_total)?),
        response_elapsed_hours: Some(response_elapsed),
        resolution_elapsed_hours: Some(resolution_elapsed),
        response_remaining_hours: response_running.then(|| (t.response_hours as f64 - response_elapsed).max(0.0)),
        resolution_remaining_hours: resolution_running
            .then(|| (t.resolution_hours as f64 - resolution_elapsed).max(0.0)),
        is_response_breached,
        is_resolution_breached,
        paused_hours: paused_total,
        status: Some(status),
    })
}

/// Full review-event SLA snapshot (response + resolution).
#[derive(Debug, Clone, Default)]
pub struct SlaReviewSnapshot {
    pub response_target_hours: Option<i64>,
    pub response_deadline: Option<String>,
    pub resolution_target_hours: Option<i64>,
    pub resolution_deadline: Option<String>,
}

fn di_has_frozen_sla(di: &InterventionRequest) -> bool {
    di.sla_target_response_hours.is_some() && di.sla_response_deadline.is_some()
}

fn status_from_frozen(di: &InterventionRequest) -> AppResult<DiSlaStatus> {
    let response_hours = di.sla_target_response_hours.unwrap_or_default();
    let response_deadline = di.sla_response_deadline.clone().unwrap_or_default();
    evaluate_clock(
        di,
        &ClockTargets {
            rule_id: di.sla_rule_id,
            response_hours,
            resolution_hours: di.sla_target_resolution_hours.unwrap_or(response_hours),
            escalation_hours: di.sla_escalation_threshold_hours.unwrap_or(response_hours),
            resolution_deadline: di
                .sla_resolution_deadline
                .clone()
                .unwrap_or_else(|| response_deadline.clone()),
            response_deadline,
        },
    )
}

/// SLA state of a request from its frozen targets and pause history (no extra database read), as
/// shown in lists and work queues. `None` when no target was frozen or a stored date does not parse.
pub fn frozen_sla_state(di: &InterventionRequest) -> Option<DiSlaLifecycleStatus> {
    if !di_has_frozen_sla(di) {
        return None;
    }
    status_from_frozen(di).ok().and_then(|s| s.status)
}

/// Hours of the response clock of request `ir` (SQL): submission → screening, else → closing,
/// else → now, minus the paused hours before screening.
macro_rules! di_response_elapsed_sql {
    () => {
        "((julianday(COALESCE(ir.screened_at, CASE WHEN ir.status = 'closed' THEN ir.closed_at END, \
            strftime('%Y-%m-%dT%H:%M:%SZ','now'))) - julianday(ir.submitted_at)) * 24.0 - "
    };
}

/// Hours of the resolution clock (SQL): submission → work order, else → closing, else → now,
/// minus every paused hour.
macro_rules! di_resolution_elapsed_sql {
    () => {
        "((julianday(COALESCE(ir.converted_at, CASE WHEN ir.status = 'closed' THEN ir.closed_at END, \
            strftime('%Y-%m-%dT%H:%M:%SZ','now'))) - julianday(ir.submitted_at)) * 24.0 - "
    };
}

/// SQL (alias `ir`): elapsed response hours, pauses excluded.
pub const SLA_RESPONSE_ELAPSED_SQL: &str = concat!(di_response_elapsed_sql!(), di_paused_response_hours_sql!(), ")");
/// SQL (alias `ir`): elapsed resolution hours, pauses excluded.
pub const SLA_RESOLUTION_ELAPSED_SQL: &str = concat!(di_resolution_elapsed_sql!(), di_paused_hours_sql!(), ")");

/// Open requests whose clock runs: open (`di::stats::OPEN_SQL`: no terminal status, no work
/// order), not paused, with a frozen target.
macro_rules! di_sla_running_sql {
    () => {
        concat!(
            "ir.status NOT IN ",
            crate::di_terminal_statuses_sql!(),
            " AND ir.status NOT IN ('returned_for_clarification', 'deferred') \
             AND ir.converted_at IS NULL \
             AND ir.sla_target_response_hours IS NOT NULL AND ir.sla_response_deadline IS NOT NULL"
        )
    };
}

/// SQL (alias `ir`): the response clock went past its target (not screened in time).
macro_rules! di_response_late_sql {
    () => {
        concat!(
            "(ir.screened_at IS NULL AND ",
            di_response_elapsed_sql!(),
            di_paused_response_hours_sql!(),
            ") > ir.sla_target_response_hours)"
        )
    };
}

/// SQL (alias `ir`): the resolution clock went past its target (no work order in time).
macro_rules! di_resolution_late_sql {
    () => {
        concat!(
            "(ir.converted_at IS NULL AND ",
            di_resolution_elapsed_sql!(),
            di_paused_hours_sql!(),
            ") > COALESCE(ir.sla_target_resolution_hours, ir.sla_target_response_hours))"
        )
    };
}

/// SQL (alias `ir`): the request went past a target — response measured at screening (or closing,
/// or now), resolution at the work order (or closing, or now), pauses excluded. Used for finished
/// requests (SLA compliance) and, with [`SLA_BREACHED_SQL`], for open ones.
pub const SLA_LATE_SQL: &str = concat!(
    "(",
    di_response_elapsed_sql!(),
    di_paused_response_hours_sql!(),
    ") > ir.sla_target_response_hours OR ",
    di_resolution_elapsed_sql!(),
    di_paused_hours_sql!(),
    ") > COALESCE(ir.sla_target_resolution_hours, ir.sla_target_response_hours))"
);

/// SQL (alias `ir`): hours past the first missed target of an open request (response while not
/// screened, else completion), pauses excluded.
pub const SLA_HOURS_OVERDUE_SQL: &str = concat!(
    "MAX(CASE WHEN ir.screened_at IS NULL THEN ",
    di_response_elapsed_sql!(),
    di_paused_response_hours_sql!(),
    ") - ir.sla_target_response_hours ELSE 0.0 END, ",
    di_resolution_elapsed_sql!(),
    di_paused_hours_sql!(),
    ") - COALESCE(ir.sla_target_resolution_hours, ir.sla_target_response_hours))"
);

/// SQL predicate (alias `ir`): open requests whose running SLA is breached — the same rule as
/// [`evaluate_clock`] (pauses excluded; a paused request is never breached).
pub const SLA_BREACHED_SQL: &str = concat!(
    "(",
    di_sla_running_sql!(),
    " AND (",
    di_response_late_sql!(),
    " OR ",
    di_resolution_late_sql!(),
    "))"
);

/// SQL predicate (alias `ir`): open requests whose running SLA is at risk or breached — past the
/// escalation threshold on a running clock, or late (same rule as [`evaluate_clock`]).
pub const SLA_AT_RISK_OR_BREACHED_SQL: &str = concat!(
    "(",
    di_sla_running_sql!(),
    " AND (",
    di_response_late_sql!(),
    " OR ",
    di_resolution_late_sql!(),
    " OR (ir.screened_at IS NULL AND ",
    di_response_elapsed_sql!(),
    di_paused_response_hours_sql!(),
    ") >= COALESCE(ir.sla_escalation_threshold_hours, ir.sla_target_response_hours)) OR ",
    di_resolution_elapsed_sql!(),
    di_paused_hours_sql!(),
    ") >= COALESCE(ir.sla_escalation_threshold_hours, ir.sla_target_response_hours)))"
);

fn status_from_rule(di: &InterventionRequest, rule: &SlaTargets) -> AppResult<DiSlaStatus> {
    let submitted_at = parse_iso(&di.submitted_at)?;
    evaluate_clock(
        di,
        &ClockTargets {
            rule_id: rule.rule_id,
            response_hours: rule.target_response_hours,
            resolution_hours: rule.target_resolution_hours,
            escalation_hours: rule.escalation_threshold_hours,
            response_deadline: format_iso(submitted_at + Duration::hours(rule.target_response_hours)),
            resolution_deadline: format_iso(submitted_at + Duration::hours(rule.target_resolution_hours)),
        },
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) resolve_sla_rule — most-specific matching rule
// ═══════════════════════════════════════════════════════════════════════════════

/// Resolve the most specific active SLA rule for the given parameters.
///
/// Priority (most specific first):
///   1. urgency + origin_type + asset_criticality_class (exact match)
///   2. urgency + origin_type + NULL class (partial match)
///   3. urgency + NULL origin + NULL class (broad match)
pub async fn resolve_sla_rule(
    db: &impl ConnectionTrait,
    urgency: &str,
    origin_type: &str,
    criticality_class: Option<&str>,
) -> AppResult<Option<DiSlaRule>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM di_sla_rules \
             WHERE urgency_level = ? AND is_active = 1",
            [urgency.into()],
        ))
        .await?;

    let rules: Vec<DiSlaRule> = rows.iter().map(map_sla_rule).collect::<AppResult<Vec<_>>>()?;

    if let Some(crit) = criticality_class {
        if let Some(rule) = rules.iter().find(|r| {
            r.origin_type.as_deref() == Some(origin_type)
                && r.asset_criticality_class
                    .as_ref()
                    .is_some_and(|rc| criticality_codes_match(rc, crit))
        }) {
            return Ok(Some(rule.clone()));
        }
    }

    if let Some(rule) = rules
        .iter()
        .find(|r| r.origin_type.as_deref() == Some(origin_type) && r.asset_criticality_class.is_none())
    {
        return Ok(Some(rule.clone()));
    }

    if let Some(rule) = rules
        .iter()
        .find(|r| r.origin_type.is_none() && r.asset_criticality_class.is_none())
    {
        return Ok(Some(rule.clone()));
    }

    Ok(None)
}

/// Targets that apply to a request: from a matching rule, else from its urgency level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlaTargets {
    /// The rule that applied, `None` when the level's own targets apply.
    pub rule_id: Option<i64>,
    pub target_response_hours: i64,
    pub target_resolution_hours: i64,
    pub escalation_threshold_hours: i64,
}

/// Most specific rule for urgency + origin + criticality, else the targets of the urgency level
/// on the shared scale. `None` when neither exists (a level without targets).
pub async fn resolve_sla_targets(
    db: &impl ConnectionTrait,
    urgency: &str,
    origin_type: &str,
    criticality_class: Option<&str>,
) -> AppResult<Option<SlaTargets>> {
    if let Some(rule) = resolve_sla_rule(db, urgency, origin_type, criticality_class).await? {
        return Ok(Some(SlaTargets {
            rule_id: Some(rule.id),
            target_response_hours: rule.target_response_hours,
            target_resolution_hours: rule.target_resolution_hours,
            escalation_threshold_hours: rule.escalation_threshold_hours,
        }));
    }
    let level = crate::reference::urgency_scale::get_urgency_level(db, urgency).await?;
    Ok(level.and_then(|l| l.targets()).map(|t| SlaTargets {
        rule_id: None,
        target_response_hours: t.response_hours,
        target_resolution_hours: t.completion_hours,
        escalation_threshold_hours: t.escalation_hours,
    }))
}

/// Freeze SLA targets/deadlines on a DI once. Idempotent (`sla_response_deadline IS NULL`).
pub async fn freeze_sla_on_di(db: &impl ConnectionTrait, di: &InterventionRequest) -> AppResult<InterventionRequest> {
    if di_has_frozen_sla(di) {
        return Ok(di.clone());
    }

    let criticality_class = lookup_asset_criticality(db, di.equipment_id).await?;
    let rule = resolve_sla_targets(db, &di.reported_urgency, &di.origin_type, criticality_class.as_deref()).await?;

    let Some(rule) = rule else {
        return Ok(di.clone());
    };

    let submitted_at = parse_iso(&di.submitted_at)?;
    let response_deadline = (submitted_at + Duration::hours(rule.target_response_hours))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let resolution_deadline = (submitted_at + Duration::hours(rule.target_resolution_hours))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE intervention_requests SET \
            sla_rule_id = ?, \
            sla_target_response_hours = ?, \
            sla_target_resolution_hours = ?, \
            sla_escalation_threshold_hours = ?, \
            sla_response_deadline = ?, \
            sla_resolution_deadline = ? \
         WHERE id = ? AND sla_response_deadline IS NULL",
        [
            rule.rule_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            rule.target_response_hours.into(),
            rule.target_resolution_hours.into(),
            rule.escalation_threshold_hours.into(),
            response_deadline.into(),
            resolution_deadline.into(),
            di.id.into(),
        ],
    ))
    .await?;

    // Re-read is caller's responsibility when full row needed; patch local clone.
    let mut updated = di.clone();
    updated.sla_rule_id = rule.rule_id;
    updated.sla_target_response_hours = Some(rule.target_response_hours);
    updated.sla_target_resolution_hours = Some(rule.target_resolution_hours);
    updated.sla_escalation_threshold_hours = Some(rule.escalation_threshold_hours);
    updated.sla_response_deadline = Some(
        (submitted_at + Duration::hours(rule.target_response_hours))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
    );
    updated.sla_resolution_deadline = Some(
        (submitted_at + Duration::hours(rule.target_resolution_hours))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
    );
    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) compute_sla_status — deadline and breach computation for a DI
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the SLA status for an intervention request.
/// Prefers immutable DI snapshot columns; falls back to live rules only when unset.
pub async fn compute_sla_status(db: &impl ConnectionTrait, di: &InterventionRequest) -> AppResult<DiSlaStatus> {
    if di_has_frozen_sla(di) {
        return status_from_frozen(di);
    }

    let criticality_class = lookup_asset_criticality(db, di.equipment_id).await?;
    let rule = resolve_sla_targets(db, &di.reported_urgency, &di.origin_type, criticality_class.as_deref()).await?;

    let Some(rule) = rule else {
        return Ok(empty_status());
    };

    status_from_rule(di, &rule)
}

/// Snapshot for review events — always from frozen DI columns when present.
pub fn snapshot_sla_for_review_event(di: &InterventionRequest) -> SlaReviewSnapshot {
    SlaReviewSnapshot {
        response_target_hours: di.sla_target_response_hours,
        response_deadline: di.sla_response_deadline.clone(),
        resolution_target_hours: di.sla_target_resolution_hours,
        resolution_deadline: di.sla_resolution_deadline.clone(),
    }
}

/// Legacy helper: response target + deadline only.
pub async fn snapshot_sla_for_di(
    db: &impl ConnectionTrait,
    di: &InterventionRequest,
) -> AppResult<(Option<i64>, Option<String>)> {
    if di_has_frozen_sla(di) {
        return Ok((di.sla_target_response_hours, di.sla_response_deadline.clone()));
    }
    let status = compute_sla_status(db, di).await?;
    Ok((status.target_response_hours, status.sla_deadline))
}

/// Evaluate historical met/breached outcome (uses frozen deadlines when present).
pub async fn evaluate_historical_outcome(
    db: &impl ConnectionTrait,
    di: &InterventionRequest,
) -> AppResult<SlaHistoricalOutcome> {
    let status = compute_sla_status(db, di).await?;
    if status.target_response_hours.is_none() {
        return Ok(SlaHistoricalOutcome::None);
    }

    // Same clock as the lists: pauses excluded, a finished request measured when it finished.
    let target_response = status.target_response_hours.unwrap_or_default() as f64;
    let target_resolution = status.target_resolution_hours.unwrap_or_default() as f64;
    let response_late = status.response_elapsed_hours.unwrap_or_default() > target_response;
    let resolution_late = status.resolution_elapsed_hours.unwrap_or_default() > target_resolution;

    if is_sla_terminal(di) {
        if response_late || resolution_late {
            return Ok(SlaHistoricalOutcome::Breached);
        }
        return Ok(SlaHistoricalOutcome::Met);
    }
    if status.status == Some(DiSlaLifecycleStatus::Breached) {
        return Ok(SlaHistoricalOutcome::Breached);
    }

    Ok(SlaHistoricalOutcome::InFlight)
}

/// Mark response breach as notified (idempotent).
pub async fn mark_response_breach_notified(db: &impl ConnectionTrait, di_id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE intervention_requests \
         SET sla_response_breach_notified_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE id = ? AND sla_response_breach_notified_at IS NULL",
        [di_id.into()],
    ))
    .await?;
    Ok(())
}

/// Mark resolution breach as notified (idempotent).
pub async fn mark_resolution_breach_notified(db: &impl ConnectionTrait, di_id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE intervention_requests \
         SET sla_resolution_breach_notified_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE id = ? AND sla_resolution_breach_notified_at IS NULL",
        [di_id.into()],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) list_sla_rules
// ═══════════════════════════════════════════════════════════════════════════════

/// List all SLA rules (active and inactive).
pub async fn list_sla_rules(db: &impl ConnectionTrait) -> AppResult<Vec<DiSlaRule>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT r.* FROM di_sla_rules r              LEFT JOIN reference_values lvl ON lvl.code = r.urgency_level AND lvl.set_id IN (                  SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id                   WHERE d.code = 'WORK.URGENCY' AND rs.status = 'published')              ORDER BY r.is_active DESC, COALESCE(lvl.sort_order, 99), r.origin_type, r.asset_criticality_class"
                .to_string(),
        ))
        .await?;

    rows.iter().map(map_sla_rule).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) update_sla_rule
// ═══════════════════════════════════════════════════════════════════════════════

/// Update an existing SLA rule. Permission check (di.admin) is in the command layer.
pub async fn update_sla_rule(db: &impl ConnectionTrait, input: SlaRuleUpdateInput) -> AppResult<DiSlaRule> {
    let urgency = crate::reference::urgency_scale::validate_urgency_code(&input.urgency_level)?;
    let origin_type = input.origin_type.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let criticality = input
        .asset_criticality_class
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if input.is_active && origin_type.is_none() && criticality.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_SLA_RULE_NEEDS_SCOPE: une règle SLA s'applique à une origine et/ou une criticité ;              les délais d'un niveau pour toutes les demandes se règlent dans l'échelle d'urgence."
                .into(),
        ]));
    }

    if input.target_response_hours <= 0 || input.target_resolution_hours <= 0 || input.escalation_threshold_hours <= 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_SLA_TARGET_HOURS_INVALID: Les heures cibles doivent être supérieures à zéro.".into(),
        ]));
    }

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE di_sla_rules SET \
                name = ?, \
                urgency_level = ?, \
                origin_type = ?, \
                asset_criticality_class = ?, \
                target_response_hours = ?, \
                target_resolution_hours = ?, \
                escalation_threshold_hours = ?, \
                is_active = ? \
             WHERE id = ?",
            [
                input.name.into(),
                urgency.as_str().into(),
                origin_type
                    .map(|s| sea_orm::Value::from(s.to_string()))
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                criticality
                    .map(|s| sea_orm::Value::from(s.to_string()))
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                input.target_response_hours.into(),
                input.target_resolution_hours.into(),
                input.escalation_threshold_hours.into(),
                i64::from(input.is_active).into(),
                input.id.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "DiSlaRule".into(),
            id: input.id.to_string(),
        });
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM di_sla_rules WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "DiSlaRule".into(),
            id: input.id.to_string(),
        })?;

    map_sla_rule(&row)
}
