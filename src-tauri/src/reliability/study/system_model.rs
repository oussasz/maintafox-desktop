//! Study step 5 — system model: a reliability block diagram (series / parallel / k-out-of-n)
//! and a fault tree (AND / OR) whose block and event values come from the study: the fitted
//! life, an equipment's recorded MTBF, or a value typed by the analyst. Nothing is fixed.
//!
//! - RBD: system reliability over the mission time, reliability of every sub-structure, and per
//!   block the Birnbaum importance `I_B = R_s(R_i = 1) − R_s(R_i = 0)` and the criticality
//!   importance `I_C = I_B · (1 − R_i) / (1 − R_s)` (share of the system unreliability).
//! - FTA: top-event probability and minimal cut sets from `fta_rbd_eta::fta_eval` (reused), and
//!   per basic event the Birnbaum importance, Fussell–Vesely `1 − P(top | p_i = 0) / P(top)` and
//!   the risk achievement worth `P(top | p_i = 1) / P(top)`.
//!
//! Evaluation reads data only; saving writes the model to `rbd_models` / `fta_models` (graph in
//! the existing format with resolved values, plus the study specification) and links it to the
//! study.

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reliability::fta_rbd_eta::domain::{
    CreateFtaModelInput, CreateRbdModelInput, UpdateFtaModelInput, UpdateRbdModelInput,
};
use crate::reliability::fta_rbd_eta::fta_eval::{evaluate_fta, FtaGraph, FtaNodeKind};
use crate::reliability::fta_rbd_eta::queries as model_queries;
use crate::reliability::life_metrics::reliability;
use crate::reliability::study::analysis::life_analysis;
use crate::reliability::study::clock::{load_clock, parse_instant, StudyClock};
use crate::reliability::study::domain::{LifeAnalysisRequest, ReliabilityStudy};
use crate::reliability::study::records::BASIS_OPERATING;
use crate::reliability::study::store::{get, get_study, STUDY_ACTIVE};

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

/// Where the value of a block or basic event comes from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LifeSource {
    /// Weibull fit of the study (step 2).
    StudyFit,
    /// Recorded MTBF of an equipment over the study period (exponential life).
    EquipmentHistory {
        equipment_id: i64,
    },
    Weibull {
        beta: f64,
        eta: f64,
    },
    Exponential {
        mtbf_hours: f64,
    },
    /// Reliability over the mission (RBD) — `1 − value` as an event probability (FTA).
    Reliability {
        value: f64,
    },
    /// Probability of the event over the mission (FTA).
    Probability {
        value: f64,
    },
    /// Constant failure rate per hour.
    Rate {
        per_hour: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RbdNode {
    pub id: String,
    pub label: String,
    /// `block` | `series` | `parallel`.
    pub node_type: String,
    #[serde(default)]
    pub children: Vec<String>,
    /// Parallel: number of children that must work (1 = redundancy, default).
    #[serde(default)]
    pub k: Option<i64>,
    #[serde(default)]
    pub source: Option<LifeSource>,
    /// Block type used to group the contributions (e.g. "Pompes").
    #[serde(default)]
    pub group: Option<String>,
    /// Mean repair time of the block (simulation).
    #[serde(default)]
    pub mttr_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RbdSpec {
    pub root_id: String,
    pub nodes: Vec<RbdNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FtNode {
    pub id: String,
    pub label: String,
    /// `basic` | `and` | `or`.
    pub node_type: String,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub source: Option<LifeSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FtSpec {
    pub top_id: String,
    pub nodes: Vec<FtNode>,
}

/// Numbers a resolved value was computed from.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct SourceBasis {
    pub kind: String,
    pub beta: Option<f64>,
    pub eta: Option<f64>,
    pub mtbf_hours: Option<f64>,
    pub failures: Option<i64>,
    pub hours: Option<f64>,
    pub asset_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelIssue {
    pub node_id: String,
    /// `missing_node` | `cycle` | `duplicate_id` | `no_source` | `no_fit` | `no_failures`
    /// | `unknown_equipment` | `invalid_value` | `invalid_k` | `empty_structure` | `wrong_source`
    /// | `needs_life_distribution` | `needs_mttr`.
    pub code: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RbdNodeResult {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub reliability: f64,
    pub birnbaum: Option<f64>,
    pub criticality: Option<f64>,
    pub basis: Option<SourceBasis>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GroupShare {
    pub group: String,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RbdResult {
    pub mission_hours: f64,
    pub system_reliability: Option<f64>,
    pub nodes: Vec<RbdNodeResult>,
    pub groups: Vec<GroupShare>,
    pub issues: Vec<ModelIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CutSetResult {
    pub event_ids: Vec<String>,
    pub labels: Vec<String>,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventResult {
    pub id: String,
    pub label: String,
    pub probability: f64,
    pub birnbaum: f64,
    pub fussell_vesely: f64,
    pub risk_achievement_worth: f64,
    pub basis: Option<SourceBasis>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FtResult {
    pub mission_hours: f64,
    pub top_probability: Option<f64>,
    /// `exact_inclusion_exclusion` | `min_cut_upper_bound`.
    pub probability_method: Option<String>,
    pub cut_sets: Vec<CutSetResult>,
    pub events: Vec<EventResult>,
    pub issues: Vec<ModelIssue>,
}

/// Data a source can draw on.
#[derive(Debug, Clone, Default)]
pub struct SourceData {
    pub fit: Option<(f64, f64)>,
    pub study_mttr_hours: Option<f64>,
    pub equipment: HashMap<i64, EquipmentLife>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EquipmentLife {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub failures: i64,
    pub hours: f64,
    pub mtbf_hours: Option<f64>,
    pub mttr_hours: Option<f64>,
}

/// Life distribution of a source: `(β, η, basis)`.
pub fn life_of(source: &LifeSource, data: &SourceData) -> Result<(f64, f64, SourceBasis), &'static str> {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    match source {
        LifeSource::StudyFit => {
            let (b, e) = data.fit.ok_or("no_fit")?;
            Ok((
                b,
                e,
                SourceBasis {
                    kind: "study_fit".into(),
                    beta: Some(b),
                    eta: Some(e),
                    ..SourceBasis::default()
                },
            ))
        }
        LifeSource::EquipmentHistory { equipment_id } => {
            let eq = data.equipment.get(equipment_id).ok_or("unknown_equipment")?;
            let mtbf = eq.mtbf_hours.ok_or("no_failures")?;
            Ok((
                1.0,
                mtbf,
                SourceBasis {
                    kind: "equipment_history".into(),
                    mtbf_hours: Some(mtbf),
                    failures: Some(eq.failures),
                    hours: Some(eq.hours),
                    asset_code: Some(eq.asset_code.clone()),
                    ..SourceBasis::default()
                },
            ))
        }
        LifeSource::Weibull { beta, eta } if ok(*beta) && ok(*eta) => Ok((
            *beta,
            *eta,
            SourceBasis {
                kind: "weibull".into(),
                beta: Some(*beta),
                eta: Some(*eta),
                ..SourceBasis::default()
            },
        )),
        LifeSource::Exponential { mtbf_hours } if ok(*mtbf_hours) => Ok((
            1.0,
            *mtbf_hours,
            SourceBasis {
                kind: "exponential".into(),
                mtbf_hours: Some(*mtbf_hours),
                ..SourceBasis::default()
            },
        )),
        LifeSource::Rate { per_hour } if ok(*per_hour) => Ok((
            1.0,
            1.0 / per_hour,
            SourceBasis {
                kind: "rate".into(),
                mtbf_hours: Some(1.0 / per_hour),
                ..SourceBasis::default()
            },
        )),
        LifeSource::Reliability { .. } | LifeSource::Probability { .. } => Err("needs_life_distribution"),
        _ => Err("invalid_value"),
    }
}

/// Reliability of a source over `t` hours.
pub fn reliability_of(source: &LifeSource, data: &SourceData, t: f64) -> Result<(f64, SourceBasis), &'static str> {
    match source {
        LifeSource::Reliability { value } if (0.0..=1.0).contains(value) => Ok((
            *value,
            SourceBasis {
                kind: "reliability".into(),
                ..SourceBasis::default()
            },
        )),
        LifeSource::Probability { value } if (0.0..=1.0).contains(value) => Ok((
            1.0 - value,
            SourceBasis {
                kind: "probability".into(),
                ..SourceBasis::default()
            },
        )),
        LifeSource::Reliability { .. } | LifeSource::Probability { .. } => Err("invalid_value"),
        other => {
            let (b, e, basis) = life_of(other, data)?;
            Ok((reliability(t, b, e), basis))
        }
    }
}

/// Probability that at least `k` of independent items with reliabilities `rs` work.
#[must_use]
pub fn k_out_of_n(k: usize, rs: &[f64]) -> f64 {
    // dp[j] = probability that exactly j items work among those seen so far.
    let mut dp = vec![0.0; rs.len() + 1];
    dp[0] = 1.0;
    for (i, r) in rs.iter().enumerate() {
        for j in (0..=i + 1).rev() {
            let work = if j > 0 { dp[j - 1] * r } else { 0.0 };
            dp[j] = dp[j] * (1.0 - r) + work;
        }
    }
    dp.iter().skip(k).sum::<f64>().clamp(0.0, 1.0)
}

/// Structure checks: unique ids, references, no cycle, types, sources and k.
pub fn check_rbd(spec: &RbdSpec) -> Vec<ModelIssue> {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    for n in &spec.nodes {
        if !seen.insert(n.id.as_str()) {
            issues.push(ModelIssue {
                node_id: n.id.clone(),
                code: "duplicate_id".into(),
            });
        }
    }
    let by_id: HashMap<&str, &RbdNode> = spec.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    if !by_id.contains_key(spec.root_id.as_str()) {
        issues.push(ModelIssue {
            node_id: spec.root_id.clone(),
            code: "missing_node".into(),
        });
        return issues;
    }
    for n in &spec.nodes {
        match n.node_type.as_str() {
            "block" => {
                if n.source.is_none() {
                    issues.push(ModelIssue {
                        node_id: n.id.clone(),
                        code: "no_source".into(),
                    });
                }
            }
            "series" | "parallel" => {
                if n.children.is_empty() {
                    issues.push(ModelIssue {
                        node_id: n.id.clone(),
                        code: "empty_structure".into(),
                    });
                }
                for c in &n.children {
                    if !by_id.contains_key(c.as_str()) {
                        issues.push(ModelIssue {
                            node_id: c.clone(),
                            code: "missing_node".into(),
                        });
                    }
                }
                if n.node_type == "parallel" {
                    let k = n.k.unwrap_or(1);
                    if k < 1 || k as usize > n.children.len().max(1) {
                        issues.push(ModelIssue {
                            node_id: n.id.clone(),
                            code: "invalid_k".into(),
                        });
                    }
                }
            }
            _ => issues.push(ModelIssue {
                node_id: n.id.clone(),
                code: "invalid_value".into(),
            }),
        }
    }
    // Cycle check from the root.
    fn visit<'a>(id: &'a str, by: &HashMap<&'a str, &'a RbdNode>, stack: &mut Vec<&'a str>, out: &mut Vec<ModelIssue>) {
        if stack.contains(&id) {
            out.push(ModelIssue {
                node_id: id.to_string(),
                code: "cycle".into(),
            });
            return;
        }
        let Some(n) = by.get(id) else { return };
        stack.push(id);
        for c in &n.children {
            visit(c.as_str(), by, stack, out);
        }
        stack.pop();
    }
    if issues.is_empty() {
        visit(spec.root_id.as_str(), &by_id, &mut Vec::new(), &mut issues);
    }
    issues
}

/// System reliability for given block reliabilities (structure assumed valid).
#[must_use]
pub fn rbd_value(spec: &RbdSpec, block_r: &HashMap<String, f64>) -> f64 {
    let by_id: HashMap<&str, &RbdNode> = spec.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    fn eval(id: &str, by: &HashMap<&str, &RbdNode>, r: &HashMap<String, f64>) -> f64 {
        let Some(n) = by.get(id) else { return 0.0 };
        match n.node_type.as_str() {
            "block" => r.get(id).copied().unwrap_or(0.0),
            "series" => n.children.iter().map(|c| eval(c, by, r)).product(),
            _ => {
                let rs: Vec<f64> = n.children.iter().map(|c| eval(c, by, r)).collect();
                k_out_of_n(n.k.unwrap_or(1).max(1) as usize, &rs)
            }
        }
    }
    eval(&spec.root_id, &by_id, block_r)
}

fn node_value(spec: &RbdSpec, id: &str, block_r: &HashMap<String, f64>) -> f64 {
    rbd_value(
        &RbdSpec {
            root_id: id.to_string(),
            nodes: spec.nodes.clone(),
        },
        block_r,
    )
}

/// Evaluate an RBD over `mission_hours` (pure).
#[must_use]
pub fn evaluate_rbd_spec(spec: &RbdSpec, data: &SourceData, mission_hours: f64) -> RbdResult {
    let mut issues = check_rbd(spec);
    if !(mission_hours.is_finite() && mission_hours > 0.0) {
        issues.push(ModelIssue {
            node_id: spec.root_id.clone(),
            code: "invalid_value".into(),
        });
    }
    let mut block_r = HashMap::new();
    let mut bases = HashMap::new();
    for n in spec.nodes.iter().filter(|n| n.node_type == "block") {
        if let Some(src) = &n.source {
            match reliability_of(src, data, mission_hours) {
                Ok((r, basis)) => {
                    block_r.insert(n.id.clone(), r);
                    bases.insert(n.id.clone(), basis);
                }
                Err(code) => issues.push(ModelIssue {
                    node_id: n.id.clone(),
                    code: code.into(),
                }),
            }
        }
    }
    if !issues.is_empty() {
        return RbdResult {
            mission_hours,
            system_reliability: None,
            nodes: Vec::new(),
            groups: Vec::new(),
            issues,
        };
    }
    let rs = rbd_value(spec, &block_r);
    let mut nodes = Vec::new();
    let mut group_crit: Vec<(String, f64)> = Vec::new();
    for n in &spec.nodes {
        if n.node_type == "block" {
            let ri = block_r[&n.id];
            let mut up = block_r.clone();
            up.insert(n.id.clone(), 1.0);
            let mut down = block_r.clone();
            down.insert(n.id.clone(), 0.0);
            let ib = rbd_value(spec, &up) - rbd_value(spec, &down);
            let ic = if rs < 1.0 { ib * (1.0 - ri) / (1.0 - rs) } else { 0.0 };
            let group = n
                .group
                .clone()
                .filter(|g| !g.trim().is_empty())
                .unwrap_or_else(|| n.label.clone());
            match group_crit.iter_mut().find(|(g, _)| *g == group) {
                Some(g) => g.1 += ic,
                None => group_crit.push((group, ic)),
            }
            nodes.push(RbdNodeResult {
                id: n.id.clone(),
                label: n.label.clone(),
                node_type: n.node_type.clone(),
                reliability: ri,
                birnbaum: Some(ib),
                criticality: Some(ic),
                basis: bases.get(&n.id).cloned(),
            });
        } else {
            nodes.push(RbdNodeResult {
                id: n.id.clone(),
                label: n.label.clone(),
                node_type: n.node_type.clone(),
                reliability: node_value(spec, &n.id, &block_r),
                birnbaum: None,
                criticality: None,
                basis: None,
            });
        }
    }
    let total: f64 = group_crit.iter().map(|g| g.1).sum();
    let mut groups: Vec<GroupShare> = group_crit
        .into_iter()
        .map(|(group, v)| GroupShare {
            group,
            share: if total > 0.0 { v / total } else { 0.0 },
        })
        .collect();
    groups.sort_by(|a, b| b.share.partial_cmp(&a.share).unwrap_or(std::cmp::Ordering::Equal));
    RbdResult {
        mission_hours,
        system_reliability: Some(rs),
        nodes,
        groups,
        issues,
    }
}

pub fn check_ft(spec: &FtSpec) -> Vec<ModelIssue> {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    for n in &spec.nodes {
        if !seen.insert(n.id.as_str()) {
            issues.push(ModelIssue {
                node_id: n.id.clone(),
                code: "duplicate_id".into(),
            });
        }
    }
    let ids: HashSet<&str> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
    match spec.nodes.iter().find(|n| n.id == spec.top_id) {
        None => issues.push(ModelIssue {
            node_id: spec.top_id.clone(),
            code: "missing_node".into(),
        }),
        Some(top) if top.node_type == "basic" => issues.push(ModelIssue {
            node_id: spec.top_id.clone(),
            code: "wrong_source".into(),
        }),
        Some(_) => {}
    }
    for n in &spec.nodes {
        match n.node_type.as_str() {
            "basic" => {
                if n.source.is_none() {
                    issues.push(ModelIssue {
                        node_id: n.id.clone(),
                        code: "no_source".into(),
                    });
                }
            }
            "and" | "or" => {
                if n.inputs.is_empty() {
                    issues.push(ModelIssue {
                        node_id: n.id.clone(),
                        code: "empty_structure".into(),
                    });
                }
                for c in &n.inputs {
                    if !ids.contains(c.as_str()) {
                        issues.push(ModelIssue {
                            node_id: c.clone(),
                            code: "missing_node".into(),
                        });
                    }
                }
            }
            _ => issues.push(ModelIssue {
                node_id: n.id.clone(),
                code: "invalid_value".into(),
            }),
        }
    }
    issues
}

fn fta_graph(spec: &FtSpec, p: &HashMap<String, f64>) -> FtaGraph {
    let nodes = spec
        .nodes
        .iter()
        .map(|n| {
            let kind = match n.node_type.as_str() {
                "basic" => FtaNodeKind::Basic {
                    p: p.get(&n.id).copied().unwrap_or(0.0),
                },
                "and" => FtaNodeKind::And {
                    inputs: n.inputs.clone(),
                },
                _ => FtaNodeKind::Or {
                    inputs: n.inputs.clone(),
                },
            };
            (n.id.clone(), kind)
        })
        .collect();
    FtaGraph {
        spec_version: 1,
        top_id: spec.top_id.clone(),
        nodes,
    }
}

/// Probability of a basic event over `t` hours.
pub fn event_probability(source: &LifeSource, data: &SourceData, t: f64) -> Result<(f64, SourceBasis), &'static str> {
    match source {
        LifeSource::Probability { value } if (0.0..=1.0).contains(value) => Ok((
            *value,
            SourceBasis {
                kind: "probability".into(),
                ..SourceBasis::default()
            },
        )),
        other => reliability_of(other, data, t).map(|(r, b)| (1.0 - r, b)),
    }
}

/// Evaluate a fault tree over `mission_hours` (pure).
#[must_use]
pub fn evaluate_ft_spec(spec: &FtSpec, data: &SourceData, mission_hours: f64) -> FtResult {
    let mut issues = check_ft(spec);
    let mut p = HashMap::new();
    let mut bases = HashMap::new();
    for n in spec.nodes.iter().filter(|n| n.node_type == "basic") {
        if let Some(src) = &n.source {
            match event_probability(src, data, mission_hours) {
                Ok((v, basis)) => {
                    p.insert(n.id.clone(), v);
                    bases.insert(n.id.clone(), basis);
                }
                Err(code) => issues.push(ModelIssue {
                    node_id: n.id.clone(),
                    code: code.into(),
                }),
            }
        }
    }
    let empty = |issues: Vec<ModelIssue>| FtResult {
        mission_hours,
        top_probability: None,
        probability_method: None,
        cut_sets: Vec::new(),
        events: Vec::new(),
        issues,
    };
    if !issues.is_empty() {
        return empty(issues);
    }
    let base = match evaluate_fta(&fta_graph(spec, &p)) {
        Ok(r) => r,
        Err(_) => {
            return empty(vec![ModelIssue {
                node_id: spec.top_id.clone(),
                code: "cycle".into(),
            }])
        }
    };
    let label: HashMap<&str, &str> = spec.nodes.iter().map(|n| (n.id.as_str(), n.label.as_str())).collect();
    let top = base.top_probability;
    let mut events = Vec::new();
    for n in spec.nodes.iter().filter(|n| n.node_type == "basic") {
        let with = |v: f64| {
            let mut q = p.clone();
            q.insert(n.id.clone(), v);
            evaluate_fta(&fta_graph(spec, &q))
                .map(|r| r.top_probability)
                .unwrap_or(f64::NAN)
        };
        let (hi, lo) = (with(1.0), with(0.0));
        events.push(EventResult {
            id: n.id.clone(),
            label: n.label.clone(),
            probability: p[&n.id],
            birnbaum: hi - lo,
            fussell_vesely: if top > 0.0 { 1.0 - lo / top } else { 0.0 },
            risk_achievement_worth: if top > 0.0 { hi / top } else { 0.0 },
            basis: bases.get(&n.id).cloned(),
        });
    }
    events.sort_by(|a, b| {
        b.fussell_vesely
            .partial_cmp(&a.fussell_vesely)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut cut_sets: Vec<CutSetResult> = base
        .minimal_cut_sets
        .iter()
        .zip(base.cut_set_probabilities.iter())
        .map(|(ids, prob)| CutSetResult {
            labels: ids
                .iter()
                .map(|i| label.get(i.as_str()).copied().unwrap_or("").to_string())
                .collect(),
            event_ids: ids.clone(),
            probability: *prob,
        })
        .collect();
    cut_sets.sort_by(|a, b| {
        b.probability
            .partial_cmp(&a.probability)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    FtResult {
        mission_hours,
        top_probability: Some(top),
        probability_method: Some(base.probability_method),
        cut_sets,
        events,
        issues,
    }
}

// ── Data and persistence ────────────────────────────────────────────────────────────────────

/// Fit, study MTTR and the recorded life of every study equipment.
pub async fn load_source_data(db: &DatabaseConnection, study: &ReliabilityStudy) -> AppResult<(SourceData, String)> {
    let (a, built) = life_analysis(
        db,
        LifeAnalysisRequest {
            study_id: study.id,
            ..LifeAnalysisRequest::default()
        },
    )
    .await?;
    let operating = a.effective_time_basis == BASIS_OPERATING;
    let start = parse_instant(&study.period_start)?;
    let end = built.period_end;
    let mut equipment = HashMap::new();
    let mut repair_sum = 0.0;
    let mut repair_n = 0_i64;
    for asset in &study.assets {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n, COALESCE(SUM(active_repair_hours), 0.0) AS rep FROM failure_events
                 WHERE equipment_id = ? AND COALESCE(is_planned, 0) = 0
                   AND datetime(COALESCE(failed_at, detected_at, created_at)) >= datetime(?)
                   AND datetime(COALESCE(failed_at, detected_at, created_at)) <= datetime(?)",
                [
                    asset.equipment_id.into(),
                    start.to_rfc3339().into(),
                    end.to_rfc3339().into(),
                ],
            ))
            .await?;
        let (n, rep): (i64, f64) = match row {
            Some(r) => (get(&r, "n")?, get(&r, "rep")?),
            None => (0, 0.0),
        };
        let clock = load_clock(db, asset.equipment_id).await?;
        let clock = if operating && clock.is_operating() {
            clock
        } else {
            StudyClock::Calendar
        };
        let hours = clock.hours_between(start, end);
        repair_sum += rep;
        repair_n += n;
        equipment.insert(
            asset.equipment_id,
            EquipmentLife {
                equipment_id: asset.equipment_id,
                asset_code: asset.asset_code.clone(),
                asset_name: asset.asset_name.clone(),
                failures: n,
                hours,
                mtbf_hours: (n > 0 && hours > 0.0).then(|| hours / n as f64),
                mttr_hours: (n > 0).then(|| rep / n as f64),
            },
        );
    }
    Ok((
        SourceData {
            fit: a.fit.as_ref().map(|f| (f.beta, f.eta)),
            study_mttr_hours: (repair_n > 0).then(|| repair_sum / repair_n as f64),
            equipment,
        },
        a.effective_time_basis,
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedModel<T> {
    pub model_id: i64,
    pub row_version: i64,
    pub title: String,
    pub spec: T,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemModel {
    pub study_id: i64,
    pub mission_hours: Option<f64>,
    pub time_basis: String,
    pub fit: Option<(f64, f64)>,
    pub study_mttr_hours: Option<f64>,
    pub equipment: Vec<EquipmentLife>,
    pub rbd: Option<SavedModel<RbdSpec>>,
    pub fta: Option<SavedModel<FtSpec>>,
    pub editable: bool,
}

#[derive(Debug, Deserialize)]
struct StoredGraph<T> {
    study_spec: T,
}

async fn study_links(db: &impl ConnectionTrait, study_id: i64) -> AppResult<(Option<i64>, Option<i64>, Option<f64>)> {
    let r = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rbd_model_id, fta_model_id, mission_hours FROM reliability_studies WHERE id = ?",
            [study_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reliability_study".into(),
            id: study_id.to_string(),
        })?;
    Ok((
        get(&r, "rbd_model_id")?,
        get(&r, "fta_model_id")?,
        get(&r, "mission_hours")?,
    ))
}

async fn load_saved<T: for<'de> Deserialize<'de>>(
    db: &impl ConnectionTrait,
    table: &str,
    id: Option<i64>,
) -> AppResult<Option<SavedModel<T>>> {
    let Some(id) = id else { return Ok(None) };
    let Some(r) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT id, row_version, title, graph_json FROM {table} WHERE id = ?"),
            [id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let graph: String = get(&r, "graph_json")?;
    let Ok(stored) = serde_json::from_str::<StoredGraph<T>>(&graph) else {
        return Ok(None);
    };
    Ok(Some(SavedModel {
        model_id: get(&r, "id")?,
        row_version: get(&r, "row_version")?,
        title: get(&r, "title")?,
        spec: stored.study_spec,
    }))
}

pub async fn get_system_model(db: &DatabaseConnection, study_id: i64) -> AppResult<SystemModel> {
    let study = get_study(db, study_id).await?;
    let (data, basis) = load_source_data(db, &study).await?;
    let (rbd_id, fta_id, mission) = study_links(db, study_id).await?;
    let mut equipment: Vec<EquipmentLife> = data.equipment.values().cloned().collect();
    equipment.sort_by(|a, b| a.asset_code.cmp(&b.asset_code));
    Ok(SystemModel {
        study_id,
        mission_hours: mission,
        time_basis: basis,
        fit: data.fit,
        study_mttr_hours: data.study_mttr_hours,
        equipment,
        rbd: load_saved(db, "rbd_models", rbd_id).await?,
        fta: load_saved(db, "fta_models", fta_id).await?,
        editable: study.status == STUDY_ACTIVE,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvaluateRbdInput {
    pub study_id: i64,
    pub mission_hours: f64,
    pub spec: RbdSpec,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvaluateFtInput {
    pub study_id: i64,
    pub mission_hours: f64,
    pub spec: FtSpec,
}

pub async fn evaluate_rbd_for_study(db: &DatabaseConnection, input: EvaluateRbdInput) -> AppResult<RbdResult> {
    let study = get_study(db, input.study_id).await?;
    let (data, _) = load_source_data(db, &study).await?;
    Ok(evaluate_rbd_spec(&input.spec, &data, input.mission_hours))
}

pub async fn evaluate_ft_for_study(db: &DatabaseConnection, input: EvaluateFtInput) -> AppResult<FtResult> {
    let study = get_study(db, input.study_id).await?;
    let (data, _) = load_source_data(db, &study).await?;
    Ok(evaluate_ft_spec(&input.spec, &data, input.mission_hours))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveSystemModelInput {
    pub study_id: i64,
    pub mission_hours: f64,
    #[serde(default)]
    pub rbd: Option<RbdSpec>,
    #[serde(default)]
    pub fta: Option<FtSpec>,
}

async fn save_graph(
    db: &DatabaseConnection,
    study: &ReliabilityStudy,
    kind: &str,
    existing: Option<i64>,
    graph_json: String,
    actor_id: i64,
) -> AppResult<i64> {
    let first = study.assets.first().ok_or_else(|| invalid("STUDY_NO_ASSETS"))?;
    let title = format!("{} · {}", if kind == "rbd" { "RBD" } else { "FTA" }, study.title);
    let current = match existing {
        Some(id) => db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT row_version FROM {}_models WHERE id = ?", kind),
                [id.into()],
            ))
            .await?
            .map(|r| get::<i64>(&r, "row_version"))
            .transpose()?
            .map(|v| (id, v)),
        None => None,
    };
    let id = match (kind, current) {
        ("rbd", Some((id, v))) => {
            model_queries::update_rbd_model(
                db,
                UpdateRbdModelInput {
                    id,
                    expected_row_version: v,
                    title: Some(title),
                    graph_json: Some(graph_json),
                    status: None,
                },
            )
            .await?
            .id
        }
        ("rbd", None) => {
            model_queries::create_rbd_model(
                db,
                Some(actor_id),
                CreateRbdModelInput {
                    equipment_id: first.equipment_id,
                    title,
                    graph_json: Some(graph_json),
                    status: None,
                },
            )
            .await?
            .id
        }
        (_, Some((id, v))) => {
            model_queries::update_fta_model(
                db,
                UpdateFtaModelInput {
                    id,
                    expected_row_version: v,
                    title: Some(title),
                    graph_json: Some(graph_json),
                    status: None,
                },
            )
            .await?
            .id
        }
        (_, None) => {
            model_queries::create_fta_model(
                db,
                Some(actor_id),
                CreateFtaModelInput {
                    equipment_id: first.equipment_id,
                    title,
                    graph_json: Some(graph_json),
                    status: None,
                },
            )
            .await?
            .id
        }
    };
    // Stored result in the existing format, computed by the existing evaluators.
    if kind == "rbd" {
        model_queries::evaluate_rbd_model(db, id).await?;
    } else {
        model_queries::evaluate_fta_model(db, id).await?;
    }
    Ok(id)
}

pub async fn save_system_model(
    db: &DatabaseConnection,
    input: SaveSystemModelInput,
    actor_id: i64,
) -> AppResult<SystemModel> {
    let study = get_study(db, input.study_id).await?;
    if study.status != STUDY_ACTIVE {
        return Err(invalid("STUDY_ARCHIVED"));
    }
    if !(input.mission_hours.is_finite() && input.mission_hours > 0.0) {
        return Err(invalid("SYSTEM_MISSION_INVALID"));
    }
    let (data, _) = load_source_data(db, &study).await?;
    let (rbd_id, fta_id, _) = study_links(db, study.id).await?;
    let mut new_rbd = rbd_id;
    let mut new_fta = fta_id;
    if let Some(spec) = &input.rbd {
        let res = evaluate_rbd_spec(spec, &data, input.mission_hours);
        if !res.issues.is_empty() {
            return Err(invalid("SYSTEM_MODEL_INCOMPLETE"));
        }
        let nodes: serde_json::Map<String, serde_json::Value> = spec
            .nodes
            .iter()
            .map(|n| {
                let v = match n.node_type.as_str() {
                    "block" => serde_json::json!({
                        "kind": "block",
                        "label": n.label,
                        "r": res.nodes.iter().find(|x| x.id == n.id).map_or(0.0, |x| x.reliability),
                    }),
                    // The stored graph format knows 1-out-of-n only: a k-out-of-n structure is
                    // kept as a block carrying its evaluated reliability.
                    "parallel" if n.k.unwrap_or(1) != 1 => serde_json::json!({
                        "kind": "block",
                        "label": n.label,
                        "r": res.nodes.iter().find(|x| x.id == n.id).map_or(0.0, |x| x.reliability),
                    }),
                    t => serde_json::json!({ "kind": t, "label": n.label, "children": n.children }),
                };
                (n.id.clone(), v)
            })
            .collect();
        let graph = serde_json::json!({
            "spec_version": 1,
            "root_id": spec.root_id,
            "nodes": nodes,
            "mission_hours": input.mission_hours,
            "study_spec": spec,
        });
        new_rbd = Some(save_graph(db, &study, "rbd", rbd_id, graph.to_string(), actor_id).await?);
    }
    if let Some(spec) = &input.fta {
        let res = evaluate_ft_spec(spec, &data, input.mission_hours);
        if !res.issues.is_empty() {
            return Err(invalid("SYSTEM_MODEL_INCOMPLETE"));
        }
        let nodes: serde_json::Map<String, serde_json::Value> = spec
            .nodes
            .iter()
            .map(|n| {
                let v = match n.node_type.as_str() {
                    "basic" => serde_json::json!({
                        "kind": "basic",
                        "label": n.label,
                        "p": res.events.iter().find(|x| x.id == n.id).map_or(0.0, |x| x.probability),
                    }),
                    t => serde_json::json!({ "kind": t, "label": n.label, "inputs": n.inputs }),
                };
                (n.id.clone(), v)
            })
            .collect();
        let graph = serde_json::json!({
            "spec_version": 1,
            "top_id": spec.top_id,
            "nodes": nodes,
            "mission_hours": input.mission_hours,
            "study_spec": spec,
        });
        new_fta = Some(save_graph(db, &study, "fta", fta_id, graph.to_string(), actor_id).await?);
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reliability_studies SET rbd_model_id = ?, fta_model_id = ?, mission_hours = ?,
            row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [
            new_rbd
                .map(sea_orm::Value::from)
                .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            new_fta
                .map(sea_orm::Value::from)
                .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            input.mission_hours.into(),
            crate::clock::now().to_rfc3339().into(),
            study.id.into(),
        ],
    ))
    .await?;
    get_system_model(db, study.id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, label: &str, r: f64, group: &str) -> RbdNode {
        RbdNode {
            id: id.into(),
            label: label.into(),
            node_type: "block".into(),
            children: vec![],
            k: None,
            source: Some(LifeSource::Reliability { value: r }),
            group: Some(group.into()),
            mttr_hours: None,
        }
    }

    fn structure(id: &str, t: &str, children: &[&str], k: Option<i64>) -> RbdNode {
        RbdNode {
            id: id.into(),
            label: id.into(),
            node_type: t.into(),
            children: children.iter().map(|c| c.to_string()).collect(),
            k,
            source: None,
            group: None,
            mttr_hours: None,
        }
    }

    /// The pumping station of the approved mockup: two trains in parallel (1 out of 2), each
    /// motor 0.83 · coupling 0.92 · pump 0.41 in series.
    fn station() -> RbdSpec {
        RbdSpec {
            root_id: "station".into(),
            nodes: vec![
                structure("station", "parallel", &["t1", "t2"], Some(1)),
                structure("t1", "series", &["m1", "c1", "p1"], None),
                structure("t2", "series", &["m2", "c2", "p2"], None),
                block("m1", "Moteur M-101", 0.83, "Moteurs"),
                block("c1", "Accouplement 1", 0.92, "Accouplements"),
                block("p1", "Pompe P-101", 0.41, "Pompes"),
                block("m2", "Moteur M-102", 0.83, "Moteurs"),
                block("c2", "Accouplement 2", 0.92, "Accouplements"),
                block("p2", "Pompe P-102", 0.41, "Pompes"),
            ],
        }
    }

    #[test]
    fn station_rbd_matches_hand_calculation() {
        let r = evaluate_rbd_spec(&station(), &SourceData::default(), 8760.0);
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        let train = 0.83 * 0.92 * 0.41;
        let rs = 1.0 - (1.0 - train) * (1.0 - train);
        assert!((r.system_reliability.unwrap() - rs).abs() < 1e-12);
        let t1 = r.nodes.iter().find(|n| n.id == "t1").unwrap();
        assert!((t1.reliability - train).abs() < 1e-12);
        // Birnbaum of P-101 = ∂Rs/∂R_p1 = (1 − R_t2) · m1 · c1.
        let p1 = r.nodes.iter().find(|n| n.id == "p1").unwrap();
        assert!((p1.birnbaum.unwrap() - (1.0 - train) * 0.83 * 0.92).abs() < 1e-12);
        let ic = p1.birnbaum.unwrap() * (1.0 - 0.41) / (1.0 - rs);
        assert!((p1.criticality.unwrap() - ic).abs() < 1e-12);
        assert_eq!(r.groups[0].group, "Pompes", "pumps dominate the unreliability");
        assert!((r.groups.iter().map(|g| g.share).sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn k_out_of_n_matches_binomial() {
        // 2 out of 3 identical items, R = 0.9: 3R² − 2R³.
        let r = k_out_of_n(2, &[0.9, 0.9, 0.9]);
        assert!((r - (3.0 * 0.81 - 2.0 * 0.729)).abs() < 1e-12);
        assert!((k_out_of_n(1, &[0.9, 0.8]) - (1.0 - 0.1 * 0.2)).abs() < 1e-12);
        assert!((k_out_of_n(2, &[0.9, 0.8]) - 0.72).abs() < 1e-12);
    }

    #[test]
    fn study_fit_blocks_use_the_weibull_reliability_and_report_missing_data() {
        let mut spec = station();
        for n in spec.nodes.iter_mut().filter(|n| n.id.starts_with('p')) {
            n.source = Some(LifeSource::StudyFit);
        }
        let none = evaluate_rbd_spec(&spec, &SourceData::default(), 8760.0);
        assert!(none.system_reliability.is_none());
        assert!(none.issues.iter().any(|i| i.code == "no_fit"));
        let data = SourceData {
            fit: Some((3.04, 4440.0)),
            ..SourceData::default()
        };
        let r = evaluate_rbd_spec(&spec, &data, 1000.0);
        let p1 = r.nodes.iter().find(|n| n.id == "p1").unwrap();
        assert!((p1.reliability - (-(1000.0f64 / 4440.0).powf(3.04)).exp()).abs() < 1e-12);
        assert_eq!(p1.basis.as_ref().unwrap().kind, "study_fit");
        let mut cyc = station();
        cyc.nodes[1].children.push("station".into());
        assert!(evaluate_rbd_spec(&cyc, &SourceData::default(), 10.0)
            .issues
            .iter()
            .any(|i| i.code == "cycle"));
    }

    #[test]
    fn fault_tree_cut_sets_and_importance() {
        // Top = (A AND B) OR C, P(A) = 0.1, P(B) = 0.2, P(C) = 0.05.
        let ev = |id: &str, p: f64| FtNode {
            id: id.into(),
            label: format!("Event {id}"),
            node_type: "basic".into(),
            inputs: vec![],
            source: Some(LifeSource::Probability { value: p }),
        };
        let spec = FtSpec {
            top_id: "top".into(),
            nodes: vec![
                FtNode {
                    id: "top".into(),
                    label: "Perte de transfert".into(),
                    node_type: "or".into(),
                    inputs: vec!["g".into(), "c".into()],
                    source: None,
                },
                FtNode {
                    id: "g".into(),
                    label: "Deux pompes".into(),
                    node_type: "and".into(),
                    inputs: vec!["a".into(), "b".into()],
                    source: None,
                },
                ev("a", 0.1),
                ev("b", 0.2),
                ev("c", 0.05),
            ],
        };
        let r = evaluate_ft_spec(&spec, &SourceData::default(), 8760.0);
        assert!(r.issues.is_empty());
        let top = 0.02 + 0.05 - 0.02 * 0.05;
        assert!((r.top_probability.unwrap() - top).abs() < 1e-12);
        assert_eq!(r.cut_sets.len(), 2);
        assert_eq!(r.cut_sets[0].labels, vec!["Event c".to_string()]);
        let a = r.events.iter().find(|e| e.id == "a").unwrap();
        // Birnbaum(A) = P(B)·(1 − P(C)); FV(A) = 1 − P(top | A = 0)/P(top) = 1 − 0.05/top.
        assert!((a.birnbaum - 0.2 * 0.95).abs() < 1e-12);
        assert!((a.fussell_vesely - (1.0 - 0.05 / top)).abs() < 1e-12);
        assert!((a.risk_achievement_worth - (0.2 + 0.05 - 0.2 * 0.05) / top).abs() < 1e-12);
        let mut bad = spec.clone();
        bad.top_id = "a".into();
        assert!(evaluate_ft_spec(&bad, &SourceData::default(), 1.0)
            .issues
            .iter()
            .any(|i| i.code == "wrong_source"));
    }
}
