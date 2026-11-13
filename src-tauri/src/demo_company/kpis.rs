//! Read-only KPI summary of the generated company, computed by the app's own KPI code where it
//! exists (reliability snapshots, PM compliance, SMRP backlog). Used by the CLI report and tests.

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;

#[derive(Debug, Clone, Default)]
pub struct AssetKpi {
    pub code: String,
    pub failures: i64,
    pub mtbf_hours: Option<f64>,
    pub mttr_hours: Option<f64>,
    /// Cost of the closed work orders of the history (DZD).
    pub cost: f64,
}

#[derive(Debug, Clone, Default)]
pub struct KpiSummary {
    pub assets: Vec<AssetKpi>,
    pub pm_due: i64,
    pub pm_on_time: i64,
    pub pm_late: i64,
    pub pm_not_done: i64,
    pub pm_compliance_pct: Option<f64>,
    pub backlog_hours: f64,
    pub backlog_weeks: Option<f64>,
    pub open_work_orders: i64,
    pub in_progress_work_orders: i64,
    pub open_requests: i64,
    pub closed_work_orders: i64,
    pub total_cost: f64,
}

impl KpiSummary {
    /// Machines with a computed MTBF.
    pub fn mtbf_values(&self) -> Vec<f64> {
        self.assets.iter().filter_map(|a| a.mtbf_hours).collect()
    }

    pub fn mttr_values(&self) -> Vec<f64> {
        self.assets.iter().filter_map(|a| a.mttr_hours).collect()
    }
}

pub async fn summarize(db: &DatabaseConnection, now: DateTime<Utc>) -> AppResult<KpiSummary> {
    let mut out = KpiSummary::default();

    // Latest reliability snapshot of each equipment (what the RAMS screens show).
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT e.asset_id_code AS code, \
                    (SELECT s.mtbf FROM reliability_kpi_snapshots s WHERE s.equipment_id = e.id \
                      ORDER BY s.period_end DESC, s.id DESC LIMIT 1) AS mtbf, \
                    (SELECT s.mttr FROM reliability_kpi_snapshots s WHERE s.equipment_id = e.id \
                      ORDER BY s.period_end DESC, s.id DESC LIMIT 1) AS mttr, \
                    (SELECT COUNT(*) FROM failure_events f WHERE f.equipment_id = e.id) AS failures, \
                    (SELECT COALESCE(SUM(w.total_cost), 0) FROM work_orders w \
                      WHERE w.equipment_id = e.id AND w.closed_at IS NOT NULL) AS cost \
             FROM equipment e WHERE e.deleted_at IS NULL ORDER BY e.asset_id_code"
                .to_string(),
        ))
        .await?;
    for r in rows {
        out.assets.push(AssetKpi {
            code: r.try_get("", "code").unwrap_or_default(),
            failures: r.try_get("", "failures").unwrap_or(0),
            mtbf_hours: r.try_get::<Option<f64>>("", "mtbf").ok().flatten(),
            mttr_hours: r.try_get::<Option<f64>>("", "mttr").ok().flatten(),
            cost: r.try_get::<Option<f64>>("", "cost").ok().flatten().unwrap_or(0.0),
        });
    }
    out.total_cost = out.assets.iter().map(|a| a.cost).sum();

    // PM compliance over the history (the figure of the PM page and the Overview tile).
    let pm = crate::pm::compliance::compliance_between(
        db,
        now - Duration::days(366),
        now,
        now,
        crate::pm::compliance::ComplianceScope::default(),
    )
    .await?;
    out.pm_due = pm.due_count;
    out.pm_on_time = pm.on_time_count;
    out.pm_late = pm.late_count;
    out.pm_not_done = pm.not_done_count;
    out.pm_compliance_pct = pm.compliance_pct;

    // Backlog as the SMRP tile computes it (planned hours of open work / weekly crew hours).
    let backlog = crate::analytics::smrp::backlog(
        db,
        crate::analytics::smrp::SmrpWindow {
            from: now - Duration::days(30),
            to: now,
        },
        now,
        None,
    )
    .await?;
    out.backlog_hours = backlog.backlog_hours;
    out.backlog_weeks = backlog.weeks;
    out.open_work_orders = backlog.open_work_orders;

    let count = |sql: &'static str| async move {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .map(|r| r.and_then(|r| r.try_get_by_index::<i64>(0).ok()).unwrap_or(0))
    };
    out.in_progress_work_orders = count(
        "SELECT COUNT(*) FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE s.code = 'in_progress'",
    )
    .await?;
    out.closed_work_orders = count("SELECT COUNT(*) FROM work_orders WHERE closed_at IS NOT NULL").await?;
    out.open_requests = count(
        "SELECT COUNT(*) FROM intervention_requests WHERE status IN ('submitted','in_review','returned_for_clarification','awaiting_approval','approved','deferred')",
    )
    .await?;
    Ok(out)
}
