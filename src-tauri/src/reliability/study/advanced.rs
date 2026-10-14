//! Advanced tools of the system model (specialists), all computed on request without writing:
//!
//! - Markov: continuous-time chain of a repairable system with degraded states. States and
//!   rates (λ, μ per hour) are typed in a table; the steady state solves `π Q = 0, Σ π = 1`
//!   (Gaussian elimination with partial pivoting). Availability `A = Σ π(up)`, failure frequency
//!   `f = Σ π_i q_ij` over up → down transitions, mean up time `A / f`, mean down time `(1 − A) / f`.
//! - Monte Carlo: discrete-event simulation of the study reliability block diagram over the
//!   mission: Weibull times to failure per block (from the block source), exponential repair
//!   times of mean MTTR, renewal after repair. Per trial: availability, system failures,
//!   downtime and cost; reported as mean and P5 / P50 / P95, plus the mission reliability
//!   (share of trials without a system failure).
//! - Event tree: an initiating event (frequency per year) followed by barriers that succeed or
//!   fail; every sequence gets its probability (through `fta_rbd_eta::eta_eval`, reused) and its
//!   frequency per year.
//!
//! The existing guardrails (`ram_advanced_guardrails`) cap the Markov states and Monte Carlo
//! trials and can switch the tools off.

use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reliability::fta_rbd_eta::eta_eval::{evaluate_eta, EtaBranch, EtaGraph, EtaNodeKind};
use crate::reliability::study::analysis::HOURS_PER_YEAR;
use crate::reliability::study::system_model::{check_rbd, life_of, rbd_value, RbdSpec, SourceData};

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

// ── Markov ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkovState {
    pub id: String,
    pub label: String,
    /// The system delivers its function in this state.
    pub up: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkovTransition {
    pub from: String,
    pub to: String,
    pub rate_per_hour: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkovInput {
    pub states: Vec<MarkovState>,
    pub transitions: Vec<MarkovTransition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkovStateResult {
    pub id: String,
    pub label: String,
    pub up: bool,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkovResult {
    pub states: Vec<MarkovStateResult>,
    pub availability: f64,
    pub failures_per_year: f64,
    pub mean_up_time_hours: Option<f64>,
    pub mean_down_time_hours: Option<f64>,
}

/// Solve `A x = b` (square, dense) by Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| {
            a[i][col]
                .abs()
                .partial_cmp(&a[j][col].abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
        if a[pivot][col].abs() < 1e-14 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let f = a[row][col] / a[col][col];
            if f == 0.0 {
                continue;
            }
            for k in col..n {
                a[row][k] -= f * a[col][k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row][row];
    }
    Some(x)
}

pub fn evaluate_markov(input: &MarkovInput, max_states: usize) -> AppResult<MarkovResult> {
    let n = input.states.len();
    if n < 2 {
        return Err(invalid("MARKOV_TOO_FEW_STATES"));
    }
    if n > max_states {
        return Err(invalid("MARKOV_TOO_MANY_STATES"));
    }
    let index: HashMap<&str, usize> = input
        .states
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();
    if index.len() != n {
        return Err(invalid("MARKOV_DUPLICATE_STATE"));
    }
    if !input.states.iter().any(|s| s.up) || input.states.iter().all(|s| s.up) {
        return Err(invalid("MARKOV_NEEDS_UP_AND_DOWN"));
    }
    let mut q = vec![vec![0.0; n]; n];
    for t in &input.transitions {
        let (Some(&i), Some(&j)) = (index.get(t.from.as_str()), index.get(t.to.as_str())) else {
            return Err(invalid("MARKOV_UNKNOWN_STATE"));
        };
        if i == j || !(t.rate_per_hour.is_finite() && t.rate_per_hour > 0.0) {
            return Err(invalid("MARKOV_RATE_INVALID"));
        }
        q[i][j] += t.rate_per_hour;
    }
    for (i, row) in q.iter_mut().enumerate() {
        let out: f64 = row.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, v)| v).sum();
        row[i] = -out;
    }
    // πQ = 0  ⇔  Qᵀ πᵀ = 0; replace the last equation by Σ π = 1.
    let mut a = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            a[j][i] = q[i][j];
        }
    }
    a[n - 1] = vec![1.0; n];
    let mut b = vec![0.0; n];
    b[n - 1] = 1.0;
    let pi = solve(a, b).ok_or_else(|| invalid("MARKOV_NOT_IRREDUCIBLE"))?;
    if pi.iter().any(|p| !p.is_finite() || *p < -1e-9) {
        return Err(invalid("MARKOV_NOT_IRREDUCIBLE"));
    }
    let availability: f64 = input.states.iter().zip(&pi).filter(|(s, _)| s.up).map(|(_, p)| p).sum();
    let mut freq = 0.0;
    for (i, si) in input.states.iter().enumerate() {
        for (j, sj) in input.states.iter().enumerate() {
            if si.up && !sj.up {
                freq += pi[i] * q[i][j];
            }
        }
    }
    Ok(MarkovResult {
        states: input
            .states
            .iter()
            .zip(&pi)
            .map(|(s, p)| MarkovStateResult {
                id: s.id.clone(),
                label: s.label.clone(),
                up: s.up,
                probability: p.max(0.0),
            })
            .collect(),
        availability,
        failures_per_year: freq * HOURS_PER_YEAR,
        mean_up_time_hours: (freq > 0.0).then(|| availability / freq),
        mean_down_time_hours: (freq > 0.0).then(|| (1.0 - availability) / freq),
    })
}

// ── Event tree ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Barrier {
    pub label: String,
    /// Probability that the barrier fails on demand.
    pub failure_probability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTreeInput {
    pub initiating_event: String,
    pub frequency_per_year: f64,
    pub barriers: Vec<Barrier>,
    /// Consequence of each sequence (index = bit mask of failed barriers); optional.
    #[serde(default)]
    pub consequences: Vec<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventSequence {
    pub index: usize,
    /// Per barrier, in order: `true` when it fails.
    pub failed: Vec<bool>,
    pub consequence: Option<String>,
    pub probability: f64,
    pub frequency_per_year: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventTreeResult {
    pub sequences: Vec<EventSequence>,
    pub total_frequency_per_year: f64,
}

pub const MAX_BARRIERS: usize = 6;

pub fn evaluate_event_tree(input: &EventTreeInput) -> AppResult<EventTreeResult> {
    let n = input.barriers.len();
    if n == 0 || n > MAX_BARRIERS {
        return Err(invalid("EVENT_TREE_BARRIERS"));
    }
    if !(input.frequency_per_year.is_finite() && input.frequency_per_year > 0.0) {
        return Err(invalid("EVENT_TREE_FREQUENCY_INVALID"));
    }
    if input
        .barriers
        .iter()
        .any(|b| !(0.0..=1.0).contains(&b.failure_probability) || b.label.trim().is_empty())
    {
        return Err(invalid("EVENT_TREE_BARRIER_INVALID"));
    }
    // Binary tree: node "d{depth}_{mask}" splits on barrier `depth`; leaves "seq_{mask}".
    let mut nodes = HashMap::new();
    for depth in 0..n {
        for mask in 0..(1usize << depth) {
            let child = |failed: bool| {
                let m = mask | (usize::from(failed) << depth);
                if depth + 1 == n {
                    format!("seq_{m}")
                } else {
                    format!("d{}_{m}", depth + 1)
                }
            };
            let p = input.barriers[depth].failure_probability;
            nodes.insert(
                format!("d{depth}_{mask}"),
                EtaNodeKind::Split {
                    label: Some(input.barriers[depth].label.clone()),
                    branches: vec![
                        EtaBranch {
                            target: child(false),
                            p: 1.0 - p,
                        },
                        EtaBranch { target: child(true), p },
                    ],
                },
            );
        }
    }
    for mask in 0..(1usize << n) {
        nodes.insert(format!("seq_{mask}"), EtaNodeKind::Outcome { label: None });
    }
    let res = evaluate_eta(&EtaGraph {
        spec_version: 1,
        root_id: "d0_0".into(),
        nodes,
    })
    .map_err(|_| invalid("EVENT_TREE_INVALID"))?;
    let mut sequences: Vec<EventSequence> = res
        .outcome_paths
        .iter()
        .filter_map(|row| {
            let mask: usize = row.node_id.strip_prefix("seq_")?.parse().ok()?;
            Some(EventSequence {
                index: mask,
                failed: (0..n).map(|d| mask & (1 << d) != 0).collect(),
                consequence: input
                    .consequences
                    .get(mask)
                    .cloned()
                    .flatten()
                    .filter(|c| !c.trim().is_empty()),
                probability: row.path_probability,
                frequency_per_year: row.path_probability * input.frequency_per_year,
            })
        })
        .collect();
    sequences.sort_by_key(|s| s.index);
    Ok(EventTreeResult {
        total_frequency_per_year: sequences.iter().map(|s| s.frequency_per_year).sum(),
        sequences,
    })
}

// ── Monte Carlo ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct SimulationInput {
    pub study_id: i64,
    pub mission_hours: f64,
    pub trials: i64,
    #[serde(default)]
    pub seed: Option<u64>,
    pub spec: RbdSpec,
    /// Cost of one hour of system downtime (lost production).
    #[serde(default)]
    pub downtime_cost_per_hour: Option<f64>,
    /// Cost of one block repair.
    #[serde(default)]
    pub repair_cost: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Spread {
    pub mean: f64,
    pub p05: f64,
    pub p50: f64,
    pub p95: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationBlock {
    pub id: String,
    pub label: String,
    pub beta: f64,
    pub eta: f64,
    pub mttr_hours: f64,
    /// `node` | `study` | `equipment`.
    pub mttr_source: String,
    pub mean_failures: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationResult {
    pub trials: i64,
    pub seed: u64,
    pub mission_hours: f64,
    pub availability: Spread,
    pub system_failures: Spread,
    pub downtime_hours: Spread,
    pub cost: Option<Spread>,
    /// Share of trials without any system failure over the mission.
    pub mission_reliability: f64,
    pub blocks: Vec<SimulationBlock>,
    pub issues: Vec<crate::reliability::study::system_model::ModelIssue>,
}

fn spread(mut v: Vec<f64>) -> Spread {
    if v.is_empty() {
        return Spread {
            mean: 0.0,
            p05: 0.0,
            p50: 0.0,
            p95: 0.0,
        };
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    Spread {
        mean: v.iter().sum::<f64>() / v.len() as f64,
        p05: q(0.05),
        p50: q(0.5),
        p95: q(0.95),
    }
}

struct SimBlock {
    id: String,
    beta: f64,
    eta: f64,
    mttr: f64,
}

/// Simulate (pure, deterministic for a seed).
#[allow(clippy::too_many_lines)]
pub fn simulate(input: &SimulationInput, data: &SourceData, max_trials: i64) -> AppResult<SimulationResult> {
    use crate::reliability::study::system_model::{LifeSource, ModelIssue};
    let mut issues = check_rbd(&input.spec);
    if !(input.mission_hours.is_finite() && input.mission_hours > 0.0) {
        return Err(invalid("SIMULATION_MISSION_INVALID"));
    }
    if input.trials < 1 || input.trials > max_trials {
        return Err(invalid("SIMULATION_TRIALS_INVALID"));
    }
    let mut blocks = Vec::new();
    let mut out_blocks = Vec::new();
    for n in input.spec.nodes.iter().filter(|n| n.node_type == "block") {
        let Some(src) = &n.source else { continue };
        match life_of(src, data) {
            Ok((beta, eta, _)) => {
                let (mttr, mttr_source) = match (n.mttr_hours.filter(|v| v.is_finite() && *v > 0.0), src) {
                    (Some(v), _) => (Some(v), "node"),
                    (None, LifeSource::StudyFit) => (data.study_mttr_hours, "study"),
                    (None, LifeSource::EquipmentHistory { equipment_id }) => {
                        (data.equipment.get(equipment_id).and_then(|e| e.mttr_hours), "equipment")
                    }
                    _ => (None, "node"),
                };
                match mttr.filter(|v| *v > 0.0) {
                    Some(m) => {
                        blocks.push(SimBlock {
                            id: n.id.clone(),
                            beta,
                            eta,
                            mttr: m,
                        });
                        out_blocks.push(SimulationBlock {
                            id: n.id.clone(),
                            label: n.label.clone(),
                            beta,
                            eta,
                            mttr_hours: m,
                            mttr_source: mttr_source.into(),
                            mean_failures: 0.0,
                        });
                    }
                    None => issues.push(ModelIssue {
                        node_id: n.id.clone(),
                        code: "needs_mttr".into(),
                    }),
                }
            }
            Err(code) => issues.push(ModelIssue {
                node_id: n.id.clone(),
                code: code.into(),
            }),
        }
    }
    let seed = input.seed.unwrap_or(0x5EED_0F_5EED);
    let empty = Spread {
        mean: 0.0,
        p05: 0.0,
        p50: 0.0,
        p95: 0.0,
    };
    if !issues.is_empty() {
        return Ok(SimulationResult {
            trials: 0,
            seed,
            mission_hours: input.mission_hours,
            availability: empty,
            system_failures: empty,
            downtime_hours: empty,
            cost: None,
            mission_reliability: 0.0,
            blocks: out_blocks,
            issues,
        });
    }
    let t_end = input.mission_hours;
    let mut rng = StdRng::seed_from_u64(seed);
    let mut avail = Vec::with_capacity(input.trials as usize);
    let mut sys_fail = Vec::with_capacity(input.trials as usize);
    let mut downtime = Vec::with_capacity(input.trials as usize);
    let mut costs = Vec::with_capacity(input.trials as usize);
    let mut survived = 0_i64;
    let mut block_failures = vec![0_i64; blocks.len()];
    let cost_h = input.downtime_cost_per_hour.filter(|v| v.is_finite() && *v >= 0.0);
    let cost_r = input.repair_cost.filter(|v| v.is_finite() && *v >= 0.0);
    for _ in 0..input.trials {
        // (time, block index, comes back up)
        let mut events: Vec<(f64, usize, bool)> = Vec::new();
        let mut trial_repairs = 0_i64;
        for (bi, b) in blocks.iter().enumerate() {
            let mut t = 0.0;
            loop {
                let u: f64 = rng.gen::<f64>().max(f64::MIN_POSITIVE);
                let ttf = b.eta * (-u.ln()).powf(1.0 / b.beta);
                t += ttf;
                if t >= t_end {
                    break;
                }
                events.push((t, bi, false));
                block_failures[bi] += 1;
                trial_repairs += 1;
                let v: f64 = rng.gen::<f64>().max(f64::MIN_POSITIVE);
                t += -b.mttr * v.ln();
                if t >= t_end {
                    break;
                }
                events.push((t, bi, true));
            }
        }
        events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut state: HashMap<String, f64> = blocks.iter().map(|b| (b.id.clone(), 1.0)).collect();
        let mut up = rbd_value(&input.spec, &state) > 0.5;
        let mut last = 0.0;
        let mut uptime = 0.0;
        let mut failures = 0_i64;
        for (t, bi, comes_up) in events {
            if up {
                uptime += t - last;
            }
            last = t;
            state.insert(blocks[bi].id.clone(), if comes_up { 1.0 } else { 0.0 });
            let now_up = rbd_value(&input.spec, &state) > 0.5;
            if up && !now_up {
                failures += 1;
            }
            up = now_up;
        }
        if up {
            uptime += t_end - last;
        }
        if failures == 0 {
            survived += 1;
        }
        let a = uptime / t_end;
        let down = t_end - uptime;
        avail.push(a);
        sys_fail.push(failures as f64);
        downtime.push(down);
        if cost_h.is_some() || cost_r.is_some() {
            costs.push(down * cost_h.unwrap_or(0.0) + trial_repairs as f64 * cost_r.unwrap_or(0.0));
        }
    }
    for (b, n) in out_blocks.iter_mut().zip(block_failures) {
        b.mean_failures = n as f64 / input.trials as f64;
    }
    Ok(SimulationResult {
        trials: input.trials,
        seed,
        mission_hours: t_end,
        availability: spread(avail),
        system_failures: spread(sys_fail),
        downtime_hours: spread(downtime),
        cost: (!costs.is_empty()).then(|| spread(costs)),
        mission_reliability: survived as f64 / input.trials as f64,
        blocks: out_blocks,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reliability::study::system_model::{LifeSource, RbdNode};

    #[test]
    fn two_state_markov_availability() {
        let r = evaluate_markov(
            &MarkovInput {
                states: vec![
                    MarkovState {
                        id: "u".into(),
                        label: "En marche".into(),
                        up: true,
                    },
                    MarkovState {
                        id: "d".into(),
                        label: "En panne".into(),
                        up: false,
                    },
                ],
                transitions: vec![
                    MarkovTransition {
                        from: "u".into(),
                        to: "d".into(),
                        rate_per_hour: 0.001,
                    },
                    MarkovTransition {
                        from: "d".into(),
                        to: "u".into(),
                        rate_per_hour: 0.1,
                    },
                ],
            },
            128,
        )
        .expect("markov");
        let a = 0.1 / 0.101;
        assert!((r.availability - a).abs() < 1e-12);
        assert!((r.failures_per_year - a * 0.001 * HOURS_PER_YEAR).abs() < 1e-9);
        assert!((r.mean_up_time_hours.unwrap() - 1000.0).abs() < 1e-9);
        assert!((r.mean_down_time_hours.unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn redundant_pair_with_one_repair_crew() {
        // States: 2 units up, 1 up (degraded), 0 up (down). λ = 0.002, μ = 0.05.
        let (l, m) = (0.002, 0.05);
        let st = |id: &str, up| MarkovState {
            id: id.into(),
            label: id.into(),
            up,
        };
        let tr = |f: &str, t: &str, r| MarkovTransition {
            from: f.into(),
            to: t.into(),
            rate_per_hour: r,
        };
        let r = evaluate_markov(
            &MarkovInput {
                states: vec![st("2", true), st("1", true), st("0", false)],
                transitions: vec![tr("2", "1", 2.0 * l), tr("1", "0", l), tr("1", "2", m), tr("0", "1", m)],
            },
            128,
        )
        .expect("markov");
        // Balance: π1 = π2·2λ/μ, π0 = π1·λ/μ.
        let p2 = 1.0 / (1.0 + 2.0 * l / m + 2.0 * l * l / (m * m));
        let p0 = p2 * 2.0 * l * l / (m * m);
        assert!((r.availability - (1.0 - p0)).abs() < 1e-12);
        assert!(evaluate_markov(
            &MarkovInput {
                states: vec![st("a", true)],
                transitions: vec![]
            },
            128
        )
        .is_err());
        assert!(
            evaluate_markov(
                &MarkovInput {
                    states: vec![st("a", true), st("b", false), st("c", false)],
                    transitions: vec![tr("a", "b", 0.1), tr("b", "a", 0.1)],
                },
                128
            )
            .is_err(),
            "state c is unreachable: no unique steady state"
        );
    }

    #[test]
    fn event_tree_sequences() {
        let r = evaluate_event_tree(&EventTreeInput {
            initiating_event: "Fuite".into(),
            frequency_per_year: 0.5,
            barriers: vec![
                Barrier {
                    label: "Détection".into(),
                    failure_probability: 0.1,
                },
                Barrier {
                    label: "Isolement".into(),
                    failure_probability: 0.05,
                },
            ],
            consequences: vec![Some("Arrêt sûr".into()), None, None, Some("Rejet".into())],
        })
        .expect("eta");
        assert_eq!(r.sequences.len(), 4);
        let both = r.sequences.iter().find(|s| s.index == 3).unwrap();
        assert!((both.probability - 0.1 * 0.05).abs() < 1e-12);
        assert!((both.frequency_per_year - 0.5 * 0.005).abs() < 1e-12);
        assert_eq!(both.consequence.as_deref(), Some("Rejet"));
        assert!((r.total_frequency_per_year - 0.5).abs() < 1e-12);
    }

    fn single(beta: f64, eta: f64, mttr: f64) -> RbdSpec {
        RbdSpec {
            root_id: "b".into(),
            nodes: vec![RbdNode {
                id: "b".into(),
                label: "Pompe".into(),
                node_type: "block".into(),
                children: vec![],
                k: None,
                source: Some(LifeSource::Weibull { beta, eta }),
                group: None,
                mttr_hours: Some(mttr),
            }],
        }
    }

    #[test]
    fn simulation_converges_to_analytic_values() {
        let data = SourceData::default();
        // Exponential life (β = 1, η = 1000 h) and repair (mean 10 h): A = 1000 / 1010.
        let r = simulate(
            &SimulationInput {
                study_id: 0,
                mission_hours: 100_000.0,
                trials: 400,
                seed: Some(7),
                spec: single(1.0, 1000.0, 10.0),
                downtime_cost_per_hour: Some(100.0),
                repair_cost: None,
            },
            &data,
            1_000_000,
        )
        .expect("sim");
        assert!(r.issues.is_empty());
        assert!(
            (r.availability.mean - 1000.0 / 1010.0).abs() < 1e-3,
            "{}",
            r.availability.mean
        );
        let cost = r.cost.expect("cost");
        assert!((cost.mean - r.downtime_hours.mean * 100.0).abs() < 1e-6);
        // Mission reliability of a Weibull block (β = 2, η = 1000 h) over 500 h: exp(−0.25).
        let r = simulate(
            &SimulationInput {
                study_id: 0,
                mission_hours: 500.0,
                trials: 20_000,
                seed: Some(11),
                spec: single(2.0, 1000.0, 10.0),
                downtime_cost_per_hour: None,
                repair_cost: None,
            },
            &data,
            1_000_000,
        )
        .expect("sim");
        assert!(
            (r.mission_reliability - (-0.25f64).exp()).abs() < 0.01,
            "{}",
            r.mission_reliability
        );
        // Same seed, same result.
        let again = simulate(
            &SimulationInput {
                study_id: 0,
                mission_hours: 500.0,
                trials: 20_000,
                seed: Some(11),
                spec: single(2.0, 1000.0, 10.0),
                downtime_cost_per_hour: None,
                repair_cost: None,
            },
            &data,
            1_000_000,
        )
        .expect("sim");
        assert_eq!(again.mission_reliability, r.mission_reliability);
        assert!(r.cost.is_none());
    }
}
