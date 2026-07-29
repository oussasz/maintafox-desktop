use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::blocking::EvaluationProfile;
use super::completeness::IsoCompletenessResult;
use super::evaluate::AssetReadinessReport;

/// Dataset-level summary for PoC outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSummary {
    pub profile: EvaluationProfile,
    pub total_assets: u64,
    pub assets_with_events: u64,
    pub assets_with_eligible_events: u64,
    pub documentary_ready_count: u64,
    pub documentary_ready_pct: f64,
    pub strict_ready_count: u64,
    pub strict_ready_pct: f64,
    pub badge_green_count: u64,
    pub badge_yellow_count: u64,
    pub badge_red_count: u64,
    pub mean_completeness_pct: f64,
    pub mean_dq: f64,
    pub blocking_breakdown: HashMap<String, u64>,
    pub mean_dimensions: IsoCompletenessResult,
}

impl DatasetSummary {
    #[must_use]
    pub fn from_reports(profile: EvaluationProfile, reports: &[AssetReadinessReport]) -> Self {
        let total_assets = reports.len() as u64;
        let mut assets_with_events = 0_u64;
        let mut assets_with_eligible = 0_u64;
        let mut documentary_ready_count = 0_u64;
        let mut strict_ready_count = 0_u64;
        let mut badge_green = 0_u64;
        let mut badge_yellow = 0_u64;
        let mut badge_red = 0_u64;
        let mut blocking_breakdown: HashMap<String, u64> = HashMap::new();
        let mut sum_c = 0.0_f64;
        let mut sum_dq = 0.0_f64;
        let mut sum_eq = 0.0_f64;
        let mut sum_int = 0.0_f64;
        let mut sum_mode = 0.0_f64;
        let mut sum_corr = 0.0_f64;
        let mut sum_events = 0_i64;

        for r in reports {
            if r.completeness.event_count > 0 {
                assets_with_events += 1;
            }
            if r.eligible_event_count > 0 {
                assets_with_eligible += 1;
            }
            if r.documentary_ready {
                documentary_ready_count += 1;
            }
            if r.strict_ready {
                strict_ready_count += 1;
            }
            match r.badge.badge.as_str() {
                "green" => badge_green += 1,
                "yellow" => badge_yellow += 1,
                _ => badge_red += 1,
            }
            for issue in &r.issues {
                if issue.severity == "blocking" {
                    *blocking_breakdown.entry(issue.issue_code.clone()).or_insert(0) += 1;
                }
            }
            sum_c += r.completeness.completeness_percent;
            sum_dq += r.data_quality_score;
            sum_eq += r.completeness.dim_equipment_id_pct;
            sum_int += r.completeness.dim_failure_interval_pct;
            sum_mode += r.completeness.dim_failure_mode_pct;
            sum_corr += r.completeness.dim_corrective_closure_pct;
            sum_events += r.completeness.event_count;
        }

        let n = total_assets.max(1) as f64;
        let mean_dimensions = IsoCompletenessResult {
            event_count: sum_events,
            completeness_percent: sum_c / n,
            dim_equipment_id_pct: sum_eq / n,
            dim_failure_interval_pct: sum_int / n,
            dim_failure_mode_pct: sum_mode / n,
            dim_corrective_closure_pct: sum_corr / n,
        };

        DatasetSummary {
            profile,
            total_assets,
            assets_with_events,
            assets_with_eligible_events: assets_with_eligible,
            documentary_ready_count,
            documentary_ready_pct: if assets_with_eligible > 0 {
                documentary_ready_count as f64 / assets_with_eligible as f64 * 100.0
            } else {
                0.0
            },
            strict_ready_count,
            strict_ready_pct: if assets_with_eligible > 0 {
                strict_ready_count as f64 / assets_with_eligible as f64 * 100.0
            } else {
                0.0
            },
            badge_green_count: badge_green,
            badge_yellow_count: badge_yellow,
            badge_red_count: badge_red,
            mean_completeness_pct: sum_c / n,
            mean_dq: sum_dq / n,
            blocking_breakdown,
            mean_dimensions,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversitySummary {
    pub university_id: String,
    pub asset_count: u64,
    pub eligible_asset_count: u64,
    pub documentary_ready_pct: f64,
    pub strict_ready_pct: f64,
    pub mean_completeness_pct: f64,
    pub mean_dq: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: String,
    pub timestamp_utc: String,
    pub git_commit: Option<String>,
    pub config_path: String,
    pub csv_path: Option<String>,
    pub profiles: Vec<String>,
    pub row_stats: LoaderRowStats,
    pub policy: PolicyManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyManifest {
    pub min_sample_n: i64,
    pub dq_green_threshold: f64,
    pub completeness_green_threshold: f64,
    pub exposure_lookback_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoaderRowStats {
    pub rows_read: u64,
    pub rows_mapped: u64,
    pub rows_skipped_empty_asset: u64,
    pub rows_skipped_parse_error: u64,
    pub rows_ppm_excluded_from_eligible: u64,
    pub unique_assets: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualRunOutput {
    pub manifest: RunManifest,
    pub documentary_summary: DatasetSummary,
    pub strict_summary: DatasetSummary,
    pub by_university_documentary: Vec<UniversitySummary>,
    pub by_university_strict: Vec<UniversitySummary>,
}
