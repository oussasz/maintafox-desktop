//! Filter-scoped Equipment registry KPI aggregates (`get_equipment_registry_metrics`).
//!
//! Every predicate comes from its module's own definition, never from codes written here:
//! - status (in service / down / retired): the availability tag of the equipment's status in the
//!   published `EQUIPMENT.STATUS` list (`taxonomy_reference::STATUS_*_SQL`);
//! - critical: the most critical level of the published `EQUIPMENT.CRITICALITY` list
//!   (`taxonomy_reference::MOST_CRITICAL_SQL`, the home page's "critical assets down" rule);
//! - open request: the DI page's open rule (`di::stats::OPEN_SQL`);
//! - open work order: the OT backlog's work still ahead (`wo::backlog::OPEN_STATUS_SQL`).
//!
//! The strip is a current state: there is no baseline, so no trend is reported.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use serde::Serialize;

use crate::assets::health::{self, AssetHealthScore};
use crate::assets::search::{build_asset_scope_where, text_matches_for, AssetSearchFilters, METRICS_FROM};
use crate::assets::taxonomy_reference::{MOST_CRITICAL_SQL, STATUS_DOWN_SQL, STATUS_UP_SQL};
use crate::errors::{AppError, AppResult};
use crate::kpi_definitions;

/// Stale meter threshold — aligned with health.rs meter recency “> 30 days” band.
const STALE_METER_DAYS: i64 = 30;
/// Cap health score sampling for performance on large fleets.
const HEALTH_SCORE_SAMPLE_CAP: u64 = 500;

/// SQL predicate (alias `e`): the equipment has an open intervention request (DI page rule).
fn open_di_exists_sql() -> String {
    format!(
        "EXISTS (SELECT 1 FROM intervention_requests ir WHERE ir.equipment_id = e.id AND {})",
        crate::di::stats::OPEN_SQL
    )
}

/// SQL predicate (alias `e`): the equipment has a work order whose work is still ahead.
fn open_wo_exists_sql() -> String {
    format!(
        "EXISTS (SELECT 1 FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE wo.equipment_id = e.id AND s.code IN {})",
        crate::wo::backlog::OPEN_STATUS_SQL
    )
}

/// SQL predicate (alias `e`): an active meter never read or not read for [`STALE_METER_DAYS`].
fn stale_meter_exists_sql() -> String {
    format!(
        "EXISTS (SELECT 1 FROM equipment_meters em \
         WHERE em.equipment_id = e.id AND em.is_active = 1 \
           AND (em.last_read_at IS NULL \
                OR datetime(em.last_read_at) < datetime('now', '-{STALE_METER_DAYS} days')))"
    )
}

/// SQL predicate (alias `e`) of a strip tile's drill focus: the list opened by a tile counts
/// exactly what the tile counts. `None` for an unknown focus.
pub(crate) fn registry_focus_sql(focus: &str) -> Option<String> {
    match focus {
        "status_down" => Some(STATUS_DOWN_SQL.to_string()),
        "critical_at_risk" => Some(format!(
            "{MOST_CRITICAL_SQL} AND ({STATUS_DOWN_SQL} OR {} OR {})",
            open_di_exists_sql(),
            open_wo_exists_sql()
        )),
        "open_work" => Some(format!("({} OR {})", open_di_exists_sql(), open_wo_exists_sql())),
        "stale_meters" => Some(stale_meter_exists_sql()),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EquipmentRegistryMetricCell {
    pub semantic_key: String,
    pub catalog_id: String,
    pub value: f64,
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
    available: bool,
    value_kind: &str,
    trend_sense: &str,
) -> EquipmentRegistryMetricCell {
    EquipmentRegistryMetricCell {
        semantic_key: key.into(),
        catalog_id: catalog_id.into(),
        value,
        available,
        value_kind: value_kind.into(),
        trend_sense: trend_sense.into(),
        active_count: None,
        down_count: None,
        series: None,
    }
}

/// `COUNT(*) AS cnt`: a missing row or an unreadable count is an error, never 0.
async fn count_sql(db: &DatabaseConnection, sql: &str, binds: Vec<Value>) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("registry metric count returned no row")))?;
    row.try_get::<i64>("", "cnt")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("registry metric count decode: {e}")))
}

async fn count_scoped(db: &DatabaseConnection, where_sql: &str, binds: &[Value], extra_and: &str) -> AppResult<i64> {
    let sql = format!("SELECT COUNT(*) AS cnt {METRICS_FROM} WHERE {where_sql} AND ({extra_and})");
    count_sql(db, &sql, binds.to_vec()).await
}

fn focus(key: &str) -> AppResult<String> {
    registry_focus_sql(key).ok_or_else(|| AppError::Internal(anyhow::anyhow!("registry focus {key} has no definition")))
}

/// Compute registry strip metrics under the same SmartFilterBar scope as `search_assets`.
pub async fn get_equipment_registry_metrics(
    db: &DatabaseConnection,
    filters: AssetSearchFilters,
) -> AppResult<EquipmentRegistryMetricsPayload> {
    let text_matches = text_matches_for(db, &filters).await?;
    let (where_sql, binds) = build_asset_scope_where(&filters, text_matches.as_deref());
    let scoped_total = count_sql(
        db,
        &format!("SELECT COUNT(*) AS cnt {METRICS_FROM} WHERE {where_sql}"),
        binds.clone(),
    )
    .await?;

    // ── EQUIP_STATUS_SUMMARY ──────────────────────────────────────────────
    let active_count = count_scoped(db, &where_sql, &binds, STATUS_UP_SQL).await?;
    let down_count = count_scoped(db, &where_sql, &binds, &focus("status_down")?).await?;
    let mut status_cell = cell(
        kpi_definitions::equipment::EQUIP_STATUS_SUMMARY,
        kpi_definitions::catalog::EQUIP_STATUS_SUMMARY,
        down_count as f64,
        true,
        "count",
        "up-bad",
    );
    status_cell.active_count = Some(active_count);
    status_cell.down_count = Some(down_count);

    // ── EQUIP_CRITICAL_AT_RISK ────────────────────────────────────────────
    let critical_at_risk = count_scoped(db, &where_sql, &binds, &focus("critical_at_risk")?).await?;
    let critical_cell = cell(
        kpi_definitions::equipment::EQUIP_CRITICAL_AT_RISK,
        kpi_definitions::catalog::EQUIP_CRITICAL_AT_RISK,
        critical_at_risk as f64,
        true,
        "count",
        "up-bad",
    );

    // ── EQUIP_OPEN_WORK ───────────────────────────────────────────────────
    let open_work = count_scoped(db, &where_sql, &binds, &focus("open_work")?).await?;
    let mut open_work_cell = cell(
        kpi_definitions::equipment::EQUIP_OPEN_WORK,
        kpi_definitions::catalog::EQUIP_OPEN_WORK,
        open_work as f64,
        true,
        "count",
        "up-bad",
    );
    open_work_cell.series = Some(load_open_work_series(db, &where_sql, &binds).await?);

    // ── EQUIP_STALE_METERS ────────────────────────────────────────────────
    let stale_meters = count_scoped(db, &where_sql, &binds, &focus("stale_meters")?).await?;
    let stale_cell = cell(
        kpi_definitions::equipment::EQUIP_STALE_METERS,
        kpi_definitions::catalog::EQUIP_STALE_METERS,
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

    let mut sum = 0.0;
    let mut n = 0i64;
    for r in rows {
        let id: i64 = r
            .try_get("", "id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("registry health sample id: {e}")))?;
        let AssetHealthScore { score, .. } = health::get_asset_health_score(db, id).await?;
        if let Some(s) = score {
            sum += s as f64;
            n += 1;
        }
    }
    if n == 0 {
        // No asset with a lifecycle history in scope: the tile says "not available", not 0.
        return Ok((0.0, false));
    }
    Ok((sum / n as f64, true))
}

/// Daily number of scoped assets that received a request or a work order over the last 30 days,
/// counting only those still open (same rules as the tile).
async fn load_open_work_series(db: &DatabaseConnection, where_sql: &str, binds: &[Value]) -> AppResult<Vec<f64>> {
    let sql = format!(
        "SELECT d AS day, COUNT(DISTINCT eid) AS cnt FROM (\
           SELECT date(ir.created_at) AS d, ir.equipment_id AS eid \
           FROM intervention_requests ir \
           WHERE ir.created_at >= datetime('now', '-30 days') \
             AND ir.equipment_id IN (SELECT e.id {METRICS_FROM} WHERE {where_sql}) \
             AND {di_open} \
           UNION ALL \
           SELECT date(wo.created_at) AS d, wo.equipment_id AS eid \
           FROM work_orders wo \
           JOIN work_order_statuses s ON s.id = wo.status_id \
           WHERE wo.created_at >= datetime('now', '-30 days') \
             AND wo.equipment_id IN (SELECT e.id {METRICS_FROM} WHERE {where_sql}) \
             AND s.code IN {wo_open} \
         ) GROUP BY d ORDER BY d ASC",
        di_open = crate::di::stats::OPEN_SQL,
        wo_open = crate::wo::backlog::OPEN_STATUS_SQL,
    );
    // The scope sub-select appears twice.
    let mut all_binds = binds.to_vec();
    all_binds.extend(binds.iter().cloned());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, all_binds))
        .await?;

    rows.iter()
        .map(|r| {
            r.try_get::<i64>("", "cnt")
                .map(|c| c as f64)
                .map_err(|e| AppError::Internal(anyhow::anyhow!("registry open-work series decode: {e}")))
        })
        .collect()
}
