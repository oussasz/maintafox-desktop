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

/// Version tag persisted on `reliability_kpi_snapshots.algorithm_version`.
/// v2: exposure is treated as uptime; A_i / A_o split; MTTF for non-repairable items;
/// additive totals persisted for pooled fleet ratios.
pub const RELIABILITY_KPI_ALGORITHM_VERSION: &str = "reliability_kpi_v2";

#[derive(Debug, Clone, PartialEq)]
pub struct ReliabilityKpiComputed {
    /// Repairable items only: ΣT_exp / F.
    pub mtbf: Option<f64>,
    pub mttr: Option<f64>,
    /// KPI-035 Operational availability A_o (alias of `availability_operational`, kept for
    /// existing consumers of `reliability_kpi_snapshots.availability`).
    pub availability: Option<f64>,
    /// Inherent availability A_i = T_up / (T_up + Σ active repair) — corrective repair only, no logistics.
    pub availability_inherent: Option<f64>,
    /// Operational availability A_o = T_up / (T_up + Σ downtime) — downtime is wall-clock equipment-down
    /// time, which already contains waiting / logistics / admin delay incurred while down.
    pub availability_operational: Option<f64>,
    /// Non-repairable items only: ΣT_exp / F (exposure accumulated to failure per failure).
    pub mttf: Option<f64>,
    pub failure_rate: Option<f64>,
    pub repeat_failure_rate: Option<f64>,
    pub event_count: i64,
    pub data_quality_score: f64,
    /// Additive totals (persisted so fleet aggregates can pool instead of averaging ratios).
    pub repair_event_count: i64,
    pub active_repair_hours_total: f64,
    pub downtime_hours_total: f64,
}

pub struct ReliabilityKpiComputeInput {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    /// Governed operating exposure (uptime hours) in the period.
    pub t_exp_hours: f64,
    pub repeat_lookback_days: i64,
    pub min_sample_n: i64,
    /// `equipment.rams_is_repairable`: MTBF for repairable items, MTTF otherwise.
    pub repairable: bool,
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

    let mean_time_to_event = if f > 0 && t_exp > 0.0 {
        Some(t_exp / f as f64)
    } else {
        None
    };
    let (mtbf, mttf) = if input.repairable {
        (mean_time_to_event, None)
    } else {
        (None, mean_time_to_event)
    };
    let mttr = if f_repair > 0 {
        Some(r_active / f_repair as f64)
    } else {
        None
    };
    let failure_rate = if t_exp > 0.0 { Some(f as f64 / t_exp) } else { None };
    // T_exp is uptime, so downtime is added to the denominator (never subtracted from uptime).
    let availability_operational = if t_exp > 0.0 {
        Some(t_exp / (t_exp + d_down.max(0.0)))
    } else {
        None
    };
    let availability_inherent = if t_exp > 0.0 {
        Some(t_exp / (t_exp + r_active.max(0.0)))
    } else {
        None
    };

    let mut repeat_n = 0_i64;
    for e in &in_period {
        if let Some(m) = e.mode {
            let has_prior = extended.iter().any(|x| {
                x.id != e.id && x.eligible && x.mode == Some(m) && x.ts < e.ts && e.ts.signed_duration_since(x.ts) <= lb
            });
            if has_prior {
                repeat_n += 1;
            }
        }
    }
    let repeat_failure_rate = if f > 0 { Some(repeat_n as f64 / f as f64) } else { None };

    let dq = if f >= min_n {
        1.0
    } else {
        0.49_f64 * (f as f64 / min_n as f64).min(1.0)
    };

    ReliabilityKpiComputed {
        mtbf,
        mttr,
        availability: availability_operational,
        availability_inherent,
        availability_operational,
        mttf,
        failure_rate,
        repeat_failure_rate,
        event_count: f,
        data_quality_score: dq,
        repair_event_count: f_repair,
        active_repair_hours_total: r_active,
        downtime_hours_total: d_down,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).expect("rfc3339").with_timezone(&Utc)
    }

    fn ev(id: i64, at: &str, downtime: f64, active: f64) -> KpiFailureEvent {
        KpiFailureEvent {
            id,
            event_ts: ts(at),
            eligible_flags_json: "{\"eligible_unplanned_mtbf\":true}".into(),
            downtime_duration_hours: downtime,
            active_repair_hours: active,
            failure_mode_id: None,
        }
    }

    fn input(repairable: bool) -> ReliabilityKpiComputeInput {
        ReliabilityKpiComputeInput {
            period_start: ts("2026-01-01T00:00:00Z"),
            period_end: ts("2026-02-01T00:00:00Z"),
            t_exp_hours: 600.0,
            repeat_lookback_days: 30,
            min_sample_n: 1,
            repairable,
            // Downtime 10 h / 20 h (incl. waiting for parts); active repair 4 h / 6 h.
            events: vec![ev(1, "2026-01-05T00:00:00Z", 10.0, 4.0), ev(2, "2026-01-20T00:00:00Z", 20.0, 6.0)],
        }
    }

    /// A_i = 600 / (600 + 10) ; A_o = 600 / (600 + 30) ; MTBF = 300 ; MTTR = 5.
    #[test]
    fn inherent_and_operational_availability_are_separated() {
        let k = compute_reliability_kpis(&input(true));
        assert!((k.availability_inherent.unwrap() - 600.0 / 610.0).abs() < 1e-12);
        assert!((k.availability_operational.unwrap() - 600.0 / 630.0).abs() < 1e-12);
        assert_eq!(k.availability, k.availability_operational);
        assert!(k.availability_inherent.unwrap() > k.availability_operational.unwrap());
        assert_eq!(k.mtbf, Some(300.0));
        assert_eq!(k.mttf, None);
        assert_eq!(k.mttr, Some(5.0));
        assert_eq!((k.repair_event_count, k.active_repair_hours_total, k.downtime_hours_total), (2, 10.0, 30.0));
    }

    #[test]
    fn non_repairable_reports_mttf_not_mtbf() {
        let k = compute_reliability_kpis(&input(false));
        assert_eq!(k.mtbf, None);
        assert_eq!(k.mttf, Some(300.0));
    }
}
