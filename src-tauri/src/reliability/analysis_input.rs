use serde::Serialize;
use sha2::{Digest, Sha256};

pub const ANALYSIS_INPUT_SPEC_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct ExposurePart {
    pub id: i64,
    pub exposure_type: String,
    pub value: f64,
    pub recorded_at: String,
    pub source_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailurePart {
    pub id: i64,
    pub ev_ts: String,
    pub eligible_flags_json: String,
    pub downtime_duration_hours: f64,
    pub active_repair_hours: f64,
    pub failure_mode_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct DatasetCanonical {
    spec_version: u32,
    equipment_id: i64,
    period_start: String,
    period_end: String,
    repeat_lookback_days: i64,
    min_sample_n: i64,
    exposure_hours: f64,
    exposure: Vec<ExposurePart>,
    failure_events: Vec<FailurePart>,
}

pub fn dataset_hash_sha256(
    equipment_id: i64,
    period_start: &str,
    period_end: &str,
    repeat_lookback_days: i64,
    min_sample_n: i64,
    exposure_hours: f64,
    exposure: Vec<ExposurePart>,
    failure_events: Vec<FailurePart>,
) -> String {
    let canonical = DatasetCanonical {
        spec_version: ANALYSIS_INPUT_SPEC_VERSION,
        equipment_id,
        period_start: period_start.to_string(),
        period_end: period_end.to_string(),
        repeat_lookback_days,
        min_sample_n,
        exposure_hours,
        exposure,
        failure_events,
    };
    let json = serde_json::to_string(&canonical).unwrap_or_else(|_| "{}".to_string());
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisInputGates {
    pub exposure_hours_positive: bool,
    pub min_eligible_events_met: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisInputSpec {
    pub spec_version: u32,
    pub exposure_hours: f64,
    pub eligible_event_count: i64,
    pub min_sample_n: i64,
    pub gates: AnalysisInputGates,
    pub analysis_ready: bool,
}

pub fn build_input_spec_json(
    exposure_hours: f64,
    eligible_event_count: i64,
    min_sample_n: i64,
) -> String {
    let gates = AnalysisInputGates {
        exposure_hours_positive: exposure_hours > 0.0,
        min_eligible_events_met: eligible_event_count >= min_sample_n,
    };
    let analysis_ready = gates.exposure_hours_positive && gates.min_eligible_events_met;
    let spec = AnalysisInputSpec {
        spec_version: ANALYSIS_INPUT_SPEC_VERSION,
        exposure_hours,
        eligible_event_count,
        min_sample_n,
        gates,
        analysis_ready,
    };
    serde_json::to_string(&spec).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_hash_golden_empty_parts_v1() {
        let h = dataset_hash_sha256(
            1,
            "2026-01-01T00:00:00Z",
            "2026-02-01T00:00:00Z",
            30,
            5,
            168.0,
            vec![],
            vec![],
        );
        assert_eq!(
            h,
            "8979cea7ed742c0839b03a3990a37545133fd62e582da6e5fec603eef4ab07d7"
        );
    }
}
