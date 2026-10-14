//! Filter-scoped Equipment registry KPI aggregates (`get_equipment_registry_metrics`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use serde::Serialize;

use crate::assets::health::{self, AssetHealthScore};
use crate::assets::search::{build_asset_scope_where, text_matches_for, AssetSearchFilters, METRICS_FROM};
use crate::errors::AppResult;
use crate::kpi_definitions;

/// Stale meter threshold — aligned with health.rs meter recency “> 30 days” band.
const STALE_METER_DAYS: i64 = 30;
/// Cap health score sampling for performance on large fleets.
const HEALTH_SCORE_SAMPLE_CAP: u64 = 500;

const ACTIVE_STATUS_SQL: &str = "UPPER(COALESCE(rs_stat.code, e.lifecycle_status)) IN (\
    'IN_SERVICE','STANDBY','ACTIVE_IN_SERVICE','ACTIVE')";
const DOWN_STATUS_SQL: &str = "UPPER(COALESCE(rs_stat.code, e.lifecycle_status)) IN (\
    'OUT_OF_SERVICE','DOWN','INACTIVE','OUTOFSERVICE')";
const CRITICAL_SQL: &str = "UPPER(COALESCE(rs_crit.code, lv.code)) IN ('A','CRITICAL','CRITIQUE')";

const OPEN_DI_EXISTS: &str = "EXISTS (\
    SELECT 1 FROM intervention_requests ir \
    WHERE ir.equipment_id = e.id \
      AND ir.status NOT IN ('rejected','converted_to_work_order','closed_as_non_executable','archived')\
  )";
const OPEN_WO_EXISTS: &str = "EXISTS (\
    SELECT 1 FROM work_orders wo \
    JOIN work_order_statuses s ON s.id = wo.status_id \
    WHERE wo.equipment_id = e.id AND s.code NOT IN ('closed','cancelled')\
  )";

#[derive(Debug, Clone, Serialize)]
pub struct EquipmentRegistryMetricCell {
    pub semantic_key: String,
    pub catalog_id: String,
    pub value: f64,
    pub previous_value: f64,
    pub available: bool,
    pub value_kind: String,
    pub trend_sense: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub down_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquipmentRegistryMetricsPayload {
    pub scoped_total: i64,
    pub cells: Vec<EquipmentRegistryMetricCell>,
}

fn cell(
    key: &str,
    catalog_id: &str,
    value: f64,
    previous: f64,
    available: bool,
    value_kind: &str,
    trend_sense: &str,
) -> EquipmentRegistryMetricCell {
    EquipmentRegistryMetricCell {
        semantic_key: key.into(),
        catalog_id: catalog_id.into(),
        value,
        previous_value: previous,
        available,
        value_kind: value_kind.into(),
        trend_sense: trend_sense.into(),
        active_count: None,
        down_count: None,
        series: None,
    }
}

async fn count_scoped(db: &DatabaseConnection, where_sql: &str, binds: &[Value], extra_and: &str) -> AppResult<i64> {
    let sql = format!("SELECT COUNT(*) AS cnt {METRICS_FROM} WHERE {where_sql} AND ({extra_and})");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds.to_vec()))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "cnt").unwrap_or(0)).unwrap_or(0))
}

async fn count_scoped_total(db: &DatabaseConnection, where_sql: &str, binds: &[Value]) -> AppResult<i64> {
    let sql = format!("SELECT COUNT(*) AS cnt {METRICS_FROM} WHERE {where_sql}");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds.to_vec()))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "cnt").unwrap_or(0)).unwrap_or(0))
}

/// Compute registry strip metrics under the same SmartFilterBar scope as `search_assets`.
pub async fn get_equipment_registry_metrics(
    db: &DatabaseConnection,
    filters: AssetSearchFilters,
) -> AppResult<EquipmentRegistryMetricsPayload> {
    let text_matches = text_matches_for(db, &filters).await?;
    let (where_sql, binds) = build_asset_scope_where(&filters, text_matches.as_deref());
    let scoped_total = count_scoped_total(db, &where_sql, &binds).await?;

    // ── EQUIP_STATUS_SUMMARY ──────────────────────────────────────────────
    let active_count = count_scoped(db, &where_sql, &binds, ACTIVE_STATUS_SQL).await?;
    let down_count = count_scoped(db, &where_sql, &binds, DOWN_STATUS_SQL).await?;
    let mut status_cell = cell(
        kpi_definitions::equipment::EQUIP_STATUS_SUMMARY,
        kpi_definitions::catalog::EQUIP_STATUS_SUMMARY,
        down_count as f64,
        down_count as f64,
        true,
        "count",
        "up-bad",
    );
    status_cell.active_count = Some(active_count);
    status_cell.down_count = Some(down_count);

    // ── EQUIP_CRITICAL_AT_RISK ────────────────────────────────────────────
    let critical_at_risk = count_scoped(
        db,
        &where_sql,
        &binds,
        &format!("{CRITICAL_SQL} AND ({DOWN_STATUS_SQL} OR {OPEN_DI_EXISTS} OR {OPEN_WO_EXISTS})"),
    )
    .await?;
    let critical_cell = cell(
        kpi_definitions::equipment::EQUIP_CRITICAL_AT_RISK,
        kpi_definitions::catalog::EQUIP_CRITICAL_AT_RISK,
        critical_at_risk as f64,
        critical_at_risk as f64,
        true,
        "count",
        "up-bad",
    );

    // ── EQUIP_OPEN_WORK ───────────────────────────────────────────────────
    let open_work = count_scoped(
        db,
        &where_sql,
        &binds,
        &format!("({OPEN_DI_EXISTS} OR {OPEN_WO_EXISTS})"),
    )
    .await?;
    let mut open_work_cell = cell(
        kpi_definitions::equipment::EQUIP_OPEN_WORK,
        kpi_definitions::catalog::EQUIP_OPEN_WORK,
        open_work as f64,
        open_work as f64,
        true,
        "count",
        "up-bad",
    );
    open_work_cell.series = load_open_work_series(db, &where_sql, &binds).await.ok();

    // ── EQUIP_STALE_METERS ────────────────────────────────────────────────
    let stale_sql = format!(
        "EXISTS (\
           SELECT 1 FROM equipment_meters em \
           WHERE em.equipment_id = e.id AND em.is_active = 1 \
             AND (em.last_read_at IS NULL \
                  OR datetime(em.last_read_at) < datetime('now', '-{STALE_METER_DAYS} days'))\
         )"
    );
    let stale_meters = count_scoped(db, &where_sql, &binds, &stale_sql).await?;
    let stale_cell = cell(
        kpi_definitions::equipment::EQUIP_STALE_METERS,
        kpi_definitions::catalog::EQUIP_STALE_METERS,
        stale_meters as f64,
        stale_meters as f64,
        true,
        "count",
        "up-bad",
    );

    // ── EQUIP_HEALTH_SCORE (sample + reuse SSOT scorer) ───────────────────
    let (avg_health, health_available) = average_health_score(db, &where_sql, &binds).await?;
    let health_cell = cell(
        kpi_definitions::equipment::EQUIP_HEALTH_SCORE,
        kpi_definitions::catalog::EQUIP_HEALTH_SCORE,
        avg_health,
        avg_health,
        health_available,
        "percent",
        "up-good",
    );

    Ok(EquipmentRegistryMetricsPayload {
        scoped_total,
        cells: vec![status_cell, critical_cell, health_cell, open_work_cell, stale_cell],
    })
}

async fn average_health_score(db: &DatabaseConnection, where_sql: &str, binds: &[Value]) -> AppResult<(f64, bool)> {
    let sql = format!(
        "SELECT e.id {METRICS_FROM} WHERE {where_sql} \
         AND EXISTS (SELECT 1 FROM equipment_lifecycle_events le WHERE le.equipment_id = e.id) \
         ORDER BY e.id ASC LIMIT {HEALTH_SCORE_SAMPLE_CAP}"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds.to_vec()))
        .await?;

    if rows.is_empty() {
        return Ok((0.0, false));
    }

    let mut sum = 0.0;
    let mut n = 0i64;
    for r in rows {
        let id: i64 = r.try_get("", "id").unwrap_or(0);
        if id <= 0 {
            continue;
        }
        let AssetHealthScore { score, .. } = health::get_asset_health_score(db, id).await?;
        if let Some(s) = score {
            sum += s as f64;
            n += 1;
        }
    }
    if n == 0 {
        return Ok((0.0, false));
    }
    Ok((sum / n as f64, true))
}

async fn load_open_work_series(db: &DatabaseConnection, where_sql: &str, binds: &[Value]) -> AppResult<Vec<f64>> {
    // Daily count of distinct scoped assets that received a new open DI or WO that day (30d).
    let sql = format!(
        "SELECT d AS day, COUNT(*) AS cnt FROM (\
           SELECT date(ir.created_at) AS d, ir.equipment_id AS eid \
           FROM intervention_requests ir \
           WHERE ir.created_at >= datetime('now', '-30 days') \
             AND ir.equipment_id IN (SELECT e.id {METRICS_FROM} WHERE {where_sql}) \
             AND ir.status NOT IN ('rejected','converted_to_work_order','closed_as_non_executable','archived') \
           UNION ALL \
           SELECT date(wo.created_at) AS d, wo.equipment_id AS eid \
           FROM work_orders wo \
           JOIN work_order_statuses s ON s.id = wo.status_id \
           WHERE wo.created_at >= datetime('now', '-30 days') \
             AND wo.equipment_id IN (SELECT e.id {METRICS_FROM} WHERE {where_sql}) \
             AND s.code NOT IN ('closed','cancelled') \
         ) GROUP BY d ORDER BY d ASC"
    );
    // binds used twice in the subquery — duplicate
    let mut all_binds = binds.to_vec();
    all_binds.extend(binds.iter().cloned());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, all_binds))
        .await?;

    let mut out = Vec::new();
    for r in rows {
        let cnt: i64 = r.try_get("", "cnt").unwrap_or(0);
        out.push(cnt as f64);
    }
    // Ensure we don't return an empty series when there's no activity — leave empty for UI gate
    Ok(out)
}
