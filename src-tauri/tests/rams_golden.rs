use chrono::{DateTime, Utc};
use maintafox_lib::reliability::compute::{
    compute_reliability_kpis, KpiFailureEvent, ReliabilityKpiComputeInput,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Period {
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct GoldenEvent {
    id: i64,
    failed_at: String,
    eligible_flags_json: String,
    downtime_duration_hours: f64,
    active_repair_hours: f64,
    failure_mode_id: Option<i64>,
}

#[derive(Deserialize)]
struct Expected {
    mtbf: Option<f64>,
    mttr: Option<f64>,
    availability: Option<f64>,
    failure_rate: Option<f64>,
    repeat_failure_rate: Option<f64>,
    event_count: Option<i64>,
    data_quality_score: Option<f64>,
}

#[derive(Deserialize)]
struct Tolerance {
    rel: f64,
}

#[derive(Deserialize)]
struct GoldenFixture {
    period: Period,
    repeat_lookback_days: i64,
    min_sample_n: i64,
    t_exp_hours: f64,
    events: Vec<GoldenEvent>,
    expected: Expected,
    tolerance: Tolerance,
}

fn parse_utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|d| d.with_timezone(&Utc))
        .expect("rfc3339")
}

fn rel_close(a: f64, b: f64, rel: f64) -> bool {
    let d = (a - b).abs();
    let scale = a.abs().max(b.abs()).max(1e-300);
    d <= rel * scale
}

fn assert_fixture(json: &str) {
    let f: GoldenFixture = serde_json::from_str(json).expect("fixture json");
    let p0 = parse_utc(&f.period.start);
    let p1 = parse_utc(&f.period.end);
    let events: Vec<KpiFailureEvent> = f
        .events
        .into_iter()
        .map(|e| KpiFailureEvent {
            id: e.id,
            event_ts: parse_utc(&e.failed_at),
            eligible_flags_json: e.eligible_flags_json,
            downtime_duration_hours: e.downtime_duration_hours,
            active_repair_hours: e.active_repair_hours,
            failure_mode_id: e.failure_mode_id,
        })
        .collect();
    let out = compute_reliability_kpis(&ReliabilityKpiComputeInput {
        period_start: p0,
        period_end: p1,
        t_exp_hours: f.t_exp_hours,
        repeat_lookback_days: f.repeat_lookback_days,
        min_sample_n: f.min_sample_n,
        events,
    });
    let rel = f.tolerance.rel;
    if let Some(exp) = f.expected.mtbf {
        let v = out.mtbf.expect("mtbf");
        assert!(rel_close(v, exp, rel), "mtbf got {v} want {exp}");
    }
    if let Some(exp) = f.expected.mttr {
        let v = out.mttr.expect("mttr");
        assert!(rel_close(v, exp, rel), "mttr got {v} want {exp}");
    }
    if let Some(exp) = f.expected.failure_rate {
        let v = out.failure_rate.expect("failure_rate");
        assert!(rel_close(v, exp, rel), "failure_rate got {v} want {exp}");
    }
    if let Some(exp) = f.expected.repeat_failure_rate {
        let v = out.repeat_failure_rate.expect("repeat_failure_rate");
        assert!(rel_close(v, exp, rel), "repeat_failure_rate got {v} want {exp}");
    }
    if let Some(exp) = f.expected.event_count {
        assert_eq!(out.event_count, exp, "event_count");
    }
    if let Some(exp) = f.expected.availability {
        let v = out.availability.expect("availability");
        assert!(rel_close(v, exp, rel), "availability got {v} want {exp}");
    }
    if let Some(exp) = f.expected.data_quality_score {
        let v = out.data_quality_score;
        assert!(rel_close(v, exp, rel), "data_quality_score got {v} want {exp}");
    }
}

#[test]
fn rams_golden_v1_fixtures() {
    assert_fixture(include_str!("fixtures/rams_golden/v1/g_mtbf_fr_01.json"));
    assert_fixture(include_str!("fixtures/rams_golden/v1/g_mttr_01.json"));
    assert_fixture(include_str!("fixtures/rams_golden/v1/g_rpt_01.json"));
    assert_fixture(include_str!("fixtures/rams_golden/v1/g_availability_01.json"));
    assert_fixture(include_str!("fixtures/rams_golden/v1/g_dq_partial_01.json"));
}
