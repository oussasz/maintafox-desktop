//! Step 2 of a study: life analysis of the retained records, compared with the PM plan in
//! place and priced with the age-replacement model.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;
use crate::reliability::study::clock::StudyClock;
use crate::reliability::study::domain::{LifeAnalysis, LifeAnalysisRequest, PmPlanCandidate, ReliabilityStudy};
use crate::reliability::study::life::{analyse_life, LifeInputs};
use crate::reliability::study::pm_interval::PlanTrigger;
use crate::reliability::study::records::{build_dataset, BuiltDataset, BASIS_OPERATING};
use crate::reliability::study::store::{get, get_cost_profile};

/// Mean calendar hours in a year.
pub const HOURS_PER_YEAR: f64 = 8766.0;

/// Operating hours per calendar hour of each equipment over the part of the period its data
/// covers (1 on calendar time).
#[must_use]
pub fn operating_ratios(built: &BuiltDataset) -> HashMap<i64, f64> {
    let mut out = HashMap::new();
    for info in &built.dataset.clocks {
        let clock = built
            .clocks
            .get(&info.equipment_id)
            .cloned()
            .unwrap_or(StudyClock::Calendar);
        let covered = info.calendar_hours * clock.coverage(built.period_start, built.period_end);
        if covered > 0.0 && info.period_hours > 0.0 {
            out.insert(info.equipment_id, info.period_hours / covered);
        }
    }
    out
}

/// PM plans that target the study equipment with an interval strategy, and their interval in
/// study hours.
pub async fn pm_candidates(
    db: &DatabaseConnection,
    study: &ReliabilityStudy,
    operating: bool,
    ratios: &HashMap<i64, f64>,
) -> AppResult<Vec<PmPlanCandidate>> {
    let mut out = Vec::new();
    for asset in &study.assets {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT p.id, p.code, p.title, p.strategy_type,
                        COALESCE(cv.trigger_definition_json,
                                 (SELECT v.trigger_definition_json FROM pm_plan_versions v
                                  WHERE v.pm_plan_id = p.id ORDER BY v.version_no DESC LIMIT 1)) AS trig
                 FROM pm_plans p
                 LEFT JOIN pm_plan_versions cv ON cv.id = p.current_version_id
                 WHERE p.asset_scope_type = 'equipment' AND p.asset_scope_id = ? AND p.is_active = 1
                   AND p.strategy_type IN ('fixed', 'floating', 'meter')
                   AND COALESCE(p.lifecycle_status, 'active') NOT IN ('retired', 'suspended')
                 ORDER BY p.code ASC",
                [asset.equipment_id.into()],
            ))
            .await?;
        for r in &rows {
            let trig: Option<String> = get(r, "trig")?;
            let strategy: String = get(r, "strategy_type")?;
            let Some(trig) = trig else {
                continue;
            };
            let meter_hours = meter_counts_hours(db, &trig).await?;
            let parsed = PlanTrigger::parse(&strategy, &trig, meter_hours);
            let (value, unit) = parsed
                .as_ref()
                .ok()
                .and_then(PlanTrigger::displayed_interval)
                .map_or((None, None), |(v, u)| (Some(v), Some(u)));
            let interval = parsed.and_then(|p| p.interval_hours(operating, ratios.get(&asset.equipment_id).copied()));
            out.push(PmPlanCandidate {
                pm_plan_id: get(r, "id")?,
                code: get(r, "code")?,
                title: get(r, "title")?,
                equipment_id: asset.equipment_id,
                asset_code: asset.asset_code.clone(),
                strategy_type: strategy,
                interval_value: value,
                interval_unit: unit,
                interval_hours: interval.ok(),
                interval_note: interval.err().map(str::to_string),
            });
        }
    }
    Ok(out)
}

/// Whether a meter trigger counts operating hours (`operating_hours` source or an `HOURS` meter).
pub async fn meter_counts_hours(db: &DatabaseConnection, trigger_json: &str) -> AppResult<bool> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(trigger_json) else {
        return Ok(false);
    };
    if v.get("meter_source").and_then(serde_json::Value::as_str).map(str::trim) == Some("operating_hours") {
        return Ok(true);
    }
    let Some(meter_id) = v.get("asset_meter_id").and_then(serde_json::Value::as_i64) else {
        return Ok(false);
    };
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM equipment_meters WHERE id = ? AND upper(meter_type) = 'HOURS'",
            [meter_id.into()],
        ))
        .await?
        .is_some())
}

/// Costs that apply: typed in the request, saved on the study, or the class default.
async fn resolve_costs(
    db: &DatabaseConnection,
    study: &ReliabilityStudy,
    req: &LifeAnalysisRequest,
) -> AppResult<(
    Option<f64>,
    Option<f64>,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
)> {
    let classes: Vec<Option<i64>> = study.assets.iter().map(|a| a.class_id).collect();
    let shared_class = match classes.first() {
        Some(Some(c)) if classes.iter().all(|x| *x == Some(*c)) => Some(*c),
        _ => None,
    };
    let class_name = shared_class.and_then(|c| {
        study
            .assets
            .iter()
            .find(|a| a.class_id == Some(c))
            .and_then(|a| a.class_name.clone())
    });
    if req.planned_cost.is_some() && req.failure_cost.is_some() {
        return Ok((
            req.planned_cost,
            req.failure_cost,
            "input".into(),
            None,
            shared_class,
            class_name,
        ));
    }
    if study.planned_cost.is_some() && study.failure_cost.is_some() {
        return Ok((
            study.planned_cost,
            study.failure_cost,
            "study".into(),
            study.cost_currency_code.clone(),
            shared_class,
            class_name,
        ));
    }
    if let Some(c) = shared_class {
        if let Some(p) = get_cost_profile(db, c).await? {
            return Ok((
                Some(p.planned_cost),
                Some(p.failure_cost),
                "class_default".into(),
                p.currency_code,
                shared_class,
                class_name,
            ));
        }
    }
    Ok((None, None, "missing".into(), None, shared_class, class_name))
}

pub async fn life_analysis(
    db: &DatabaseConnection,
    req: LifeAnalysisRequest,
) -> AppResult<(LifeAnalysis, BuiltDataset)> {
    let built = build_dataset(db, req.study_id).await?;
    let study = &built.dataset.study;
    let operating = built.dataset.effective_time_basis == BASIS_OPERATING;
    let ratios = operating_ratios(&built);
    let plans = pm_candidates(db, study, operating, &ratios).await?;
    let selected = req
        .pm_plan_id
        .or(study.pm_plan_id)
        .filter(|id| plans.iter().any(|p| p.pm_plan_id == *id))
        .or_else(|| plans.iter().find(|p| p.interval_hours.is_some()).map(|p| p.pm_plan_id));
    let current_interval =
        selected.and_then(|id| plans.iter().find(|p| p.pm_plan_id == id).and_then(|p| p.interval_hours));
    let hours_per_year = if ratios.is_empty() {
        None
    } else {
        Some(ratios.values().sum::<f64>() * HOURS_PER_YEAR)
    };
    let operating_ratio = (!ratios.is_empty()).then(|| ratios.values().sum::<f64>() / ratios.len() as f64);
    let (planned, failure, cost_source, currency, cost_class_id, cost_class_name) =
        resolve_costs(db, study, &req).await?;
    let result = analyse_life(&LifeInputs {
        observations: built.observations.clone(),
        planned_cost: planned,
        failure_cost: failure,
        current_interval,
        hours_per_year,
    });
    let analysis = LifeAnalysis {
        effective_time_basis: built.dataset.effective_time_basis.clone(),
        n_failures: result.n_failures,
        n_suspensions: result.n_suspensions,
        min_failures: crate::reliability::weibull_fit::WEIBULL_MIN_ADEQUATE_N as i64,
        fit: result.fit,
        fit_issue: result.fit_issue.map(str::to_string),
        plot_points: result.plot_points,
        band: result.band,
        curve: result.curve,
        cost: result.cost,
        cost_source,
        cost_currency_code: currency,
        cost_class_id,
        cost_class_name,
        pm_plans: plans,
        selected_pm_plan_id: selected,
        operating_hours_per_year: hours_per_year,
        asset_count: study.assets.len() as i64,
        operating_ratio,
    };
    Ok((analysis, built))
}
