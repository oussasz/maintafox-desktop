//! Fleet view of the reliability module: KPIs of the filtered fleet over a period and the
//! equipment ranked by downtime, computed live from the failure records of the period.
//!
//! The KPIs and the per-equipment MTBF / MTTR come from the one shared calculation
//! (`study::fleet_kpis`, definitions there): MTBF over every unplanned failure, MTTR = mean time
//! to restore, A_o = MTBF / (MTBF + MTTR), data quality = completely coded records.

use std::cmp::Ordering;
use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::reliability::study::clock::SOURCE_CALENDAR;
use crate::reliability::study::domain::{AssetReliabilityFigures, FleetAssetRow, FleetView, FleetViewFilter};
use crate::reliability::study::fleet_kpis::{
    filter_options, fleet_hours, kpis_of, load_asset_aggs, missing_date_count, outside_service_count, scope_from_sql,
    AssetAgg, AssetHours, FleetScope,
};
use crate::reliability::study::store::get;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;
/// Columns the ranking can be sorted by.
const SORT_KEYS: [&str; 4] = ["failures", "downtime", "mtbf", "cost"];

/// MTBF of one equipment: its hours / its unplanned failures (the fleet-figure definition).
fn asset_mtbf(a: &AssetAgg, hours: &HashMap<i64, AssetHours>, too_short: bool) -> Option<f64> {
    let h = hours.get(&a.equipment_id).map_or(0.0, |h| h.hours);
    (!too_short && a.failures > 0 && h > 0.0).then(|| h / a.failures as f64)
}

/// Cost of the work behind the failure records of the scope, per equipment (work-order total
/// cost; imported history labour + parts + services).
async fn failure_costs(db: &DatabaseConnection, scope: &FleetScope) -> AppResult<HashMap<i64, f64>> {
    let sql = format!(
        "SELECT fe2.equipment_id AS equipment_id,
                COALESCE(SUM(CASE WHEN fe2.source_type = 'work_order' THEN w.total_cost
                                  WHEN fe2.source_type = 'history_record'
                                  THEN COALESCE(h.labor_cost, 0) + COALESCE(h.parts_cost, 0) + COALESCE(h.service_cost, 0)
                             END), 0.0) AS cost
         FROM failure_events fe2
         LEFT JOIN work_orders w ON fe2.source_type = 'work_order' AND w.id = fe2.source_id
         LEFT JOIN wo_history_records h ON fe2.source_type = 'history_record' AND h.id = fe2.source_id
         WHERE fe2.id IN (SELECT fe.id {from})
         GROUP BY fe2.equipment_id",
        from = scope_from_sql()
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, scope.binds()))
        .await?;
    let mut out = HashMap::new();
    for r in &rows {
        let id: i64 = get(r, "equipment_id")?;
        let cost: f64 = get(r, "cost")?;
        out.insert(id, cost);
    }
    Ok(out)
}

/// Failure figures of one equipment over a period, from the same calculation as the fleet view.
pub async fn asset_figures(
    db: &DatabaseConnection,
    equipment_id: i64,
    period_start: &str,
    period_end: &str,
) -> AppResult<AssetReliabilityFigures> {
    let scope = FleetScope::new(period_start, period_end, None, None)?;
    let aggs = load_asset_aggs(db, &scope).await?;
    let hours = fleet_hours(db, &scope).await?;
    let too_short = scope.window_too_short();
    let agg = aggs.iter().find(|a| a.equipment_id == equipment_id);
    Ok(AssetReliabilityFigures {
        equipment_id,
        period_start: scope.start.to_rfc3339(),
        period_end: scope.end.to_rfc3339(),
        window_too_short: too_short,
        failures: agg.map_or(0, |a| a.failures),
        downtime_hours: agg.map_or(0.0, |a| a.downtime),
        mtbf_hours: agg.and_then(|a| asset_mtbf(a, &hours, too_short)),
        mttr_hours: agg
            .and_then(|a| (!too_short && a.restore_count > 0).then(|| a.restore_hours / a.restore_count as f64)),
    })
}

pub async fn fleet_view(db: &DatabaseConnection, filter: FleetViewFilter) -> AppResult<FleetView> {
    let scope = FleetScope::new(
        &filter.period_start,
        &filter.period_end,
        filter.equipment_class_id,
        filter.org_node_id,
    )?;
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = filter.offset.unwrap_or(0).max(0);
    if let Some(key) = filter.sort_by.as_deref() {
        if !SORT_KEYS.contains(&key) {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_REL_FLEET_SORT_INVALID: sort_by must be one of {SORT_KEYS:?}."
            )]));
        }
    }
    let mut aggs = load_asset_aggs(db, &scope).await?;
    let costs = failure_costs(db, &scope).await?;
    // Equipment of the filtered fleet with no failure still count in the hours (A_o, MTBF).
    let hours = fleet_hours(db, &scope).await?;
    let missing_date = missing_date_count(db, &scope).await?;
    let outside_service = outside_service_count(db, &scope).await?;
    let kpis = kpis_of(&scope, &aggs, &hours, missing_date, outside_service);
    let downtime_total = kpis.downtime_hours;
    let too_short = scope.window_too_short();

    // The ranking is ordered before it is paged, so a page is the Nth slice of the chosen column.
    // Without a chosen column the default order (downtime, failures, code) is kept.
    if let Some(key) = filter.sort_by.as_deref() {
        let desc = filter.sort_desc.unwrap_or(key != "mtbf");
        let value = |a: &AssetAgg| -> Option<f64> {
            match key {
                "failures" => Some(a.failures as f64),
                "downtime" => Some(a.downtime),
                "cost" => Some(costs.get(&a.equipment_id).copied().unwrap_or(0.0)),
                _ => asset_mtbf(a, &hours, too_short),
            }
        };
        aggs.sort_by(|x, y| match (value(x), value(y)) {
            (Some(a), Some(b)) => {
                let o = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
                (if desc { o.reverse() } else { o }).then_with(|| x.asset_code.cmp(&y.asset_code))
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => x.asset_code.cmp(&y.asset_code),
        });
    }

    let total_rows = aggs.len() as i64;
    let mut out_rows = Vec::new();
    // Pages of the ranking: the fleet KPIs above always cover every asset.
    for a in aggs.iter().skip(offset as usize).take(limit as usize) {
        let (asset_hours, source) = hours
            .get(&a.equipment_id)
            .map_or((0.0, SOURCE_CALENDAR), |h| (h.hours, h.source));
        let last = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT s.id, s.title, s.updated_at FROM reliability_studies s
                 JOIN reliability_study_assets sa ON sa.study_id = s.id
                 WHERE sa.equipment_id = ? AND s.status = 'active'
                 ORDER BY s.updated_at DESC, s.id DESC LIMIT 1",
                [a.equipment_id.into()],
            ))
            .await?;
        out_rows.push(FleetAssetRow {
            equipment_id: a.equipment_id,
            asset_code: a.asset_code.clone(),
            asset_name: a.asset_name.clone(),
            class_id: a.class_id,
            class_name: a.class_name.clone(),
            failures: a.failures,
            downtime_hours: a.downtime,
            downtime_share: if downtime_total > 0.0 {
                a.downtime / downtime_total
            } else {
                0.0
            },
            // Same definitions as the fleet figures (every unplanned failure, time to restore).
            mtbf_hours: (!too_short && a.failures > 0 && asset_hours > 0.0).then(|| asset_hours / a.failures as f64),
            mttr_hours: (!too_short && a.restore_count > 0).then(|| a.restore_hours / a.restore_count as f64),
            failure_cost: costs.get(&a.equipment_id).copied().filter(|c| *c > 0.0),
            time_basis: source.to_string(),
            data_quality: if source == SOURCE_CALENDAR {
                "no_operating_hours"
            } else if a.to_check > 0 {
                "to_check"
            } else {
                "complete"
            }
            .into(),
            records_to_check: a.to_check,
            last_study_id: last.as_ref().map(|r| get(r, "id")).transpose()?,
            last_study_title: last.as_ref().map(|r| get(r, "title")).transpose()?,
            last_study_updated_at: last.as_ref().map(|r| get(r, "updated_at")).transpose()?,
        });
    }

    let (org_nodes, equipment_classes) = filter_options(db).await?;
    Ok(FleetView {
        period_start: kpis.period_start.clone(),
        period_end: kpis.period_end.clone(),
        kpis,
        rows: out_rows,
        total_rows,
        org_nodes,
        equipment_classes,
    })
}
