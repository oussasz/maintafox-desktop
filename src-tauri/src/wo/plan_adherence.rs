//! Plan-vs-actual adherence metrics for a work order.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoPlanAdherence {
    pub wo_id: i64,
    pub parts_planned: i64,
    pub parts_used: i64,
    pub parts_unused: i64,
    pub parts_extra: i64,
    pub tasks_planned: i64,
    pub tasks_done: i64,
    pub tasks_added: i64,
    pub tasks_cancelled: i64,
    pub planned_hours: Option<f64>,
    pub actual_hours: Option<f64>,
    pub time_variance_hours: Option<f64>,
    /// planned / actual × 100 when actual > 0.
    pub efficiency_pct: Option<f64>,
    pub planned_parts_cost: f64,
    pub actual_parts_cost: f64,
    pub planned_labor_cost: f64,
    pub actual_labor_cost: f64,
    pub cost_variance_pct: Option<f64>,
    pub planned_downtime_hours: Option<f64>,
    pub actual_downtime_hours: f64,
    pub downtime_variance_hours: Option<f64>,
    pub primary_downtime_cause: Option<String>,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("WO plan adherence decode failed for '{column}': {e}"))
}

pub async fn get_plan_adherence(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoPlanAdherence> {
    let wo = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT expected_duration_hours, actual_duration_hours, \
                    planned_downtime_hours, \
                    COALESCE(labor_cost, 0) AS labor_cost, \
                    COALESCE(parts_cost, 0) AS parts_cost, \
                    COALESCE(active_labor_hours, 0) AS active_labor_hours, \
                    COALESCE(downtime_hours, 0) AS downtime_hours, \
                    COALESCE(total_waiting_hours, 0) AS waiting_hours \
               FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    let planned_hours: Option<f64> = wo
        .try_get("", "expected_duration_hours")
        .map_err(|e| decode_err("expected_duration_hours", e))?;
    let mut actual_hours: Option<f64> = wo
        .try_get("", "actual_duration_hours")
        .map_err(|e| decode_err("actual_duration_hours", e))?;
    let active_labor: f64 = wo
        .try_get("", "active_labor_hours")
        .map_err(|e| decode_err("active_labor_hours", e))?;
    if actual_hours.is_none() && active_labor > 0.0 {
        actual_hours = Some(active_labor);
    }
    // Prefer sum of labor entries when WO rollup empty
    if actual_hours.unwrap_or(0.0) <= 0.0 {
        if let Some(sum_row) = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(SUM(hours_worked), 0.0) AS h FROM work_order_interveners WHERE work_order_id = ?",
                [wo_id.into()],
            ))
            .await?
        {
            let h: f64 = sum_row.try_get("", "h").unwrap_or(0.0);
            if h > 0.0 {
                actual_hours = Some(h);
            }
        }
    }

    let planned_downtime: Option<f64> = wo
        .try_get("", "planned_downtime_hours")
        .map_err(|e| decode_err("planned_downtime_hours", e))?;
    let downtime_hours: f64 = wo
        .try_get("", "downtime_hours")
        .map_err(|e| decode_err("downtime_hours", e))?;
    let waiting_hours: f64 = wo
        .try_get("", "waiting_hours")
        .map_err(|e| decode_err("waiting_hours", e))?;
    let actual_downtime = downtime_hours + waiting_hours;

    let actual_labor_cost: f64 = wo.try_get("", "labor_cost").map_err(|e| decode_err("labor_cost", e))?;
    let actual_parts_cost: f64 = wo.try_get("", "parts_cost").map_err(|e| decode_err("parts_cost", e))?;

    let parts = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                SUM(CASE WHEN origin = 'planned' THEN 1 ELSE 0 END) AS planned, \
                SUM(CASE WHEN consumption_status = 'used' THEN 1 ELSE 0 END) AS used, \
                SUM(CASE WHEN consumption_status = 'not_used' THEN 1 ELSE 0 END) AS unused, \
                SUM(CASE WHEN origin = 'execution_added' THEN 1 ELSE 0 END) AS extra, \
                COALESCE(SUM(CASE WHEN origin = 'planned' \
                  THEN quantity_planned * COALESCE(unit_cost, 0.0) ELSE 0.0 END), 0.0) AS planned_cost, \
                COALESCE(SUM(CASE WHEN consumption_status = 'used' \
                  THEN COALESCE(quantity_used, 0.0) * COALESCE(unit_cost, 0.0) ELSE 0.0 END), 0.0) AS used_cost \
             FROM work_order_parts WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await?;

    let (parts_planned, parts_used, parts_unused, parts_extra, planned_parts_cost, used_parts_cost) =
        if let Some(p) = parts {
            (
                p.try_get::<Option<i64>>("", "planned").ok().flatten().unwrap_or(0),
                p.try_get::<Option<i64>>("", "used").ok().flatten().unwrap_or(0),
                p.try_get::<Option<i64>>("", "unused").ok().flatten().unwrap_or(0),
                p.try_get::<Option<i64>>("", "extra").ok().flatten().unwrap_or(0),
                p.try_get::<f64>("", "planned_cost").unwrap_or(0.0),
                p.try_get::<f64>("", "used_cost").unwrap_or(0.0),
            )
        } else {
            (0, 0, 0, 0, 0.0, 0.0)
        };

    let tasks = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                SUM(CASE WHEN origin = 'planned' THEN 1 ELSE 0 END) AS planned, \
                SUM(CASE WHEN is_completed = 1 AND COALESCE(result_code, '') != 'cancelled' THEN 1 ELSE 0 END) AS done, \
                SUM(CASE WHEN origin = 'execution_added' THEN 1 ELSE 0 END) AS added, \
                SUM(CASE WHEN COALESCE(result_code, '') = 'cancelled' THEN 1 ELSE 0 END) AS cancelled \
             FROM work_order_tasks WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await?;

    let (tasks_planned, tasks_done, tasks_added, tasks_cancelled) = if let Some(t) = tasks {
        (
            t.try_get::<Option<i64>>("", "planned").ok().flatten().unwrap_or(0),
            t.try_get::<Option<i64>>("", "done").ok().flatten().unwrap_or(0),
            t.try_get::<Option<i64>>("", "added").ok().flatten().unwrap_or(0),
            t.try_get::<Option<i64>>("", "cancelled").ok().flatten().unwrap_or(0),
        )
    } else {
        (0, 0, 0, 0)
    };

    let time_variance = match (planned_hours, actual_hours) {
        (Some(p), Some(a)) => Some(a - p),
        _ => None,
    };
    let efficiency_pct = match (planned_hours, actual_hours) {
        (Some(p), Some(a)) if a > 0.0 => Some((p / a) * 100.0),
        _ => None,
    };

    let planned_labor_cost = match planned_hours {
        Some(h) if h > 0.0 && actual_hours.unwrap_or(0.0) > 0.0 && actual_labor_cost > 0.0 => {
            actual_labor_cost * (h / actual_hours.unwrap_or(h))
        }
        Some(h) => h * 0.0, // no rate known → 0 planned labor money
        None => 0.0,
    };

    let planned_total = planned_parts_cost + planned_labor_cost;
    let actual_total = if actual_parts_cost > 0.0 || actual_labor_cost > 0.0 {
        used_parts_cost.max(actual_parts_cost) + actual_labor_cost
    } else {
        used_parts_cost + actual_labor_cost
    };
    let cost_variance_pct = if planned_total > 0.0 {
        Some(((actual_total - planned_total) / planned_total) * 100.0)
    } else {
        None
    };

    let downtime_variance = planned_downtime.map(|p| actual_downtime - p);

    let cause_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(rv.label, ds.comment, wds.comment) AS cause \
               FROM work_order_delay_segments ds \
               LEFT JOIN reference_values rv ON rv.id = ds.delay_reason_id \
               LEFT JOIN work_order_downtime_segments wds ON wds.work_order_id = ds.work_order_id \
              WHERE ds.work_order_id = ? \
              ORDER BY ds.started_at DESC LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    let primary_cause = cause_row.and_then(|r| r.try_get::<Option<String>>("", "cause").ok().flatten());

    Ok(WoPlanAdherence {
        wo_id,
        parts_planned,
        parts_used,
        parts_unused,
        parts_extra,
        tasks_planned,
        tasks_done,
        tasks_added,
        tasks_cancelled,
        planned_hours,
        actual_hours,
        time_variance_hours: time_variance,
        efficiency_pct,
        planned_parts_cost,
        actual_parts_cost: used_parts_cost.max(actual_parts_cost),
        planned_labor_cost,
        actual_labor_cost,
        cost_variance_pct,
        planned_downtime_hours: planned_downtime,
        actual_downtime_hours: actual_downtime,
        downtime_variance_hours: downtime_variance,
        primary_downtime_cause: primary_cause,
    })
}
