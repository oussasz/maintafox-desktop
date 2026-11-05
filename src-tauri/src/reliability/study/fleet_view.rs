//! Fleet view of the reliability module: KPIs of the filtered fleet over a period and the
//! equipment ranked by downtime, computed live from the failure records of the period.
//!
//! The KPIs and the per-equipment MTBF / MTTR come from the one shared calculation
//! (`study::fleet_kpis`, definitions there): MTBF over MTBF-eligible failures, MTTR = mean time
//! to restore, A_o = MTBF / (MTBF + MTTR), data quality = completely coded records.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;
use crate::reliability::study::clock::SOURCE_CALENDAR;
use crate::reliability::study::domain::{FleetAssetRow, FleetView, FleetViewFilter};
use crate::reliability::study::fleet_kpis::{filter_options, fleet_hours, kpis_of, load_asset_aggs, FleetScope};
use crate::reliability::study::store::get;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

pub async fn fleet_view(db: &DatabaseConnection, filter: FleetViewFilter) -> AppResult<FleetView> {
    let scope = FleetScope::new(
        &filter.period_start,
        &filter.period_end,
        filter.equipment_class_id,
        filter.org_node_id,
    )?;
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = filter.offset.unwrap_or(0).max(0);
    let aggs = load_asset_aggs(db, &scope).await?;
    // Equipment of the filtered fleet with no failure still count in the hours (A_o, MTBF).
    let hours = fleet_hours(db, &scope).await?;
    let kpis = kpis_of(&scope, &aggs, &hours);
    let downtime_total = kpis.downtime_hours;
    let too_short = scope.window_too_short();

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
            // Same definitions as the fleet figures (eligible failures, time to restore).
            mtbf_hours: (!too_short && a.eligible > 0 && asset_hours > 0.0).then(|| asset_hours / a.eligible as f64),
            mttr_hours: (!too_short && a.restore_count > 0).then(|| a.restore_hours / a.restore_count as f64),
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
