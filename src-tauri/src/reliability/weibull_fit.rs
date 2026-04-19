//! Two-parameter Weibull MLE on positive complete samples (inter-arrival hours).
//! Confidence intervals: asymptotic normal from inverse observed Hessian of negative log-likelihood.

pub const WEIBULL_MIN_ADEQUATE_N: usize = 5;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WeibullFitResult {
    pub n: usize,
    pub beta: f64,
    pub eta: f64,
    pub beta_ci_low: f64,
    pub beta_ci_high: f64,
    pub eta_ci_low: f64,
    pub eta_ci_high: f64,
    pub adequate_sample: bool,
    pub message: String,
}

fn neg_log_likelihood(times: &[f64], beta: f64, eta: f64) -> f64 {
    let n = times.len() as f64;
    let sum_ln: f64 = times.iter().map(|t| t.ln()).sum();
    let sum_pow: f64 = times.iter().map(|t| (t / eta).powf(beta)).sum();
    -n * beta.ln() + n * beta * eta.ln() - (beta - 1.0) * sum_ln + sum_pow
}

fn scale_from_shape(times: &[f64], beta: f64) -> f64 {
    let n = times.len() as f64;
    let sum_tb: f64 = times.iter().map(|t| t.powf(beta)).sum();
    (sum_tb / n).powf(1.0 / beta)
}

fn g_shape(times: &[f64], beta: f64, sum_log: f64, n: usize) -> f64 {
    let nf = n as f64;
    let sum_tb: f64 = times.iter().map(|t| t.powf(beta)).sum();
    if sum_tb <= 0.0 {
        return f64::NAN;
    }
    let sum_tb_log: f64 = times.iter().map(|t| t.powf(beta) * t.ln()).sum();
    1.0 / beta + sum_log / nf - sum_tb_log / sum_tb
}

/// MLE (beta, eta) for 2-parameter Weibull, complete data.
pub fn fit_weibull_mle(times: &[f64]) -> Option<(f64, f64)> {
    if times.len() < 2 {
        return None;
    }
    if times.iter().any(|t| !t.is_finite() || *t <= 0.0) {
        return None;
    }
    let n = times.len();
    let sum_log: f64 = times.iter().map(|t| t.ln()).sum();
    let mut lo = 0.05_f64;
    let mut hi = 80.0_f64;
    for _ in 0..120 {
        let mid = (lo + hi) / 2.0;
        let g = g_shape(times, mid, sum_log, n);
        if !g.is_finite() {
            break;
        }
        if g > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
        if (hi - lo) < 1e-12 {
            break;
        }
    }
    let beta = ((lo + hi) / 2.0).clamp(0.05, 80.0);
    let eta = scale_from_shape(times, beta);
    if !eta.is_finite() || eta <= 0.0 {
        return None;
    }
    Some((beta, eta))
}

fn hessian_neg_ll(times: &[f64], beta: f64, eta: f64) -> [[f64; 2]; 2] {
    let eps_b = (beta * 1e-4).max(1e-6);
    let eps_e = (eta * 1e-4).max(1e-6);
    let f = |b: f64, e: f64| neg_log_likelihood(times, b, e);
    let f00 = f(beta, eta);
    let fpp = f(beta + eps_b, eta);
    let fmm = f(beta - eps_b, eta);
    let f00e = f(beta, eta + eps_e);
    let f0me = f(beta, eta - eps_e);
    let fpe = f(beta + eps_b, eta + eps_e);
    let fme = f(beta - eps_b, eta + eps_e);
    let d2_db2 = (fpp - 2.0 * f00 + fmm) / (eps_b * eps_b);
    let d2_de2 = (f00e - 2.0 * f00 + f0me) / (eps_e * eps_e);
    let d2_dbde = (fpe - fpp - f00e + f00) / (eps_b * eps_e);
    let d2_dedb = (fpe - fme - f00e + f0me) / (eps_b * eps_e);
    let d2_mixed = 0.5 * (d2_dbde + d2_dedb);
    [[d2_db2, d2_mixed], [d2_mixed, d2_de2]]
}

fn inv2x2(m: [[f64; 2]; 2]) -> Option<[[f64; 2]; 2]> {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if det.abs() < 1e-18 {
        return None;
    }
    let inv = [
        [m[1][1] / det, -m[0][1] / det],
        [-m[1][0] / det, m[0][0] / det],
    ];
    Some(inv)
}

pub fn fit_weibull_with_ci(times: &[f64]) -> WeibullFitResult {
    if times.len() < WEIBULL_MIN_ADEQUATE_N {
        return WeibullFitResult {
            n: times.len(),
            beta: 0.0,
            eta: 0.0,
            beta_ci_low: 0.0,
            beta_ci_high: 0.0,
            eta_ci_low: 0.0,
            eta_ci_high: 0.0,
            adequate_sample: false,
            message: format!(
                "need at least {WEIBULL_MIN_ADEQUATE_N} inter-arrival points for Weibull fit + CI"
            ),
        };
    }
    let Some((beta, eta)) = fit_weibull_mle(times) else {
        return WeibullFitResult {
            n: times.len(),
            beta: 0.0,
            eta: 0.0,
            beta_ci_low: 0.0,
            beta_ci_high: 0.0,
            eta_ci_low: 0.0,
            eta_ci_high: 0.0,
            adequate_sample: false,
            message: "MLE did not converge or invalid data".into(),
        };
    };
    let h = hessian_neg_ll(times, beta, eta);
    let z = 1.96_f64;
    let (beta_lo, beta_hi, eta_lo, eta_hi, msg) = if let Some(inv) = inv2x2(h) {
        let v_b = inv[0][0].max(0.0);
        let v_e = inv[1][1].max(0.0);
        let se_b = v_b.sqrt();
        let se_e = v_e.sqrt();
        (
            (beta - z * se_b).max(0.01),
            beta + z * se_b,
            (eta - z * se_e).max(1e-6),
            eta + z * se_e,
            String::new(),
        )
    } else {
        (
            beta,
            beta,
            eta,
            eta,
            "Hessian singular; returning point estimates only".into(),
        )
    };
    WeibullFitResult {
        n: times.len(),
        beta,
        eta,
        beta_ci_low: beta_lo,
        beta_ci_high: beta_hi,
        eta_ci_low: eta_lo,
        eta_ci_high: eta_hi,
        adequate_sample: true,
        message: msg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weibull_mle_finite_on_sample() {
        let t = vec![80.0, 95.0, 100.0, 105.0, 120.0];
        let (b, e) = fit_weibull_mle(&t).expect("mle");
        assert!(b > 0.0 && e > 0.0 && b.is_finite() && e.is_finite());
    }
}
