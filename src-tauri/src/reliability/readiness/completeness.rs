use serde::{Deserialize, Serialize};

use super::event::ReadinessEvent;

/// ISO 14224-inspired completeness scores for a set of failure records on one asset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IsoCompletenessResult {
    pub event_count: i64,
    pub completeness_percent: f64,
    pub dim_equipment_id_pct: f64,
    pub dim_failure_interval_pct: f64,
    pub dim_failure_mode_pct: f64,
    pub dim_corrective_closure_pct: f64,
}

/// Pure-Rust equivalent of `iso_14224_failure_dataset_completeness` SQL in `queries.rs`.
#[must_use]
pub fn evaluate_iso14224_completeness(events: &[ReadinessEvent]) -> IsoCompletenessResult {
    let n = events.len() as i64;
    if n == 0 {
        return IsoCompletenessResult {
            event_count: 0,
            completeness_percent: 0.0,
            dim_equipment_id_pct: 0.0,
            dim_failure_interval_pct: 0.0,
            dim_failure_mode_pct: 0.0,
            dim_corrective_closure_pct: 0.0,
        };
    }

    let n_f = n as f64;
    let d_eq = events
        .iter()
        .filter(|e| e.equipment_identified)
        .count() as f64
        / n_f;
    let d_win = events
        .iter()
        .filter(|e| e.interval_complete)
        .count() as f64
        / n_f;
    let d_mode = events
        .iter()
        .filter(|e| e.failure_mode_coded)
        .count() as f64
        / n_f;
    let d_corr = events
        .iter()
        .filter(|e| e.corrective_documented)
        .count() as f64
        / n_f;

    let completeness_percent = ((d_eq + d_win + d_mode + d_corr) / 4.0) * 100.0;

    IsoCompletenessResult {
        event_count: n,
        completeness_percent,
        dim_equipment_id_pct: d_eq * 100.0,
        dim_failure_interval_pct: d_win * 100.0,
        dim_failure_mode_pct: d_mode * 100.0,
        dim_corrective_closure_pct: d_corr * 100.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use crate::reliability::readiness::event::ReadinessEvent;

    fn ts(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn empty_events_zero_completeness() {
        let r = evaluate_iso14224_completeness(&[]);
        assert_eq!(r.event_count, 0);
        assert_eq!(r.completeness_percent, 0.0);
    }

    #[test]
    fn full_completeness_all_dimensions() {
        let e = ReadinessEvent {
            id: 1,
            asset_key: "a".into(),
            university_id: None,
            event_ts: ts(""),
            eligible: true,
            equipment_identified: true,
            interval_complete: true,
            failure_mode_coded: true,
            corrective_documented: true,
            eligible_flags_json: ReadinessEvent::eligible_flags_json(true),
            failure_mode_id: Some(1),
            downtime_duration_hours: 1.0,
            active_repair_hours: 1.0,
        };
        let r = evaluate_iso14224_completeness(&[e]);
        assert!((r.completeness_percent - 100.0).abs() < 1e-9);
    }
}
