//! Backlog heatmap (PRD §6.5): planned workload per day against crew capacity, plus the work
//! that is not on the calendar (unscheduled, overdue) and the work that is blocked, by delay
//! reason category.
//!
//! - Planned load of a day: open work orders whose `planned_start` falls on that day, summed on
//!   `expected_duration_hours`. Orders without an estimate are counted separately (never as 0 h).
//! - Capacity of a day: for every active crew, the hours its members' Classes horaires give for
//!   work orders that day plus the planner's overtime ceiling (`personnel::capacity`, the same
//!   figure planning books against). Without any active crew the capacity is unknown (`None`)
//!   and no day is called overloaded.
//! - Blocked work: orders on hold, grouped by the semantic tag of their open delay reason.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Statuses whose work is still ahead (not finished, not abandoned).
const OPEN_STATUS_SQL: &str = "('draft', 'planning', 'ready', 'in_progress', 'on_hold')";
pub const MAX_WEEKS: i64 = 12;

#[derive(Debug, Clone, Deserialize)]
pub struct WoBacklogHeatmapInput {
    /// First day (`YYYY-MM-DD`); today when omitted.
    #[serde(default)]
    pub start_date: Option<String>,
    /// Number of weeks shown (1..=12).
    pub weeks: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BacklogDay {
    pub date: String,
    pub planned_hours: f64,
    pub wo_count: i64,
    /// Orders planned that day without an estimated duration.
    pub unestimated_count: i64,
    pub capacity_hours: Option<f64>,
    pub overloaded: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BacklogBucket {
    pub wo_count: i64,
    pub estimated_hours: f64,
    pub unestimated_count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BlockedCategory {
    /// Delay reason semantic tag (`parts`, `permit`, `shutdown`, …); `unknown` when the hold has
    /// no open delay segment.
    pub category: String,
    pub wo_count: i64,
    /// Hours on hold so far, summed over the orders.
    pub waiting_hours: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WoBacklogHeatmap {
    pub start_date: String,
    pub days: Vec<BacklogDay>,
    pub capacity_configured: bool,
    pub unscheduled: BacklogBucket,
    pub overdue: BacklogBucket,
    pub blocked: Vec<BlockedCategory>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("backlog heatmap decode '{field}': {err}"))
}

async fn bucket(
    db: &impl ConnectionTrait,
    where_clause: &str,
    values: Vec<sea_orm::Value>,
) -> AppResult<BacklogBucket> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS n, COALESCE(SUM(wo.expected_duration_hours), 0.0) AS h, \
                 SUM(CASE WHEN wo.expected_duration_hours IS NULL THEN 1 ELSE 0 END) AS u \
                 FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE s.code IN {OPEN_STATUS_SQL} AND {where_clause}"
            ),
            values,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("backlog bucket")))?;
    Ok(BacklogBucket {
        wo_count: row.try_get("", "n").map_err(|e| decode_err("n", e))?,
        estimated_hours: row.try_get("", "h").map_err(|e| decode_err("h", e))?,
        unestimated_count: row
            .try_get::<Option<i64>>("", "u")
            .map_err(|e| decode_err("u", e))?
            .unwrap_or(0),
    })
}

/// Capacity of each day: the sum over the active crews of their capacity (`personnel::capacity`).
async fn capacity_by_day(db: &impl ConnectionTrait, days: &[NaiveDate]) -> AppResult<Option<BTreeMap<String, f64>>> {
    let (Some(first), Some(last)) = (days.iter().min(), days.iter().max()) else {
        return Ok(None);
    };
    let teams: Vec<i64> = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM teams WHERE status = 'active' AND deleted_at IS NULL ORDER BY id".to_string(),
        ))
        .await?
        .iter()
        .map(|r| r.try_get::<i64>("", "id").map_err(|e| decode_err("id", e)))
        .collect::<AppResult<_>>()?;
    if teams.is_empty() {
        return Ok(None);
    }
    let mut out: BTreeMap<String, f64> = days.iter().map(|d| (d.format("%Y-%m-%d").to_string(), 0.0)).collect();
    for team in teams {
        for day in crate::personnel::capacity::team_capacity_span(db, team, *first, *last).await? {
            if let Some(total) = out.get_mut(&day.day) {
                *total += day.available_hours + day.rule_overtime_hours.unwrap_or(0.0);
            }
        }
    }
    Ok(Some(out))
}

pub async fn get_wo_backlog_heatmap(
    db: &impl ConnectionTrait,
    input: WoBacklogHeatmapInput,
) -> AppResult<WoBacklogHeatmap> {
    if !(1..=MAX_WEEKS).contains(&input.weeks) {
        return Err(AppError::ValidationFailed(vec![format!(
            "weeks must be between 1 and {MAX_WEEKS}."
        )]));
    }
    let start = match input.start_date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .map_err(|_| AppError::ValidationFailed(vec!["start_date must be YYYY-MM-DD.".into()]))?,
        None => Utc::now().date_naive(),
    };
    let days: Vec<NaiveDate> = (0..input.weeks * 7).map(|i| start + Duration::days(i)).collect();
    let end = start + Duration::days(input.weeks * 7);

    let planned = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT date(wo.planned_start) AS d, COUNT(*) AS n, \
                 COALESCE(SUM(wo.expected_duration_hours), 0.0) AS h, \
                 SUM(CASE WHEN wo.expected_duration_hours IS NULL THEN 1 ELSE 0 END) AS u \
                 FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE s.code IN {OPEN_STATUS_SQL} AND wo.planned_start IS NOT NULL \
                   AND date(wo.planned_start) >= ? AND date(wo.planned_start) < ? \
                 GROUP BY date(wo.planned_start)"
            ),
            [
                start.format("%Y-%m-%d").to_string().into(),
                end.format("%Y-%m-%d").to_string().into(),
            ],
        ))
        .await?;
    let mut by_day: BTreeMap<String, (i64, f64, i64)> = BTreeMap::new();
    for r in &planned {
        let d: String = r.try_get("", "d").map_err(|e| decode_err("d", e))?;
        let n: i64 = r.try_get("", "n").map_err(|e| decode_err("n", e))?;
        let h: f64 = r.try_get("", "h").map_err(|e| decode_err("h", e))?;
        let u: Option<i64> = r.try_get("", "u").map_err(|e| decode_err("u", e))?;
        by_day.insert(d, (n, h, u.unwrap_or(0)));
    }
    let capacity = capacity_by_day(db, &days).await?;
    let out_days = days
        .iter()
        .map(|d| {
            let key = d.format("%Y-%m-%d").to_string();
            let (n, h, u) = by_day.get(&key).copied().unwrap_or((0, 0.0, 0));
            let cap = capacity.as_ref().and_then(|c| c.get(&key).copied());
            BacklogDay {
                overloaded: cap.is_some_and(|c| h > c),
                date: key,
                planned_hours: h,
                wo_count: n,
                unestimated_count: u,
                capacity_hours: cap,
            }
        })
        .collect();

    let now = Utc::now().to_rfc3339();
    let unscheduled = bucket(
        db,
        "wo.planned_start IS NULL AND s.code IN ('draft', 'planning', 'ready')",
        vec![],
    )
    .await?;
    let overdue = bucket(
        db,
        "wo.planned_end IS NOT NULL AND wo.planned_end < ?",
        vec![now.clone().into()],
    )
    .await?;

    let blocked_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(rv.semantic_tag, 'unknown') AS category, COUNT(DISTINCT wo.id) AS n, \
             COALESCE(SUM((julianday(?) - julianday(ds.started_at)) * 24.0), 0.0) AS h \
             FROM work_orders wo \
             JOIN work_order_statuses s ON s.id = wo.status_id \
             LEFT JOIN work_order_delay_segments ds ON ds.work_order_id = wo.id AND ds.ended_at IS NULL \
             LEFT JOIN reference_values rv ON rv.id = ds.delay_reason_id \
             WHERE s.code = 'on_hold' \
             GROUP BY COALESCE(rv.semantic_tag, 'unknown') \
             ORDER BY n DESC, category",
            [now.into()],
        ))
        .await?;
    let mut blocked = Vec::with_capacity(blocked_rows.len());
    for r in &blocked_rows {
        blocked.push(BlockedCategory {
            category: r.try_get("", "category").map_err(|e| decode_err("category", e))?,
            wo_count: r.try_get("", "n").map_err(|e| decode_err("n", e))?,
            waiting_hours: r.try_get("", "h").map_err(|e| decode_err("h", e))?,
        });
    }

    Ok(WoBacklogHeatmap {
        start_date: start.format("%Y-%m-%d").to_string(),
        days: out_days,
        capacity_configured: capacity.is_some(),
        unscheduled,
        overdue,
        blocked,
    })
}
