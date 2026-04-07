//! DI SLA engine — rule resolution, deadline computation, and breach detection.
//!
//! Phase 2 - Sub-phase 04 - File 03 - Sprint S1.
//!
//! SLA rules are admin-managed reference data in `di_sla_rules`. The engine
//! resolves the most specific matching rule for a given urgency + origin +
//! criticality combination and computes breach flags from DI timestamps.

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

/// Computed SLA status for a single DI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiSlaStatus {
    pub rule_id: Option<i64>,
    pub target_response_hours: Option<i64>,
    pub target_resolution_hours: Option<i64>,
    pub sla_deadline: Option<String>,
    pub response_elapsed_hours: Option<f64>,
    pub resolution_elapsed_hours: Option<f64>,
    pub is_response_breached: bool,
    pub is_resolution_breached: bool,
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
    // Fetch all active rules for this urgency level
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

    // Priority 1: exact match (urgency + origin + criticality)
    if let Some(crit) = criticality_class {
        if let Some(rule) = rules.iter().find(|r| {
            r.origin_type.as_deref() == Some(origin_type)
                && r.asset_criticality_class.as_deref() == Some(crit)
        }) {
            return Ok(Some(rule.clone()));
        }
    }

    // Priority 2: partial match (urgency + origin, NULL class)
    if let Some(rule) = rules.iter().find(|r| {
        r.origin_type.as_deref() == Some(origin_type)
            && r.asset_criticality_class.is_none()
    }) {
        return Ok(Some(rule.clone()));
    }

    // Priority 3: broad match (urgency only, NULL origin, NULL class)
    if let Some(rule) = rules.iter().find(|r| {
        r.origin_type.is_none() && r.asset_criticality_class.is_none()
    }) {
        return Ok(Some(rule.clone()));
    }

    Ok(None)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) compute_sla_status — deadline and breach computation for a DI
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the SLA status for an intervention request.
///
/// Resolves the applicable SLA rule, calculates deadlines and elapsed times,
/// and determines whether response or resolution SLAs have been breached.
pub async fn compute_sla_status(
    db: &impl ConnectionTrait,
    di: &InterventionRequest,
) -> AppResult<DiSlaStatus> {
    // Attempt to look up asset criticality class from asset_registry
    let criticality_class: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT criticality_class FROM asset_registry WHERE id = ?",
            [di.asset_id.into()],
        ))
        .await?
        .and_then(|row| {
            row.try_get::<Option<String>>("", "criticality_class")
                .ok()
                .flatten()
        });

    let rule = resolve_sla_rule(
        db,
        &di.reported_urgency,
        &di.origin_type,
        criticality_class.as_deref(),
    )
    .await?;

    let Some(rule) = rule else {
        // No matching SLA rule — return neutral status
        return Ok(DiSlaStatus {
            rule_id: None,
            target_response_hours: None,
            target_resolution_hours: None,
            sla_deadline: None,
            response_elapsed_hours: None,
            resolution_elapsed_hours: None,
            is_response_breached: false,
            is_resolution_breached: false,
        });
    };

    let submitted_at = parse_iso(&di.submitted_at)?;
    let now = Utc::now().naive_utc();

    // SLA deadline = submitted_at + target_response_hours
    let deadline = submitted_at + Duration::hours(rule.target_response_hours);
    let sla_deadline = deadline.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Response elapsed: submitted_at → screened_at (or now if not yet screened)
    let response_end = match &di.screened_at {
        Some(s) => parse_iso(s)?,
        None => now,
    };
    let response_elapsed = hours_between(&submitted_at, &response_end);

    // Resolution elapsed: submitted_at → converted_at (or now if not yet converted)
    let resolution_end = match &di.converted_at {
        Some(s) => parse_iso(s)?,
        None => now,
    };
    let resolution_elapsed = hours_between(&submitted_at, &resolution_end);

    // Breach: only flag if the milestone has NOT been reached
    let is_response_breached =
        di.screened_at.is_none() && response_elapsed > rule.target_response_hours as f64;
    let is_resolution_breached =
        di.converted_at.is_none() && resolution_elapsed > rule.target_resolution_hours as f64;

    Ok(DiSlaStatus {
        rule_id: Some(rule.id),
        target_response_hours: Some(rule.target_response_hours),
        target_resolution_hours: Some(rule.target_resolution_hours),
        sla_deadline: Some(sla_deadline),
        response_elapsed_hours: Some(response_elapsed),
        resolution_elapsed_hours: Some(resolution_elapsed),
        is_response_breached,
        is_resolution_breached,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) list_sla_rules
// ═══════════════════════════════════════════════════════════════════════════════

/// List all SLA rules (active and inactive).
pub async fn list_sla_rules(
    db: &impl ConnectionTrait,
) -> AppResult<Vec<DiSlaRule>> {
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
    // Validate urgency_level
    let valid_urgencies = ["low", "medium", "high", "critical"];
    if !valid_urgencies.contains(&input.urgency_level.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Niveau d'urgence invalide : '{}'. Valeurs autorisées : low, medium, high, critical.",
            input.urgency_level
        )]));
    }

    // Validate target hours are positive
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

    // Re-fetch the updated row
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
