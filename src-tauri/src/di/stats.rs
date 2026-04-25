//! DI dashboard statistics aggregation.

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;

#[derive(Debug, Clone, Deserialize)]
pub struct DiStatsFilter {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub entity_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiStatusCount {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiPriorityCount {
    pub priority: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiTypeCount {
    pub origin_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiTrendPoint {
    pub period: String,
    pub created: i64,
    pub closed: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiEquipmentCount {
    pub asset_id: i64,
    pub asset_label: String,
    pub count: i64,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiOverdueDi {
    pub id: i64,
    pub code: String,
    pub title: String,
    pub priority: String,
    pub days_overdue: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiStatsPayload {
    pub total: i64,
    pub pending: i64,
    pub in_progress: i64,
    pub closed: i64,
    pub closed_this_month: i64,
    pub overdue: i64,
    pub sla_met_count: i64,
    pub sla_total: i64,
    pub safety_issues: i64,
    pub status_distribution: Vec<DiStatusCount>,
    pub priority_distribution: Vec<DiPriorityCount>,
    pub type_distribution: Vec<DiTypeCount>,
    pub monthly_trend: Vec<DiTrendPoint>,
    pub available_years: Vec<i32>,
    pub avg_age_days: f64,
    pub max_age_days: f64,
    pub top_equipment: Vec<DiEquipmentCount>,
    pub overdue_dis: Vec<DiOverdueDi>,
}

fn base_scope_sql() -> &'static str {
    " (? IS NULL OR date(ir.submitted_at) >= date(?))
      AND (? IS NULL OR date(ir.submitted_at) <= date(?))
      AND (? IS NULL OR ir.org_node_id = ?) "
}

fn scope_binds(filter: &DiStatsFilter) -> [sea_orm::Value; 6] {
    [
        filter.date_from.clone().into(),
        filter.date_from.clone().into(),
        filter.date_to.clone().into(),
        filter.date_to.clone().into(),
        filter.entity_id.into(),
        filter.entity_id.into(),
    ]
}

fn row_i64(row: &QueryResult, col: &str) -> AppResult<i64> {
    Ok(row.try_get("", col)?)
}

fn row_f64(row: &QueryResult, col: &str) -> AppResult<f64> {
    Ok(row.try_get("", col)?)
}

pub async fn get_di_stats(db: &DatabaseConnection, filter: DiStatsFilter) -> AppResult<DiStatsPayload> {
    let binds = scope_binds(&filter);
    let base_where = base_scope_sql();

    let total = scalar_i64(
        db,
        &format!("SELECT COUNT(*) AS value FROM intervention_requests ir WHERE {base_where}"),
        binds.to_vec(),
    )
    .await?;

    let pending = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where} AND ir.status = 'pending_review'"
        ),
        binds.to_vec(),
    )
    .await?;

    let in_progress = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND ir.status IN (
               'submitted',
               'pending_review',
               'screened',
               'awaiting_approval',
               'approved_for_planning',
               'deferred'
             )"
        ),
        binds.to_vec(),
    )
    .await?;

    let closed = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND ir.status IN ('closed_as_non_executable', 'archived')"
        ),
        binds.to_vec(),
    )
    .await?;

    let closed_this_month = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND ir.closed_at IS NOT NULL
             AND strftime('%Y-%m', ir.closed_at) = strftime('%Y-%m', 'now')"
        ),
        binds.to_vec(),
    )
    .await?;

    let safety_issues = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where} AND ir.safety_flag = 1"
        ),
        binds.to_vec(),
    )
    .await?;

    let overdue = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND ir.status IN ('submitted', 'pending_review', 'screened', 'awaiting_approval')
             AND (julianday('now') - julianday(ir.submitted_at)) > 7"
        ),
        binds.to_vec(),
    )
    .await?;

    let status_distribution = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT ir.status AS status, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY ir.status
                     ORDER BY count DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiStatusCount {
                    status: row.try_get("", "status")?,
                    count: row_i64(&row, "count")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let priority_distribution = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT ir.reported_urgency AS priority, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY ir.reported_urgency
                     ORDER BY count DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiPriorityCount {
                    priority: row.try_get("", "priority")?,
                    count: row_i64(&row, "count")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let type_distribution = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT ir.origin_type AS origin_type, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY ir.origin_type
                     ORDER BY count DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiTypeCount {
                    origin_type: row.try_get("", "origin_type")?,
                    count: row_i64(&row, "count")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let monthly_trend = {
        let created_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT strftime('%Y-%m', ir.submitted_at) AS period, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY period
                     ORDER BY period ASC"
                ),
                binds.to_vec(),
            ))
            .await?;
        let closed_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT strftime('%Y-%m', ir.closed_at) AS period, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     AND ir.closed_at IS NOT NULL
                     GROUP BY period
                     ORDER BY period ASC"
                ),
                binds.to_vec(),
            ))
            .await?;

        let mut trend_map: BTreeMap<String, DiTrendPoint> = BTreeMap::new();
        for row in created_rows {
            let period: String = row.try_get("", "period")?;
            let created_count = row_i64(&row, "count")?;
            trend_map
                .entry(period.clone())
                .and_modify(|p| p.created = created_count)
                .or_insert(DiTrendPoint {
                    period,
                    created: created_count,
                    closed: 0,
                });
        }
        for row in closed_rows {
            let period: String = row.try_get("", "period")?;
            let closed_count = row_i64(&row, "count")?;
            trend_map
                .entry(period.clone())
                .and_modify(|p| p.closed = closed_count)
                .or_insert(DiTrendPoint {
                    period,
                    created: 0,
                    closed: closed_count,
                });
        }
        trend_map.into_values().collect::<Vec<_>>()
    };

    let available_years = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT DISTINCT CAST(strftime('%Y', ir.submitted_at) AS INTEGER) AS year
                     FROM intervention_requests ir
                     WHERE {base_where}
                     ORDER BY year DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| Ok(row.try_get::<i32>("", "year")?))
            .collect::<AppResult<Vec<_>>>()?
    };

    let (avg_age_days, max_age_days) = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT
                       COALESCE(AVG(julianday('now') - julianday(ir.submitted_at)), 0) AS avg_age_days,
                       COALESCE(MAX(julianday('now') - julianday(ir.submitted_at)), 0) AS max_age_days
                     FROM intervention_requests ir
                     WHERE {base_where}
                     AND ir.status IN (
                       'submitted',
                       'pending_review',
                       'screened',
                       'awaiting_approval',
                       'approved_for_planning',
                       'deferred'
                     )"
                ),
                binds.to_vec(),
            ))
            .await?;
        if let Some(row) = row {
            (row_f64(&row, "avg_age_days")?, row_f64(&row, "max_age_days")?)
        } else {
            (0.0, 0.0)
        }
    };

    let top_equipment = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT
                       ir.asset_id AS asset_id,
                       COALESCE(e.name, ('Asset #' || ir.asset_id)) AS asset_label,
                       COUNT(*) AS count
                     FROM intervention_requests ir
                     LEFT JOIN equipment e ON e.id = ir.asset_id
                     WHERE {base_where}
                     GROUP BY ir.asset_id, asset_label
                     ORDER BY count DESC, asset_label ASC
                     LIMIT 10"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                let count = row_i64(&row, "count")?;
                let percentage = if total > 0 {
                    (count as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                Ok(DiEquipmentCount {
                    asset_id: row.try_get("", "asset_id")?,
                    asset_label: row.try_get("", "asset_label")?,
                    count,
                    percentage,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let overdue_dis = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT
                       ir.id,
                       ir.code,
                       ir.title,
                       ir.reported_urgency AS priority,
                       CAST(julianday('now') - julianday(ir.submitted_at) AS INTEGER) AS days_overdue
                     FROM intervention_requests ir
                     WHERE {base_where}
                     AND ir.status IN ('submitted', 'pending_review', 'screened', 'awaiting_approval')
                     AND (julianday('now') - julianday(ir.submitted_at)) > 7
                     ORDER BY days_overdue DESC, ir.id DESC
                     LIMIT 10"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiOverdueDi {
                    id: row.try_get("", "id")?,
                    code: row.try_get("", "code")?,
                    title: row.try_get("", "title")?,
                    priority: row.try_get("", "priority")?,
                    days_overdue: row_i64(&row, "days_overdue")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    Ok(DiStatsPayload {
        total,
        pending,
        in_progress,
        closed,
        closed_this_month,
        overdue,
        // Keep explicit zeroes until SLA aggregation is fully reinstated.
        sla_met_count: 0,
        sla_total: 0,
        safety_issues,
        status_distribution,
        priority_distribution,
        type_distribution,
        monthly_trend,
        available_years,
        avg_age_days,
        max_age_days,
        top_equipment,
        overdue_dis,
    })
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str, binds: Vec<sea_orm::Value>) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql.to_string(),
            binds,
        ))
        .await?;
    if let Some(row) = row {
        row_i64(&row, "value")
    } else {
        Ok(0)
    }
}
