use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Deserialize)]
pub struct RbdGraph {
    pub spec_version: u32,
    pub root_id: String,
    pub nodes: HashMap<String, RbdNodeKind>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RbdNodeKind {
    Block { r: f64 },
    Series { children: Vec<String> },
    Parallel { children: Vec<String> },
}

#[derive(Debug, serde::Serialize)]
pub struct RbdEvalResult {
    pub system_reliability: f64,
    pub message: String,
}

fn eval_rbd(
    graph: &RbdGraph,
    id: &str,
    visiting: &mut HashSet<String>,
) -> Result<f64, String> {
    if visiting.contains(id) {
        return Err(format!("RBD cycle at '{id}'"));
    }
    visiting.insert(id.to_string());
    let node = graph
        .nodes
        .get(id)
        .ok_or_else(|| format!("RBD missing node '{id}'"))?;
    let r = match node {
        RbdNodeKind::Block { r } => Ok(r.clamp(0.0, 1.0)),
        RbdNodeKind::Series { children } => {
            if children.is_empty() {
                Ok(1.0)
            } else {
                let mut prod = 1.0f64;
                for c in children {
                    prod *= eval_rbd(graph, c, visiting)?;
                }
                Ok(prod.clamp(0.0, 1.0))
            }
        }
        RbdNodeKind::Parallel { children } => {
            if children.is_empty() {
                Ok(0.0)
            } else {
                let mut prod = 1.0f64;
                for c in children {
                    let rc = eval_rbd(graph, c, visiting)?;
                    prod *= 1.0 - rc;
                }
                Ok((1.0 - prod).clamp(0.0, 1.0))
            }
        }
    };
    visiting.remove(id);
    r
}

pub fn evaluate_rbd(graph: &RbdGraph) -> Result<RbdEvalResult, String> {
    let mut vis = HashSet::new();
    let sys = eval_rbd(graph, &graph.root_id, &mut vis)?;
    Ok(RbdEvalResult {
        system_reliability: sys,
        message: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rbd_series_two() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "root".into(),
            RbdNodeKind::Series {
                children: vec!["a".into(), "b".into()],
            },
        );
        nodes.insert("a".into(), RbdNodeKind::Block { r: 0.9 });
        nodes.insert("b".into(), RbdNodeKind::Block { r: 0.8 });
        let g = RbdGraph {
            spec_version: 1,
            root_id: "root".into(),
            nodes,
        };
        let r = evaluate_rbd(&g).unwrap();
        assert!((r.system_reliability - 0.72).abs() < 1e-9);
    }

    #[test]
    fn rbd_parallel_two() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "root".into(),
            RbdNodeKind::Parallel {
                children: vec!["a".into(), "b".into()],
            },
        );
        nodes.insert("a".into(), RbdNodeKind::Block { r: 0.9 });
        nodes.insert("b".into(), RbdNodeKind::Block { r: 0.8 });
        let g = RbdGraph {
            spec_version: 1,
            root_id: "root".into(),
            nodes,
        };
        let r = evaluate_rbd(&g).unwrap();
        let exp = 1.0 - 0.1 * 0.2;
        assert!((r.system_reliability - exp).abs() < 1e-9);
    }

    #[test]
    fn rbd_empty_series_one() {
        let mut nodes = HashMap::new();
        nodes.insert("root".into(), RbdNodeKind::Series { children: vec![] });
        let g = RbdGraph {
            spec_version: 1,
            root_id: "root".into(),
            nodes,
        };
        let r = evaluate_rbd(&g).unwrap();
        assert!((r.system_reliability - 1.0).abs() < 1e-9);
    }

    #[test]
    fn rbd_empty_parallel_zero() {
        let mut nodes = HashMap::new();
        nodes.insert("root".into(), RbdNodeKind::Parallel { children: vec![] });
        let g = RbdGraph {
            spec_version: 1,
            root_id: "root".into(),
            nodes,
        };
        let r = evaluate_rbd(&g).unwrap();
        assert!(r.system_reliability.abs() < 1e-9);
    }
}
