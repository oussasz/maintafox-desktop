//! Parity between pure-Rust completeness and documented SQL semantics.

use super::{evaluate_iso14224_completeness, ReadinessEvent};

fn thesis_20_events() -> Vec<ReadinessEvent> {
    let specs: [(bool, bool, bool, bool); 20] = [
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (true, true, true, true),
        (false, true, true, true),
        (false, false, true, true),
        (false, false, false, true),
    ];
    specs
        .into_iter()
        .enumerate()
        .map(|(i, (interval_complete, failure_mode_coded, corrective_documented, equipment_identified))| {
            let id = (i + 1) as i64;
            ReadinessEvent {
                id,
                asset_key: "a".into(),
                university_id: None,
                event_ts: chrono::Utc::now(),
                eligible: true,
                equipment_identified,
                interval_complete,
                failure_mode_coded,
                corrective_documented,
                eligible_flags_json: ReadinessEvent::eligible_flags_json(true),
                failure_mode_id: if failure_mode_coded { Some(id) } else { None },
                downtime_duration_hours: 0.0,
                active_repair_hours: 0.0,
            }
        })
        .collect()
}

#[test]
fn thesis_20_event_completeness_matches_ch3() {
    let events = thesis_20_events();
    let r = evaluate_iso14224_completeness(&events);
    assert_eq!(r.event_count, 20);
    assert!((r.completeness_percent - 92.5).abs() < 0.11);
    assert!((r.dim_failure_interval_pct - 85.0).abs() < 0.01);
}
