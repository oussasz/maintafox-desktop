use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::reliability::readiness::blocking::blocking_codes;
use crate::reliability::readiness::evaluate::AssetReadinessReport;
use crate::reliability::readiness::policy::{COMPLETENESS_GREEN_THRESHOLD, DQ_GREEN_THRESHOLD};

/// Exclusive waterfall bucket (one reason per not-ready asset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WaterfallReason {
    DocumentaryReady,
    MissingFailureMode,
    LowDq,
    IntervalGap,
    FailureModeGap,
    CorrectiveGap,
    EquipmentIdGap,
    MultipleGates,
}

impl WaterfallReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DocumentaryReady => "DOCUMENTARY_READY",
            Self::MissingFailureMode => "MISSING_FAILURE_MODE",
            Self::LowDq => "LOW_DQ",
            Self::IntervalGap => "INTERVAL_GAP",
            Self::FailureModeGap => "FAILURE_MODE_GAP",
            Self::CorrectiveGap => "CORRECTIVE_GAP",
            Self::EquipmentIdGap => "EQUIPMENT_ID_GAP",
            Self::MultipleGates => "MULTIPLE_GATES",
        }
    }
}

/// Multi-label overlap gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OverlapGate {
    MissingFailureMode,
    LowDq,
    LowCompleteness,
    IntervalGap,
    FailureModeGap,
    CorrectiveGap,
    EquipmentIdGap,
}

impl OverlapGate {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingFailureMode => "MISSING_FAILURE_MODE",
            Self::LowDq => "LOW_DQ",
            Self::LowCompleteness => "LOW_COMPLETENESS",
            Self::IntervalGap => "INTERVAL_GAP",
            Self::FailureModeGap => "FAILURE_MODE_GAP",
            Self::CorrectiveGap => "CORRECTIVE_GAP",
            Self::EquipmentIdGap => "EQUIPMENT_ID_GAP",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaterfallBucket {
    pub reason: String,
    pub asset_count: u64,
    pub pct_of_eligible: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapBucket {
    pub gate: String,
    pub asset_count: u64,
    pub pct_of_eligible: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionMethodology {
    pub denominator: String,
    pub eligible_asset_count: u64,
    pub waterfall_priority: Vec<String>,
    pub thresholds: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionOutput {
    pub methodology: DecompositionMethodology,
    pub waterfall: Vec<WaterfallBucket>,
    pub overlap: Vec<OverlapBucket>,
}

fn has_missing_failure_mode(report: &AssetReadinessReport) -> bool {
    blocking_codes(&report.issues)
        .iter()
        .any(|c| c == "MISSING_FAILURE_MODE")
}

fn has_low_dq(report: &AssetReadinessReport) -> bool {
    report.data_quality_score < DQ_GREEN_THRESHOLD
}

fn has_low_completeness(report: &AssetReadinessReport) -> bool {
    report.completeness.completeness_percent < COMPLETENESS_GREEN_THRESHOLD
}

fn weakest_dimension_below_threshold(report: &AssetReadinessReport) -> Option<WaterfallReason> {
    let c = &report.completeness;
    let threshold = COMPLETENESS_GREEN_THRESHOLD;

    let dims: [(WaterfallReason, f64); 4] = [
        (WaterfallReason::IntervalGap, c.dim_failure_interval_pct),
        (WaterfallReason::FailureModeGap, c.dim_failure_mode_pct),
        (WaterfallReason::CorrectiveGap, c.dim_corrective_closure_pct),
        (WaterfallReason::EquipmentIdGap, c.dim_equipment_id_pct),
    ];

    let below: Vec<_> = dims.iter().filter(|(_, v)| *v < threshold).collect();
    if below.is_empty() {
        return None;
    }

    below
        .into_iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(reason, _)| *reason)
}

/// Assign exclusive primary reason for a not-ready eligible asset.
#[must_use]
pub fn assign_waterfall_reason(report: &AssetReadinessReport) -> WaterfallReason {
    if report.documentary_ready {
        return WaterfallReason::DocumentaryReady;
    }

    if has_missing_failure_mode(report) {
        return WaterfallReason::MissingFailureMode;
    }

    let low_dq = has_low_dq(report);
    let low_c = has_low_completeness(report);

    if low_dq && !low_c {
        return WaterfallReason::LowDq;
    }

    if low_c && !low_dq {
        return weakest_dimension_below_threshold(report).unwrap_or(WaterfallReason::MultipleGates);
    }

    if low_dq && low_c {
        return WaterfallReason::MultipleGates;
    }

    WaterfallReason::MultipleGates
}

/// Collect overlap gates for an eligible asset (multi-label).
#[must_use]
pub fn collect_overlap_gates(report: &AssetReadinessReport) -> Vec<OverlapGate> {
    let mut gates = Vec::new();
    if has_missing_failure_mode(report) {
        gates.push(OverlapGate::MissingFailureMode);
    }
    if has_low_dq(report) {
        gates.push(OverlapGate::LowDq);
    }
    if has_low_completeness(report) {
        gates.push(OverlapGate::LowCompleteness);
    }
    let c = &report.completeness;
    if c.dim_failure_interval_pct < COMPLETENESS_GREEN_THRESHOLD {
        gates.push(OverlapGate::IntervalGap);
    }
    if c.dim_failure_mode_pct < COMPLETENESS_GREEN_THRESHOLD {
        gates.push(OverlapGate::FailureModeGap);
    }
    if c.dim_corrective_closure_pct < COMPLETENESS_GREEN_THRESHOLD {
        gates.push(OverlapGate::CorrectiveGap);
    }
    if c.dim_equipment_id_pct < COMPLETENESS_GREEN_THRESHOLD {
        gates.push(OverlapGate::EquipmentIdGap);
    }
    gates
}

/// Compute waterfall and overlap decomposition for documentary profile reports.
#[must_use]
pub fn compute_decomposition(reports: &[AssetReadinessReport]) -> DecompositionOutput {
    let eligible: Vec<&AssetReadinessReport> = reports.iter().filter(|r| r.eligible_event_count > 0).collect();
    let eligible_n = eligible.len() as u64;
    let denom = eligible_n.max(1) as f64;

    let mut waterfall_counts: HashMap<WaterfallReason, u64> = HashMap::new();
    let mut overlap_counts: HashMap<OverlapGate, u64> = HashMap::new();

    for report in &eligible {
        let reason = assign_waterfall_reason(report);
        *waterfall_counts.entry(reason).or_insert(0) += 1;

        if !report.documentary_ready {
            for gate in collect_overlap_gates(report) {
                *overlap_counts.entry(gate).or_insert(0) += 1;
            }
        }
    }

    let waterfall_order = [
        WaterfallReason::DocumentaryReady,
        WaterfallReason::MissingFailureMode,
        WaterfallReason::LowDq,
        WaterfallReason::IntervalGap,
        WaterfallReason::FailureModeGap,
        WaterfallReason::CorrectiveGap,
        WaterfallReason::EquipmentIdGap,
        WaterfallReason::MultipleGates,
    ];

    let waterfall: Vec<WaterfallBucket> = waterfall_order
        .iter()
        .filter_map(|reason| {
            let count = waterfall_counts.get(reason).copied().unwrap_or(0);
            if count == 0 && *reason != WaterfallReason::DocumentaryReady {
                return None;
            }
            Some(WaterfallBucket {
                reason: reason.as_str().to_string(),
                asset_count: count,
                pct_of_eligible: count as f64 / denom * 100.0,
            })
        })
        .collect();

    let overlap_order = [
        OverlapGate::MissingFailureMode,
        OverlapGate::LowDq,
        OverlapGate::LowCompleteness,
        OverlapGate::IntervalGap,
        OverlapGate::FailureModeGap,
        OverlapGate::CorrectiveGap,
        OverlapGate::EquipmentIdGap,
    ];

    let overlap: Vec<OverlapBucket> = overlap_order
        .iter()
        .filter_map(|gate| {
            let count = overlap_counts.get(gate).copied().unwrap_or(0);
            if count == 0 {
                return None;
            }
            Some(OverlapBucket {
                gate: gate.as_str().to_string(),
                asset_count: count,
                pct_of_eligible: count as f64 / denom * 100.0,
            })
        })
        .collect();

    let mut thresholds = HashMap::new();
    thresholds.insert("dq_green".to_string(), DQ_GREEN_THRESHOLD);
    thresholds.insert("completeness_green".to_string(), COMPLETENESS_GREEN_THRESHOLD);

    DecompositionOutput {
        methodology: DecompositionMethodology {
            denominator: "assets_with_eligible_events".to_string(),
            eligible_asset_count: eligible_n,
            waterfall_priority: vec![
                "MISSING_FAILURE_MODE".into(),
                "LOW_DQ".into(),
                "LOW_COMPLETENESS (weakest dimension)".into(),
                "MULTIPLE_GATES".into(),
            ],
            thresholds,
        },
        waterfall,
        overlap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reliability::readiness::blocking::{EvaluationProfile, ReadinessIssue};
    use crate::reliability::readiness::completeness::IsoCompletenessResult;

    fn make_report(
        eligible_n: i64,
        dq: f64,
        c_pct: f64,
        dim_int: f64,
        dim_mode: f64,
        issues: Vec<ReadinessIssue>,
        doc_ready: bool,
    ) -> AssetReadinessReport {
        AssetReadinessReport {
            asset_key: "test".into(),
            university_id: None,
            profile: EvaluationProfile::CmmsDocumentary,
            completeness: IsoCompletenessResult {
                event_count: eligible_n.max(1),
                completeness_percent: c_pct,
                dim_equipment_id_pct: 100.0,
                dim_failure_interval_pct: dim_int,
                dim_failure_mode_pct: dim_mode,
                dim_corrective_closure_pct: 100.0,
            },
            data_quality_score: dq,
            eligible_event_count: eligible_n,
            exposure_hours: 0.0,
            analysis_ready: false,
            badge: crate::reliability::readiness::badge::BadgeResult {
                badge: "yellow".into(),
                blocking_issue_codes: vec![],
            },
            issues,
            documentary_ready: doc_ready,
            strict_ready: false,
        }
    }

    #[test]
    fn waterfall_low_dq_exclusive() {
        let r = make_report(2, 0.2, 90.0, 90.0, 100.0, vec![], false);
        assert_eq!(assign_waterfall_reason(&r), WaterfallReason::LowDq);
    }

    #[test]
    fn waterfall_interval_gap_when_dq_ok() {
        let r = make_report(10, 1.0, 70.0, 40.0, 100.0, vec![], false);
        assert_eq!(assign_waterfall_reason(&r), WaterfallReason::IntervalGap);
    }

    #[test]
    fn waterfall_sums_to_eligible_population() {
        let reports = vec![
            make_report(10, 1.0, 95.0, 90.0, 100.0, vec![], true),
            make_report(2, 0.2, 90.0, 90.0, 100.0, vec![], false),
            make_report(10, 1.0, 70.0, 40.0, 100.0, vec![], false),
        ];
        let out = compute_decomposition(&reports);
        let total: u64 = out.waterfall.iter().map(|b| b.asset_count).sum();
        assert_eq!(total, 3);
        let pct_sum: f64 = out.waterfall.iter().map(|b| b.pct_of_eligible).sum();
        assert!((pct_sum - 100.0).abs() < 0.01);
    }
}
