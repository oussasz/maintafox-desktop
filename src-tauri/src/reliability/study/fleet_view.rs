//! Fleet view of the reliability module: KPIs of the filtered fleet over a period and the
//! equipment ranked by downtime, computed from the failure records of the period.
//!
//! Fleet KPIs are ratios of sums (`reliability::fleet::pool_fleet`): MTBF = Σ hours / Σ failures,
//! MTTR = Σ active repair / Σ repairs, A_o = Σ hours / (Σ hours + Σ downtime). Hours are operating
//! hours when the equipment has run-hour data (see `study::clock`), calendar hours otherwise; the
//! number of each is returned so the screen says which.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;
use crate::reliability::fleet::{pool_fleet, FleetSnapshotRow};
use crate::reliability::study::clock::{load_clock, parse_instant};
use crate::reliability::study::domain::{FilterOption, FleetAssetRow, FleetKpis, FleetView, FleetViewFilter};
use crate::reliability::study::store::{get, opt_i64};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

struct AssetAgg {
    equipment_id: i64,
    asset_code: String,
    asset_name: String,
    class_id: Option<i64>,
    class_name: Option<String>,
    failures: i64,
    downtime: f64,
    repair_hours: f64,
    missing_mode: i64,
    missing_cause: i64,
    di_earlier: i64,
}

pub async fn fleet_view(db: &DatabaseConnection, filter: FleetViewFilter) -> AppResult<FleetView> {
    let start = parse_instant(&filter.period_start)?;
    let end = {
        let e = parse_instant(&filter.period_end)?;
        let now = crate::clock::now();
        if e > now {
            now
        } else {
            e
        }
    };
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = filter.offset.unwrap_or(0).max(0);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, e.asset_id_code, e.name, e.class_id, ec.name AS class_name,
                    COUNT(fe.id) AS failures,
                    COALESCE(SUM(fe.downtime_duration_hours), 0.0) AS downtime,
                    COALESCE(SUM(fe.active_repair_hours), 0.0) AS repair_hours,
                    SUM(CASE WHEN fe.failure_mode_id IS NULL THEN 1 ELSE 0 END) AS missing_mode,
                    SUM(CASE WHEN fe.failure_mode_id IS NOT NULL AND fe.failure_cause_id IS NULL
                              AND COALESCE(fe.cause_not_determined, 0) = 0 THEN 1 ELSE 0 END) AS missing_cause,
                    SUM(CASE WHEN di.id IS NOT NULL AND fe.failed_at IS NOT NULL
                              AND (julianday(fe.failed_at) - julianday(COALESCE(di.observed_at, di.submitted_at))) * 24.0 > 1.0
                         THEN 1 ELSE 0 END) AS di_earlier
             FROM failure_events fe
             JOIN equipment e ON e.id = fe.equipment_id AND e.deleted_at IS NULL
             LEFT JOIN equipment_classes ec ON ec.id = e.class_id
             LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
             LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
             LEFT JOIN intervention_requests di ON di.id = wo.source_di_id
             WHERE COALESCE(fe.is_planned, 0) = 0
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) >= datetime(?)
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) <= datetime(?)
               AND (? IS NULL OR e.class_id = ?)
               AND (? IS NULL OR n.ancestor_path LIKE '%/' || ? || '/%')
             GROUP BY e.id
             ORDER BY downtime DESC, failures DESC, e.asset_id_code ASC",
            [
                start.to_rfc3339().into(),
                end.to_rfc3339().into(),
                opt_i64(filter.equipment_class_id),
                opt_i64(filter.equipment_class_id),
                opt_i64(filter.org_node_id),
                opt_i64(filter.org_node_id),
            ],
        ))
        .await?;
    let mut aggs = Vec::with_capacity(rows.len());
    for r in &rows {
        aggs.push(AssetAgg {
            equipment_id: get(r, "id")?,
            asset_code: get(r, "asset_id_code")?,
            asset_name: get(r, "name")?,
            class_id: get(r, "class_id")?,
            class_name: get(r, "class_name")?,
            failures: get(r, "failures")?,
            downtime: get(r, "downtime")?,
            repair_hours: get(r, "repair_hours")?,
            missing_mode: get(r, "missing_mode")?,
            missing_cause: get(r, "missing_cause")?,
            di_earlier: get(r, "di_earlier")?,
        });
    }

    // Equipment of the filtered fleet with no failure still count in the hours (A_o, MTBF).
    let fleet = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id FROM equipment e LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
             WHERE e.deleted_at IS NULL
               AND (? IS NULL OR e.class_id = ?)
               AND (? IS NULL OR n.ancestor_path LIKE '%/' || ? || '/%')",
            [
                opt_i64(filter.equipment_class_id),
                opt_i64(filter.equipment_class_id),
                opt_i64(filter.org_node_id),
                opt_i64(filter.org_node_id),
            ],
        ))
        .await?;
    let by_id: HashMap<i64, &AssetAgg> = aggs.iter().map(|a| (a.equipment_id, a)).collect();
    let mut pooled_rows = Vec::with_capacity(fleet.len());
    let mut asset_hours: HashMap<i64, (f64, &'static str)> = HashMap::new();
    let (mut operating_count, mut calendar_count) = (0_i64, 0_i64);
    for r in &fleet {
        let id: i64 = get(r, "id")?;
        let clock = load_clock(db, id).await?;
        let hours = clock.hours_between(start, end);
        if clock.is_operating() {
            operating_count += 1;
        } else {
            calendar_count += 1;
        }
        asset_hours.insert(id, (hours, clock.source()));
        let agg = by_id.get(&id);
        pooled_rows.push(FleetSnapshotRow {
            equipment_id: id,
            data_quality_score: 1.0,
            repairable: true,
            event_count: agg.map_or(0, |a| a.failures),
            exposure_hours: Some(hours),
            repair_event_count: Some(agg.map_or(0, |a| a.failures)),
            active_repair_hours_total: Some(agg.map_or(0.0, |a| a.repair_hours)),
            downtime_hours_total: Some(agg.map_or(0.0, |a| a.downtime)),
        });
    }
    let pooled = pool_fleet(&pooled_rows, 0.0);
    let failure_count: i64 = aggs.iter().map(|a| a.failures).sum();
    let downtime_total: f64 = aggs.iter().map(|a| a.downtime).sum();
    let missing_mode: i64 = aggs.iter().map(|a| a.missing_mode).sum();
    let missing_cause: i64 = aggs.iter().map(|a| a.missing_cause).sum();
    let kpis = FleetKpis {
        equipment_count: fleet.len() as i64,
        failure_count,
        downtime_hours: downtime_total,
        availability_operational: pooled.availability_operational,
        mtbf_hours: pooled.mtbf_hours,
        mttr_hours: pooled.mttr_hours,
        data_quality: (failure_count > 0)
            .then(|| (failure_count - missing_mode - missing_cause).max(0) as f64 / failure_count as f64),
        records_missing_mode: missing_mode,
        records_missing_cause: missing_cause,
        operating_basis_count: operating_count,
        calendar_basis_count: calendar_count,
    };

    let total_rows = aggs.len() as i64;
    let mut out_rows = Vec::new();
    // Pages of the ranking: the fleet KPIs above always cover every asset.
    for a in aggs.iter().skip(offset as usize).take(limit as usize) {
        let (hours, source) = asset_hours.get(&a.equipment_id).copied().unwrap_or((0.0, "calendar"));
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
        let to_check = a.missing_mode + a.missing_cause + a.di_earlier;
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
            mtbf_hours: (a.failures > 0 && hours > 0.0).then(|| hours / a.failures as f64),
            mttr_hours: (a.failures > 0).then(|| a.repair_hours / a.failures as f64),
            time_basis: source.to_string(),
            data_quality: if source == "calendar" {
                "no_operating_hours"
            } else if to_check > 0 {
                "to_check"
            } else {
                "complete"
            }
            .into(),
            records_to_check: to_check,
            last_study_id: last.as_ref().map(|r| get(r, "id")).transpose()?,
            last_study_title: last.as_ref().map(|r| get(r, "title")).transpose()?,
            last_study_updated_at: last.as_ref().map(|r| get(r, "updated_at")).transpose()?,
        });
    }

    let org_nodes = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT n.id, n.name, n.depth, n.ancestor_path FROM org_nodes n
             WHERE n.deleted_at IS NULL AND EXISTS (
                SELECT 1 FROM equipment e JOIN org_nodes h ON h.id = e.installed_at_node_id
                WHERE e.deleted_at IS NULL AND h.ancestor_path LIKE '%/' || n.id || '/%')
             ORDER BY n.ancestor_path ASC"
                .to_string(),
        ))
        .await?
        .iter()
        .map(|r| {
            Ok(FilterOption {
                id: get(r, "id")?,
                label: get(r, "name")?,
                depth: get(r, "depth")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let equipment_classes = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT ec.id, ec.name FROM equipment_classes ec
             JOIN equipment e ON e.class_id = ec.id AND e.deleted_at IS NULL
             ORDER BY ec.name ASC"
                .to_string(),
        ))
        .await?
        .iter()
        .map(|r| {
            Ok(FilterOption {
                id: get(r, "id")?,
                label: get(r, "name")?,
                depth: 0,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(FleetView {
        period_start: start.to_rfc3339(),
        period_end: end.to_rfc3339(),
        kpis,
        rows: out_rows,
        total_rows,
        org_nodes,
        equipment_classes,
    })
}
