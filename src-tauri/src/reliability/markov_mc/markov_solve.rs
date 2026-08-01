use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MarkovSpec {
    pub spec_version: u32,
    pub kind: String,
    pub states: Vec<String>,
    pub matrix: Vec<Vec<f64>>,
}

#[derive(Debug, serde::Serialize)]
pub struct MarkovEvalResult {
    pub steady_state: Vec<f64>,
    pub iterations: usize,
    pub state_labels: Vec<String>,
    pub message: String,
}

pub fn solve_dtmc_steady_state(spec: &MarkovSpec, max_iter: usize, eps: f64) -> Result<MarkovEvalResult, String> {
    if spec.kind != "discrete" {
        return Err(format!("unsupported Markov kind '{}'", spec.kind));
    }
    let n = spec.states.len();
    if n == 0 {
        return Err("empty states".into());
    }
    if spec.matrix.len() != n {
        return Err("matrix rows != states".into());
    }
    for (i, row) in spec.matrix.iter().enumerate() {
        if row.len() != n {
            return Err(format!("matrix row {i} width"));
        }
        let s: f64 = row.iter().sum();
        if (s - 1.0).abs() > 1e-5 {
            return Err(format!("row {i} not stochastic (sum={s})"));
        }
    }
    let mut pi = vec![1.0 / (n as f64); n];
    for it in 0..max_iter {
        let mut next = vec![0.0; n];
        for j in 0..n {
            for i in 0..n {
                next[j] += pi[i] * spec.matrix[i][j];
            }
        }
        let diff: f64 = pi
            .iter()
            .zip(next.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);
        pi = next;
        if diff < eps {
            return Ok(MarkovEvalResult {
                steady_state: pi,
                iterations: it + 1,
                state_labels: spec.states.clone(),
                message: String::new(),
            });
        }
    }
    Ok(MarkovEvalResult {
        steady_state: pi,
        iterations: max_iter,
        state_labels: spec.states.clone(),
        message: "max iterations reached".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_state_ergodic() {
        let spec = MarkovSpec {
            spec_version: 1,
            kind: "discrete".into(),
            states: vec!["0".into(), "1".into()],
            matrix: vec![vec![0.9, 0.1], vec![0.4, 0.6]],
        };
        let r = solve_dtmc_steady_state(&spec, 10_000, 1e-12).unwrap();
        let p0 = r.steady_state[0];
        let p1 = r.steady_state[1];
        assert!((p0 + p1 - 1.0).abs() < 1e-6);
        assert!(p0 > 0.7 && p0 < 0.9);
    }

    #[test]
    fn absorbing_absorbing() {
        let spec = MarkovSpec {
            spec_version: 1,
            kind: "discrete".into(),
            states: vec!["a".into(), "b".into()],
            matrix: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
        };
        let r = solve_dtmc_steady_state(&spec, 100, 1e-9).unwrap();
        assert!((r.steady_state[0] + r.steady_state[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn reject_non_stochastic_row() {
        let spec = MarkovSpec {
            spec_version: 1,
            kind: "discrete".into(),
            states: vec!["a".into(), "b".into()],
            matrix: vec![vec![0.5, 0.5], vec![0.5, 0.3]],
        };
        assert!(solve_dtmc_steady_state(&spec, 10, 1e-9).is_err());
    }
}
