//! Asset maintenance profile: what maintaining one asset has cost and how often it failed.
//!
//! Benchmark: IBM Maximo asset "Costs" roll-up and SAP PM equipment cost analysis (IW39 / PM
//! information system). Figures come from the asset's work orders; cancelled work orders are
//! ignored. With `include_descendants` the figures roll up the asset and every component below
//! it in the equipment hierarchy (active relations only).
//!
//! Each work order's cost is read through [`crate::wo::costs::get_cost_summary`], so an open work
//! order counts its live labour and parts exactly as its own cost card shows them.

use std::collections::BTreeMap;

use chrono::{DateTime, Months, Utc};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

// Failure work = corrective natures, planned maintenance = preventive natures (EN 13306 nature of
// the OT type, `wo::types::MaintenanceNature`).
/// Window of the "recent" cost figure, in months.
pub const RECENT_COST_WINDOW_MONTHS: u32 = 12;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AssetCostBreakdown {
    pub labor: f64,
    pub parts: f64,
    pub services: f64,
    pub total: f64,
}

impl AssetCostBreakdown {
    fn add(&mut self, labor: f64, parts: f64, services: f64, total: f64) {
        self.labor += labor;
        self.parts += parts;
        self.services += services;
        self.total += total;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetYearCost {
    pub year: i32,
    pub total: f64,
    pub work_order_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPmPlanLink {
    pub id: i64,
    pub code: String,
    pub title: String,
    pub lifecycle_status: String,
    /// Earliest open occurrence (not completed, cancelled or missed).
    pub next_due_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMaintenanceProfile {
    pub asset_id: i64,
    pub include_descendants: bool,
    /// Assets covered (1 without roll-up).
    pub asset_count: i64,
    pub work_order_count: i64,
    /// OTs not yet closed (`wo::backlog::NOT_CLOSED_SQL`), the rule named by `open_work_order_rule`.
    pub open_work_order_count: i64,
    /// `wo::backlog::RULE_NOT_YET_CLOSED`.
    pub open_work_order_rule: String,
    pub failure_work_order_count: i64,
    pub planned_work_order_count: i64,
    pub downtime_hours: f64,
    pub last_failure_at: Option<String>,
    pub cost_lifetime: AssetCostBreakdown,
    pub cost_recent: AssetCostBreakdown,
    pub recent_window_months: u32,
    pub cost_by_year: Vec<AssetYearCost>,
    pub replacement_value: Option<f64>,
    /// Lifetime maintenance cost / replacement value (Maximo "cost vs. replacement" indicator).
    pub cost_to_replacement_ratio: Option<f64>,
    pub pm_plans: Vec<AssetPmPlanLink>,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "asset maintenance profile decode failed for column '{column}': {e}"
    ))
}

fn get<T: sea_orm::TryGetable>(row: &QueryResult, column: &str) -> AppResult<T> {
    row.try_get::<T>("", column).map_err(|e| decode_err(column, e))
}

/// The asset itself and, when asked, every asset below it through active hierarchy links.
async fn scope_asset_ids(db: &impl ConnectionTrait, asset_id: i64, include_descendants: bool) -> AppResult<Vec<i64>> {
    if !include_descendants {
        return Ok(vec![asset_id]);
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE tree(id, depth) AS ( \
                SELECT ?, 0 \
                UNION \
                SELECT h.child_equipment_id, t.depth + 1 FROM equipment_hierarchy h \
                JOIN tree t ON h.parent_equipment_id = t.id \
                WHERE h.effective_to IS NULL AND t.depth < 32 \
             ) \
             SELECT DISTINCT t.id FROM tree t JOIN equipment e ON e.id = t.id WHERE e.deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?;
    rows.iter().map(|r| get::<i64>(r, "id")).collect()
}

/// Builds the maintenance profile of one asset as of `now`.
pub async fn get_asset_maintenance_profile(
    db: &impl ConnectionTrait,
    asset_id: i64,
    include_descendants: bool,
    now: DateTime<Utc>,
) -> AppResult<AssetMaintenanceProfile> {
    let asset = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT replacement_value FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    let replacement_value: Option<f64> = get(&asset, "replacement_value")?;

    let ids = scope_asset_ids(db, asset_id, include_descendants).await?;
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let binds: Vec<sea_orm::Value> = ids.iter().map(|&id| id.into()).collect();

    let wo_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id, t.semantic_tag AS nature_code, s.code AS status_code, \
                    CASE WHEN {not_closed} THEN 1 ELSE 0 END AS not_closed, \
                    COALESCE(wo.downtime_hours, 0.0) AS downtime_hours, \
                    COALESCE(wo.actual_start, wo.created_at) AS failure_at, \
                    COALESCE(wo.closed_at, wo.actual_end, wo.actual_start, wo.created_at) AS cost_at \
                 FROM work_orders wo \
                 LEFT JOIN reference_values t ON t.id = wo.type_id \
                 JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE wo.equipment_id IN ({placeholders}) AND s.code <> 'cancelled'",
                not_closed = crate::wo::backlog::NOT_CLOSED_SQL
            ),
            binds,
        ))
        .await?;

    let recent_from = now
        .checked_sub_months(Months::new(RECENT_COST_WINDOW_MONTHS))
        .unwrap_or(now)
        .to_rfc3339();

    let mut profile = AssetMaintenanceProfile {
        asset_id,
        include_descendants,
        asset_count: i64::try_from(ids.len()).unwrap_or(i64::MAX),
        work_order_count: 0,
        open_work_order_count: 0,
        open_work_order_rule: crate::wo::backlog::RULE_NOT_YET_CLOSED.to_string(),
        failure_work_order_count: 0,
        planned_work_order_count: 0,
        downtime_hours: 0.0,
        last_failure_at: None,
        cost_lifetime: AssetCostBreakdown::default(),
        cost_recent: AssetCostBreakdown::default(),
        recent_window_months: RECENT_COST_WINDOW_MONTHS,
        cost_by_year: Vec::new(),
        replacement_value,
        cost_to_replacement_ratio: None,
        pm_plans: Vec::new(),
    };
    let mut by_year: BTreeMap<i32, AssetYearCost> = BTreeMap::new();

    for row in &wo_rows {
        let wo_id: i64 = get(row, "id")?;
        let nature_code: Option<String> = get(row, "nature_code")?;
        let nature = nature_code
            .as_deref()
            .and_then(crate::wo::types::MaintenanceNature::parse);
        let not_closed: i64 = get(row, "not_closed")?;
        let cost_at: String = get(row, "cost_at")?;
        profile.work_order_count += 1;
        if not_closed == 1 {
            profile.open_work_order_count += 1;
        }
        if nature.is_some_and(|n| n.is_corrective()) {
            profile.failure_work_order_count += 1;
            let failure_at: String = get(row, "failure_at")?;
            if profile
                .last_failure_at
                .as_deref()
                .is_none_or(|last| failure_at.as_str() > last)
            {
                profile.last_failure_at = Some(failure_at);
            }
        } else if nature.is_some_and(|n| n.is_preventive()) {
            profile.planned_work_order_count += 1;
        }
        profile.downtime_hours += get::<f64>(row, "downtime_hours")?;

        let cost = crate::wo::costs::get_cost_summary(db, wo_id).await?;
        profile
            .cost_lifetime
            .add(cost.labor_cost, cost.parts_cost, cost.service_cost, cost.total_cost);
        if cost_at.as_str() >= recent_from.as_str() {
            profile
                .cost_recent
                .add(cost.labor_cost, cost.parts_cost, cost.service_cost, cost.total_cost);
        }
        if let Some(year) = cost_at.get(0..4).and_then(|y| y.parse::<i32>().ok()) {
            let entry = by_year.entry(year).or_insert(AssetYearCost {
                year,
                total: 0.0,
                work_order_count: 0,
            });
            entry.total += cost.total_cost;
            entry.work_order_count += 1;
        }
    }
    profile.cost_by_year = by_year.into_values().collect();
    profile.cost_to_replacement_ratio = replacement_value
        .filter(|v| *v > 0.0)
        .map(|v| profile.cost_lifetime.total / v);

    let plan_binds: Vec<sea_orm::Value> = ids.iter().map(|&id| id.into()).collect();
    let plan_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT p.id, p.code, p.title, p.lifecycle_status, \
                    (SELECT MIN(o.due_at) FROM pm_occurrences o \
                      WHERE o.pm_plan_id = p.id AND o.status NOT IN ('completed','cancelled','missed')) AS next_due_at \
                 FROM pm_plans p \
                 WHERE p.asset_scope_type = 'equipment' AND p.asset_scope_id IN ({placeholders}) \
                 ORDER BY p.code ASC"
            ),
            plan_binds,
        ))
        .await?;
    profile.pm_plans = plan_rows
        .iter()
        .map(|r| {
            Ok(AssetPmPlanLink {
                id: get(r, "id")?,
                code: get(r, "code")?,
                title: get(r, "title")?,
                lifecycle_status: get(r, "lifecycle_status")?,
                next_due_at: get(r, "next_due_at")?,
            })
        })
        .collect::<AppResult<_>>()?;

    Ok(profile)
}
