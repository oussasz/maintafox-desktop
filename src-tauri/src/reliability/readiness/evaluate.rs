use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::badge::evaluate_badge;
use super::badge::BadgeResult;
use super::blocking::{evaluate_issues, BlockingInput, EvaluationProfile, ReadinessIssue};
use super::completeness::IsoCompletenessResult;
use super::event::ReadinessEvent;
use super::policy::{COMPLETENESS_GREEN_THRESHOLD, DQ_GREEN_THRESHOLD, MIN_SAMPLE_N_DEFAULT};
use crate::reliability::compute::{compute_reliability_kpis, KpiFailureEvent, ReliabilityKpiComputeInput};

/// Per-asset readiness evaluation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReadinessReport {
    pub asset_key: String,
    pub university_id: Option<String>,
    pub profile: EvaluationProfile,
    pub completeness: IsoCompletenessResult,
    pub data_quality_score: f64,
    pub eligible_event_count: i64,
    pub exposure_hours: f64,
    pub analysis_ready: bool,
    pub badge: BadgeResult,
    pub issues: Vec<ReadinessIssue>,
    /// Primary documentary readiness (completeness + DQ + no documentary blocking).
    pub documentary_ready: bool,
    /// Strict RAM readiness (analysis_ready + green badge under strict profile).
    pub strict_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateAssetInput {
    pub asset_key: String,
    pub university_id: Option<String>,
    pub events: Vec<ReadinessEvent>,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub exposure_hours: f64,
    pub min_sample_n: i64,
    pub profile: EvaluationProfile,
    pub inspection_coverage_ratio: Option<f64>,
}

#[must_use]
pub fn evaluate_asset(input: &EvaluateAssetInput) -> AssetReadinessReport {
    let min_n = input.min_sample_n.max(1);
    let kpi_events: Vec<KpiFailureEvent> = input
        .events
        .iter()
        .map(|e| KpiFailureEvent {
            id: e.id,
            event_ts: e.event_ts,
            eligible_flags_json: e.eligible_flags_json.clone(),
            downtime_duration_hours: e.downtime_duration_hours,
            active_repair_hours: e.active_repair_hours,
            failure_mode_id: e.failure_mode_id,
        })
        .collect();

    let kpis = compute_reliability_kpis(&ReliabilityKpiComputeInput {
        period_start: input.period_start,
        period_end: input.period_end,
        t_exp_hours: input.exposure_hours.max(0.0),
        repeat_lookback_days: 30,
        min_sample_n: min_n,
        events: kpi_events,
    });

    let completeness = super::completeness::evaluate_iso14224_completeness(&input.events);

    let eligible_missing_mode = input
        .events
        .iter()
        .filter(|e| e.eligible && !e.failure_mode_coded)
        .count() as i64;

    let blocking_input = BlockingInput {
        exposure_hours: input.exposure_hours,
        data_quality_score: kpis.data_quality_score,
        eligible_event_count: kpis.event_count,
        min_sample_n: min_n,
        eligible_missing_failure_mode_count: eligible_missing_mode,
        inspection_coverage_ratio: input.inspection_coverage_ratio,
    };

    let issues = evaluate_issues(&blocking_input, input.profile);
    let badge = evaluate_badge(Some(kpis.data_quality_score), &issues);

    let analysis_ready = input.exposure_hours > 0.0 && kpis.event_count >= min_n;

    let documentary_blocking = issues
        .iter()
        .any(|i| i.severity == "blocking" && i.issue_code != "MISSING_EXPOSURE");

    let documentary_ready = !documentary_blocking
        && kpis.data_quality_score >= DQ_GREEN_THRESHOLD
        && completeness.completeness_percent >= COMPLETENESS_GREEN_THRESHOLD;

    let strict_ready = analysis_ready && badge.badge == "green";

    AssetReadinessReport {
        asset_key: input.asset_key.clone(),
        university_id: input.university_id.clone(),
        profile: input.profile,
        completeness,
        data_quality_score: kpis.data_quality_score,
        eligible_event_count: kpis.event_count,
        exposure_hours: input.exposure_hours,
        analysis_ready,
        badge,
        issues,
        documentary_ready,
        strict_ready,
    }
}

/// Evaluate one asset under both profiles (dual reporting).
#[must_use]
pub fn evaluate_asset_dual(input_base: &EvaluateAssetInput) -> (AssetReadinessReport, AssetReadinessReport) {
    let mut doc = input_base.clone();
    doc.profile = EvaluationProfile::CmmsDocumentary;
    let documentary = evaluate_asset(&doc);

    let mut strict = input_base.clone();
    strict.profile = EvaluationProfile::StrictRam;
    let strict_report = evaluate_asset(&strict);

    (documentary, strict_report)
}

impl EvaluateAssetInput {
    #[must_use]
    pub fn with_defaults(
        asset_key: String,
        events: Vec<ReadinessEvent>,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        exposure_hours: f64,
        profile: EvaluationProfile,
    ) -> Self {
        let university_id = events.first().and_then(|e| e.university_id.clone());
        Self {
            asset_key,
            university_id,
            events,
            period_start,
            period_end,
            exposure_hours,
            min_sample_n: MIN_SAMPLE_N_DEFAULT,
            profile,
            inspection_coverage_ratio: None,
        }
    }
}
