//! Fault-tree evaluation through minimal cut sets (MOCUS-style expansion).
//!
//! The top event is expanded into cut sets bottom-up (`OR` = union of the children's cut sets,
//! `AND` = cross product), keeping only minimal sets (absorption) at every gate. The top-event
//! probability is then computed from the minimal cut sets, assuming independent basic events:
//!
//! - up to [`EXACT_MCS_LIMIT`] minimal cut sets: exact, by inclusion–exclusion over the cut sets
//!   (a basic event shared by several branches is counted once, unlike gate-by-gate evaluation);
//! - above that: the min-cut upper bound `1 − Π(1 − P(Cᵢ))`, which is conservative and reported
//!   as such in `probability_method`.

use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Persisted with each evaluation so stored results can be traced to the method that produced them.
pub const FTA_ALGORITHM_VERSION: &str = "fta_mcs_v2";
/// Number of minimal cut sets up to which the top probability is exact (2^n inclusion–exclusion terms).
pub const EXACT_MCS_LIMIT: usize = 16;
/// Guard against combinatorial explosion of intermediate cut sets.
pub const MAX_CUT_SETS: usize = 50_000;

pub const METHOD_EXACT: &str = "exact_inclusion_exclusion";
pub const METHOD_MCUB: &str = "min_cut_upper_bound";

#[derive(Debug, Deserialize)]
pub struct FtaGraph {
    pub spec_version: u32,
    pub top_id: String,
    pub nodes: HashMap<String, FtaNodeKind>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FtaNodeKind {
    Basic { p: f64 },
    And { inputs: Vec<String> },
    Or { inputs: Vec<String> },
}

#[derive(Debug, serde::Serialize)]
pub struct FtaEvalResult {
    pub top_probability: f64,
    pub minimal_cut_sets: Vec<Vec<String>>,
    /// Probability of each minimal cut set (same order as `minimal_cut_sets`).
    pub cut_set_probabilities: Vec<f64>,
    pub basic_event_ids: Vec<String>,
    /// `exact_inclusion_exclusion` or `min_cut_upper_bound`.
    pub probability_method: String,
    pub algorithm_version: String,
    pub message: String,
}

type CutSet = BTreeSet<String>;

/// Keeps only minimal sets: drops duplicates and every set that contains another one.
fn minimize(mut sets: Vec<CutSet>) -> Vec<CutSet> {
    sets.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    sets.dedup();
    let mut minimal: Vec<CutSet> = Vec::with_capacity(sets.len());
    for s in sets {
        if !minimal.iter().any(|m| m.is_subset(&s)) {
            minimal.push(s);
        }
    }
    minimal
}

fn cut_sets(graph: &FtaGraph, id: &str, visiting: &mut HashSet<String>) -> Result<Vec<CutSet>, String> {
    if !visiting.insert(id.to_string()) {
        return Err(format!("FTA cycle at '{id}'"));
    }
    let node = graph.nodes.get(id).ok_or_else(|| format!("FTA missing node '{id}'"))?;
    let out = match node {
        FtaNodeKind::Basic { p } => {
            if !p.is_finite() || !(0.0..=1.0).contains(p) {
                return Err(format!("FTA basic event '{id}' has probability {p} outside [0, 1]"));
            }
            vec![CutSet::from([id.to_string()])]
        }
        // An OR gate without inputs never fails: no cut set.
        FtaNodeKind::Or { inputs } => {
            let mut all = Vec::new();
            for c in inputs {
                all.extend(cut_sets(graph, c, visiting)?);
                if all.len() > MAX_CUT_SETS {
                    return Err(format!("FTA has more than {MAX_CUT_SETS} cut sets under '{id}'"));
                }
            }
            minimize(all)
        }
        // An AND gate without inputs is always failed: the empty cut set.
        FtaNodeKind::And { inputs } => {
            let mut acc: Vec<CutSet> = vec![CutSet::new()];
            for c in inputs {
                let child = cut_sets(graph, c, visiting)?;
                let mut next = Vec::with_capacity(acc.len() * child.len().max(1));
                for a in &acc {
                    for b in &child {
                        next.push(a.union(b).cloned().collect::<CutSet>());
                    }
                }
                if next.len() > MAX_CUT_SETS {
                    return Err(format!("FTA has more than {MAX_CUT_SETS} cut sets under '{id}'"));
                }
                acc = minimize(next);
            }
            acc
        }
    };
    visiting.remove(id);
    Ok(out)
}

fn basic_probability(graph: &FtaGraph, id: &str) -> f64 {
    match graph.nodes.get(id) {
        Some(FtaNodeKind::Basic { p }) => *p,
        _ => 0.0,
    }
}

fn set_probability(graph: &FtaGraph, set: &CutSet) -> f64 {
    set.iter().map(|b| basic_probability(graph, b)).product()
}

/// Exact P(∪ Cᵢ) by inclusion–exclusion; the probability of an intersection of cut sets is the
/// product over the union of their basic events (independent events).
fn inclusion_exclusion(graph: &FtaGraph, sets: &[CutSet]) -> f64 {
    let n = sets.len();
    let mut total = 0.0_f64;
    for mask in 1usize..(1usize << n) {
        let mut union = CutSet::new();
        for (i, s) in sets.iter().enumerate() {
            if mask & (1 << i) != 0 {
                union.extend(s.iter().cloned());
            }
        }
        let term = set_probability(graph, &union);
        if mask.count_ones() % 2 == 1 {
            total += term;
        } else {
            total -= term;
        }
    }
    total.clamp(0.0, 1.0)
}

pub fn evaluate_fta(graph: &FtaGraph) -> Result<FtaEvalResult, String> {
    let mut basics: Vec<String> = graph
        .nodes
        .iter()
        .filter_map(|(id, n)| matches!(n, FtaNodeKind::Basic { .. }).then_some(id.clone()))
        .collect();
    basics.sort();

    let mut visiting = HashSet::new();
    let minimal = cut_sets(graph, &graph.top_id, &mut visiting)?;
    let probs: Vec<f64> = minimal.iter().map(|s| set_probability(graph, s)).collect();

    let (top, method, message) = if minimal.is_empty() {
        (
            0.0,
            METHOD_EXACT,
            "The top event cannot occur (no cut set).".to_string(),
        )
    } else if minimal.len() <= EXACT_MCS_LIMIT {
        (inclusion_exclusion(graph, &minimal), METHOD_EXACT, String::new())
    } else {
        let survive: f64 = probs.iter().map(|p| 1.0 - p).product();
        (
            (1.0 - survive).clamp(0.0, 1.0),
            METHOD_MCUB,
            format!(
                "{} minimal cut sets: top probability is the min-cut upper bound (conservative).",
                minimal.len()
            ),
        )
    };

    Ok(FtaEvalResult {
        top_probability: top,
        minimal_cut_sets: minimal.into_iter().map(|s| s.into_iter().collect()).collect(),
        cut_set_probabilities: probs,
        basic_event_ids: basics,
        probability_method: method.to_string(),
        algorithm_version: FTA_ALGORITHM_VERSION.to_string(),
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(p: f64) -> FtaNodeKind {
        FtaNodeKind::Basic { p }
    }
    fn or(inputs: &[&str]) -> FtaNodeKind {
        FtaNodeKind::Or {
            inputs: inputs.iter().map(|s| (*s).to_string()).collect(),
        }
    }
    fn and(inputs: &[&str]) -> FtaNodeKind {
        FtaNodeKind::And {
            inputs: inputs.iter().map(|s| (*s).to_string()).collect(),
        }
    }
    fn graph(nodes: Vec<(&str, FtaNodeKind)>) -> FtaGraph {
        FtaGraph {
            spec_version: 1,
            top_id: "top".into(),
            nodes: nodes.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }

    #[test]
    fn or_of_two_basics() {
        let g = graph(vec![("top", or(&["a", "b"])), ("a", basic(0.1)), ("b", basic(0.2))]);
        let r = evaluate_fta(&g).unwrap();
        assert!((r.top_probability - (1.0 - 0.9 * 0.8)).abs() < 1e-12);
        assert_eq!(r.minimal_cut_sets, vec![vec!["a".to_string()], vec!["b".to_string()]]);
        assert_eq!(r.probability_method, METHOD_EXACT);
    }

    /// A basic event shared by two branches: gate-by-gate evaluation double-counts it.
    /// top = AND(OR(a, b), OR(a, c)) ⇒ MCS {a}, {b, c}; P = pa + pb·pc − pa·pb·pc.
    #[test]
    fn shared_event_is_counted_once() {
        let (pa, pb, pc) = (0.1, 0.2, 0.3);
        let g = graph(vec![
            ("top", and(&["g1", "g2"])),
            ("g1", or(&["a", "b"])),
            ("g2", or(&["a", "c"])),
            ("a", basic(pa)),
            ("b", basic(pb)),
            ("c", basic(pc)),
        ]);
        let r = evaluate_fta(&g).unwrap();
        assert_eq!(
            r.minimal_cut_sets,
            vec![vec!["a".to_string()], vec!["b".to_string(), "c".to_string()]]
        );
        let exact = pa + pb * pc - pa * pb * pc;
        assert!((r.top_probability - exact).abs() < 1e-12, "{}", r.top_probability);
        let gate_by_gate = (1.0 - (1.0 - pa) * (1.0 - pb)) * (1.0 - (1.0 - pa) * (1.0 - pc));
        assert!((r.top_probability - gate_by_gate).abs() > 1e-3);
        assert_eq!(r.cut_set_probabilities.len(), 2);
    }

    #[test]
    fn absorption_removes_non_minimal_sets() {
        // top = OR(a, AND(a, b)) ⇒ {a} only.
        let g = graph(vec![
            ("top", or(&["a", "g"])),
            ("g", and(&["a", "b"])),
            ("a", basic(0.5)),
            ("b", basic(0.5)),
        ]);
        let r = evaluate_fta(&g).unwrap();
        assert_eq!(r.minimal_cut_sets, vec![vec!["a".to_string()]]);
        assert!((r.top_probability - 0.5).abs() < 1e-12);
    }

    #[test]
    fn many_cut_sets_use_the_upper_bound() {
        let names: Vec<String> = (0..20).map(|i| format!("e{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut nodes = vec![("top", or(&refs))];
        for n in &refs {
            nodes.push((*n, basic(0.01)));
        }
        let r = evaluate_fta(&graph(nodes)).unwrap();
        assert_eq!(r.probability_method, METHOD_MCUB);
        // Disjoint single-event cut sets: the bound is exact here.
        assert!((r.top_probability - (1.0 - 0.99_f64.powi(20))).abs() < 1e-12);
    }

    #[test]
    fn empty_or_never_fails_and_empty_and_always_fails() {
        let r = evaluate_fta(&graph(vec![("top", or(&[]))])).unwrap();
        assert!(r.top_probability.abs() < 1e-12);
        assert!(r.minimal_cut_sets.is_empty());
        let r = evaluate_fta(&graph(vec![("top", and(&[]))])).unwrap();
        assert!((r.top_probability - 1.0).abs() < 1e-12);
    }

    #[test]
    fn invalid_probability_and_cycles_are_rejected() {
        assert!(evaluate_fta(&graph(vec![("top", basic(2.0))])).is_err());
        let g = graph(vec![("top", or(&["g"])), ("g", and(&["top"]))]);
        assert!(evaluate_fta(&g).unwrap_err().contains("cycle"));
        assert!(evaluate_fta(&graph(vec![("top", or(&["missing"]))])).is_err());
    }
}
