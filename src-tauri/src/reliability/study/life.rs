//! Life analysis of a study dataset (pure): right-censored Weibull MLE (`weibull_fit`), Fisher
//! matrix bounds at 90 %, life metrics and the age-replacement cost model (`life_metrics`).
//!
//! Bounds follow the usual Fisher-matrix construction of reliability software:
//! - β, η: log-normal bounds `β·exp(±z·se_β/β)`, `η·exp(±z·se_η/η)`;
//! - unreliability: bounds on `u = β(ln t − ln η)` with
//!   `Var(u) = (u/β)²·Var β + (β/η)²·Var η − 2·(u/η)·Cov(β, η)`, then `F = 1 − exp(−eᵘ)`;
//! - B-life: bounds on `ln t_p = ln η + y/β` (`y = ln(−ln(1 − p))`) with
//!   `Var = Var η/η² + y²·Var β/β⁴ − 2·y·Cov(β, η)/(η·β²)`.

use crate::reliability::life_metrics::{
    age_replacement_cost_rate, b_life, hazard, mean_life, optimal_age_replacement, probability_plot_correlation,
    probability_plot_points, reliability,
};
use crate::reliability::study::domain::{
    BandPoint, CostAnalysis, CostPoint, CurvePoint, LifeFit, PlotPoint, StrategyCost,
};
use crate::reliability::weibull_fit::{
    covariance, fit_weibull_censored_with_ci, WeibullObservation, WEIBULL_MIN_ADEQUATE_N,
};

/// Two-sided confidence level of every bound shown in a study.
pub const LIFE_CONFIDENCE: f64 = 0.90;
/// Standard normal quantile for a two-sided 90 % interval.
const Z_90: f64 = 1.644_853_626_951_472_2;

pub const ISSUE_NOT_ENOUGH_FAILURES: &str = "not_enough_failures";
pub const ISSUE_NO_FINITE_MLE: &str = "no_finite_mle";
pub const ISSUE_NO_COVARIANCE: &str = "no_covariance";

pub const PATTERN_EARLY_LIFE: &str = "early_life";
pub const PATTERN_RANDOM: &str = "random";
pub const PATTERN_WEAR_OUT: &str = "wear_out";

#[derive(Debug, Clone, Default)]
pub struct LifeInputs {
    pub observations: Vec<WeibullObservation>,
    pub planned_cost: Option<f64>,
    pub failure_cost: Option<f64>,
    /// Current replacement interval in the same hours as the observations.
    pub current_interval: Option<f64>,
    /// Hours per year in the same time base, summed over the study equipment.
    pub hours_per_year: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct LifeResult {
    pub n_failures: i64,
    pub n_suspensions: i64,
    pub fit: Option<LifeFit>,
    pub fit_issue: Option<&'static str>,
    pub plot_points: Vec<PlotPoint>,
    pub band: Vec<BandPoint>,
    pub curve: Vec<CurvePoint>,
    pub cost: Option<CostAnalysis>,
}

struct Fitted {
    fit: LifeFit,
    cov: [[f64; 2]; 2],
}

fn positive(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

fn log_bounds(value: f64, var: f64) -> (f64, f64) {
    let k = (Z_90 * var.max(0.0).sqrt() / value).exp();
    (value / k, value * k)
}

/// Fisher bounds on the B-life `p` (fraction failed).
fn b_life_bounds(p: f64, beta: f64, eta: f64, cov: &[[f64; 2]; 2]) -> Option<(f64, f64)> {
    let tp = b_life(p, beta, eta)?;
    let y = (-(1.0 - p).ln()).ln();
    let var = cov[1][1] / (eta * eta) + y * y * cov[0][0] / beta.powi(4) - 2.0 * y * cov[0][1] / (eta * beta * beta);
    if !var.is_finite() || var < 0.0 {
        return None;
    }
    let s = Z_90 * var.sqrt();
    Some((tp * (-s).exp(), tp * s.exp()))
}

/// Unreliability F(t) with its Fisher bounds.
#[must_use]
pub fn unreliability_bounds(t: f64, beta: f64, eta: f64, cov: &[[f64; 2]; 2]) -> BandPoint {
    let u = beta * (t.ln() - eta.ln());
    let du_db = u / beta;
    let du_de = -beta / eta;
    let var = du_db * du_db * cov[0][0] + du_de * du_de * cov[1][1] + 2.0 * du_db * du_de * cov[0][1];
    let s = Z_90 * var.max(0.0).sqrt();
    let f = |x: f64| 1.0 - (-(x.exp())).exp();
    BandPoint {
        time: t,
        unreliability: f(u),
        lower: f(u - s),
        upper: f(u + s),
    }
}

fn fit(observations: &[WeibullObservation]) -> Result<Fitted, &'static str> {
    let failures = observations.iter().filter(|o| !o.censored).count();
    if failures < WEIBULL_MIN_ADEQUATE_N {
        return Err(ISSUE_NOT_ENOUGH_FAILURES);
    }
    let base = fit_weibull_censored_with_ci(observations);
    if !base.adequate_sample || !(base.beta > 0.0 && base.eta > 0.0) {
        return Err(ISSUE_NO_FINITE_MLE);
    }
    let (beta, eta) = (base.beta, base.eta);
    let cov = covariance(observations, beta, eta).ok_or(ISSUE_NO_COVARIANCE)?;
    if !(cov[0][0] >= 0.0 && cov[1][1] >= 0.0) {
        return Err(ISSUE_NO_COVARIANCE);
    }
    let (beta_lower, beta_upper) = log_bounds(beta, cov[0][0]);
    let (eta_lower, eta_upper) = log_bounds(eta, cov[1][1]);
    let b10 = b_life(0.10, beta, eta).ok_or(ISSUE_NO_FINITE_MLE)?;
    let b10_bounds = b_life_bounds(0.10, beta, eta, &cov);
    let pairs: Vec<(f64, bool)> = observations.iter().map(|o| (o.hours, o.censored)).collect();
    let rho = probability_plot_correlation(&probability_plot_points(&pairs));
    let pattern = if beta_upper < 1.0 {
        PATTERN_EARLY_LIFE
    } else if beta_lower > 1.0 {
        PATTERN_WEAR_OUT
    } else {
        PATTERN_RANDOM
    };
    Ok(Fitted {
        fit: LifeFit {
            beta,
            eta,
            confidence: LIFE_CONFIDENCE,
            beta_lower,
            beta_upper,
            eta_lower,
            eta_upper,
            b10,
            b10_lower: b10_bounds.map(|b| b.0),
            b10_upper: b10_bounds.map(|b| b.1),
            mean_life: mean_life(beta, eta).ok_or(ISSUE_NO_FINITE_MLE)?,
            median_life: b_life(0.5, beta, eta).ok_or(ISSUE_NO_FINITE_MLE)?,
            rho,
            pattern: pattern.to_string(),
        },
        cov,
    })
}

fn log_grid(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if !(lo > 0.0 && hi > lo) || n < 2 {
        return Vec::new();
    }
    let (a, b) = (lo.ln(), hi.ln());
    (0..n)
        .map(|i| (a + (b - a) * i as f64 / (n - 1) as f64).exp())
        .collect()
}

fn cost_analysis(inputs: &LifeInputs, beta: f64, eta: f64, mttf: f64) -> Option<CostAnalysis> {
    let planned = positive(inputs.planned_cost)?;
    let failure = positive(inputs.failure_cost)?;
    let hpy = positive(inputs.hours_per_year);
    let annual = |rate: f64| hpy.map(|h| rate * h);
    let rtf_rate = failure / mttf;
    let run_to_failure = StrategyCost {
        interval: None,
        cost_rate: rtf_rate,
        annual_cost: annual(rtf_rate),
    };
    let current = positive(inputs.current_interval).and_then(|t| {
        age_replacement_cost_rate(t, planned, failure, beta, eta).map(|rate| StrategyCost {
            interval: Some(t),
            cost_rate: rate,
            annual_cost: annual(rate),
        })
    });
    let (proposed, rejection, saving) = match optimal_age_replacement(planned, failure, beta, eta) {
        Ok(opt) => (
            Some(StrategyCost {
                interval: Some(opt.interval),
                cost_rate: opt.cost_rate,
                annual_cost: annual(opt.cost_rate),
            }),
            None,
            Some(opt.saving_vs_run_to_failure),
        ),
        Err(rej) => (None, Some(rej), None),
    };
    let annual_saving_vs_current = match (&current, &proposed) {
        (Some(c), Some(p)) => hpy.map(|h| (c.cost_rate - p.cost_rate) * h),
        _ => None,
    };
    let span_hi = (eta * 2.5).max(inputs.current_interval.unwrap_or(0.0) * 1.3);
    let span_lo = eta * 0.05;
    let curve = (0..=60)
        .filter_map(|i| {
            let t = span_lo + (span_hi - span_lo) * f64::from(i) / 60.0;
            age_replacement_cost_rate(t, planned, failure, beta, eta).map(|c| CostPoint {
                interval: t,
                cost_rate: c,
            })
        })
        .collect();
    Some(CostAnalysis {
        planned_cost: planned,
        failure_cost: failure,
        run_to_failure,
        current,
        proposed,
        rejection,
        saving_vs_run_to_failure: saving,
        annual_saving_vs_current,
        curve,
    })
}

/// Analyse the observations (ages of failures and suspensions in one time base).
#[must_use]
pub fn analyse_life(inputs: &LifeInputs) -> LifeResult {
    let obs: Vec<WeibullObservation> = inputs
        .observations
        .iter()
        .copied()
        .filter(|o| o.hours.is_finite() && o.hours > 0.0)
        .collect();
    let n_failures = obs.iter().filter(|o| !o.censored).count() as i64;
    let n_suspensions = obs.len() as i64 - n_failures;
    let pairs: Vec<(f64, bool)> = obs.iter().map(|o| (o.hours, o.censored)).collect();
    let plot_points: Vec<PlotPoint> = probability_plot_points(&pairs)
        .into_iter()
        .map(|p| PlotPoint {
            time: p.time,
            median_rank: p.median_rank,
        })
        .collect();

    let fitted = match fit(&obs) {
        Ok(f) => f,
        Err(issue) => {
            return LifeResult {
                n_failures,
                n_suspensions,
                fit: None,
                fit_issue: Some(issue),
                plot_points,
                band: Vec::new(),
                curve: Vec::new(),
                cost: None,
            }
        }
    };
    let (beta, eta) = (fitted.fit.beta, fitted.fit.eta);
    let t_min = obs.iter().map(|o| o.hours).fold(f64::INFINITY, f64::min);
    let t_max = obs.iter().map(|o| o.hours).fold(0.0_f64, f64::max);
    let band = log_grid((t_min * 0.5).min(fitted.fit.b10 * 0.5), t_max.max(eta) * 2.5, 61)
        .into_iter()
        .map(|t| unreliability_bounds(t, beta, eta, &fitted.cov))
        .collect();
    let horizon = t_max.max(eta * 2.0).max(inputs.current_interval.unwrap_or(0.0) * 1.2);
    let curve = (1..=80)
        .map(|i| {
            let t = horizon * f64::from(i) / 80.0;
            CurvePoint {
                time: t,
                reliability: reliability(t, beta, eta),
                hazard: hazard(t, beta, eta),
            }
        })
        .collect();
    let cost = cost_analysis(inputs, beta, eta, fitted.fit.mean_life);
    LifeResult {
        n_failures,
        n_suspensions,
        fit: Some(fitted.fit),
        fit_issue: None,
        plot_points,
        band,
        curve,
        cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reliability::life_metrics::AgeReplacementRejection;

    fn close(actual: f64, expected: f64, rel: f64) {
        let err = ((actual - expected) / expected).abs();
        assert!(err < rel, "expected {expected}, got {actual} (relative error {err:e})");
    }

    fn obs(fail: &[f64], cens: &[f64]) -> Vec<WeibullObservation> {
        fail.iter()
            .copied()
            .map(WeibullObservation::failure)
            .chain(cens.iter().copied().map(WeibullObservation::censored))
            .collect()
    }

    /// 6 failures + 4 suspensions at 150 h. References computed independently with SciPy 1.17.1:
    /// Nelder–Mead MLE of the censored log-likelihood (`weibull_min.logpdf` / `logsf`), covariance
    /// = inverse of a central-difference Hessian (step 1e-4 relative), z = `norm.ppf(0.95)`.
    #[test]
    fn censored_fit_bounds_match_scipy_reference() {
        let data = obs(&[16.0, 34.0, 53.0, 75.0, 93.0, 120.0], &[150.0, 150.0, 150.0, 150.0]);
        let r = analyse_life(&LifeInputs {
            observations: data.clone(),
            ..LifeInputs::default()
        });
        let f = r.fit.as_ref().expect("fit");
        assert_eq!((r.n_failures, r.n_suspensions), (6, 4));
        close(f.beta, 1.243_842_656_1, 1e-6);
        close(f.eta, 154.515_228_451_4, 1e-6);
        close(f.beta_lower, 0.682_515_124_9, 1e-4);
        close(f.beta_upper, 2.266_828_489_9, 1e-4);
        close(f.eta_lower, 88.204_959_204_7, 1e-4);
        close(f.eta_upper, 270.675_886_676_5, 1e-4);
        close(f.b10, 25.307_150_174_6, 1e-6);
        close(f.b10_lower.expect("b10 lower"), 8.599_438_825_7, 1e-4);
        close(f.b10_upper.expect("b10 upper"), 74.476_005_113_7, 1e-4);
        let cov = covariance(&data, f.beta, f.eta).expect("cov");
        for (t, mid, lo, hi) in [
            (30.0, 0.122_069_519_9, 0.037_182_281_0, 0.360_650_641_0),
            (100.0, 0.441_240_122_2, 0.254_208_694_4, 0.684_934_765_3),
            (300.0, 0.897_972_501_9, 0.583_413_072_8, 0.997_393_229_1),
        ] {
            let p = unreliability_bounds(t, f.beta, f.eta, &cov);
            close(p.unreliability, mid, 1e-6);
            close(p.lower, lo, 1e-4);
            close(p.upper, hi, 1e-4);
        }
        assert_eq!(f.pattern, PATTERN_RANDOM, "β CI straddles 1");
        assert!(r.band.windows(2).all(|w| w[0].time < w[1].time));
        assert!(r
            .band
            .iter()
            .all(|b| b.lower <= b.unreliability && b.unreliability <= b.upper));
    }

    /// The pump bearing example of the approved mockup: 8 failures, 2 preventive removals.
    /// ρ is the SciPy reference of `life_metrics`; costs follow the age-replacement model.
    #[test]
    fn pump_example_reports_wear_out_rho_and_costs() {
        let r = analyse_life(&LifeInputs {
            observations: obs(
                &[1850.0, 2610.0, 3100.0, 3620.0, 4080.0, 4510.0, 5180.0, 5920.0],
                &[2200.0, 2900.0],
            ),
            planned_cost: Some(180_000.0),
            failure_cost: Some(1_450_000.0),
            current_interval: Some(6000.0),
            hours_per_year: Some(16_000.0),
        });
        let f = r.fit.as_ref().expect("fit");
        close(f.rho.expect("rho"), 0.999_645_685_6, 1e-9);
        assert_eq!(f.pattern, PATTERN_WEAR_OUT);
        close(f.mean_life, mean_life(f.beta, f.eta).unwrap(), 1e-12);
        let c = r.cost.as_ref().expect("cost");
        let opt = optimal_age_replacement(180_000.0, 1_450_000.0, f.beta, f.eta).expect("optimum");
        let p = c.proposed.expect("proposed");
        close(p.interval.unwrap(), opt.interval, 1e-12);
        close(p.annual_cost.unwrap(), opt.cost_rate * 16_000.0, 1e-12);
        let cur = c.current.expect("current");
        close(
            cur.cost_rate,
            age_replacement_cost_rate(6000.0, 180_000.0, 1_450_000.0, f.beta, f.eta).unwrap(),
            1e-12,
        );
        close(
            c.annual_saving_vs_current.unwrap(),
            (cur.cost_rate - p.cost_rate) * 16_000.0,
            1e-9,
        );
        assert!(c.annual_saving_vs_current.unwrap() > 0.0);
        close(c.run_to_failure.cost_rate, 1_450_000.0 / f.mean_life, 1e-12);
        assert_eq!(r.plot_points.len(), 8);
        assert_eq!(r.curve.len(), 80);
    }

    #[test]
    fn too_few_failures_and_no_wear_out_are_reported() {
        let r = analyse_life(&LifeInputs {
            observations: obs(&[100.0, 200.0, 300.0, 400.0], &[500.0]),
            ..LifeInputs::default()
        });
        assert!(r.fit.is_none());
        assert_eq!(r.fit_issue, Some(ISSUE_NOT_ENOUGH_FAILURES));
        assert_eq!(r.plot_points.len(), 4);

        // Exponential-looking data: no age-replacement interval is recommended.
        let r = analyse_life(&LifeInputs {
            observations: obs(&[50.0, 400.0, 90.0, 1200.0, 700.0, 30.0, 2600.0, 260.0], &[]),
            planned_cost: Some(100.0),
            failure_cost: Some(1000.0),
            ..LifeInputs::default()
        });
        let f = r.fit.expect("fit");
        assert!(f.beta < 1.0);
        let c = r.cost.expect("cost");
        assert!(c.proposed.is_none());
        assert_eq!(c.rejection, Some(AgeReplacementRejection::NoWearOut));
        assert!(c.annual_saving_vs_current.is_none());
    }

    #[test]
    fn costs_are_only_used_when_both_are_positive() {
        let base = obs(
            &[1850.0, 2610.0, 3100.0, 3620.0, 4080.0, 4510.0, 5180.0, 5920.0],
            &[2200.0, 2900.0],
        );
        let r = analyse_life(&LifeInputs {
            observations: base,
            planned_cost: Some(180_000.0),
            failure_cost: None,
            ..LifeInputs::default()
        });
        assert!(r.fit.is_some() && r.cost.is_none());
    }
}
