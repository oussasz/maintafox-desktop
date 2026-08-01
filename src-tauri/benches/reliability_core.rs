use chrono::{DateTime, Duration, Utc};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use maintafox_lib::reliability::compute::{
    compute_reliability_kpis, KpiFailureEvent, ReliabilityKpiComputeInput,
};

fn make_input(n: usize) -> ReliabilityKpiComputeInput {
    let p0: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
    let p1: DateTime<Utc> = "2026-02-01T00:00:00Z".parse().unwrap();
    let flags = r#"{"eligible_unplanned_mtbf":true}"#;
    let mut events = Vec::with_capacity(n);
    for i in 0..n {
        let ts = p0 + Duration::hours(i as i64);
        events.push(KpiFailureEvent {
            id: i as i64,
            event_ts: ts,
            eligible_flags_json: flags.to_string(),
            downtime_duration_hours: 0.0,
            active_repair_hours: 1.0,
            failure_mode_id: Some(10),
        });
    }
    ReliabilityKpiComputeInput {
        period_start: p0,
        period_end: p1,
        t_exp_hours: 1_000_000.0,
        repeat_lookback_days: 30,
        min_sample_n: 5,
        events,
    }
}

fn bench_compute_2k(c: &mut Criterion) {
    let input = make_input(2000);
    c.bench_function("compute_reliability_kpis_2000_events", |b| {
        b.iter(|| compute_reliability_kpis(black_box(&input)))
    });
}

fn bench_compute_10k(c: &mut Criterion) {
    let input = make_input(10_000);
    c.bench_function("compute_reliability_kpis_10000_events", |b| {
        b.iter(|| compute_reliability_kpis(black_box(&input)))
    });
}

criterion_group!(benches, bench_compute_2k, bench_compute_10k);
criterion_main!(benches);
