//! In-app RAM readiness gate (audit P1-5).
//!
//! Maps native `failure_events` + the governed exposure basis onto the same pure evaluator used by the
//! `rams-readiness-eval` CLI (`readiness::evaluate_asset`, StrictRam profile) and turns the report into
//! an ALLOW / DENY decision. Weibull fits, R(t) and RUL are refused on DENY.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reliability::compute::eligible_unplanned_mtbf;
use crate::reliability::exposure_inference::load_exposure_basis;
use crate::reliability::readiness::{
    evaluate_asset, AssetReadinessReport, EvaluateAssetInput, EvaluationProfile, ReadinessEvent, MIN_SAMPLE_N_DEFAULT,
};

pub const DECISION_ALLOW: &str = "ALLOW";
pub const DECISION_DENY: &str = "DENY";

/// Blocking reason emitted when the eligible event count is below `min_sample_n`.
pub const REASON_INSUFFICIENT_EVENTS: &str = "INSUFFICIENT_EVENTS";

/// Default analysis window when the caller gives none (same window as RUL / Weibull dashboard).
pub const DEFAULT_WINDOW_DAYS: i64 = 365;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityReadinessInput {
    pub equipment_id: i64,
    #[serde(default)]
    pub period_start: Option<String>,
    #[serde(default)]
    pub period_end: Option<String>,
    #[serde(default)]
    pub min_sample_n: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityReadinessGate {
    pub equipment_id: i64,
    pub period_start: String,
    pub period_end: String,
    /// `ALLOW` or `DENY`.
    pub decision: String,
    /// Machine codes (`MISSING_EXPOSURE`, `MISSING_FAILURE_MODE`, `INSUFFICIENT_EVENTS`); empty on ALLOW.
    pub blocking_reasons: Vec<String>,
    /// Non-blocking issue codes (e.g. `LOW_SAMPLE`).
    pub warnings: Vec<String>,
    pub eligible_event_count: i64,
    pub min_sample_n: i64,
    pub exposure_hours: f64,
    /// `None` when the equipment has neither an operating schedule nor runtime logs.
    pub exposure_source: Option<String>,
    pub data_quality_score: f64,
    pub completeness_percent: f64,
    pub badge: String,
}

impl ReliabilityReadinessGate {
    #[must_use]
    pub fn allowed(&self) -> bool {
        self.decision == DECISION_ALLOW
    }

    /// Error returned by analyses that must not run on DENY.
    #[must_use]
    pub fn deny_error(&self) -> AppError {
        AppError::ValidationFailed(self.blocking_reasons.iter().map(|r| deny_refusal(r)).collect())
    }
}

/// The coded refusal of one blocking reason of the readiness gate: the three reasons the gate can
/// raise each have their own text; any other reason rides as a parameter of the generic code.
fn deny_refusal(reason: &str) -> String {
    match reason {
        "MISSING_EXPOSURE" => "GATE_REL_READINESS_NO_EXPOSURE: no runtime exposure for this equipment.".to_string(),
        "MISSING_FAILURE_MODE" => {
            "GATE_REL_READINESS_NO_FAILURE_MODE: eligible events without a failure mode.".to_string()
        }
        REASON_INSUFFICIENT_EVENTS => {
            "GATE_REL_READINESS_FEW_EVENTS: not enough eligible events in the period.".to_string()
        }
        other => format!("GATE_REL_READINESS_DENIED:reason={other}: the reliability data is not ready ({other})."),
    }
}

/// Pure decision from a StrictRam report: DENY on any blocking issue or when the eligible sample is
/// below `min_sample_n`. Reasons are deduplicated and ordered as first seen.
#[must_use]
pub fn decide(report: &AssetReadinessReport, min_sample_n: i64) -> (bool, Vec<String>, Vec<String>) {
    let mut blocking: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for issue in &report.issues {
        let bucket = if issue.severity == "blocking" {
            &mut blocking
        } else {
            &mut warnings
        };
        if !bucket.contains(&issue.issue_code) {
            bucket.push(issue.issue_code.clone());
        }
    }
    if report.exposure_hours <= 0.0 && !blocking.iter().any(|c| c == "MISSING_EXPOSURE") {
        blocking.push("MISSING_EXPOSURE".into());
    }
    if report.eligible_event_count < min_sample_n.max(1) {
        blocking.push(REASON_INSUFFICIENT_EVENTS.into());
    }
    (blocking.is_empty(), blocking, warnings)
}

fn parse_dt(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::ValidationFailed(vec![format!("GATE_REL_DATE_INVALID: invalid timestamp '{s}': {e}")]))
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("readiness_gate decode '{field}': {err}"))
}

async fn load_readiness_events(
    db: &DatabaseConnection,
    equipment_id: i64,
    p0: &str,
    p1: &str,
) -> AppResult<Vec<ReadinessEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT fe.id, COALESCE(fe.failed_at, fe.detected_at, fe.created_at) AS ev_ts,
                    fe.eligible_flags_json, fe.failure_mode_id,
                    fe.downtime_duration_hours, fe.active_repair_hours,
                    CASE WHEN (fe.failed_at IS NOT NULL OR fe.detected_at IS NOT NULL)
                              AND fe.restored_at IS NOT NULL THEN 1 ELSE 0 END AS interval_complete,
                    CASE WHEN fe.restored_at IS NOT NULL
                              OR (fe.source_type = 'work_order' AND wo.closed_at IS NOT NULL)
                         THEN 1 ELSE 0 END AS corrective_documented
             FROM failure_events fe
             LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
             WHERE fe.equipment_id = ?
               AND COALESCE(fe.failed_at, fe.detected_at, fe.created_at) >= ?
               AND COALESCE(fe.failed_at, fe.detected_at, fe.created_at) <= ?
             ORDER BY ev_ts ASC, fe.id ASC",
            [equipment_id.into(), p0.into(), p1.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let ts_s: String = r.try_get("", "ev_ts").map_err(|e| decode_err("ev_ts", e))?;
        let flags: String = r
            .try_get("", "eligible_flags_json")
            .map_err(|e| decode_err("eligible_flags_json", e))?;
        let failure_mode_id: Option<i64> = r
            .try_get("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?;
        let interval_complete: i64 = r
            .try_get("", "interval_complete")
            .map_err(|e| decode_err("interval_complete", e))?;
        let corrective_documented: i64 = r
            .try_get("", "corrective_documented")
            .map_err(|e| decode_err("corrective_documented", e))?;
        out.push(ReadinessEvent {
            id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            asset_key: equipment_id.to_string(),
            university_id: None,
            event_ts: parse_dt(&ts_s)?,
            eligible: eligible_unplanned_mtbf(&flags),
            equipment_identified: true,
            interval_complete: interval_complete != 0,
            failure_mode_coded: failure_mode_id.is_some(),
            corrective_documented: corrective_documented != 0,
            eligible_flags_json: flags,
            failure_mode_id,
            downtime_duration_hours: r
                .try_get("", "downtime_duration_hours")
                .map_err(|e| decode_err("downtime_duration_hours", e))?,
            active_repair_hours: r
                .try_get("", "active_repair_hours")
                .map_err(|e| decode_err("active_repair_hours", e))?,
        });
    }
    Ok(out)
}

/// Evaluate the StrictRam readiness gate for one equipment over `[period_start, period_end]`
/// (default: the last [`DEFAULT_WINDOW_DAYS`] days). `min_sample_n` defaults to
/// [`MIN_SAMPLE_N_DEFAULT`] — the policy SSOT shared with the CLI.
pub async fn evaluate_equipment_readiness(
    db: &DatabaseConnection,
    input: ReliabilityReadinessInput,
) -> AppResult<ReliabilityReadinessGate> {
    let now = Utc::now();
    let p1 = match input.period_end.as_deref() {
        Some(s) => parse_dt(s)?,
        None => now,
    };
    let p0 = match input.period_start.as_deref() {
        Some(s) => parse_dt(s)?,
        None => p1 - Duration::days(DEFAULT_WINDOW_DAYS),
    };
    if p1 < p0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_REL_PERIOD_ORDER: period_end before period_start.".into(),
        ]));
    }
    // A caller may ask for a stricter sample, never a looser one than the governed default.
    let min_n = input
        .min_sample_n
        .unwrap_or(MIN_SAMPLE_N_DEFAULT)
        .max(MIN_SAMPLE_N_DEFAULT);
    let p0s = p0.to_rfc3339_opts(SecondsFormat::Secs, true);
    let p1s = p1.to_rfc3339_opts(SecondsFormat::Secs, true);

    let basis = load_exposure_basis(db, input.equipment_id).await?;
    let exposure_end = if p1 < now { p1 } else { now };
    let exposure_hours = basis.as_ref().map_or(0.0, |b| b.hours_between(p0, exposure_end));
    let events = load_readiness_events(db, input.equipment_id, &p0s, &p1s).await?;

    let report = evaluate_asset(&EvaluateAssetInput {
        asset_key: input.equipment_id.to_string(),
        university_id: None,
        events,
        period_start: p0,
        period_end: p1,
        exposure_hours,
        min_sample_n: min_n,
        profile: EvaluationProfile::StrictRam,
        inspection_coverage_ratio: None,
    });
    let (allow, blocking_reasons, warnings) = decide(&report, min_n);

    Ok(ReliabilityReadinessGate {
        equipment_id: input.equipment_id,
        period_start: p0s,
        period_end: p1s,
        decision: if allow { DECISION_ALLOW } else { DECISION_DENY }.into(),
        blocking_reasons,
        warnings,
        eligible_event_count: report.eligible_event_count,
        min_sample_n: min_n,
        exposure_hours,
        exposure_source: basis.map(|b| b.source().to_string()),
        data_quality_score: report.data_quality_score,
        completeness_percent: report.completeness.completeness_percent,
        badge: report.badge.badge,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: i64, day: u32, eligible: bool, coded: bool) -> ReadinessEvent {
        ReadinessEvent {
            id,
            asset_key: "1".into(),
            university_id: None,
            event_ts: parse_dt(&format!("2026-01-{day:02}T00:00:00Z")).unwrap(),
            eligible,
            equipment_identified: true,
            interval_complete: true,
            failure_mode_coded: coded,
            corrective_documented: true,
            eligible_flags_json: ReadinessEvent::eligible_flags_json(eligible),
            failure_mode_id: coded.then_some(7),
            downtime_duration_hours: 2.0,
            active_repair_hours: 1.0,
        }
    }

    fn report(events: Vec<ReadinessEvent>, exposure: f64) -> AssetReadinessReport {
        evaluate_asset(&EvaluateAssetInput {
            asset_key: "1".into(),
            university_id: None,
            events,
            period_start: parse_dt("2026-01-01T00:00:00Z").unwrap(),
            period_end: parse_dt("2026-02-01T00:00:00Z").unwrap(),
            exposure_hours: exposure,
            min_sample_n: 5,
            profile: EvaluationProfile::StrictRam,
            inspection_coverage_ratio: None,
        })
    }

    #[test]
    fn allow_when_exposure_and_sample_met() {
        let r = report((1..=5).map(|i| ev(i, i as u32, true, true)).collect(), 500.0);
        let (allow, blocking, _) = decide(&r, 5);
        assert!(allow, "{blocking:?}");
    }

    #[test]
    fn deny_on_missing_exposure_and_small_sample() {
        let r = report(vec![ev(1, 1, true, true)], 0.0);
        let (allow, blocking, warnings) = decide(&r, 5);
        assert!(!allow);
        assert_eq!(
            blocking,
            vec!["MISSING_EXPOSURE".to_string(), REASON_INSUFFICIENT_EVENTS.to_string()]
        );
        assert!(warnings.contains(&"LOW_SAMPLE".to_string()));
    }

    #[test]
    fn deny_on_uncoded_eligible_failure() {
        let mut events: Vec<ReadinessEvent> = (1..=5).map(|i| ev(i, i as u32, true, true)).collect();
        events.push(ev(6, 6, true, false));
        let (allow, blocking, _) = decide(&report(events, 500.0), 5);
        assert!(!allow);
        assert_eq!(blocking, vec!["MISSING_FAILURE_MODE".to_string()]);
    }

    #[test]
    fn planned_events_do_not_count_toward_sample() {
        let mut events: Vec<ReadinessEvent> = (1..=4).map(|i| ev(i, i as u32, true, true)).collect();
        events.push(ev(5, 5, false, true));
        let (allow, blocking, _) = decide(&report(events, 500.0), 5);
        assert!(!allow);
        assert_eq!(blocking, vec![REASON_INSUFFICIENT_EVENTS.to_string()]);
    }
}
