use serde::Deserialize;
use std::collections::{HashMap, HashSet};

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
    pub basic_event_ids: Vec<String>,
    pub message: String,
}

fn eval_node(
    graph: &FtaGraph,
    id: &str,
    visiting: &mut HashSet<String>,
    failed: &HashSet<String>,
) -> Result<f64, String> {
    if visiting.contains(id) {
        return Err(format!("FTA cycle at '{id}'"));
    }
    visiting.insert(id.to_string());
    let node = graph
        .nodes
        .get(id)
        .ok_or_else(|| format!("FTA missing node '{id}'"))?;
    let r: Result<f64, String> = match node {
        FtaNodeKind::Basic { p } => {
            let v = if failed.contains(id) { 1.0 } else { *p };
            Ok(v.clamp(0.0, 1.0))
        }
        FtaNodeKind::And { inputs } => {
            if inputs.is_empty() {
                Ok(1.0)
            } else {
                let mut prod = 1.0f64;
                for c in inputs {
                    prod *= eval_node(graph, c, visiting, failed)?;
                }
                Ok(prod.clamp(0.0, 1.0))
            }
        }
        FtaNodeKind::Or { inputs } => {
            if inputs.is_empty() {
                Ok(0.0)
            } else {
                let mut prod = 1.0f64;
                for c in inputs {
                    let pc = eval_node(graph, c, visiting, failed)?;
                    prod *= 1.0 - pc;
                }
                Ok((1.0 - prod).clamp(0.0, 1.0))
            }
        }
    };
    visiting.remove(id);
    r
}

pub fn evaluate_fta(graph: &FtaGraph) -> Result<FtaEvalResult, String> {
    let mut basics: Vec<String> = graph
        .nodes
        .iter()
        .filter_map(|(id, n)| matches!(n, FtaNodeKind::Basic { .. }).then_some(id.clone()))
        .collect();
    basics.sort();
    let empty: HashSet<String> = HashSet::new();
    let mut vis = HashSet::new();
    let top = eval_node(graph, &graph.top_id, &mut vis, &empty)?;
    let mut mcs: Vec<Vec<String>> = Vec::new();
    let n = basics.len();
    let mut msg = String::new();
    if n > 0 && n <= 16 {
        let mut cut_sets: Vec<HashSet<String>> = Vec::new();
        for mask in 1usize..(1usize << n) {
            let mut failed: HashSet<String> = HashSet::new();
            for i in 0..n {
                if (mask & (1 << i)) != 0 {
                    failed.insert(basics[i].clone());
                }
            }
            vis.clear();
            let pt = eval_node(graph, &graph.top_id, &mut vis, &failed)?;
            if pt >= 0.999 {
                cut_sets.push(failed);
            }
        }
        cut_sets.sort_by_key(|s| s.len());
        let mut minimal: Vec<HashSet<String>> = Vec::new();
        for s in cut_sets {
            if minimal.iter().any(|m| m.is_subset(&s)) {
                continue;
            }
            minimal.retain(|m| !s.is_subset(m));
            minimal.push(s);
        }
        mcs = minimal
            .into_iter()
            .map(|mut h| {
                let mut v: Vec<String> = h.drain().collect();
                v.sort();
                v
            })
            .collect();
        mcs.sort();
    } else if n > 16 {
        msg = format!("MCS skipped ({n} basic events); use ≤16 for brute-force MCS.");
    }
    Ok(FtaEvalResult {
        top_probability: top,
        minimal_cut_sets: mcs,
        basic_event_ids: basics,
        message: msg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn fta_or_of_two_basics() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "top".into(),
            FtaNodeKind::Or {
                inputs: vec!["a".into(), "b".into()],
            },
        );
        nodes.insert("a".into(), FtaNodeKind::Basic { p: 0.1 });
        nodes.insert("b".into(), FtaNodeKind::Basic { p: 0.2 });
        let g = FtaGraph {
            spec_version: 1,
            top_id: "top".into(),
            nodes,
        };
        let r = evaluate_fta(&g).unwrap();
        assert!((r.top_probability - (1.0 - 0.9 * 0.8)).abs() < 1e-9);
    }

    #[test]
    fn fta_empty_or_zero() {
        let mut nodes = HashMap::new();
        nodes.insert("top".into(), FtaNodeKind::Or { inputs: vec![] });
        let g = FtaGraph {
            spec_version: 1,
            top_id: "top".into(),
            nodes,
        };
        let r = evaluate_fta(&g).unwrap();
        assert!(r.top_probability.abs() < 1e-9);
    }

    #[test]
    fn fta_basic_prob_clamped() {
        let mut nodes = HashMap::new();
        nodes.insert("top".into(), FtaNodeKind::Basic { p: 2.0 });
        let g = FtaGraph {
            spec_version: 1,
            top_id: "top".into(),
            nodes,
        };
        let r = evaluate_fta(&g).unwrap();
        assert!((r.top_probability - 1.0).abs() < 1e-9);
    }
}
