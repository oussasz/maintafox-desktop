//! Two-parameter Weibull MLE on positive exposure-time samples, with right censoring.
//!
//! Log-likelihood for `r` failures `t_i` and right-censored points `c_j` (all `x_k = t_i ∪ c_j`):
//!
//! ```text
//! ℓ(β, η) = Σ_i ln f(t_i) + Σ_j ln R(c_j)
//!         = r ln β − r β ln η + (β − 1) Σ_i ln t_i − Σ_k (x_k / η)^β
//! ```
//!
//! Failures contribute `ln f`, censored points contribute `ln R = −(c/η)^β`.
//! Profiling `η` out gives `η^β = Σ_k x_k^β / r`, and the shape equation
//! `1/β + (1/r) Σ_i ln t_i − Σ_k x_k^β ln x_k / Σ_k x_k^β = 0`, which is strictly decreasing in β
//! and is solved by bisection.
//!
//! Confidence intervals: asymptotic Wald intervals on `ln β` and `ln η` from the inverse of the
//! analytic observed information matrix (delta method), so bounds are always positive.

pub const WEIBULL_MIN_ADEQUATE_N: usize = 5;

/// Persisted on every fit so stored results can be traced to the estimator that produced them.
/// v2: right-censored MLE on exposure hours, analytic observed information, log-scale Wald CIs.
pub const WEIBULL_ALGORITHM_VERSION: &str = "weibull_mle_right_censored_v2";

const Z_95: f64 = 1.959_963_984_540_054;
const BETA_LO: f64 = 0.02;
const BETA_HI: f64 = 100.0;

/// One observation of time-to-failure (or time-on-test when `censored`), in exposure hours.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WeibullObservation {
    pub hours: f64,
    pub censored: bool,
}

impl WeibullObservation {
    #[must_use]
    pub fn failure(hours: f64) -> Self {
        Self { hours, censored: false }
    }

    #[must_use]
    pub fn censored(hours: f64) -> Self {
        Self { hours, censored: true }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WeibullFitResult {
    pub n: usize,
    pub n_failures: usize,
    pub n_censored: usize,
    pub beta: f64,
    pub eta: f64,
    pub beta_ci_low: f64,
    pub beta_ci_high: f64,
    pub eta_ci_low: f64,
    pub eta_ci_high: f64,
    pub adequate_sample: bool,
    pub message: String,
}

fn counts(obs: &[WeibullObservation]) -> (usize, usize) {
    let r = obs.iter().filter(|o| !o.censored).count();
    (r, obs.len() - r)
}

fn valid(obs: &[WeibullObservation]) -> bool {
    obs.iter().all(|o| o.hours.is_finite() && o.hours > 0.0)
}

/// Negative log-likelihood of the right-censored 2-parameter Weibull.
#[must_use]
pub fn neg_log_likelihood(obs: &[WeibullObservation], beta: f64, eta: f64) -> f64 {
    let (r, _) = counts(obs);
    let r = r as f64;
    let sum_ln_fail: f64 = obs.iter().filter(|o| !o.censored).map(|o| o.hours.ln()).sum();
    let sum_pow: f64 = obs.iter().map(|o| (o.hours / eta).powf(beta)).sum();
    -r * beta.ln() + r * beta * eta.ln() - (beta - 1.0) * sum_ln_fail + sum_pow
}

/// Shape score `g(β)` evaluated on values pre-scaled by `x_max` (so `u ≤ 1`, no overflow).
/// `ln x` terms are shift-invariant in `g`, so scaling does not change the root.
fn g_shape(scaled: &[WeibullObservation], beta: f64, mean_ln_fail: f64) -> f64 {
    let mut s0 = 0.0_f64;
    let mut s1 = 0.0_f64;
    for o in scaled {
        let ub = o.hours.powf(beta);
        s0 += ub;
        s1 += ub * o.hours.ln();
    }
    if s0 <= 0.0 {
        return f64::NAN;
    }
    1.0 / beta + mean_ln_fail - s1 / s0
}

/// MLE `(β, η)` for the 2-parameter Weibull with right censoring.
/// Returns `None` when fewer than 2 failures, invalid times, or no finite maximum exists
/// (e.g. every failure time equals the largest observation).
#[must_use]
pub fn fit_weibull_mle_censored(obs: &[WeibullObservation]) -> Option<(f64, f64)> {
    let (r, _) = counts(obs);
    if r < 2 || !valid(obs) {
        return None;
    }
    let x_max = obs.iter().map(|o| o.hours).fold(0.0_f64, f64::max);
    let scaled: Vec<WeibullObservation> = obs
        .iter()
        .map(|o| WeibullObservation {
            hours: o.hours / x_max,
            censored: o.censored,
        })
        .collect();
    let mean_ln_fail = scaled.iter().filter(|o| !o.censored).map(|o| o.hours.ln()).sum::<f64>() / r as f64;

    let mut lo = BETA_LO;
    let mut hi = BETA_HI;
    let g_lo = g_shape(&scaled, lo, mean_ln_fail);
    let g_hi = g_shape(&scaled, hi, mean_ln_fail);
    if !(g_lo.is_finite() && g_hi.is_finite()) || g_lo <= 0.0 || g_hi >= 0.0 {
        return None;
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        let g = g_shape(&scaled, mid, mean_ln_fail);
        if !g.is_finite() {
            return None;
        }
        if g > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-13 * mid.max(1.0) {
            break;
        }
    }
    let beta = 0.5 * (lo + hi);
    let s0: f64 = scaled.iter().map(|o| o.hours.powf(beta)).sum();
    let eta = x_max * (s0 / r as f64).powf(1.0 / beta);
    if !eta.is_finite() || eta <= 0.0 {
        return None;
    }
    Some((beta, eta))
}

/// MLE `(β, η)` for complete (uncensored) data.
#[must_use]
pub fn fit_weibull_mle(times: &[f64]) -> Option<(f64, f64)> {
    let obs: Vec<WeibullObservation> = times.iter().copied().map(WeibullObservation::failure).collect();
    fit_weibull_mle_censored(&obs)
}

/// Analytic observed information (Hessian of the negative log-likelihood) in `(β, η)`.
fn observed_information(obs: &[WeibullObservation], beta: f64, eta: f64) -> [[f64; 2]; 2] {
    let (r, _) = counts(obs);
    let r = r as f64;
    let mut s0 = 0.0_f64; // Σ u^β
    let mut s1 = 0.0_f64; // Σ u^β ln u
    let mut s2 = 0.0_f64; // Σ u^β (ln u)²
    for o in obs {
        let u = o.hours / eta;
        let lu = u.ln();
        let ub = u.powf(beta);
        s0 += ub;
        s1 += ub * lu;
        s2 += ub * lu * lu;
    }
    let d2_bb = r / (beta * beta) + s2;
    let d2_ee = (-r * beta + beta * (beta + 1.0) * s0) / (eta * eta);
    let d2_be = (r - s0 - beta * s1) / eta;
    [[d2_bb, d2_be], [d2_be, d2_ee]]
}

fn inv2x2(m: [[f64; 2]; 2]) -> Option<[[f64; 2]; 2]> {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if !det.is_finite() || det <= 0.0 || m[0][0] <= 0.0 {
        // Not positive definite: not a proper maximum.
        return None;
    }
    Some([[m[1][1] / det, -m[0][1] / det], [-m[1][0] / det, m[0][0] / det]])
}

/// Standard errors `(se_β, se_η)` from the inverse observed information.
#[must_use]
pub fn standard_errors(obs: &[WeibullObservation], beta: f64, eta: f64) -> Option<(f64, f64)> {
    let inv = inv2x2(observed_information(obs, beta, eta))?;
    let (vb, ve) = (inv[0][0], inv[1][1]);
    if vb.is_finite() && ve.is_finite() && vb >= 0.0 && ve >= 0.0 {
        Some((vb.sqrt(), ve.sqrt()))
    } else {
        None
    }
}

fn inadequate(obs: &[WeibullObservation], message: String) -> WeibullFitResult {
    let (r, c) = counts(obs);
    WeibullFitResult {
        n: obs.len(),
        n_failures: r,
        n_censored: c,
        beta: 0.0,
        eta: 0.0,
        beta_ci_low: 0.0,
        beta_ci_high: 0.0,
        eta_ci_low: 0.0,
        eta_ci_high: 0.0,
        adequate_sample: false,
        message,
    }
}

/// Fit with 95% CIs. Adequacy requires at least [`WEIBULL_MIN_ADEQUATE_N`] *failures*;
/// censored observations add information but never count toward the minimum.
#[must_use]
pub fn fit_weibull_censored_with_ci(obs: &[WeibullObservation]) -> WeibullFitResult {
    let (r, c) = counts(obs);
    if r < WEIBULL_MIN_ADEQUATE_N {
        return inadequate(
            obs,
            format!("need at least {WEIBULL_MIN_ADEQUATE_N} failures for Weibull fit + CI (have {r}, censored {c})"),
        );
    }
    let Some((beta, eta)) = fit_weibull_mle_censored(obs) else {
        return inadequate(
            obs,
            "MLE has no finite maximum for this dataset or data are invalid".into(),
        );
    };
    let (beta_lo, beta_hi, eta_lo, eta_hi, message) = match standard_errors(obs, beta, eta) {
        Some((se_b, se_e)) => {
            let kb = (Z_95 * se_b / beta).exp();
            let ke = (Z_95 * se_e / eta).exp();
            (beta / kb, beta * kb, eta / ke, eta * ke, String::new())
        }
        None => (
            beta,
            beta,
            eta,
            eta,
            "observed information not positive definite; returning point estimates only".into(),
        ),
    };
    WeibullFitResult {
        n: obs.len(),
        n_failures: r,
        n_censored: c,
        beta,
        eta,
        beta_ci_low: beta_lo,
        beta_ci_high: beta_hi,
        eta_ci_low: eta_lo,
        eta_ci_high: eta_hi,
        adequate_sample: true,
        message,
    }
}

/// Complete-data convenience wrapper.
#[must_use]
pub fn fit_weibull_with_ci(times: &[f64]) -> WeibullFitResult {
    let obs: Vec<WeibullObservation> = times.iter().copied().map(WeibullObservation::failure).collect();
    fit_weibull_censored_with_ci(&obs)
}

/// Fixed $n=10$ inter-arrivées (h) from $\mathrm{Weibull}(\beta=2,\eta=500)$ via
/// $t_i = \eta(-\ln(1-u_i))^{1/\beta}$ with $u_i = i/(n+1)$, $i=1,\ldots,n$.
pub const VALIDATION_WEIBULL_SAMPLE: [f64; 10] = [
    154.361_734_089_382_48,
    223.981_414_107_371_42,
    282.158_524_201_615_53,
    336.149_194_459_490_45,
    389.273_619_569_293_3,
    443.975_607_540_625_5,
    502.891_865_036_232_16,
    569.930_474_735_791_9,
    652.830_010_844_788_3,
    774.256_945_851_693_7,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn rel_close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs())
    }

    fn obs(fail: &[f64], cens: &[f64]) -> Vec<WeibullObservation> {
        fail.iter()
            .copied()
            .map(WeibullObservation::failure)
            .chain(cens.iter().copied().map(WeibullObservation::censored))
            .collect()
    }

    // Reference values below were produced independently with SciPy 1.17.1:
    //   scipy.stats.weibull_min.fit(CensoredData(uncensored=F, right=C), floc=0),
    //   refined by Nelder–Mead on −(Σ logpdf(F) + Σ logsf(C)) (xatol 1e-12), and standard errors
    //   from the inverse of a central-difference Hessian of that NLL (step 1e-5 relative).

    /// 6 failures + 4 units suspended at 150 h (type-I censored life test).
    #[test]
    fn censored_mle_matches_scipy_reference() {
        let o = obs(&[16.0, 34.0, 53.0, 75.0, 93.0, 120.0], &[150.0, 150.0, 150.0, 150.0]);
        let (b, e) = fit_weibull_mle_censored(&o).expect("mle");
        assert!(rel_close(b, 1.243_842_656_1, 1e-6), "beta {b}");
        assert!(rel_close(e, 154.515_228_451_4, 1e-6), "eta {e}");
        let nll = neg_log_likelihood(&o, b, e);
        assert!((nll - 36.477_250_470_4).abs() < 1e-8, "nll {nll}");
        let (se_b, se_e) = standard_errors(&o, b, e).expect("se");
        assert!(rel_close(se_b, 0.453_850_53, 1e-4), "se_b {se_b}");
        assert!(rel_close(se_e, 52.664_223_09, 1e-4), "se_e {se_e}");

        let fit = fit_weibull_censored_with_ci(&o);
        assert!(fit.adequate_sample);
        assert_eq!((fit.n, fit.n_failures, fit.n_censored), (10, 6, 4));
        // Log-scale Wald 95%: β·exp(±1.96·se/β)
        let kb = (Z_95 * se_b / b).exp();
        assert!(rel_close(fit.beta_ci_low, b / kb, 1e-12) && rel_close(fit.beta_ci_high, b * kb, 1e-12));
        assert!(fit.beta_ci_low > 0.0 && fit.eta_ci_low > 0.0);
    }

    /// Exposure-time inter-arrivals with a single censored tail (time since last failure).
    #[test]
    fn single_censored_tail_matches_scipy_reference() {
        let o = obs(&[120.0, 340.0, 560.0, 800.0, 1020.0, 1500.0], &[400.0]);
        let (b, e) = fit_weibull_mle_censored(&o).expect("mle");
        assert!(rel_close(b, 1.672_021_589_7, 1e-6), "beta {b}");
        assert!(rel_close(e, 839.803_993_616_9, 1e-6), "eta {e}");
        let (se_b, se_e) = standard_errors(&o, b, e).expect("se");
        assert!(rel_close(se_b, 0.536_801_72, 1e-4), "se_b {se_b}");
        assert!(rel_close(se_e, 210.501_737_11, 1e-4), "se_e {se_e}");
    }

    /// Complete data must reduce to the classic MLE.
    #[test]
    fn complete_data_matches_scipy_reference() {
        let t = [80.0, 95.0, 100.0, 105.0, 120.0];
        let (b, e) = fit_weibull_mle(&t).expect("mle");
        assert!(rel_close(b, 8.543_735_721_0, 1e-6), "beta {b}");
        assert!(rel_close(e, 105.745_988_300_4, 1e-6), "eta {e}");
        let (se_b, se_e) = standard_errors(&obs(&t, &[]), b, e).expect("se");
        assert!(rel_close(se_b, 2.941_526_97, 1e-4), "se_b {se_b}");
        assert!(rel_close(se_e, 5.856_016_30, 1e-4), "se_e {se_e}");
    }

    /// Regression for P1-1: treating the censored tail as a failure biases η downward.
    #[test]
    fn censored_tail_is_not_counted_as_failure() {
        let fail = [120.0, 340.0, 560.0, 800.0, 1020.0, 1500.0];
        let as_failure = fit_weibull_mle(&[&fail[..], &[400.0]].concat()).expect("mle");
        let as_censored = fit_weibull_mle_censored(&obs(&fail, &[400.0])).expect("mle");
        assert!(as_censored.1 > as_failure.1, "censoring must not shorten η");
        let fit = fit_weibull_censored_with_ci(&obs(&fail, &[400.0]));
        assert_eq!((fit.n_failures, fit.n_censored), (6, 1));
    }

    #[test]
    fn censored_points_do_not_count_toward_minimum() {
        let fit = fit_weibull_censored_with_ci(&obs(&[100.0, 200.0, 300.0, 400.0], &[50.0, 60.0, 70.0]));
        assert!(!fit.adequate_sample);
        assert_eq!(fit.n_failures, 4);
    }

    #[test]
    fn no_finite_mle_when_all_failures_tie_at_max() {
        assert!(fit_weibull_mle(&[100.0, 100.0, 100.0]).is_none());
        assert!(fit_weibull_mle_censored(&obs(&[10.0], &[20.0, 30.0])).is_none());
    }

    #[test]
    fn analytic_information_matches_finite_difference() {
        let o = obs(&[16.0, 34.0, 53.0, 75.0, 93.0, 120.0], &[150.0, 150.0, 150.0, 150.0]);
        let (b, e) = (1.3, 140.0);
        let h = observed_information(&o, b, e);
        let (db, de) = (1e-4, 1e-2);
        let f = |bb: f64, ee: f64| neg_log_likelihood(&o, bb, ee);
        let fbb = (f(b + db, e) - 2.0 * f(b, e) + f(b - db, e)) / (db * db);
        let fee = (f(b, e + de) - 2.0 * f(b, e) + f(b, e - de)) / (de * de);
        let fbe = (f(b + db, e + de) - f(b + db, e - de) - f(b - db, e + de) + f(b - db, e - de)) / (4.0 * db * de);
        assert!(rel_close(h[0][0], fbb, 1e-5), "bb {} vs {fbb}", h[0][0]);
        assert!(rel_close(h[1][1], fee, 1e-4), "ee {} vs {fee}", h[1][1]);
        assert!(rel_close(h[0][1], fbe, 1e-4), "be {} vs {fbe}", h[0][1]);
    }

    #[test]
    fn weibull_mle_validation_sample_matches_thesis() {
        let times: Vec<f64> = VALIDATION_WEIBULL_SAMPLE.to_vec();
        let (beta, eta) = fit_weibull_mle(&times).expect("mle");
        let fit = fit_weibull_with_ci(&times);
        let nll = neg_log_likelihood(&obs(&times, &[]), beta, eta);
        assert!((beta - 2.5417).abs() < 0.02, "beta hat {beta}");
        assert!((eta - 489.30).abs() < 2.0, "eta hat {eta}");
        assert!(fit.adequate_sample);
        assert!(nll.is_finite() && nll > 0.0);
        assert!(fit.beta_ci_low < 2.0 && fit.beta_ci_high > 2.0);
        assert!(fit.eta_ci_low < 500.0 && fit.eta_ci_high > 500.0);
    }
}
