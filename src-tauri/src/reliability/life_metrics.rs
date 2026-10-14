//! Life metrics and age-replacement optimisation for a fitted 2-parameter Weibull model.
//!
//! Pure functions (no database), used by the reliability study screens:
//! - reliability R(t), hazard h(t), B-life, mean life (MTTF = η·Γ(1+1/β));
//! - probability-plot points with suspensions (Johnson adjusted ranks, Benard median ranks) and
//!   their correlation coefficient ρ, the goodness-of-fit figure reliability tools show next to
//!   a Weibull plot;
//! - the age-replacement policy (Barlow & Proschan): replacing a part preventively at age T costs
//!   `C(T) = [Cp·R(T) + Cf·(1 − R(T))] / ∫₀ᵀ R(t) dt` per operating hour. The optimum T* minimises
//!   C(T); it exists only when the failure rate increases with age (β > 1) and a breakdown costs
//!   more than a planned replacement. Run-to-failure costs `Cf / MTTF` per hour.
//!
//! Reference values in the tests were computed independently with SciPy (`integrate.quad`,
//! `optimize.minimize_scalar`, `math.gamma`).

use serde::Serialize;

/// R(t) = exp(−(t/η)^β). Returns 1 for t ≤ 0.
pub fn reliability(t: f64, beta: f64, eta: f64) -> f64 {
    if t <= 0.0 {
        return 1.0;
    }
    (-(t / eta).powf(beta)).exp()
}

/// h(t) = (β/η)·(t/η)^(β−1), the instantaneous failure rate at age t.
pub fn hazard(t: f64, beta: f64, eta: f64) -> f64 {
    if t <= 0.0 {
        return if beta < 1.0 {
            f64::INFINITY
        } else if beta == 1.0 {
            1.0 / eta
        } else {
            0.0
        };
    }
    beta / eta * (t / eta).powf(beta - 1.0)
}

/// Age by which `fraction_failed` of the population has failed (B10 → 0.10).
pub fn b_life(fraction_failed: f64, beta: f64, eta: f64) -> Option<f64> {
    if !(fraction_failed > 0.0 && fraction_failed < 1.0) || !valid_params(beta, eta) {
        return None;
    }
    Some(eta * (-(1.0 - fraction_failed).ln()).powf(1.0 / beta))
}

/// Mean life MTTF = η·Γ(1 + 1/β).
pub fn mean_life(beta: f64, eta: f64) -> Option<f64> {
    if !valid_params(beta, eta) {
        return None;
    }
    Some(eta * ln_gamma(1.0 + 1.0 / beta).exp())
}

/// ∫₀ᵀ R(t) dt: expected operating time per cycle when replacing at age T (composite 10-point
/// Gauss–Legendre; the integrand is smooth and bounded by 1).
pub fn integrated_reliability(interval: f64, beta: f64, eta: f64) -> f64 {
    if interval <= 0.0 {
        return 0.0;
    }
    const PANELS: usize = 200;
    let h = interval / PANELS as f64;
    let half = h / 2.0;
    let mut sum = 0.0;
    for p in 0..PANELS {
        let mid = p as f64 * h + half;
        for (x, w) in GL10.iter() {
            sum += w * reliability(mid + half * x, beta, eta);
            sum += w * reliability(mid - half * x, beta, eta);
        }
    }
    sum * half
}

/// Expected cost per operating hour when replacing preventively at age `interval`.
pub fn age_replacement_cost_rate(
    interval: f64,
    planned_cost: f64,
    failure_cost: f64,
    beta: f64,
    eta: f64,
) -> Option<f64> {
    if interval <= 0.0 || !valid_params(beta, eta) || planned_cost <= 0.0 || failure_cost <= 0.0 {
        return None;
    }
    let r = reliability(interval, beta, eta);
    let cycle = integrated_reliability(interval, beta, eta);
    if cycle <= 0.0 {
        return None;
    }
    Some((planned_cost * r + failure_cost * (1.0 - r)) / cycle)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgeReplacementOptimum {
    /// Optimal replacement age T* (same unit as η, normally operating hours).
    pub interval: f64,
    /// Cost per operating hour at T*.
    pub cost_rate: f64,
    /// Cost per operating hour when running to failure (Cf / MTTF).
    pub run_to_failure_cost_rate: f64,
    /// Relative saving of T* against run-to-failure (0–1).
    pub saving_vs_run_to_failure: f64,
}

/// Why no age-replacement interval is recommended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgeReplacementRejection {
    /// β, η or a cost is not a positive finite number.
    InvalidInput,
    /// β ≤ 1: the failure rate does not increase with age, so replacing earlier never pays.
    NoWearOut,
    /// A breakdown does not cost more than a planned replacement.
    FailureNotCostlier,
    /// The best interval does not beat running to failure by a meaningful margin (< 1 %).
    NoBenefit,
}

/// Minimum relative saving against run-to-failure for an interval to be recommended.
const MIN_RELATIVE_SAVING: f64 = 0.01;

/// Optimal age-replacement interval (coarse logarithmic scan, then golden-section refinement).
pub fn optimal_age_replacement(
    planned_cost: f64,
    failure_cost: f64,
    beta: f64,
    eta: f64,
) -> Result<AgeReplacementOptimum, AgeReplacementRejection> {
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if !valid_params(beta, eta) || !positive(planned_cost) || !positive(failure_cost) {
        return Err(AgeReplacementRejection::InvalidInput);
    }
    if beta <= 1.0 {
        return Err(AgeReplacementRejection::NoWearOut);
    }
    if failure_cost <= planned_cost {
        return Err(AgeReplacementRejection::FailureNotCostlier);
    }
    let mttf = mean_life(beta, eta).ok_or(AgeReplacementRejection::InvalidInput)?;
    let run_to_failure = failure_cost / mttf;
    let cost = |t: f64| age_replacement_cost_rate(t, planned_cost, failure_cost, beta, eta).unwrap_or(f64::INFINITY);

    // Coarse scan on a log grid over [0.001η, 5η]; the cost curve is unimodal for β > 1.
    const STEPS: usize = 400;
    let (lo_ln, hi_ln) = ((eta * 1e-3).ln(), (eta * 5.0).ln());
    let grid: Vec<f64> = (0..=STEPS)
        .map(|i| (lo_ln + (hi_ln - lo_ln) * i as f64 / STEPS as f64).exp())
        .collect();
    let mut best_idx = 0;
    let mut best_cost = f64::INFINITY;
    for (i, t) in grid.iter().enumerate() {
        let c = cost(*t);
        if c < best_cost {
            best_cost = c;
            best_idx = i;
        }
    }
    let mut a = grid[best_idx.saturating_sub(1)];
    let mut b = grid[(best_idx + 1).min(STEPS)];

    // Golden-section refinement inside the bracket.
    let inv_phi = (5f64.sqrt() - 1.0) / 2.0;
    let mut c = b - inv_phi * (b - a);
    let mut d = a + inv_phi * (b - a);
    let (mut fc, mut fd) = (cost(c), cost(d));
    while (b - a).abs() > eta * 1e-9 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - inv_phi * (b - a);
            fc = cost(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + inv_phi * (b - a);
            fd = cost(d);
        }
    }
    let interval = (a + b) / 2.0;
    let cost_rate = cost(interval);
    let saving = 1.0 - cost_rate / run_to_failure;
    if saving.is_nan() || saving < MIN_RELATIVE_SAVING {
        return Err(AgeReplacementRejection::NoBenefit);
    }
    Ok(AgeReplacementOptimum {
        interval,
        cost_rate,
        run_to_failure_cost_rate: run_to_failure,
        saving_vs_run_to_failure: saving,
    })
}

/// One point of a Weibull probability plot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ProbabilityPlotPoint {
    /// Age at failure.
    pub time: f64,
    /// Median-rank estimate of the fraction failed at that age.
    pub median_rank: f64,
}

/// Probability-plot points for failures among `(age, is_suspension)` observations, using Johnson's
/// adjusted ranks for suspensions and Benard's median-rank approximation.
pub fn probability_plot_points(observations: &[(f64, bool)]) -> Vec<ProbabilityPlotPoint> {
    let mut obs: Vec<(f64, bool)> = observations
        .iter()
        .copied()
        .filter(|(t, _)| t.is_finite() && *t > 0.0)
        .collect();
    obs.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let n = obs.len() as f64;
    let mut previous_rank = 0.0;
    let mut points = Vec::new();
    for (k, (time, suspended)) in obs.iter().enumerate() {
        if *suspended {
            continue;
        }
        let reverse_rank = n - k as f64;
        previous_rank += (n + 1.0 - previous_rank) / (1.0 + reverse_rank);
        points.push(ProbabilityPlotPoint {
            time: *time,
            median_rank: (previous_rank - 0.3) / (n + 0.4),
        });
    }
    points
}

/// Correlation coefficient ρ of the points on Weibull paper (ln t vs ln(−ln(1−F))). Close to 1 when
/// the data follow a Weibull distribution. `None` with fewer than 3 failures.
pub fn probability_plot_correlation(points: &[ProbabilityPlotPoint]) -> Option<f64> {
    if points.len() < 3 {
        return None;
    }
    let xs: Vec<f64> = points.iter().map(|p| p.time.ln()).collect();
    let ys: Vec<f64> = points.iter().map(|p| (-(1.0 - p.median_rank).ln()).ln()).collect();
    let n = xs.len() as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in xs.iter().zip(ys.iter()) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return None;
    }
    Some(sxy / (sxx * syy).sqrt())
}

fn valid_params(beta: f64, eta: f64) -> bool {
    beta.is_finite() && eta.is_finite() && beta > 0.0 && eta > 0.0
}

/// ln Γ(x) for x > 0 (Lanczos approximation, g = 7, n = 9; relative error < 1e-14).
fn ln_gamma(x: f64) -> f64 {
    const COEF: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let z = x - 1.0;
    let mut acc = COEF[0];
    for (i, c) in COEF.iter().enumerate().skip(1) {
        acc += c / (z + i as f64);
    }
    let t = z + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + acc.ln()
}

/// 10-point Gauss–Legendre nodes (positive half) and weights on [−1, 1].
const GL10: [(f64, f64); 5] = [
    (0.148_874_338_981_631_2, 0.295_524_224_714_752_9),
    (0.433_395_394_129_247_2, 0.269_266_719_309_996_4),
    (0.679_409_568_299_024_4, 0.219_086_362_515_982_04),
    (0.865_063_366_688_984_5, 0.149_451_349_150_580_6),
    (0.973_906_528_517_171_7, 0.066_671_344_308_688_14),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f64, expected: f64, rel: f64) {
        let err = ((actual - expected) / expected).abs();
        assert!(err < rel, "expected {expected}, got {actual} (relative error {err:e})");
    }

    // (β, η, Cp, Cf, T*, C(T*), Cf/MTTF, MTTF, B10) — SciPy references.
    const CASES: [(f64, f64, f64, f64, f64, f64, f64, f64, f64); 4] = [
        (
            2.0,
            1000.0,
            1.0,
            5.0,
            510.6552,
            0.004_085_24,
            0.005_641_895_835,
            886.226_925,
            324.592_846,
        ),
        (
            3.04,
            4440.0,
            180_000.0,
            1_450_000.0,
            1851.9821,
            146.087_178_74,
            365.500_062_99,
            3967.167_579,
            2117.851_150,
        ),
        (
            1.5,
            2000.0,
            100.0,
            1000.0,
            756.2890,
            0.415_080_82,
            0.553_866_08,
            1805.490_586,
            446.151_051,
        ),
        (
            2.5,
            8760.0,
            50_000.0,
            400_000.0,
            3438.7149,
            24.566_429_58,
            51.463_949_68,
            7772.431_041,
            3561.026_956,
        ),
    ];

    #[test]
    fn mean_life_and_b10_match_reference() {
        for (beta, eta, _, _, _, _, _, mttf, b10) in CASES {
            close(mean_life(beta, eta).unwrap(), mttf, 1e-8);
            close(b_life(0.10, beta, eta).unwrap(), b10, 1e-8);
        }
    }

    #[test]
    fn optimal_age_replacement_matches_reference() {
        for (beta, eta, cp, cf, t_opt, c_opt, rtf, _, _) in CASES {
            let opt = optimal_age_replacement(cp, cf, beta, eta).expect("optimum");
            close(opt.interval, t_opt, 1e-4);
            close(opt.cost_rate, c_opt, 1e-6);
            close(opt.run_to_failure_cost_rate, rtf, 1e-7);
            close(opt.saving_vs_run_to_failure, 1.0 - c_opt / rtf, 1e-5);
        }
    }

    #[test]
    fn cost_rate_at_a_given_interval_matches_reference() {
        // Current practice in the pump example: replacement at 6 000 h.
        close(
            age_replacement_cost_rate(6000.0, 180_000.0, 1_450_000.0, 3.04, 4440.0).unwrap(),
            343.815_804_76,
            1e-7,
        );
        close(
            age_replacement_cost_rate(6000.0, 50_000.0, 400_000.0, 2.5, 8760.0).unwrap(),
            30.095_253_34,
            1e-7,
        );
    }

    #[test]
    fn no_interval_without_wear_out_or_cost_gap() {
        assert_eq!(
            optimal_age_replacement(100.0, 1000.0, 1.0, 2000.0),
            Err(AgeReplacementRejection::NoWearOut)
        );
        assert_eq!(
            optimal_age_replacement(100.0, 1000.0, 0.7, 2000.0),
            Err(AgeReplacementRejection::NoWearOut)
        );
        assert_eq!(
            optimal_age_replacement(1000.0, 1000.0, 2.0, 2000.0),
            Err(AgeReplacementRejection::FailureNotCostlier)
        );
        assert_eq!(
            optimal_age_replacement(-1.0, 1000.0, 2.0, 2000.0),
            Err(AgeReplacementRejection::InvalidInput)
        );
        assert_eq!(
            optimal_age_replacement(100.0, 1000.0, f64::NAN, 2000.0),
            Err(AgeReplacementRejection::InvalidInput)
        );
    }

    #[test]
    fn marginal_benefit_is_not_recommended() {
        // β barely above 1 and a small cost gap: the optimum saves less than 1 % vs run-to-failure.
        assert_eq!(
            optimal_age_replacement(900.0, 1000.0, 1.05, 2000.0),
            Err(AgeReplacementRejection::NoBenefit)
        );
    }

    #[test]
    fn probability_plot_correlation_matches_reference() {
        let with_suspensions = [
            (1850.0, false),
            (2610.0, false),
            (3100.0, false),
            (2200.0, true),
            (3620.0, false),
            (4080.0, false),
            (4510.0, false),
            (2900.0, true),
            (5180.0, false),
            (5920.0, false),
        ];
        let pts = probability_plot_points(&with_suspensions);
        assert_eq!(pts.len(), 8);
        close(probability_plot_correlation(&pts).unwrap(), 0.999_645_685_6, 1e-9);

        let complete: Vec<(f64, bool)> = [1200.0, 1850.0, 2400.0, 2950.0, 3600.0, 4300.0, 5100.0, 6200.0]
            .iter()
            .map(|t| (*t, false))
            .collect();
        close(
            probability_plot_correlation(&probability_plot_points(&complete)).unwrap(),
            0.999_268_193_4,
            1e-9,
        );
        assert_eq!(
            probability_plot_correlation(&probability_plot_points(&complete[..2])),
            None
        );
    }

    #[test]
    fn reliability_and_hazard_basics() {
        close(reliability(1000.0, 2.0, 1000.0), (-1.0f64).exp(), 1e-12);
        assert_eq!(reliability(0.0, 2.0, 1000.0), 1.0);
        close(hazard(500.0, 2.0, 1000.0), 0.001, 1e-12);
        // ∫₀^∞ R = MTTF; R(10η) = e^-100, so integrating to 10η is exact to machine precision.
        close(integrated_reliability(10_000.0, 2.0, 1000.0), 886.226_925, 1e-8);
    }
}
