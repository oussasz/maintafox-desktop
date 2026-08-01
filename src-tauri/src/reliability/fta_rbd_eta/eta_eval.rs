use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Deserialize)]
pub struct EtaGraph {
    pub spec_version: u32,
    pub root_id: String,
    pub nodes: HashMap<String, EtaNodeKind>,
}

#[derive(Debug, Deserialize)]
pub struct EtaBranch {
    pub target: String,
    pub p: f64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EtaNodeKind {
    Split {
        label: Option<String>,
        branches: Vec<EtaBranch>,
    },
    Outcome {
        label: Option<String>,
    },
}

#[derive(Debug, serde::Serialize)]
pub struct EtaPathRow {
    pub node_id: String,
    pub label: String,
    pub path_probability: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct EtaEvalResult {
    pub outcome_paths: Vec<EtaPathRow>,
    pub total_probability: f64,
    pub message: String,
}

fn walk(
    graph: &EtaGraph,
    id: &str,
    path_p: f64,
    visiting: &mut HashSet<String>,
    out: &mut Vec<EtaPathRow>,
) -> Result<(), String> {
    if visiting.contains(id) {
        return Err(format!("Event tree cycle at '{id}'"));
    }
    visiting.insert(id.to_string());
    let node = graph
        .nodes
        .get(id)
        .ok_or_else(|| format!("Event tree missing node '{id}'"))?;
    match node {
        EtaNodeKind::Outcome { label } => {
            out.push(EtaPathRow {
                node_id: id.to_string(),
                label: label.clone().unwrap_or_default(),
                path_probability: path_p.clamp(0.0, 1.0),
            });
        }
        EtaNodeKind::Split { branches, .. } => {
            let mut sum = 0.0f64;
            for b in branches {
                let pp = (b.p.clamp(0.0, 1.0)) * path_p;
                sum += b.p.clamp(0.0, 1.0);
                walk(graph, &b.target, pp, visiting, out)?;
            }
            if sum > 1.0 + 1e-6 {
                return Err(format!(
                    "Event tree branch probabilities sum to {sum} (>1) at '{id}'"
                ));
            }
        }
    }
    visiting.remove(id);
    Ok(())
}

pub fn evaluate_eta(graph: &EtaGraph) -> Result<EtaEvalResult, String> {
    let mut out = Vec::new();
    let mut vis = HashSet::new();
    walk(graph, &graph.root_id, 1.0, &mut vis, &mut out)?;
    let total: f64 = out.iter().map(|r| r.path_probability).sum();
    let msg = if total > 1.0 + 1e-6 {
        format!("total path probability {total} exceeds 1")
    } else {
        String::new()
    };
    Ok(EtaEvalResult {
        outcome_paths: out,
        total_probability: total,
        message: msg,
    })
}
