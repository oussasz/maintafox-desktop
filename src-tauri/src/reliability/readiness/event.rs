use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Normalized failure / work-order record for readiness evaluation (DB or external CMMS).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadinessEvent {
    pub id: i64,
    pub asset_key: String,
    /// FMUCD university identifier when sourced from external CMMS (optional).
    pub university_id: Option<String>,
    pub event_ts: DateTime<Utc>,
    /// Counts toward `N_f` / DQ when true (UPM / unplanned MTBF-eligible).
    pub eligible: bool,
    pub equipment_identified: bool,
    /// `(failed_at OR detected_at) AND restored_at` equivalent.
    pub interval_complete: bool,
    pub failure_mode_coded: bool,
    pub corrective_documented: bool,
    /// Passed through to `compute::compute_reliability_kpis`.
    pub eligible_flags_json: String,
    pub failure_mode_id: Option<i64>,
    pub downtime_duration_hours: f64,
    pub active_repair_hours: f64,
}

impl ReadinessEvent {
    #[must_use]
    pub fn eligible_flags_json(eligible: bool) -> String {
        if eligible {
            super::policy::ELIGIBLE_FLAGS_JSON_TRUE.to_string()
        } else {
            super::policy::ELIGIBLE_FLAGS_JSON_FALSE.to_string()
        }
    }
}
