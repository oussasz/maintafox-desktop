use chrono::{DateTime, Duration, Utc};

/// PRD §6.10.2 — inclusion flag for unplanned MTBF-eligible failure events.
pub fn eligible_unplanned_mtbf(flags_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(flags_json)
        .ok()
        .and_then(|v| {
            v.get("eligible_unplanned_mtbf").and_then(|x| {
                if x.as_bool() == Some(true) {
                    Some(true)
                } else if x.as_u64() == Some(1) {
                    Some(true)
                } else {
                    x.as_i64().map(|i| i == 1)
                }
            })
        })
        == Some(true)
}

#[derive(Debug, Clone)]
pub struct KpiFailureEvent {
    pub id: i64,
    pub event_ts: DateTime<Utc>,
    pub eligible_flags_json: String,
    pub downtime_duration_hours: f64,
    pub active_repair_hours: f64,
    pub failure_mode_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReliabilityKpiComputed {
    pub mtbf: Option<f64>,
    pub mttr: Option<f64>,
    pub availability: Option<f64>,
    pub failure_rate: Option<f64>,
    pub repeat_failure_rate: Option<f64>,
    pub event_count: i64,
    pub data_quality_score: f64,
}

pub struct ReliabilityKpiComputeInput {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub t_exp_hours: f64,
    pub repeat_lookback_days: i64,
    pub min_sample_n: i64,
    pub events: Vec<KpiFailureEvent>,
}

struct Ev {
    id: i64,
    ts: DateTime<Utc>,
    eligible: bool,
    downtime: f64,
    active_r: f64,
    mode: Option<i64>,
}

/// Maintafox repeat failure rate: count of eligible events flagged as repeat / total eligible events
/// in period (repeat = same `failure_mode_id` with a prior eligible event within lookback).
pub fn compute_reliability_kpis(input: &ReliabilityKpiComputeInput) -> ReliabilityKpiComputed {
    let p0 = input.period_start;
    let p1 = input.period_end;
    let lb = Duration::days(input.repeat_lookback_days.max(1));
    let t_exp = input.t_exp_hours.max(0.0);
    let min_n = input.min_sample_n.max(1);

    let extended: Vec<Ev> = input
        .events
        .iter()
        .map(|e| Ev {
            id: e.id,
            ts: e.event_ts,
            eligible: eligible_unplanned_mtbf(&e.eligible_flags_json),
            downtime: e.downtime_duration_hours,
            active_r: e.active_repair_hours,
            mode: e.failure_mode_id,
        })
        .collect();

    let in_period: Vec<&Ev> = extended
        .iter()
        .filter(|e| e.eligible && e.ts >= p0 && e.ts <= p1)
        .collect();
    let f = in_period.len() as i64;
    let d_down: f64 = in_period.iter().map(|e| e.downtime).sum();
    let f_repair = in_period.iter().filter(|e| e.active_r > 0.0).count() as i64;
    let r_active: f64 = in_period.iter().filter(|e| e.active_r > 0.0).map(|e| e.active_r).sum();

    let mtbf = if f > 0 && t_exp > 0.0 {
        Some(t_exp / f as f64)
    } else {
        None
    };
    let mttr = if f_repair > 0 {
        Some(r_active / f_repair as f64)
    } else {
        None
    };
    let failure_rate = if t_exp > 0.0 {
        Some(f as f64 / t_exp)
    } else {
        None
    };
    let availability = if t_exp > 0.0 {
        Some((t_exp - d_down).max(0.0) / t_exp)
    } else {
        None
    };

    let mut repeat_n = 0_i64;
    for e in &in_period {
        if let Some(m) = e.mode {
            let has_prior = extended.iter().any(|x| {
                x.id != e.id
                    && x.eligible
                    && x.mode == Some(m)
                    && x.ts < e.ts
                    && e.ts.signed_duration_since(x.ts) <= lb
            });
            if has_prior {
                repeat_n += 1;
            }
        }
    }
    let repeat_failure_rate = if f > 0 {
        Some(repeat_n as f64 / f as f64)
    } else {
        None
    };

    let dq = if f >= min_n {
        1.0
    } else {
        0.49_f64 * (f as f64 / min_n as f64).min(1.0)
    };

    ReliabilityKpiComputed {
        mtbf,
        mttr,
        availability,
        failure_rate,
        repeat_failure_rate,
        event_count: f,
        data_quality_score: dq,
    }
}
