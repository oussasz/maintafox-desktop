use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use super::config::FmucdMappingConfig;
use super::mapper::{map_row_to_event, MapRowOutcome};
use super::schema::FmucdRow;
use crate::reliability::readiness::event::ReadinessEvent;
use crate::reliability::readiness::report::LoaderRowStats;

const PROGRESS_EVERY: u64 = 250_000;

#[derive(Debug, Clone)]
pub struct LoadProgress {
    pub stats: LoaderRowStats,
    pub assets: HashMap<String, Vec<ReadinessEvent>>,
}

/// Stream-parse FMUCD CSV and aggregate events by asset key.
pub fn load_fmucd_csv(path: &Path, config: &FmucdMappingConfig) -> Result<LoadProgress> {
    let file = File::open(path).with_context(|| format!("open FMUCD CSV {}", path.display()))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(BufReader::new(file));

    let headers: Vec<String> = reader
        .headers()
        .context("FMUCD CSV headers")?
        .iter()
        .map(str::to_string)
        .collect();

    let mut stats = LoaderRowStats::default();
    let mut assets: HashMap<String, Vec<ReadinessEvent>> = HashMap::new();
    let mut row_index: i64 = 0;

    for result in reader.records() {
        stats.rows_read += 1;
        let record = result.with_context(|| format!("CSV row {}", stats.rows_read + 1))?;
        let row = FmucdRow::from_csv_record(stats.rows_read + 1, &headers, &record);

        let (event, outcome) = map_row_to_event(config, &row, row_index)?;
        row_index += 1;

        match outcome {
            MapRowOutcome::SkippedEmptyAsset => {
                stats.rows_skipped_empty_asset += 1;
            }
            MapRowOutcome::SkippedParseError => {
                stats.rows_skipped_parse_error += 1;
            }
            MapRowOutcome::Mapped => {
                if !event.eligible {
                    stats.rows_ppm_excluded_from_eligible += 1;
                }
                stats.rows_mapped += 1;
                assets.entry(event.asset_key.clone()).or_default().push(event);
            }
        }

        if stats.rows_read % PROGRESS_EVERY == 0 {
            info!(
                rows = stats.rows_read,
                assets = assets.len(),
                "FMUCD load progress"
            );
        }
    }

    stats.unique_assets = assets.len() as u64;
    Ok(LoadProgress { stats, assets })
}
