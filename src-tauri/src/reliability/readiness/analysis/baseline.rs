use serde::{Deserialize, Serialize};

use crate::reliability::readiness::evaluate::AssetReadinessReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineScenario {
    pub scenario: String,
    pub asset_count: u64,
    pub pct_of_all_assets: f64,
    pub pct_of_eligible: f64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineComparison {
    pub total_assets: u64,
    pub eligible_assets: u64,
    pub min_sample_n: i64,
    pub scenarios: Vec<BaselineScenario>,
}

/// Compare naive entry rules vs framework gating.
#[must_use]
pub fn compute_baseline_comparison(
    doc_reports: &[AssetReadinessReport],
    strict_reports: &[AssetReadinessReport],
    min_sample_n: i64,
) -> BaselineComparison {
    let total = doc_reports.len() as u64;
    let eligible = doc_reports
        .iter()
        .filter(|r| r.eligible_event_count > 0)
        .count() as u64;
    let denom_all = total.max(1) as f64;
    let denom_eligible = eligible.max(1) as f64;
    let n_min = min_sample_n.max(1);

    let naive_any_event = eligible;
    let naive_mtbf_sample = doc_reports
        .iter()
        .filter(|r| r.eligible_event_count >= n_min)
        .count() as u64;
    let framework_primary = doc_reports
        .iter()
        .filter(|r| r.documentary_ready)
        .count() as u64;
    let framework_strict = strict_reports
        .iter()
        .filter(|r| r.strict_ready)
        .count() as u64;

    let scenarios = vec![
        BaselineScenario {
            scenario: "naive_any_upm_event".to_string(),
            asset_count: naive_any_event,
            pct_of_all_assets: naive_any_event as f64 / denom_all * 100.0,
            pct_of_eligible: 100.0,
            description: "Asset enters analysis if at least one UPM (unplanned) work order exists."
                .to_string(),
        },
        BaselineScenario {
            scenario: "naive_mtbf_sample_n".to_string(),
            asset_count: naive_mtbf_sample,
            pct_of_all_assets: naive_mtbf_sample as f64 / denom_all * 100.0,
            pct_of_eligible: naive_mtbf_sample as f64 / denom_eligible * 100.0,
            description: format!(
                "Asset has at least N_f >= {n_min} eligible events (no completeness/DQ gate)."
            ),
        },
        BaselineScenario {
            scenario: "framework_documentary".to_string(),
            asset_count: framework_primary,
            pct_of_all_assets: framework_primary as f64 / denom_all * 100.0,
            pct_of_eligible: framework_primary as f64 / denom_eligible * 100.0,
            description: "Passes documentary readiness (DQ + completeness + no blocking issues)."
                .to_string(),
        },
        BaselineScenario {
            scenario: "framework_strict_ram".to_string(),
            asset_count: framework_strict,
            pct_of_all_assets: framework_strict as f64 / denom_all * 100.0,
            pct_of_eligible: framework_strict as f64 / denom_eligible * 100.0,
            description: "Passes strict RAM readiness (exposure + analysis_ready + green badge)."
                .to_string(),
        },
    ];

    BaselineComparison {
        total_assets: total,
        eligible_assets: eligible,
        min_sample_n: n_min,
        scenarios,
    }
}
