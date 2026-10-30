//! Vue d'ensemble Overview KPI values — SSOT in Rust (`get_overview_kpis`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::analytics::as_of::{self, PmDueWindow};
use crate::analytics::smrp;
use crate::commands::dashboard::kpi_quality_badge;
use crate::errors::{AppError, AppResult};
use crate::inventory::queries::reorder_triggers_by_article;
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
}

/// `SELECT COUNT(*) AS cnt …` — a missing row or an unreadable count is an error, never 0.
async fn count_scalar(db: &DatabaseConnection, sql: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("overview count returned no row")))?;
    row.try_get::<i64>("", "cnt")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("overview count decode: {e}")))
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
) -> AppResult<OverviewKpisPayload> {
    let perms = PermFlags {
        can_di,
        can_ot,
        can_eq,
        can_pm,
        can_rep,
        can_inv,
    };
    let mut cells = Vec::new();

    // Point-in-time measures: now vs the same measure at the start of the horizon (`as_of`).
    let now = crate::clock::now();
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
        // Workload, not a fault (CMMS Advisor D15): only overdue PM is shown red.
        "neutral",
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

    // Critical assets down (`assets::taxonomy_reference::CRITICAL_DOWN_SQL`: most critical level
    // of Données de référence, status tagged `down`). Asset status is not kept per instant, so
    // there is no baseline.
    let crit_now = if perms.can_eq {
        count_scalar(
            db,
            &format!(
                "SELECT COUNT(*) AS cnt FROM equipment e WHERE {}",
                crate::assets::taxonomy_reference::CRITICAL_DOWN_SQL
            ),
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
        // Per article (the article list's "below reorder point" filter), not per article × store.
        let reorder = reorder_triggers_by_article(db).await?;
        (stock_out, i64::try_from(reorder.len()).unwrap_or(i64::MAX))
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

    Ok(OverviewKpisPayload {
        horizon_days: OVERVIEW_HORIZON_DAYS,
        cells,
    })
}
