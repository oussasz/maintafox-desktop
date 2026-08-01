use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::reliability::readiness::blocking::EvaluationProfile;
use crate::reliability::readiness::evaluate::AssetReadinessReport;
use crate::reliability::readiness::fmucd::{
    aggregate_by_asset, evaluate_dataset_dual, load_fmucd_csv, AggregatedDataset, FmucdMappingConfig,
};
use crate::reliability::readiness::report::DatasetSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NminSensitivityRow {
    pub n_min: i64,
    pub documentary_ready_pct: f64,
    pub strict_ready_pct: f64,
    pub mean_dq: f64,
    pub badge_green_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingSensitivityRow {
    pub mapping_variant: String,
    pub config_path: String,
    pub documentary_ready_pct: f64,
    pub strict_ready_pct: f64,
    pub mean_completeness_pct: f64,
    pub mean_interval_dim_pct: f64,
    pub missing_failure_mode_block_pct: f64,
}

fn evaluate_with_nmin(
    dataset: &AggregatedDataset,
    n_min: i64,
) -> (Vec<AssetReadinessReport>, Vec<AssetReadinessReport>) {
    use crate::reliability::readiness::evaluate::{evaluate_asset_dual, EvaluateAssetInput};

    let mut documentary = Vec::with_capacity(dataset.assets.len());
    let mut strict = Vec::with_capacity(dataset.assets.len());

    for (asset_key, events) in &dataset.assets {
        let base = EvaluateAssetInput {
            asset_key: asset_key.clone(),
            university_id: events.first().and_then(|e| e.university_id.clone()),
            events: events.clone(),
            period_start: dataset.period_start,
            period_end: dataset.period_end,
            exposure_hours: dataset.exposure_hours,
            min_sample_n: n_min,
            profile: EvaluationProfile::CmmsDocumentary,
            inspection_coverage_ratio: None,
        };
        let (doc, str) = evaluate_asset_dual(&base);
        documentary.push(doc);
        strict.push(str);
    }

    (documentary, strict)
}

/// Re-evaluate dataset at multiple N_min values without re-loading CSV.
#[must_use]
pub fn run_nmin_sensitivity(dataset: &AggregatedDataset, n_values: &[i64]) -> Vec<NminSensitivityRow> {
    n_values
        .iter()
        .map(|&n_min| {
            let (doc_reports, strict_reports) = evaluate_with_nmin(dataset, n_min);
            let doc_summary = DatasetSummary::from_reports(EvaluationProfile::CmmsDocumentary, &doc_reports);
            let strict_summary = DatasetSummary::from_reports(EvaluationProfile::StrictRam, &strict_reports);
            let badge_green_pct = doc_summary.badge_green_count as f64 / doc_summary.total_assets.max(1) as f64 * 100.0;

            NminSensitivityRow {
                n_min,
                documentary_ready_pct: doc_summary.documentary_ready_pct,
                strict_ready_pct: strict_summary.strict_ready_pct,
                mean_dq: doc_summary.mean_dq,
                badge_green_pct,
            }
        })
        .collect()
}

/// Load and evaluate CSV under each mapping config variant.
pub fn run_mapping_sensitivity(
    csv_path: &Path,
    configs: &[(&str, &Path)],
) -> anyhow::Result<Vec<MappingSensitivityRow>> {
    let mut rows = Vec::new();
    for (variant, config_path) in configs {
        let config = FmucdMappingConfig::from_toml_file(config_path)?;
        let load = load_fmucd_csv(csv_path, &config)?;
        let dataset = aggregate_by_asset(load, &config);
        let (doc_reports, strict_reports) = evaluate_dataset_dual(&dataset);
        let doc_summary = DatasetSummary::from_reports(EvaluationProfile::CmmsDocumentary, &doc_reports);
        let strict_summary = DatasetSummary::from_reports(EvaluationProfile::StrictRam, &strict_reports);

        let missing_mode = doc_reports
            .iter()
            .filter(|r| {
                r.issues
                    .iter()
                    .any(|i| i.issue_code == "MISSING_FAILURE_MODE" && i.severity == "blocking")
            })
            .count();
        let missing_mode_pct = missing_mode as f64 / doc_reports.len().max(1) as f64 * 100.0;

        rows.push(MappingSensitivityRow {
            mapping_variant: (*variant).to_string(),
            config_path: config_path.display().to_string(),
            documentary_ready_pct: doc_summary.documentary_ready_pct,
            strict_ready_pct: strict_summary.strict_ready_pct,
            mean_completeness_pct: doc_summary.mean_completeness_pct,
            mean_interval_dim_pct: doc_summary.mean_dimensions.dim_failure_interval_pct,
            missing_failure_mode_block_pct: missing_mode_pct,
        });
    }
    Ok(rows)
}
