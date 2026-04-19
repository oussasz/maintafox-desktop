use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct McSpec {
    pub spec_version: u32,
    pub kind: String,
    pub distribution: McDist,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McDist {
    Uniform { low: f64, high: f64 },
    Bernoulli { p: f64 },
}

#[derive(Debug, serde::Serialize)]
pub struct McEvalResult {
    pub sample_mean: f64,
    pub sample_std: f64,
    pub p05: f64,
    pub p95: f64,
    pub trials: i64,
    pub seed_used: u64,
    pub message: String,
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    let idx = ((n.saturating_sub(1)) as f64 * q).round() as usize;
    sorted[idx.min(n - 1)]
}

pub fn evaluate_mc(graph_json: &str, trials: i64, seed: Option<i64>) -> Result<McEvalResult, String> {
    let t = trials.max(1).min(10_000_000);
    let spec: McSpec = serde_json::from_str(graph_json).map_err(|e| format!("MC graph_json: {e}"))?;
    if spec.kind != "mc_sample" {
        return Err(format!("unsupported MC kind '{}'", spec.kind));
    }
    let seed_u = seed.map(|s| s as u64).unwrap_or_else(|| 0xC0FFEE_u64 ^ t as u64);
    let mut rng = StdRng::seed_from_u64(seed_u);
    let mut xs: Vec<f64> = Vec::with_capacity(t as usize);
    match &spec.distribution {
        McDist::Uniform { low, high } => {
            let lo = low.min(*high);
            let hi = high.max(*low);
            for _ in 0..t {
                xs.push(rng.gen_range(lo..=hi));
            }
        }
        McDist::Bernoulli { p } => {
            let pp = p.clamp(0.0, 1.0);
            for _ in 0..t {
                let u: f64 = rng.gen();
                xs.push(if u < pp { 1.0 } else { 0.0 });
            }
        }
    }
    let sum: f64 = xs.iter().sum();
    let mean = sum / (t as f64);
    let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (t as f64).max(1.0);
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(McEvalResult {
        sample_mean: mean,
        sample_std: var.sqrt(),
        p05: percentile(&xs, 0.05),
        p95: percentile(&xs, 0.95),
        trials: t,
        seed_used: seed_u,
        message: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_uniform_reproducible() {
        let g = r#"{"spec_version":1,"kind":"mc_sample","distribution":{"type":"uniform","low":0,"high":1}}"#;
        let a = evaluate_mc(g, 5000, Some(42)).unwrap();
        let b = evaluate_mc(g, 5000, Some(42)).unwrap();
        assert!((a.sample_mean - b.sample_mean).abs() < 1e-12);
    }

    #[test]
    fn mc_bernoulli_zero_trials_one() {
        let g = r#"{"spec_version":1,"kind":"mc_sample","distribution":{"type":"bernoulli","p":0}}"#;
        let r = evaluate_mc(g, 1, Some(99)).unwrap();
        assert_eq!(r.trials, 1);
        assert!(r.sample_mean.abs() < 1e-12);
    }

    #[test]
    fn mc_bernoulli_one_all_ones() {
        let g = r#"{"spec_version":1,"kind":"mc_sample","distribution":{"type":"bernoulli","p":1}}"#;
        let r = evaluate_mc(g, 100, Some(1)).unwrap();
        assert!((r.sample_mean - 1.0).abs() < 1e-12);
    }
}
