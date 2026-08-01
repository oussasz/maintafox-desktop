//! DI SLA engine — rule resolution, deadline computation, and breach detection.
//!
//! Phase 2 - Sub-phase 04 - File 03 - Sprint S1 / Phase 1 completion.
//!
//! SLA rules are admin-managed workflow policy in `di_sla_rules`. The engine
//! resolves the most specific matching rule for a given urgency + origin +
//! criticality combination and computes breach flags from DI timestamps.

use crate::assets::identity::normalize_criticality_to_canon;
use crate::errors::{AppError, AppResult};
use chrono::{Duration, NaiveDateTime, Utc};
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
}

impl DiSlaLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnTrack => "on_track",
            Self::AtRisk => "at_risk",
            Self::Breached => "breached",
            Self::Completed => "completed",
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
    /// `on_track` | `at_risk` | `breached` | `completed`, or null when no rule.
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
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        name: row
            .try_get::<String>("", "name")
            .map_err(|e| decode_err("name", e))?,
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
        status: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse an ISO 8601 datetime string (`YYYY-MM-DDTHH:MM:SSZ`) into a `NaiveDateTime`.
fn parse_iso(s: &str) -> AppResult<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Failed to parse datetime '{s}': {e}"))
    })
}

/// Hours elapsed between two datetimes as f64.
fn hours_between(from: &NaiveDateTime, to: &NaiveDateTime) -> f64 {
    let diff = *to - *from;
    diff.num_minutes() as f64 / 60.0
}

fn criticality_codes_match(rule_class: &str, asset_class: &str) -> bool {
    let rule_n = normalize_criticality_to_canon(rule_class).unwrap_or_else(|_| {
        rule_class.trim().to_ascii_uppercase()
    });
    let asset_n = normalize_criticality_to_canon(asset_class).unwrap_or_else(|_| {
        asset_class.trim().to_ascii_uppercase()
    });
    rule_n == asset_n
}

/// Resolve asset criticality from production `equipment` + reference/lookup JOINs.
pub async fn lookup_asset_criticality(
    db: &impl ConnectionTrait,
    asset_id: i64,
) -> AppResult<Option<String>> {
    let raw: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(rs_crit.code, lv.code) AS criticality_code \
             FROM equipment e \
             LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id \
             LEFT JOIN reference_values rs_crit ON rs_crit.id = e.equipment_criticality_ref_id \
             WHERE e.id = ? AND e.deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .and_then(|row| {
            row.try_get::<Option<String>>("", "criticality_code")
                .ok()
                .flatten()
        });

    Ok(match raw {
        Some(code) => Some(
            normalize_criticality_to_canon(&code).unwrap_or_else(|_| code.trim().to_ascii_uppercase()),
        ),
        None => None,
    })
}

fn is_sla_terminal(di: &InterventionRequest) -> bool {
    if di.converted_at.is_some() {
        return true;
    }
    matches!(
        di.status.as_str(),
        "closed"
    )
}

fn derive_lifecycle_status(
    di: &InterventionRequest,
    escalation_threshold_hours: i64,
    response_elapsed: f64,
    resolution_elapsed: f64,
    is_response_breached: bool,
    is_resolution_breached: bool,
) -> DiSlaLifecycleStatus {
    if is_sla_terminal(di) {
        return DiSlaLifecycleStatus::Completed;
    }
    if is_response_breached || is_resolution_breached {
        return DiSlaLifecycleStatus::Breached;
    }

    let escalation = escalation_threshold_hours as f64;
    let response_at_risk = di.screened_at.is_none() && response_elapsed >= escalation;
    let resolution_at_risk = di.converted_at.is_none() && resolution_elapsed >= escalation;
    if response_at_risk || resolution_at_risk {
        return DiSlaLifecycleStatus::AtRisk;
    }

    DiSlaLifecycleStatus::OnTrack
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
    let target_response = di.sla_target_response_hours.unwrap();
    let target_resolution = di
        .sla_target_resolution_hours
        .unwrap_or(target_response);
    let escalation = di.sla_escalation_threshold_hours.unwrap_or(target_response);
    let sla_deadline = di.sla_response_deadline.clone().unwrap();
    let resolution_deadline = di
        .sla_resolution_deadline
        .clone()
        .unwrap_or_else(|| sla_deadline.clone());

    let submitted_at = parse_iso(&di.submitted_at)?;
    let now = Utc::now().naive_utc();

    let response_end = match &di.screened_at {
        Some(s) => parse_iso(s)?,
        None => now,
    };
    let response_elapsed = hours_between(&submitted_at, &response_end);

    let resolution_end = match &di.converted_at {
        Some(s) => parse_iso(s)?,
        None => now,
    };
    let resolution_elapsed = hours_between(&submitted_at, &resolution_end);

    let is_response_breached =
        di.screened_at.is_none() && response_elapsed > target_response as f64;
    let is_resolution_breached =
        di.converted_at.is_none() && resolution_elapsed > target_resolution as f64;

    let response_remaining_hours = if di.screened_at.is_none() {
        Some((target_response as f64 - response_elapsed).max(0.0))
    } else {
        None
    };
    let resolution_remaining_hours = if di.converted_at.is_none() && !is_sla_terminal(di) {
        Some((target_resolution as f64 - resolution_elapsed).max(0.0))
    } else {
        None
    };

    let status = Some(derive_lifecycle_status(
        di,
        escalation,
        response_elapsed,
        resolution_elapsed,
        is_response_breached,
        is_resolution_breached,
    ));

    Ok(DiSlaStatus {
        rule_id: di.sla_rule_id,
        target_response_hours: Some(target_response),
        target_resolution_hours: Some(target_resolution),
        sla_deadline: Some(sla_deadline),
        resolution_deadline: Some(resolution_deadline),
        response_elapsed_hours: Some(response_elapsed),
        resolution_elapsed_hours: Some(resolution_elapsed),
        response_remaining_hours,
        resolution_remaining_hours,
        is_response_breached,
        is_resolution_breached,
        status,
    })
}

fn status_from_rule(di: &InterventionRequest, rule: &DiSlaRule) -> AppResult<DiSlaStatus> {
    let submitted_at = parse_iso(&di.submitted_at)?;
    let now = Utc::now().naive_utc();

    let response_deadline_dt = submitted_at + Duration::hours(rule.target_response_hours);
    let resolution_deadline_dt = submitted_at + Duration::hours(rule.target_resolution_hours);
    let sla_deadline = response_deadline_dt.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let resolution_deadline = resolution_deadline_dt
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let response_end = match &di.screened_at {
        Some(s) => parse_iso(s)?,
        None => now,
    };
    let response_elapsed = hours_between(&submitted_at, &response_end);

    let resolution_end = match &di.converted_at {
        Some(s) => parse_iso(s)?,
        None => now,
    };
    let resolution_elapsed = hours_between(&submitted_at, &resolution_end);

    let is_response_breached =
        di.screened_at.is_none() && response_elapsed > rule.target_response_hours as f64;
    let is_resolution_breached =
        di.converted_at.is_none() && resolution_elapsed > rule.target_resolution_hours as f64;

    let response_remaining_hours = if di.screened_at.is_none() {
        Some((rule.target_response_hours as f64 - response_elapsed).max(0.0))
    } else {
        None
    };
    let resolution_remaining_hours = if di.converted_at.is_none() && !is_sla_terminal(di) {
        Some((rule.target_resolution_hours as f64 - resolution_elapsed).max(0.0))
    } else {
        None
    };

    let status = Some(derive_lifecycle_status(
        di,
        rule.escalation_threshold_hours,
        response_elapsed,
        resolution_elapsed,
        is_response_breached,
        is_resolution_breached,
    ));

    Ok(DiSlaStatus {
        rule_id: Some(rule.id),
        target_response_hours: Some(rule.target_response_hours),
        target_resolution_hours: Some(rule.target_resolution_hours),
        sla_deadline: Some(sla_deadline),
        resolution_deadline: Some(resolution_deadline),
        response_elapsed_hours: Some(response_elapsed),
        resolution_elapsed_hours: Some(resolution_elapsed),
        response_remaining_hours,
        resolution_remaining_hours,
        is_response_breached,
        is_resolution_breached,
        status,
    })
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

    let rules: Vec<DiSlaRule> = rows
        .iter()
        .map(map_sla_rule)
        .collect::<AppResult<Vec<_>>>()?;

    if let Some(crit) = criticality_class {
        if let Some(rule) = rules.iter().find(|r| {
            r.origin_type.as_deref() == Some(origin_type)
                && r
                    .asset_criticality_class
                    .as_ref()
                    .is_some_and(|rc| criticality_codes_match(rc, crit))
        }) {
            return Ok(Some(rule.clone()));
        }
    }

    if let Some(rule) = rules.iter().find(|r| {
        r.origin_type.as_deref() == Some(origin_type) && r.asset_criticality_class.is_none()
    }) {
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

/// Freeze SLA targets/deadlines on a DI once. Idempotent (`sla_response_deadline IS NULL`).
pub async fn freeze_sla_on_di(
    db: &impl ConnectionTrait,
    di: &InterventionRequest,
) -> AppResult<InterventionRequest> {
    if di_has_frozen_sla(di) {
        return Ok(di.clone());
    }

    let criticality_class = lookup_asset_criticality(db, di.asset_id).await?;
    let rule = resolve_sla_rule(
        db,
        &di.reported_urgency,
        &di.origin_type,
        criticality_class.as_deref(),
    )
    .await?;

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
            rule.id.into(),
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
    updated.sla_rule_id = Some(rule.id);
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
pub async fn compute_sla_status(
    db: &impl ConnectionTrait,
    di: &InterventionRequest,
) -> AppResult<DiSlaStatus> {
    if di_has_frozen_sla(di) {
        return status_from_frozen(di);
    }

    let criticality_class = lookup_asset_criticality(db, di.asset_id).await?;
    let rule = resolve_sla_rule(
        db,
        &di.reported_urgency,
        &di.origin_type,
        criticality_class.as_deref(),
    )
    .await?;

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
        return Ok((
            di.sla_target_response_hours,
            di.sla_response_deadline.clone(),
        ));
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

    let submitted_at = parse_iso(&di.submitted_at)?;
    let now = Utc::now().naive_utc();
    let target_response = status.target_response_hours.unwrap() as f64;
    let target_resolution = status.target_resolution_hours.unwrap_or(0) as f64;

    let response_late = match &di.screened_at {
        Some(s) => hours_between(&submitted_at, &parse_iso(s)?) > target_response,
        None => hours_between(&submitted_at, &now) > target_response,
    };

    let resolution_late = match &di.converted_at {
        Some(s) => hours_between(&submitted_at, &parse_iso(s)?) > target_resolution,
        None => {
            if is_sla_terminal(di) {
                false
            } else {
                hours_between(&submitted_at, &now) > target_resolution
            }
        }
    };

    if is_sla_terminal(di) {
        if response_late || resolution_late {
            return Ok(SlaHistoricalOutcome::Breached);
        }
        return Ok(SlaHistoricalOutcome::Met);
    }

    if response_late || resolution_late {
        return Ok(SlaHistoricalOutcome::Breached);
    }

    Ok(SlaHistoricalOutcome::InFlight)
}

/// Mark response breach as notified (idempotent).
pub async fn mark_response_breach_notified(
    db: &impl ConnectionTrait,
    di_id: i64,
) -> AppResult<()> {
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
pub async fn mark_resolution_breach_notified(
    db: &impl ConnectionTrait,
    di_id: i64,
) -> AppResult<()> {
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
            "SELECT * FROM di_sla_rules ORDER BY urgency_level, origin_type".to_string(),
        ))
        .await?;

    rows.iter().map(map_sla_rule).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) update_sla_rule
// ═══════════════════════════════════════════════════════════════════════════════

/// Update an existing SLA rule. Permission check (di.admin) is in the command layer.
pub async fn update_sla_rule(
    db: &impl ConnectionTrait,
    input: SlaRuleUpdateInput,
) -> AppResult<DiSlaRule> {
    let valid_urgencies = ["low", "medium", "high", "critical"];
    if !valid_urgencies.contains(&input.urgency_level.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Niveau d'urgence invalide : '{}'. Valeurs autorisées : low, medium, high, critical.",
            input.urgency_level
        )]));
    }

    if input.target_response_hours <= 0
        || input.target_resolution_hours <= 0
        || input.escalation_threshold_hours <= 0
    {
        return Err(AppError::ValidationFailed(vec![
            "Les heures cibles doivent être supérieures à zéro.".into(),
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
                input.urgency_level.into(),
                input
                    .origin_type
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                input
                    .asset_criticality_class
                    .map(sea_orm::Value::from)
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
