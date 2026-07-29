use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::loader::LoadProgress;
use crate::reliability::readiness::blocking::EvaluationProfile;
use crate::reliability::readiness::evaluate::{evaluate_asset, evaluate_asset_dual, AssetReadinessReport, EvaluateAssetInput};
use crate::reliability::readiness::event::ReadinessEvent;
use crate::reliability::readiness::fmucd::config::FmucdMappingConfig;
use crate::reliability::readiness::report::{DatasetSummary, LoaderRowStats};

#[derive(Debug)]
pub struct AggregatedDataset {
    pub assets: HashMap<String, Vec<ReadinessEvent>>,
    pub row_stats: LoaderRowStats,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub exposure_hours: f64,
    pub min_sample_n: i64,
}

pub fn aggregate_by_asset(load: LoadProgress, config: &FmucdMappingConfig) -> AggregatedDataset {
    let mut period_start = Utc::now();
    let mut period_end = DateTime::<Utc>::MIN_UTC;
    for events in load.assets.values() {
        for e in events {
            if e.event_ts < period_start {
                period_start = e.event_ts;
            }
            if e.event_ts > period_end {
                period_end = e.event_ts;
            }
        }
    }
    if period_end < period_start {
        period_end = period_start;
    }

    AggregatedDataset {
        assets: load.assets,
        row_stats: load.stats,
        period_start,
        period_end,
        exposure_hours: config.evaluation.exposure_hours_external,
        min_sample_n: config.evaluation.min_sample_n,
    }
}

pub fn evaluate_dataset_documentary(
    dataset: &AggregatedDataset,
) -> Vec<AssetReadinessReport> {
    evaluate_dataset_profile(dataset, EvaluationProfile::CmmsDocumentary)
}

pub fn evaluate_dataset_strict(dataset: &AggregatedDataset) -> Vec<AssetReadinessReport> {
    evaluate_dataset_profile(dataset, EvaluationProfile::StrictRam)
}

fn evaluate_dataset_profile(
    dataset: &AggregatedDataset,
    profile: EvaluationProfile,
) -> Vec<AssetReadinessReport> {
    let mut reports = Vec::with_capacity(dataset.assets.len());
    for (asset_key, events) in &dataset.assets {
        let input = EvaluateAssetInput {
            asset_key: asset_key.clone(),
            university_id: events.first().and_then(|e| e.university_id.clone()),
            events: events.clone(),
            period_start: dataset.period_start,
            period_end: dataset.period_end,
            exposure_hours: dataset.exposure_hours,
            min_sample_n: dataset.min_sample_n,
            profile,
            inspection_coverage_ratio: None,
        };
        reports.push(evaluate_asset(&input));
    }
    reports
}

pub fn evaluate_dataset_dual(
    dataset: &AggregatedDataset,
) -> (Vec<AssetReadinessReport>, Vec<AssetReadinessReport>) {
    let mut documentary = Vec::new();
    let mut strict = Vec::new();
    for (asset_key, events) in &dataset.assets {
        let base = EvaluateAssetInput {
            asset_key: asset_key.clone(),
            university_id: events.first().and_then(|e| e.university_id.clone()),
            events: events.clone(),
            period_start: dataset.period_start,
            period_end: dataset.period_end,
            exposure_hours: dataset.exposure_hours,
            min_sample_n: dataset.min_sample_n,
            profile: EvaluationProfile::CmmsDocumentary,
            inspection_coverage_ratio: None,
        };
        let (doc, str) = evaluate_asset_dual(&base);
        documentary.push(doc);
        strict.push(str);
    }
    (documentary, strict)
}

pub fn summarize_by_university(reports: &[AssetReadinessReport]) -> Vec<crate::reliability::readiness::report::UniversitySummary> {
    use std::collections::HashMap;
    use crate::reliability::readiness::report::UniversitySummary;

    let mut by_uni: HashMap<String, Vec<&AssetReadinessReport>> = HashMap::new();
    for r in reports {
        let uid = r
            .university_id
            .clone()
            .unwrap_or_else(|| "unknown".into());
        by_uni.entry(uid).or_default().push(r);
    }

    let mut out: Vec<UniversitySummary> = by_uni
        .into_iter()
        .map(|(university_id, reps)| {
            let n = reps.len() as u64;
            let eligible = reps.iter().filter(|r| r.eligible_event_count > 0).count() as u64;
            let doc_ready = reps.iter().filter(|r| r.documentary_ready).count() as u64;
            let strict_ready = reps.iter().filter(|r| r.strict_ready).count() as u64;
            let mean_c: f64 = reps.iter().map(|r| r.completeness.completeness_percent).sum::<f64>()
                / n.max(1) as f64;
            let mean_dq: f64 =
                reps.iter().map(|r| r.data_quality_score).sum::<f64>() / n.max(1) as f64;
            UniversitySummary {
                university_id,
                asset_count: n,
                eligible_asset_count: eligible,
                documentary_ready_pct: if eligible > 0 {
                    doc_ready as f64 / eligible as f64 * 100.0
                } else {
                    0.0
                },
                strict_ready_pct: if eligible > 0 {
                    strict_ready as f64 / eligible as f64 * 100.0
                } else {
                    0.0
                },
                mean_completeness_pct: mean_c,
                mean_dq,
            }
        })
        .collect();
    out.sort_by(|a, b| a.university_id.cmp(&b.university_id));
    out
}

pub fn dataset_summary(
    profile: EvaluationProfile,
    reports: &[AssetReadinessReport],
) -> DatasetSummary {
    DatasetSummary::from_reports(profile, reports)
}
