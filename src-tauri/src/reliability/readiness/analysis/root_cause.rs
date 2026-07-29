use serde::{Deserialize, Serialize};

use super::baseline::BaselineComparison;
use super::decomposition::DecompositionOutput;
use crate::reliability::readiness::report::DatasetSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseNode {
    pub label: String,
    pub asset_count: Option<u64>,
    pub pct: Option<f64>,
    pub children: Vec<RootCauseNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeceptiveAggregate {
    pub mean_global_completeness_pct: f64,
    pub mean_interval_dimension_pct: f64,
    pub gap_pp: f64,
    pub interpretation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseOutput {
    pub strict_tree: RootCauseNode,
    pub primary_tree: RootCauseNode,
    pub deceptive_aggregate: DeceptiveAggregate,
    pub discussion_snippet_md: String,
}

/// Build root-cause trees and discussion snippet from analysis outputs.
#[must_use]
pub fn build_root_cause(
    doc_summary: &DatasetSummary,
    strict_summary: &DatasetSummary,
    decomposition: &DecompositionOutput,
    baseline: &BaselineComparison,
) -> RootCauseOutput {
    let total = doc_summary.total_assets;
    let eligible = doc_summary.assets_with_eligible_events;
    let strict_exposure = strict_summary
        .blocking_breakdown
        .get("MISSING_EXPOSURE")
        .copied()
        .unwrap_or(total);

    let strict_tree = RootCauseNode {
        label: format!("All assets ({total})"),
        asset_count: Some(total),
        pct: Some(100.0),
        children: vec![
            RootCauseNode {
                label: format!("MISSING_EXPOSURE ({strict_exposure})"),
                asset_count: Some(strict_exposure),
                pct: Some(strict_exposure as f64 / total.max(1) as f64 * 100.0),
                children: vec![RootCauseNode {
                    label: format!(
                        "Strict-ready: {} ({:.1}%)",
                        strict_summary.strict_ready_count, strict_summary.strict_ready_pct
                    ),
                    asset_count: Some(strict_summary.strict_ready_count),
                    pct: Some(strict_summary.strict_ready_pct),
                    children: vec![],
                }],
            },
        ],
    };

    let ready_pct = doc_summary.documentary_ready_pct;
    let fail_pct = 100.0 - ready_pct;

    let mut failure_children: Vec<RootCauseNode> = decomposition
        .waterfall
        .iter()
        .filter(|b| b.reason != "DOCUMENTARY_READY")
        .map(|b| RootCauseNode {
            label: format!("{} ({})", b.reason, b.asset_count),
            asset_count: Some(b.asset_count),
            pct: Some(b.pct_of_eligible),
            children: vec![],
        })
        .collect();

    failure_children.sort_by(|a, b| {
        b.pct
            .unwrap_or(0.0)
            .partial_cmp(&a.pct.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let primary_tree = RootCauseNode {
        label: format!("Eligible assets with UPM events ({eligible})"),
        asset_count: Some(eligible),
        pct: Some(100.0),
        children: vec![
            RootCauseNode {
                label: format!(
                    "Documentary-ready: {} ({ready_pct:.1}%)",
                    doc_summary.documentary_ready_count
                ),
                asset_count: Some(doc_summary.documentary_ready_count),
                pct: Some(ready_pct),
                children: vec![],
            },
            RootCauseNode {
                label: format!("Not ready ({fail_pct:.1}%)"),
                asset_count: Some(eligible - doc_summary.documentary_ready_count),
                pct: Some(fail_pct),
                children: failure_children,
            },
        ],
    };

    let mean_c = doc_summary.mean_completeness_pct;
    let mean_int = doc_summary.mean_dimensions.dim_failure_interval_pct;
    let deceptive_aggregate = DeceptiveAggregate {
        mean_global_completeness_pct: mean_c,
        mean_interval_dimension_pct: mean_int,
        gap_pp: mean_c - mean_int,
        interpretation: "Mean global completeness appears acceptable while the interval dimension remains critically low — aggregate scores can mask field-level gaps.".to_string(),
    };

    let naive = baseline
        .scenarios
        .iter()
        .find(|s| s.scenario == "naive_any_upm_event")
        .map(|s| s.pct_of_all_assets)
        .unwrap_or(0.0);
    let framework = baseline
        .scenarios
        .iter()
        .find(|s| s.scenario == "framework_documentary")
        .map(|s| s.pct_of_eligible)
        .unwrap_or(ready_pct);

    let top_failures: String = decomposition
        .waterfall
        .iter()
        .filter(|b| b.reason != "DOCUMENTARY_READY")
        .take(3)
        .map(|b| format!("- **{}**: {:.1}% of eligible assets", b.reason, b.pct_of_eligible))
        .collect::<Vec<_>>()
        .join("\n");

    let discussion_snippet_md = format!(
        r#"## Discussion snippet (auto-generated from PoC run)

### Why strict readiness is 0%

All {total} assets fail strict RAM readiness because operational exposure is absent from the external CMMS export (`MISSING_EXPOSURE` blocks {strict_exposure} assets, 100%). Without runtime exposure logs, native MTBF/availability computation cannot proceed regardless of event richness.

### Why primary readiness is {ready_pct:.1}%

Among {eligible} assets with at least one unplanned maintenance event, only {ready_count} ({ready_pct:.1}%) pass documentary readiness (DQ ≥ threshold AND completeness ≥ 85% AND no blocking data-quality issues). The remaining {fail_pct:.1}% fail primarily due to:

{top_failures}

### Deceptive aggregate completeness

Mean global completeness C = **{mean_c:.1}%** while mean interval dimension = **{mean_int:.1}%** (gap = {gap:.1} pp). This supports the claim that a single aggregate score can overstate readiness when critical dimensions (especially failure time intervals) remain incomplete.

### Baseline comparison

Without a readiness framework, **{naive:.1}%** of all assets would enter reliability analysis (any UPM event). With the framework, only **{framework:.1}%** of eligible assets qualify — a {reduction:.1} percentage-point reduction in assets that would otherwise produce unstable or misleading KPIs.
"#,
        total = total,
        strict_exposure = strict_exposure,
        ready_pct = ready_pct,
        eligible = eligible,
        ready_count = doc_summary.documentary_ready_count,
        fail_pct = fail_pct,
        top_failures = top_failures,
        mean_c = mean_c,
        mean_int = mean_int,
        gap = mean_c - mean_int,
        naive = naive,
        framework = framework,
        reduction = naive - (framework * eligible as f64 / total.max(1) as f64),
    );

    RootCauseOutput {
        strict_tree,
        primary_tree,
        deceptive_aggregate,
        discussion_snippet_md,
    }
}
