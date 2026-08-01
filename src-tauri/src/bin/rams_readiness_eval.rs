//! CLI for RAM data-readiness PoC (ground-truth fixtures + FMUCD evaluation).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use maintafox_lib::reliability::readiness::analysis::{
    build_root_cause, compute_baseline_comparison, compute_decomposition, run_mapping_sensitivity, run_nmin_sensitivity,
};
use maintafox_lib::reliability::readiness::blocking::EvaluationProfile;
use maintafox_lib::reliability::readiness::fmucd::{
    aggregate_by_asset, evaluate_dataset_dual, load_fmucd_csv, summarize_by_university, FmucdMappingConfig,
};
use maintafox_lib::reliability::readiness::policy::{
    COMPLETENESS_GREEN_THRESHOLD, DQ_GREEN_THRESHOLD, EXPOSURE_LOOKBACK_DAYS,
};
use maintafox_lib::reliability::readiness::report::{DatasetSummary, DualRunOutput, PolicyManifest, RunManifest};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "rams-readiness-eval")]
#[command(about = "Evaluate RAM data-readiness on fixtures or FMUCD CSV")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run golden readiness fixtures (Stage 1 ground truth).
    EvaluateFixtures {
        #[arg(long, default_value = "src-tauri/tests/fixtures/rams_golden/v1")]
        fixtures_dir: PathBuf,
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Evaluate FMUCD CSV (Stage 2 real CMMS data).
    EvaluateFmucd {
        #[arg(long)]
        csv: PathBuf,
        #[arg(long, default_value = "research/readiness-poc/config/fmucd_mapping.v1.toml")]
        config: PathBuf,
        #[arg(long, value_enum, default_value = "both")]
        profile: ProfileArg,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, default_value_t = true)]
        with_analysis: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ProfileArg {
    Primary,
    Secondary,
    Both,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::EvaluateFixtures {
            fixtures_dir,
            output_dir,
        } => run_evaluate_fixtures(&fixtures_dir, output_dir.as_deref())?,
        Commands::EvaluateFmucd {
            csv,
            config,
            profile,
            output_dir,
            with_analysis,
        } => run_evaluate_fmucd(&csv, &config, profile, &output_dir, with_analysis)?,
    }
    Ok(())
}

fn run_evaluate_fixtures(fixtures_dir: &Path, output_dir: Option<&Path>) -> Result<()> {
    info!(dir = %fixtures_dir.display(), "Running cargo test readiness_golden");
    let status = Command::new("cargo")
        .args(["test", "--test", "readiness_golden", "--", "--nocapture"])
        .current_dir(workspace_root()?)
        .status()
        .context("spawn cargo test readiness_golden")?;
    if !status.success() {
        anyhow::bail!("readiness_golden tests failed");
    }
    if let Some(out) = output_dir {
        fs::create_dir_all(out)?;
        let summary = serde_json::json!({
            "stage": 1,
            "status": "passed",
            "fixtures_dir": fixtures_dir.display().to_string(),
            "timestamp_utc": Utc::now().to_rfc3339(),
        });
        fs::write(out.join("stage1_summary.json"), serde_json::to_string_pretty(&summary)?)?;
    }
    info!("Stage 1 fixtures: all tests passed");
    Ok(())
}

fn run_evaluate_fmucd(
    csv: &Path,
    config_path: &Path,
    profile: ProfileArg,
    output_dir: &Path,
    with_analysis: bool,
) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    let figures_dir = output_dir.join("figures");
    fs::create_dir_all(&figures_dir)?;

    let config = FmucdMappingConfig::from_toml_file(config_path)?;
    info!(csv = %csv.display(), "Loading FMUCD");
    let load = load_fmucd_csv(csv, &config)?;
    let dataset = aggregate_by_asset(load, &config);

    let (doc_reports, strict_reports) = evaluate_dataset_dual(&dataset);
    let doc_summary = DatasetSummary::from_reports(EvaluationProfile::CmmsDocumentary, &doc_reports);
    let strict_summary = DatasetSummary::from_reports(EvaluationProfile::StrictRam, &strict_reports);

    let by_uni_doc = summarize_by_university(&doc_reports);
    let by_uni_strict = summarize_by_university(&strict_reports);

    let run_id = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let manifest = RunManifest {
        run_id: run_id.clone(),
        timestamp_utc: Utc::now().to_rfc3339(),
        git_commit: git_commit_hash(),
        config_path: config_path.display().to_string(),
        csv_path: Some(csv.display().to_string()),
        profiles: match profile {
            ProfileArg::Primary => vec!["cmms_documentary".into()],
            ProfileArg::Secondary => vec!["strict_ram".into()],
            ProfileArg::Both => vec!["cmms_documentary".into(), "strict_ram".into()],
        },
        row_stats: dataset.row_stats.clone(),
        policy: PolicyManifest {
            min_sample_n: config.evaluation.min_sample_n,
            dq_green_threshold: DQ_GREEN_THRESHOLD,
            completeness_green_threshold: COMPLETENESS_GREEN_THRESHOLD,
            exposure_lookback_days: EXPOSURE_LOOKBACK_DAYS,
        },
    };

    let dual = DualRunOutput {
        manifest,
        documentary_summary: doc_summary.clone(),
        strict_summary: strict_summary.clone(),
        by_university_documentary: by_uni_doc.clone(),
        by_university_strict: by_uni_strict,
    };

    fs::write(output_dir.join("summary.json"), serde_json::to_string_pretty(&dual)?)?;

    write_university_csv(&output_dir.join("by_university_documentary.csv"), &by_uni_doc)?;
    write_blocking_csv(
        &output_dir.join("blocking_breakdown_documentary.csv"),
        &doc_summary.blocking_breakdown,
    )?;
    write_blocking_csv(
        &output_dir.join("blocking_breakdown_strict.csv"),
        &strict_summary.blocking_breakdown,
    )?;
    write_asset_sample(&output_dir.join("by_asset_sample.csv"), &doc_reports, &strict_reports)?;

    fs::write(
        output_dir.join("run_manifest.json"),
        serde_json::to_string_pretty(&dual.manifest)?,
    )?;

    print_summary(&doc_summary, &strict_summary);

    if with_analysis {
        run_scientific_analysis(
            csv,
            config_path,
            &dataset,
            &doc_reports,
            &strict_reports,
            &doc_summary,
            &strict_summary,
            output_dir,
        )?;
    }

    info!(dir = %output_dir.display(), "FMUCD evaluation complete");
    Ok(())
}

fn run_scientific_analysis(
    csv: &Path,
    config_path: &Path,
    dataset: &maintafox_lib::reliability::readiness::fmucd::AggregatedDataset,
    doc_reports: &[maintafox_lib::reliability::readiness::evaluate::AssetReadinessReport],
    strict_reports: &[maintafox_lib::reliability::readiness::evaluate::AssetReadinessReport],
    doc_summary: &DatasetSummary,
    strict_summary: &DatasetSummary,
    output_dir: &Path,
) -> Result<()> {
    let analysis_dir = output_dir.join("analysis");
    fs::create_dir_all(&analysis_dir)?;

    info!("Running scientific analysis layer");

    let decomposition = compute_decomposition(doc_reports);
    fs::write(
        analysis_dir.join("decomposition.json"),
        serde_json::to_string_pretty(&decomposition)?,
    )?;
    write_decomposition_csvs(&analysis_dir, &decomposition)?;

    let baseline = compute_baseline_comparison(doc_reports, strict_reports, dataset.min_sample_n);
    fs::write(
        analysis_dir.join("baseline_comparison.json"),
        serde_json::to_string_pretty(&baseline)?,
    )?;

    let nmin_rows = run_nmin_sensitivity(dataset, &[3, 5, 10]);
    write_nmin_sensitivity_csv(&analysis_dir.join("sensitivity_nmin.csv"), &nmin_rows)?;

    let config_dir = config_path.parent().context("config parent dir")?;
    let mapping_configs: Vec<(&str, PathBuf)> = vec![
        ("strict", config_dir.join("fmucd_mapping.strict.toml")),
        ("baseline", config_path.to_path_buf()),
        ("relaxed", config_dir.join("fmucd_mapping.relaxed.toml")),
    ];
    let mapping_refs: Vec<(&str, &Path)> = mapping_configs
        .iter()
        .map(|(name, path)| (*name, path.as_path()))
        .collect();
    let mapping_rows = run_mapping_sensitivity(csv, &mapping_refs)?;
    write_mapping_sensitivity_csv(&analysis_dir.join("sensitivity_mapping.csv"), &mapping_rows)?;

    let root_cause = build_root_cause(doc_summary, strict_summary, &decomposition, &baseline);
    fs::write(
        analysis_dir.join("root_cause.json"),
        serde_json::to_string_pretty(&root_cause)?,
    )?;
    fs::write(
        analysis_dir.join("DISCUSSION_SNIPPET.md"),
        &root_cause.discussion_snippet_md,
    )?;

    print_analysis_summary(&decomposition, &baseline, &nmin_rows, &mapping_rows);
    info!(dir = %analysis_dir.display(), "Scientific analysis complete");
    Ok(())
}

fn write_decomposition_csvs(
    analysis_dir: &Path,
    decomposition: &maintafox_lib::reliability::readiness::analysis::DecompositionOutput,
) -> Result<()> {
    let mut w = csv::Writer::from_path(analysis_dir.join("decomposition_waterfall.csv"))?;
    w.write_record(["reason", "asset_count", "pct_of_eligible"])?;
    for b in &decomposition.waterfall {
        w.write_record([
            b.reason.as_str(),
            &b.asset_count.to_string(),
            &format!("{:.2}", b.pct_of_eligible),
        ])?;
    }
    w.flush()?;

    let mut w = csv::Writer::from_path(analysis_dir.join("decomposition_overlap.csv"))?;
    w.write_record(["gate", "asset_count", "pct_of_eligible"])?;
    for b in &decomposition.overlap {
        w.write_record([
            b.gate.as_str(),
            &b.asset_count.to_string(),
            &format!("{:.2}", b.pct_of_eligible),
        ])?;
    }
    w.flush()?;
    Ok(())
}

fn write_nmin_sensitivity_csv(
    path: &Path,
    rows: &[maintafox_lib::reliability::readiness::analysis::NminSensitivityRow],
) -> Result<()> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record([
        "n_min",
        "documentary_ready_pct",
        "strict_ready_pct",
        "mean_dq",
        "badge_green_pct",
    ])?;
    for r in rows {
        w.write_record([
            &r.n_min.to_string(),
            &format!("{:.2}", r.documentary_ready_pct),
            &format!("{:.2}", r.strict_ready_pct),
            &format!("{:.4}", r.mean_dq),
            &format!("{:.2}", r.badge_green_pct),
        ])?;
    }
    w.flush()?;
    Ok(())
}

fn write_mapping_sensitivity_csv(
    path: &Path,
    rows: &[maintafox_lib::reliability::readiness::analysis::MappingSensitivityRow],
) -> Result<()> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record([
        "mapping_variant",
        "config_path",
        "documentary_ready_pct",
        "strict_ready_pct",
        "mean_completeness_pct",
        "mean_interval_dim_pct",
        "missing_failure_mode_block_pct",
    ])?;
    for r in rows {
        w.write_record([
            r.mapping_variant.as_str(),
            r.config_path.as_str(),
            &format!("{:.2}", r.documentary_ready_pct),
            &format!("{:.2}", r.strict_ready_pct),
            &format!("{:.2}", r.mean_completeness_pct),
            &format!("{:.2}", r.mean_interval_dim_pct),
            &format!("{:.2}", r.missing_failure_mode_block_pct),
        ])?;
    }
    w.flush()?;
    Ok(())
}

fn print_analysis_summary(
    decomposition: &maintafox_lib::reliability::readiness::analysis::DecompositionOutput,
    baseline: &maintafox_lib::reliability::readiness::analysis::BaselineComparison,
    nmin_rows: &[maintafox_lib::reliability::readiness::analysis::NminSensitivityRow],
    mapping_rows: &[maintafox_lib::reliability::readiness::analysis::MappingSensitivityRow],
) {
    println!("\n=== SCIENTIFIC ANALYSIS ===");
    println!("Waterfall decomposition (% of eligible assets):");
    for b in &decomposition.waterfall {
        println!("  {}: {:.1}% ({} assets)", b.reason, b.pct_of_eligible, b.asset_count);
    }
    println!("Overlap gates (non-exclusive):");
    for b in &decomposition.overlap {
        println!("  {}: {:.1}%", b.gate, b.pct_of_eligible);
    }
    println!("Baseline comparison:");
    for s in &baseline.scenarios {
        println!(
            "  {}: {} assets ({:.1}% all, {:.1}% eligible)",
            s.scenario, s.asset_count, s.pct_of_all_assets, s.pct_of_eligible
        );
    }
    println!("N_min sensitivity:");
    for r in nmin_rows {
        println!(
            "  N={}: primary {:.1}%, strict {:.1}%",
            r.n_min, r.documentary_ready_pct, r.strict_ready_pct
        );
    }
    println!("Mapping sensitivity:");
    for r in mapping_rows {
        println!(
            "  {}: primary {:.1}%, interval dim {:.1}%",
            r.mapping_variant, r.documentary_ready_pct, r.mean_interval_dim_pct
        );
    }
}

fn write_university_csv(
    path: &Path,
    rows: &[maintafox_lib::reliability::readiness::report::UniversitySummary],
) -> Result<()> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record([
        "university_id",
        "asset_count",
        "eligible_asset_count",
        "documentary_ready_pct",
        "strict_ready_pct",
        "mean_completeness_pct",
        "mean_dq",
    ])?;
    for r in rows {
        w.write_record([
            r.university_id.as_str(),
            &r.asset_count.to_string(),
            &r.eligible_asset_count.to_string(),
            &format!("{:.2}", r.documentary_ready_pct),
            &format!("{:.2}", r.strict_ready_pct),
            &format!("{:.2}", r.mean_completeness_pct),
            &format!("{:.4}", r.mean_dq),
        ])?;
    }
    w.flush()?;
    Ok(())
}

fn write_blocking_csv(path: &Path, breakdown: &std::collections::HashMap<String, u64>) -> Result<()> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record(["issue_code", "asset_count"])?;
    let mut pairs: Vec<_> = breakdown.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1));
    for (code, count) in pairs {
        w.write_record([code.as_str(), &count.to_string()])?;
    }
    w.flush()?;
    Ok(())
}

fn write_asset_sample(
    path: &Path,
    doc: &[maintafox_lib::reliability::readiness::evaluate::AssetReadinessReport],
    strict: &[maintafox_lib::reliability::readiness::evaluate::AssetReadinessReport],
) -> Result<()> {
    let mut w = csv::Writer::from_path(path)?;
    w.write_record([
        "asset_key",
        "university_id",
        "eligible_events",
        "completeness_pct",
        "dq",
        "doc_badge",
        "doc_ready",
        "strict_badge",
        "strict_ready",
        "blocking_strict",
    ])?;
    let n = doc.len().min(200);
    for i in 0..n {
        let d = &doc[i];
        let s = &strict[i];
        w.write_record([
            d.asset_key.as_str(),
            d.university_id.as_deref().unwrap_or(""),
            &d.eligible_event_count.to_string(),
            &format!("{:.2}", d.completeness.completeness_percent),
            &format!("{:.4}", d.data_quality_score),
            d.badge.badge.as_str(),
            &d.documentary_ready.to_string(),
            s.badge.badge.as_str(),
            &s.strict_ready.to_string(),
            &s.badge.blocking_issue_codes.join(";"),
        ])?;
    }
    w.flush()?;
    Ok(())
}

fn print_summary(doc: &DatasetSummary, strict: &DatasetSummary) {
    println!("\n=== READINESS PoC RESULTS ===");
    println!(
        "Assets: {} total, {} with eligible UPM events",
        doc.total_assets, doc.assets_with_eligible_events
    );
    println!(
        "PRIMARY (documentary): {:.1}% ready | mean C={:.1}% | mean DQ={:.3}",
        doc.documentary_ready_pct, doc.mean_completeness_pct, doc.mean_dq
    );
    println!(
        "STRICT (full RAM): {:.1}% ready | red badges={}",
        strict.strict_ready_pct, strict.badge_red_count
    );
    println!(
        "Dimensions (mean): eq={:.1}% int={:.1}% mode={:.1}% corr={:.1}%",
        doc.mean_dimensions.dim_equipment_id_pct,
        doc.mean_dimensions.dim_failure_interval_pct,
        doc.mean_dimensions.dim_failure_mode_pct,
        doc.mean_dimensions.dim_corrective_closure_pct,
    );
    if !strict.blocking_breakdown.is_empty() {
        println!("Strict blocking breakdown:");
        for (k, v) in &strict.blocking_breakdown {
            println!("  {k}: {v}");
        }
    }
}

fn workspace_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest.parent().context("workspace root")?.to_path_buf())
}

fn git_commit_hash() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(workspace_root().ok()?)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}
