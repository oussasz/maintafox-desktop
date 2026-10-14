//! Vue d'ensemble Overview KPI values — SSOT in Rust (`get_overview_kpis`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::analytics::as_of::{self, PmDueWindow};
use crate::analytics::smrp;
use crate::commands::dashboard::kpi_quality_badge;
use crate::errors::AppResult;
use crate::inventory::queries::evaluate_reorder;
use crate::kpi_definitions;
use crate::reliability::fleet::{pooled_fleet_kpis, FleetWindow};
use crate::reliability::readiness::DQ_GREEN_THRESHOLD;

const OVERVIEW_HORIZON_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize)]
pub struct OverviewKpiCell {
    pub semantic_key: String,
    pub catalog_id: String,
    pub value: f64,
    pub previous_value: f64,
    pub available: bool,
    pub quality_badge: Option<String>,
    /// `false` when no record history lets the measure be rebuilt at the start of the window
    /// (`previous_value` then repeats `value` and no trend must be shown).
    pub baseline_available: bool,
    pub value_kind: String,
    pub trend_sense: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewKpisPayload {
    pub horizon_days: i64,
    pub cells: Vec<OverviewKpiCell>,
}

struct PermFlags {
    can_di: bool,
    can_ot: bool,
    can_eq: bool,
    can_pm: bool,
    can_rep: bool,
    can_inv: bool,
    can_fin: bool,
}

async fn count_scalar(db: &DatabaseConnection, sql: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
        .await?;
    Ok(row
        .and_then(|r| sea_orm::TryGetable::try_get_by(&r, "cnt").ok())
        .unwrap_or(0))
}

async fn sum_scalar(db: &DatabaseConnection, sql: &str) -> AppResult<f64> {
    use sea_orm::TryGetable;
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
        .await?;
    Ok(row.map(|r| f64::try_get(&r, "", "total").unwrap_or(0.0)).unwrap_or(0.0))
}

fn count_cell(
    key: &str,
    catalog_id: &str,
    value: i64,
    previous: i64,
    available: bool,
    trend_sense: &str,
) -> OverviewKpiCell {
    OverviewKpiCell {
        semantic_key: key.into(),
        catalog_id: catalog_id.into(),
        value: value as f64,
        previous_value: previous as f64,
        available,
        quality_badge: if available {
            kpi_quality_badge(previous, value)
        } else {
            None
        },
        baseline_available: available,
        value_kind: "count".into(),
        trend_sense: trend_sense.into(),
        series: None,
    }
}

fn float_cell(
    key: &str,
    catalog_id: &str,
    value: f64,
    previous: f64,
    available: bool,
    value_kind: &str,
    trend_sense: &str,
) -> OverviewKpiCell {
    OverviewKpiCell {
        semantic_key: key.into(),
        catalog_id: catalog_id.into(),
        value,
        previous_value: previous,
        available,
        quality_badge: if available {
            kpi_quality_badge(previous.round() as i64, value.round() as i64)
        } else {
            None
        },
        baseline_available: available,
        value_kind: value_kind.into(),
        trend_sense: trend_sense.into(),
        series: None,
    }
}

fn without_baseline(mut cell: OverviewKpiCell) -> OverviewKpiCell {
    cell.baseline_available = false;
    cell.quality_badge = None;
    cell
}

pub async fn build_overview_kpis(
    db: &DatabaseConnection,
    can_di: bool,
    can_ot: bool,
    can_eq: bool,
    can_pm: bool,
    can_rep: bool,
    can_inv: bool,
    can_fin: bool,
) -> AppResult<OverviewKpisPayload> {
    let perms = PermFlags {
        can_di,
        can_ot,
        can_eq,
        can_pm,
        can_rep,
        can_inv,
        can_fin,
    };
    let mut cells = Vec::new();

    // Point-in-time measures: now vs the same measure at the start of the horizon (`as_of`).
    let now = chrono::Utc::now();
    let start = as_of::days_before(now, OVERVIEW_HORIZON_DAYS);

    let (di_now, di_prev) = if perms.can_di {
        (
            as_of::di_open_at(db, now, None, None).await?,
            as_of::di_open_at(db, start, None, None).await?,
        )
    } else {
        (0, 0)
    };
    cells.push(count_cell(
        kpi_definitions::overview::DI_OPEN,
        kpi_definitions::catalog::DI_OPEN,
        di_now,
        di_prev,
        perms.can_di,
        "up-bad",
    ));

    let (ot_ip_now, ot_ip_prev) = if perms.can_ot {
        (
            as_of::wo_in_progress_at(db, now, None).await?,
            as_of::wo_in_progress_at(db, start, None).await?,
        )
    } else {
        (0, 0)
    };
    cells.push(count_cell(
        kpi_definitions::overview::OT_IN_PROGRESS,
        kpi_definitions::catalog::OT_IN_PROGRESS,
        ot_ip_now,
        ot_ip_prev,
        perms.can_ot,
        "up-bad",
    ));

    let (ot_od_now, ot_od_prev) = if perms.can_ot {
        (
            as_of::wo_overdue_at(db, now, None).await?,
            as_of::wo_overdue_at(db, start, None).await?,
        )
    } else {
        (0, 0)
    };
    cells.push(count_cell(
        kpi_definitions::overview::OT_OVERDUE,
        kpi_definitions::catalog::OT_OVERDUE,
        ot_od_now,
        ot_od_prev,
        perms.can_ot,
        "up-bad",
    ));

    // PM compliance (SMRP): occurrences done within their tolerance window ÷ occurrences due whose
    // outcome is known, last 30 days vs the 30 days before — `pm::compliance` through
    // `analytics::smrp::pm_compliance`, the same figure as the PM page.
    let (pm_comp, pm_comp_prev) = if perms.can_pm {
        let window = smrp::SmrpWindow::last_days(now, OVERVIEW_HORIZON_DAYS);
        (
            smrp::pm_compliance(db, window, now, None).await?,
            smrp::pm_compliance(db, window.previous(), now, None).await?,
        )
    } else {
        (smrp::SmrpRatio::of(0.0, 0.0), smrp::SmrpRatio::of(0.0, 0.0))
    };
    let mut pm_cell = float_cell(
        kpi_definitions::overview::PM_COMPLIANCE,
        kpi_definitions::catalog::PM_ADHERENCE,
        pm_comp.value_pct.unwrap_or(0.0),
        pm_comp_prev.value_pct.unwrap_or(0.0),
        perms.can_pm && pm_comp.value_pct.is_some(),
        "percent",
        "up-good",
    );
    pm_cell.baseline_available = pm_cell.available && pm_comp_prev.value_pct.is_some();
    if !pm_cell.baseline_available {
        pm_cell.quality_badge = None;
    }
    cells.push(pm_cell);

    let (pm_today, pm_today_prev) = if perms.can_pm {
        (
            as_of::pm_due_at(db, now, PmDueWindow::SameDay, None).await?,
            as_of::pm_due_at(db, start, PmDueWindow::SameDay, None).await?,
        )
    } else {
        (0, 0)
    };
    cells.push(count_cell(
        kpi_definitions::overview::PM_DUE_TODAY,
        kpi_definitions::catalog::PM_DUE_TODAY,
        pm_today,
        pm_today_prev,
        perms.can_pm,
        "up-bad",
    ));

    let (backlog_now, backlog_prev) = if perms.can_ot {
        (
            as_of::wo_backlog_hours_at(db, now, None).await?,
            as_of::wo_backlog_hours_at(db, start, None).await?,
        )
    } else {
        (0.0, 0.0)
    };
    cells.push(float_cell(
        kpi_definitions::overview::MAINT_BACKLOG,
        kpi_definitions::catalog::MAINT_BACKLOG,
        backlog_now,
        backlog_prev,
        perms.can_ot,
        "hours",
        "up-bad",
    ));

    // Fleet RAMS KPIs: pooled ratios over the latest snapshot per equipment (never mean of ratios).
    let (mtbf, mtbf_prev, mttr, mttr_prev, avail, avail_prev, dq, dq_prev) = if perms.can_rep {
        let now = pooled_fleet_kpis(
            db,
            FleetWindow {
                since_days: OVERVIEW_HORIZON_DAYS,
                until_days: None,
            },
            None,
            DQ_GREEN_THRESHOLD,
        )
        .await?;
        let prev = pooled_fleet_kpis(
            db,
            FleetWindow {
                since_days: OVERVIEW_HORIZON_DAYS * 2,
                until_days: Some(OVERVIEW_HORIZON_DAYS),
            },
            None,
            DQ_GREEN_THRESHOLD,
        )
        .await?;
        (
            now.mtbf_hours,
            prev.mtbf_hours,
            now.mttr_hours,
            prev.mttr_hours,
            now.availability_operational,
            prev.availability_operational,
            now.mean_data_quality_score,
            prev.mean_data_quality_score,
        )
    } else {
        (None, None, None, None, None, None, None, None)
    };

    cells.push(float_cell(
        kpi_definitions::overview::MTBF,
        kpi_definitions::catalog::MTBF,
        mtbf.unwrap_or(0.0),
        mtbf_prev.unwrap_or(0.0),
        perms.can_rep && mtbf.is_some(),
        "hours",
        "up-good",
    ));
    cells.push(float_cell(
        kpi_definitions::overview::MTTR,
        kpi_definitions::catalog::MTTR,
        mttr.unwrap_or(0.0),
        mttr_prev.unwrap_or(0.0),
        perms.can_rep && mttr.is_some(),
        "hours",
        "up-bad",
    ));
    cells.push(float_cell(
        kpi_definitions::overview::AVAILABILITY,
        kpi_definitions::catalog::AVAILABILITY,
        avail.unwrap_or(0.0) * 100.0,
        avail_prev.unwrap_or(0.0) * 100.0,
        perms.can_rep && avail.is_some(),
        "percent",
        "up-good",
    ));
    cells.push(float_cell(
        kpi_definitions::overview::DATA_QUALITY,
        kpi_definitions::catalog::DATA_QUALITY,
        dq.unwrap_or(0.0) * 100.0,
        dq_prev.unwrap_or(0.0) * 100.0,
        perms.can_rep && dq.is_some(),
        "percent",
        "up-good",
    ));

    // Critical assets down: asset status is not kept per instant, so there is no baseline.
    let crit_now = if perms.can_eq {
        count_scalar(
            db,
            "SELECT COUNT(*) AS cnt FROM equipment e \
             LEFT JOIN reference_values rs_crit ON rs_crit.id = e.equipment_criticality_ref_id \
             LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id \
             WHERE e.deleted_at IS NULL \
               AND (rs_crit.code IN ('A', 'critical', 'CRITICAL') \
                    OR lv.code IN ('A', 'critical', 'CRITICAL')) \
               AND e.lifecycle_status IN ('down', 'out_of_service', 'inactive', 'decommissioned')",
        )
        .await?
    } else {
        0
    };
    cells.push(without_baseline(count_cell(
        kpi_definitions::overview::CRITICAL_DOWN,
        kpi_definitions::catalog::CRITICAL_DOWN,
        crit_now,
        crit_now,
        perms.can_eq,
        "up-bad",
    )));

    let (stock_out, reorder_n) = if perms.can_inv {
        let stock_out = count_scalar(
            db,
            "SELECT COUNT(*) AS cnt FROM ( \
               SELECT a.id FROM articles a \
               LEFT JOIN stock_balances sb ON sb.article_id = a.id \
               WHERE a.is_active = 1 \
               GROUP BY a.id \
               HAVING COALESCE(SUM(sb.available_qty), 0) <= 0 \
             )",
        )
        .await?;
        let reorder = evaluate_reorder(db, None).await?;
        (stock_out, reorder.len() as i64)
    } else {
        (0, 0)
    };
    // Current stock balances only (no per-instant history): no baseline.
    cells.push(without_baseline(count_cell(
        kpi_definitions::overview::STOCK_OUT,
        kpi_definitions::catalog::STOCK_OUT,
        stock_out,
        stock_out,
        perms.can_inv,
        "up-bad",
    )));
    // Current stock balances only (no per-instant history): no baseline.
    cells.push(without_baseline(count_cell(
        kpi_definitions::overview::REORDER_ALERT,
        kpi_definitions::catalog::REORDER_ALERT,
        reorder_n,
        reorder_n,
        perms.can_inv,
        "up-bad",
    )));

    let (cost_now, cost_prev) = if perms.can_fin {
        (
            sum_scalar(
                db,
                &format!(
                    "SELECT COALESCE(SUM(wo.total_cost), 0) AS total FROM work_orders wo \
                     WHERE wo.closed_at IS NOT NULL \
                       AND datetime(wo.closed_at) >= datetime('now', '-{OVERVIEW_HORIZON_DAYS} days')"
                ),
            )
            .await?,
            sum_scalar(
                db,
                &format!(
                    "SELECT COALESCE(SUM(wo.total_cost), 0) AS total FROM work_orders wo \
                     WHERE wo.closed_at IS NOT NULL \
                       AND datetime(wo.closed_at) >= datetime('now', '-{} days') \
                       AND datetime(wo.closed_at) < datetime('now', '-{OVERVIEW_HORIZON_DAYS} days')",
                    OVERVIEW_HORIZON_DAYS * 2
                ),
            )
            .await?,
        )
    } else {
        (0.0, 0.0)
    };
    cells.push(float_cell(
        kpi_definitions::overview::TOTAL_COST,
        kpi_definitions::catalog::MAINTENANCE_COST,
        cost_now,
        cost_prev,
        perms.can_fin,
        "currency",
        "up-bad",
    ));

    if perms.can_di {
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::DI_OPEN)
        {
            cell.series = load_di_series(db).await.ok();
        }
    }
    if perms.can_ot {
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::OT_IN_PROGRESS)
        {
            cell.series = load_ot_in_progress_series(db).await.ok();
        }
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::MAINT_BACKLOG)
        {
            cell.series = load_backlog_series(db).await.ok();
        }
    }
    if perms.can_pm {
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::PM_DUE_TODAY)
        {
            cell.series = load_pm_due_series(db).await.ok();
        }
    }
    if perms.can_fin {
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::TOTAL_COST)
        {
            cell.series = load_cost_series(db).await.ok();
        }
    }
    if perms.can_rep {
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::MTBF)
        {
            cell.series = load_reliability_series(db, "mtbf").await.ok();
        }
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::MTTR)
        {
            cell.series = load_reliability_series(db, "mttr").await.ok();
        }
        if let Some(cell) = cells
            .iter_mut()
            .find(|c| c.semantic_key == kpi_definitions::overview::AVAILABILITY)
        {
            cell.series = load_reliability_series(db, "availability").await.ok();
        }
    }

    Ok(OverviewKpisPayload {
        horizon_days: OVERVIEW_HORIZON_DAYS,
        cells,
    })
}

async fn load_daily_count_series(db: &DatabaseConnection, sql: &str) -> AppResult<Vec<f64>> {
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let cnt: i64 = sea_orm::TryGetable::try_get_by(&r, "cnt").unwrap_or(0);
        out.push(cnt as f64);
    }
    Ok(out)
}

async fn load_daily_float_series(db: &DatabaseConnection, sql: &str) -> AppResult<Vec<f64>> {
    use sea_orm::TryGetable;
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let v: f64 = f64::try_get(&r, "", "v").unwrap_or(0.0);
        out.push(v);
    }
    Ok(out)
}

async fn load_di_series(db: &DatabaseConnection) -> AppResult<Vec<f64>> {
    load_daily_count_series(
        db,
        "SELECT date(created_at) AS d, COUNT(*) AS cnt \
         FROM intervention_requests \
         WHERE created_at >= datetime('now', '-30 days') \
         GROUP BY date(created_at) ORDER BY d ASC",
    )
    .await
}

async fn load_ot_in_progress_series(db: &DatabaseConnection) -> AppResult<Vec<f64>> {
    // Daily WO starts into in_progress (status history proxy via updated_at while in_progress).
    load_daily_count_series(
        db,
        "SELECT date(wo.updated_at) AS d, COUNT(*) AS cnt \
         FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code = 'in_progress' \
           AND wo.updated_at >= datetime('now', '-30 days') \
         GROUP BY date(wo.updated_at) ORDER BY d ASC",
    )
    .await
}

async fn load_backlog_series(db: &DatabaseConnection) -> AppResult<Vec<f64>> {
    // Daily planned hours on WOs created/updated that day (open pipeline proxy).
    load_daily_float_series(
        db,
        "SELECT date(wo.created_at) AS d, \
                COALESCE(SUM(COALESCE(wo.expected_duration_hours, 0)), 0) AS v \
         FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code NOT IN ('closed', 'cancelled', 'completed') \
           AND wo.created_at >= datetime('now', '-30 days') \
         GROUP BY date(wo.created_at) ORDER BY d ASC",
    )
    .await
}

async fn load_pm_due_series(db: &DatabaseConnection) -> AppResult<Vec<f64>> {
    load_daily_count_series(
        db,
        "SELECT date(due_at) AS d, COUNT(*) AS cnt \
         FROM pm_occurrences \
         WHERE due_at IS NOT NULL \
           AND due_at >= datetime('now', '-30 days') \
           AND due_at <= datetime('now', '+1 days') \
           AND status NOT IN ('cancelled') \
         GROUP BY date(due_at) ORDER BY d ASC",
    )
    .await
}

async fn load_cost_series(db: &DatabaseConnection) -> AppResult<Vec<f64>> {
    load_daily_float_series(
        db,
        "SELECT date(wo.closed_at) AS d, COALESCE(SUM(wo.total_cost), 0) AS v \
         FROM work_orders wo \
         WHERE wo.closed_at IS NOT NULL \
           AND datetime(wo.closed_at) >= datetime('now', '-30 days') \
         GROUP BY date(wo.closed_at) ORDER BY d ASC",
    )
    .await
}

async fn load_reliability_series(db: &DatabaseConnection, field: &str) -> AppResult<Vec<f64>> {
    let col = match field {
        "mtbf" => "AVG(CASE WHEN s.mtbf IS NOT NULL AND s.mtbf > 0 THEN s.mtbf END)",
        "mttr" => "AVG(CASE WHEN s.mttr IS NOT NULL AND s.mttr > 0 THEN s.mttr END)",
        "availability" => "AVG(CASE WHEN s.availability IS NOT NULL THEN s.availability * 100.0 END)",
        _ => return Ok(vec![]),
    };
    let sql = format!(
        "SELECT date(s.period_end) AS d, {col} AS v \
         FROM reliability_kpi_snapshots s \
         WHERE datetime(s.period_end) >= datetime('now', '-30 days') \
         GROUP BY date(s.period_end) ORDER BY d ASC"
    );
    load_daily_float_series(db, &sql).await
}
