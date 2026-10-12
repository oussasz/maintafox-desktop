//! Named analytics projections for Executive Dashboards (shared by widgets; no formula recompute in TS).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TryGetable};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;
use crate::kpi_definitions;
use crate::reliability::fleet::{pooled_fleet_kpis, pooled_fleet_series, FleetWindow};
use crate::reliability::readiness::DQ_GREEN_THRESHOLD;

#[derive(Debug, Clone, Deserialize)]
pub struct AnalyticsExecutiveFilter {
    /// Inclusive horizon length in days (7, 30, or 90).
    pub horizon_days: i64,
    /// Optional org node scope (`org_nodes.id`). Applied as DI.org_node_id / WO.entity_id / equipment.installed_at_node_id.
    pub org_node_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AnalyticsKpiValue {
    pub key: String,
    pub value: f64,
    pub previous_value: f64,
    pub available: bool,
    pub quality_badge: Option<String>,
    pub catalog_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalyticsWorkloadDay {
    pub date: String,
    pub di_created: i64,
    pub wo_completed: i64,
    pub pm_due: i64,
}

#[derive(Debug, Serialize)]
pub struct AnalyticsDiStatusSegment {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct AnalyticsReliabilitySeriesPoint {
    pub date: String,
    pub avg_mtbf: Option<f64>,
    pub avg_mttr: Option<f64>,
    pub avg_availability: Option<f64>,
    pub avg_data_quality: Option<f64>,
    pub maintenance_cost: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AnalyticsBadActorAsset {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_label: String,
    pub event_count: i64,
    pub mtbf_hours: Option<f64>,
    pub availability: Option<f64>,
    pub maintenance_cost: f64,
    pub cost_available: bool,
}

#[derive(Debug, Serialize)]
pub struct AnalyticsExecutiveDashboard {
    pub horizon_days: i64,
    pub org_node_id: Option<i64>,
    pub open_dis: AnalyticsKpiValue,
    pub open_wos: AnalyticsKpiValue,
    pub total_assets: AnalyticsKpiValue,
    pub overdue_items: AnalyticsKpiValue,
    pub pm_overdue: AnalyticsKpiValue,
    pub pm_due_today: AnalyticsKpiValue,
    pub pm_due_next_7d: AnalyticsKpiValue,
    pub avg_mtbf_hours: AnalyticsKpiValue,
    pub avg_mttr_hours: AnalyticsKpiValue,
    pub avg_availability: AnalyticsKpiValue,
    pub avg_data_quality: AnalyticsKpiValue,
    pub maintenance_cost: AnalyticsKpiValue,
    pub reliability_snapshot_count: i64,
    pub reliability_total_events: i64,
    pub workload_days: Vec<AnalyticsWorkloadDay>,
    pub workload_quality_badge: Option<String>,
    pub di_status_segments: Vec<AnalyticsDiStatusSegment>,
    pub reliability_series: Vec<AnalyticsReliabilitySeriesPoint>,
    pub bad_actor_assets: Vec<AnalyticsBadActorAsset>,
}

fn normalize_horizon(days: i64) -> i64 {
    match days {
        7 | 30 | 90 => days,
        _ => 30,
    }
}

fn kpi_badge(previous: f64, value: f64) -> Option<String> {
    if previous == 0.0 && value != 0.0 {
        Some(kpi_definitions::quality_badge::INSUFFICIENT_BASELINE.to_string())
    } else {
        None
    }
}

fn denied_kpi(key: &str, catalog_id: Option<&str>) -> AnalyticsKpiValue {
    AnalyticsKpiValue {
        key: key.to_string(),
        value: 0.0,
        previous_value: 0.0,
        available: false,
        quality_badge: None,
        catalog_id: catalog_id.map(str::to_string),
    }
}

fn int_kpi(key: &str, value: i64, previous: i64, catalog_id: Option<&str>) -> AnalyticsKpiValue {
    let v = value as f64;
    let p = previous as f64;
    AnalyticsKpiValue {
        key: key.to_string(),
        value: v,
        previous_value: p,
        available: true,
        quality_badge: kpi_badge(p, v),
        catalog_id: catalog_id.map(str::to_string),
    }
}

fn float_kpi(key: &str, value: Option<f64>, previous: Option<f64>, catalog_id: Option<&str>) -> AnalyticsKpiValue {
    let v = value.unwrap_or(0.0);
    let p = previous.unwrap_or(0.0);
    AnalyticsKpiValue {
        key: key.to_string(),
        value: v,
        previous_value: p,
        available: value.is_some(),
        quality_badge: if value.is_some() { kpi_badge(p, v) } else { None },
        catalog_id: catalog_id.map(str::to_string),
    }
}

async fn count_i64(db: &DatabaseConnection, sql: &str, binds: Vec<sea_orm::Value>) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?;
    match row {
        Some(r) => Ok(i64::try_get(&r, "", "cnt").unwrap_or(0)),
        None => Ok(0),
    }
}

/// Build executive dashboard projection for the given filter + permission flags.
#[allow(clippy::too_many_arguments)]
pub async fn build_executive_dashboard(
    db: &DatabaseConnection,
    filter: &AnalyticsExecutiveFilter,
    can_di: bool,
    can_ot: bool,
    can_eq: bool,
    can_pm: bool,
    can_rep: bool,
) -> AppResult<AnalyticsExecutiveDashboard> {
    let horizon = normalize_horizon(filter.horizon_days);
    let org = filter.org_node_id;
    let org_bind: sea_orm::Value = org.into();

    // ── Ops KPIs (current open counts; org-scoped) ─────────────────────────
    let open_dis = if can_di {
        let sql = "SELECT COUNT(*) AS cnt FROM intervention_requests ir
             WHERE ir.status NOT IN ('rejected','converted_to_work_order','closed_as_non_executable','archived')
               AND (? IS NULL OR ir.org_node_id = ?)";
        let n = count_i64(db, sql, vec![org_bind.clone(), org_bind.clone()]).await?;
        let prev_sql = "SELECT COUNT(*) AS cnt FROM intervention_requests ir
             WHERE ir.status NOT IN ('rejected','converted_to_work_order','closed_as_non_executable','archived')
               AND datetime(ir.created_at) <= datetime('now', ?)
               AND (? IS NULL OR ir.org_node_id = ?)";
        let shift = format!("-{horizon} days");
        let prev = count_i64(db, prev_sql, vec![shift.into(), org_bind.clone(), org_bind.clone()]).await?;
        int_kpi("open_dis", n, prev, None)
    } else {
        denied_kpi("open_dis", None)
    };

    let open_wos = if can_ot {
        let sql = "SELECT COUNT(*) AS cnt FROM work_orders wo
             JOIN work_order_statuses s ON s.id = wo.status_id
             WHERE s.code NOT IN ('closed','cancelled')
               AND (? IS NULL OR wo.entity_id = ?)";
        let n = count_i64(db, sql, vec![org_bind.clone(), org_bind.clone()]).await?;
        let prev_sql = "SELECT COUNT(*) AS cnt FROM work_orders wo
             JOIN work_order_statuses s ON s.id = wo.status_id
             WHERE s.code NOT IN ('closed','cancelled')
               AND datetime(wo.created_at) <= datetime('now', ?)
               AND (? IS NULL OR wo.entity_id = ?)";
        let shift = format!("-{horizon} days");
        let prev = count_i64(db, prev_sql, vec![shift.into(), org_bind.clone(), org_bind.clone()]).await?;
        int_kpi("open_wos", n, prev, None)
    } else {
        denied_kpi("open_wos", None)
    };

    let total_assets = if can_eq {
        let sql = "SELECT COUNT(*) AS cnt FROM equipment e
             WHERE e.deleted_at IS NULL
               AND (? IS NULL OR e.installed_at_node_id = ?)";
        let n = count_i64(db, sql, vec![org_bind.clone(), org_bind.clone()]).await?;
        int_kpi("total_assets", n, n, None)
    } else {
        denied_kpi("total_assets", None)
    };

    let overdue_items = if can_di || can_pm {
        let mut total = 0_i64;
        let mut prev_total = 0_i64;
        if can_di {
            let di_sql = "SELECT COUNT(*) AS cnt FROM intervention_requests ir
                 WHERE ir.status NOT IN ('rejected','converted_to_work_order','closed_as_non_executable','archived')
                   AND datetime(ir.created_at) <= datetime('now', '-7 days')
                   AND (? IS NULL OR ir.org_node_id = ?)";
            total += count_i64(db, di_sql, vec![org_bind.clone(), org_bind.clone()]).await?;
            let di_prev = "SELECT COUNT(*) AS cnt FROM intervention_requests ir
                 WHERE ir.status NOT IN ('rejected','converted_to_work_order','closed_as_non_executable','archived')
                   AND datetime(ir.created_at) <= datetime('now', '-14 days')
                   AND (? IS NULL OR ir.org_node_id = ?)";
            prev_total += count_i64(db, di_prev, vec![org_bind.clone(), org_bind.clone()]).await?;
        }
        if can_pm {
            let pm_sql = "SELECT COUNT(*) AS cnt FROM pm_occurrences po
                 JOIN pm_plans pp ON pp.id = po.pm_plan_id
                 LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment'
                 WHERE po.status NOT IN ('completed','cancelled','missed')
                   AND po.due_at IS NOT NULL
                   AND po.due_at < datetime('now')
                   AND (? IS NULL OR e.installed_at_node_id = ?)";
            let pm_overdue_now = count_i64(db, pm_sql, vec![org_bind.clone(), org_bind.clone()]).await?;
            total += pm_overdue_now;
            // No shifted PM definition: the PM part of the baseline is the current PM count
            // (adding `total` here counted the requests twice).
            prev_total += pm_overdue_now;
        }
        int_kpi("overdue_items", total, prev_total, None)
    } else {
        denied_kpi("overdue_items", None)
    };

    // ── PM due buckets (as-of now; org via plan equipment) ─────────────────
    let (pm_overdue, pm_due_today, pm_due_next_7d) = if can_pm {
        let overdue = count_i64(
            db,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences po
             JOIN pm_plans pp ON pp.id = po.pm_plan_id
             LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment'
             WHERE po.status NOT IN ('completed','cancelled','missed')
               AND po.due_at IS NOT NULL AND po.due_at < datetime('now')
               AND (? IS NULL OR e.installed_at_node_id = ?)",
            vec![org_bind.clone(), org_bind.clone()],
        )
        .await?;
        let today = count_i64(
            db,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences po
             JOIN pm_plans pp ON pp.id = po.pm_plan_id
             LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment'
             WHERE po.status NOT IN ('completed','cancelled','missed')
               AND po.due_at IS NOT NULL AND date(po.due_at) = date('now')
               AND (? IS NULL OR e.installed_at_node_id = ?)",
            vec![org_bind.clone(), org_bind.clone()],
        )
        .await?;
        let next7 = count_i64(
            db,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences po
             JOIN pm_plans pp ON pp.id = po.pm_plan_id
             LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment'
             WHERE po.status NOT IN ('completed','cancelled','missed')
               AND po.due_at IS NOT NULL
               AND po.due_at >= datetime('now')
               AND po.due_at <= datetime('now', '+7 days')
               AND (? IS NULL OR e.installed_at_node_id = ?)",
            vec![org_bind.clone(), org_bind.clone()],
        )
        .await?;
        (
            int_kpi(
                "pm_overdue",
                overdue,
                overdue,
                Some(kpi_definitions::catalog::PM_ADHERENCE),
            ),
            int_kpi(
                "pm_due_today",
                today,
                today,
                Some(kpi_definitions::catalog::PM_ADHERENCE),
            ),
            int_kpi(
                "pm_due_next_7d",
                next7,
                next7,
                Some(kpi_definitions::catalog::PM_ADHERENCE),
            ),
        )
    } else {
        (
            denied_kpi("pm_overdue", Some(kpi_definitions::catalog::PM_ADHERENCE)),
            denied_kpi("pm_due_today", Some(kpi_definitions::catalog::PM_ADHERENCE)),
            denied_kpi("pm_due_next_7d", Some(kpi_definitions::catalog::PM_ADHERENCE)),
        )
    };

    // ── Reliability aggregates over horizon ────────────────────────────────
    let (
        avg_mtbf_hours,
        avg_mttr_hours,
        avg_availability,
        avg_data_quality,
        reliability_snapshot_count,
        reliability_total_events,
    ) = if can_rep {
        // Pooled ratios over the latest snapshot per equipment (never mean of ratios).
        let cur = pooled_fleet_kpis(
            db,
            FleetWindow {
                since_days: horizon,
                until_days: None,
            },
            org,
            DQ_GREEN_THRESHOLD,
        )
        .await?;
        let prev = pooled_fleet_kpis(
            db,
            FleetWindow {
                since_days: horizon * 2,
                until_days: Some(horizon),
            },
            org,
            DQ_GREEN_THRESHOLD,
        )
        .await?;
        let (snap_n, mtbf, mttr, avail, dq, ev) = (
            cur.equipment_count,
            cur.mtbf_hours,
            cur.mttr_hours,
            cur.availability_operational,
            cur.mean_data_quality_score,
            cur.total_failure_events,
        );
        let (p_mtbf, p_mttr, p_avail, p_dq) = (
            prev.mtbf_hours,
            prev.mttr_hours,
            prev.availability_operational,
            prev.mean_data_quality_score,
        );
        (
            float_kpi("avg_mtbf_hours", mtbf, p_mtbf, Some(kpi_definitions::catalog::MTBF)),
            float_kpi("avg_mttr_hours", mttr, p_mttr, Some(kpi_definitions::catalog::MTTR)),
            float_kpi(
                "avg_availability",
                avail,
                p_avail,
                Some(kpi_definitions::catalog::AVAILABILITY),
            ),
            float_kpi("avg_data_quality", dq, p_dq, None),
            snap_n,
            ev,
        )
    } else {
        (
            denied_kpi("avg_mtbf_hours", Some(kpi_definitions::catalog::MTBF)),
            denied_kpi("avg_mttr_hours", Some(kpi_definitions::catalog::MTTR)),
            denied_kpi("avg_availability", Some(kpi_definitions::catalog::AVAILABILITY)),
            denied_kpi("avg_data_quality", None),
            0,
            0,
        )
    };

    // ── Maintenance cost (closed WO total_cost in horizon) ─────────────────
    let maintenance_cost = if can_ot {
        let shift = format!("-{horizon} days");
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(SUM(wo.total_cost), 0) AS total
                 FROM work_orders wo
                 JOIN work_order_statuses s ON s.id = wo.status_id
                 WHERE s.code = 'closed'
                   AND wo.closed_at IS NOT NULL
                   AND datetime(wo.closed_at) >= datetime('now', ?)
                   AND (? IS NULL OR wo.entity_id = ?)",
                [shift.clone().into(), org_bind.clone(), org_bind.clone()],
            ))
            .await?;
        let prev_shift = format!("-{} days", horizon * 2);
        let prev_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(SUM(wo.total_cost), 0) AS total
                 FROM work_orders wo
                 JOIN work_order_statuses s ON s.id = wo.status_id
                 WHERE s.code = 'closed'
                   AND wo.closed_at IS NOT NULL
                   AND datetime(wo.closed_at) >= datetime('now', ?)
                   AND datetime(wo.closed_at) < datetime('now', ?)
                   AND (? IS NULL OR wo.entity_id = ?)",
                [prev_shift.into(), shift.into(), org_bind.clone(), org_bind.clone()],
            ))
            .await?;
        let cur = row
            .as_ref()
            .and_then(|r| f64::try_get(r, "", "total").ok())
            .unwrap_or(0.0);
        let prev = prev_row
            .as_ref()
            .and_then(|r| f64::try_get(r, "", "total").ok())
            .unwrap_or(0.0);
        AnalyticsKpiValue {
            key: "maintenance_cost".into(),
            value: cur,
            previous_value: prev,
            available: true,
            quality_badge: kpi_badge(prev, cur),
            catalog_id: Some(kpi_definitions::catalog::MAINTENANCE_COST.to_string()),
        }
    } else {
        denied_kpi("maintenance_cost", Some(kpi_definitions::catalog::MAINTENANCE_COST))
    };

    // ── Workload chart (grouped; one query per series) ─────────────────────
    let mut workload_days = Vec::new();
    let mut workload_quality_badge = None;
    if can_di || can_ot || can_pm {
        use std::collections::BTreeMap;
        let mut by_date: BTreeMap<String, AnalyticsWorkloadDay> = BTreeMap::new();
        let shift = format!("-{horizon} days");

        // Seed continuous axis in Rust (avoid N round-trips for calendar labels).
        {
            use chrono::{Duration, Utc};
            let today = Utc::now().date_naive();
            for offset in (0..horizon).rev() {
                let d = today - Duration::days(offset);
                let date = d.format("%Y-%m-%d").to_string();
                by_date.insert(
                    date.clone(),
                    AnalyticsWorkloadDay {
                        date,
                        di_created: 0,
                        wo_completed: 0,
                        pm_due: 0,
                    },
                );
            }
        }

        if can_di {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT date(ir.created_at) AS d, COUNT(*) AS cnt
                     FROM intervention_requests ir
                     WHERE date(ir.created_at) >= date('now', ?)
                       AND (? IS NULL OR ir.org_node_id = ?)
                     GROUP BY date(ir.created_at)",
                    [shift.clone().into(), org_bind.clone(), org_bind.clone()],
                ))
                .await?;
            for r in rows {
                let d: String = r.try_get("", "d").unwrap_or_default();
                let cnt: i64 = r.try_get("", "cnt").unwrap_or(0);
                if let Some(day) = by_date.get_mut(&d) {
                    day.di_created = cnt;
                }
            }
        }
        if can_ot {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT date(wo.closed_at) AS d, COUNT(*) AS cnt
                     FROM work_orders wo
                     JOIN work_order_statuses s ON s.id = wo.status_id
                     WHERE s.code = 'closed' AND wo.closed_at IS NOT NULL
                       AND date(wo.closed_at) >= date('now', ?)
                       AND (? IS NULL OR wo.entity_id = ?)
                     GROUP BY date(wo.closed_at)",
                    [shift.clone().into(), org_bind.clone(), org_bind.clone()],
                ))
                .await?;
            for r in rows {
                let d: String = r.try_get("", "d").unwrap_or_default();
                let cnt: i64 = r.try_get("", "cnt").unwrap_or(0);
                if let Some(day) = by_date.get_mut(&d) {
                    day.wo_completed = cnt;
                }
            }
        }
        if can_pm {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT date(po.due_at) AS d, COUNT(*) AS cnt
                     FROM pm_occurrences po
                     JOIN pm_plans pp ON pp.id = po.pm_plan_id
                     LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment'
                     WHERE po.due_at IS NOT NULL
                       AND date(po.due_at) >= date('now', ?)
                       AND (? IS NULL OR e.installed_at_node_id = ?)
                     GROUP BY date(po.due_at)",
                    [shift.into(), org_bind.clone(), org_bind.clone()],
                ))
                .await?;
            for r in rows {
                let d: String = r.try_get("", "d").unwrap_or_default();
                let cnt: i64 = r.try_get("", "cnt").unwrap_or(0);
                if let Some(day) = by_date.get_mut(&d) {
                    day.pm_due = cnt;
                }
            }
        }

        workload_days = by_date.into_values().collect();
        let all_zero = workload_days
            .iter()
            .all(|d| d.di_created == 0 && d.wo_completed == 0 && d.pm_due == 0);
        if all_zero {
            workload_quality_badge = Some(kpi_definitions::quality_badge::SPARSE_WORKLOAD.to_string());
        }
    }

    // ── DI status distribution ─────────────────────────────────────────────
    let di_status_segments = if can_di {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT ir.status AS status, COUNT(*) AS cnt
                 FROM intervention_requests ir
                 WHERE (? IS NULL OR ir.org_node_id = ?)
                 GROUP BY ir.status
                 ORDER BY cnt DESC",
                [org_bind.clone(), org_bind.clone()],
            ))
            .await?;
        let mut out = Vec::new();
        for r in rows {
            let status: String = r.try_get("", "status").unwrap_or_else(|_| "unknown".into());
            let count: i64 = r.try_get("", "cnt").unwrap_or(0);
            out.push(AnalyticsDiStatusSegment { status, count });
        }
        out
    } else {
        Vec::new()
    };

    // ── RAMS series (pooled fleet ratios by snapshot date) + closed-WO cost by day ─
    let mut reliability_series: Vec<AnalyticsReliabilitySeriesPoint> = Vec::new();
    if can_rep {
        let horizon_shift = format!("-{horizon} days");
        use std::collections::BTreeMap;
        let mut by_date: BTreeMap<String, AnalyticsReliabilitySeriesPoint> = BTreeMap::new();
        // Pooled per date over the latest snapshot per equipment on that date.
        for (date, k) in pooled_fleet_series(
            db,
            FleetWindow {
                since_days: horizon,
                until_days: None,
            },
            org,
            DQ_GREEN_THRESHOLD,
        )
        .await?
        {
            by_date.insert(
                date.clone(),
                AnalyticsReliabilitySeriesPoint {
                    date,
                    avg_mtbf: k.mtbf_hours,
                    avg_mttr: k.mttr_hours,
                    avg_availability: k.availability_operational,
                    avg_data_quality: k.mean_data_quality_score,
                    maintenance_cost: None,
                },
            );
        }
        if can_ot {
            let cost_rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT date(wo.closed_at) AS d, COALESCE(SUM(wo.total_cost), 0) AS total
                     FROM work_orders wo
                     JOIN work_order_statuses st ON st.id = wo.status_id
                     WHERE st.code = 'closed' AND wo.closed_at IS NOT NULL
                       AND date(wo.closed_at) >= date('now', ?)
                       AND (? IS NULL OR wo.entity_id = ?)
                     GROUP BY date(wo.closed_at)",
                    [horizon_shift.into(), org_bind.clone(), org_bind.clone()],
                ))
                .await?;
            for r in cost_rows {
                let date: String = r.try_get("", "d").unwrap_or_default();
                if date.is_empty() {
                    continue;
                }
                let total = f64::try_get(&r, "", "total").ok();
                by_date
                    .entry(date.clone())
                    .and_modify(|p| p.maintenance_cost = total)
                    .or_insert(AnalyticsReliabilitySeriesPoint {
                        date,
                        avg_mtbf: None,
                        avg_mttr: None,
                        avg_availability: None,
                        avg_data_quality: None,
                        maintenance_cost: total,
                    });
            }
        }
        reliability_series = by_date.into_values().collect();
    }

    // ── Top 5 bad-actor assets (latest snapshot in horizon, most events) ──
    let mut bad_actor_assets: Vec<AnalyticsBadActorAsset> = Vec::new();
    if can_rep {
        let horizon_shift = format!("-{horizon} days");
        let sql = if can_ot {
            "WITH ranked AS (
                SELECT s.equipment_id, s.event_count, s.mtbf, s.availability,
                       ROW_NUMBER() OVER (
                         PARTITION BY s.equipment_id ORDER BY s.period_end DESC, s.id DESC
                       ) AS rn
                FROM reliability_kpi_snapshots s
                JOIN equipment e ON e.id = s.equipment_id
                WHERE s.equipment_id IS NOT NULL
                  AND datetime(s.period_end) >= datetime('now', ?)
                  AND (? IS NULL OR e.installed_at_node_id = ?)
             )
             SELECT e.id AS equipment_id,
                    e.asset_id_code AS asset_code,
                    e.name AS asset_label,
                    r.event_count,
                    r.mtbf,
                    r.availability,
                    COALESCE(c.cost, 0) AS cost
             FROM ranked r
             JOIN equipment e ON e.id = r.equipment_id
             LEFT JOIN (
                SELECT wo.equipment_id, COALESCE(SUM(wo.total_cost), 0) AS cost
                FROM work_orders wo
                JOIN work_order_statuses st ON st.id = wo.status_id
                WHERE st.code = 'closed' AND wo.closed_at IS NOT NULL
                  AND datetime(wo.closed_at) >= datetime('now', ?)
                GROUP BY wo.equipment_id
             ) c ON c.equipment_id = e.id
             WHERE r.rn = 1
             ORDER BY r.event_count DESC, e.id ASC
             LIMIT 5"
        } else {
            "WITH ranked AS (
                SELECT s.equipment_id, s.event_count, s.mtbf, s.availability,
                       ROW_NUMBER() OVER (
                         PARTITION BY s.equipment_id ORDER BY s.period_end DESC, s.id DESC
                       ) AS rn
                FROM reliability_kpi_snapshots s
                JOIN equipment e ON e.id = s.equipment_id
                WHERE s.equipment_id IS NOT NULL
                  AND datetime(s.period_end) >= datetime('now', ?)
                  AND (? IS NULL OR e.installed_at_node_id = ?)
             )
             SELECT e.id AS equipment_id,
                    e.asset_id_code AS asset_code,
                    e.name AS asset_label,
                    r.event_count,
                    r.mtbf,
                    r.availability,
                    0 AS cost
             FROM ranked r
             JOIN equipment e ON e.id = r.equipment_id
             WHERE r.rn = 1
             ORDER BY r.event_count DESC, e.id ASC
             LIMIT 5"
        };
        let binds: Vec<sea_orm::Value> = if can_ot {
            vec![
                horizon_shift.clone().into(),
                org_bind.clone(),
                org_bind.clone(),
                horizon_shift.into(),
            ]
        } else {
            vec![horizon_shift.into(), org_bind.clone(), org_bind.clone()]
        };
        let rows = db
            .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
            .await?;
        for r in rows {
            bad_actor_assets.push(AnalyticsBadActorAsset {
                equipment_id: i64::try_get(&r, "", "equipment_id").unwrap_or(0),
                asset_code: String::try_get(&r, "", "asset_code").unwrap_or_default(),
                asset_label: String::try_get(&r, "", "asset_label").unwrap_or_default(),
                event_count: i64::try_get(&r, "", "event_count").unwrap_or(0),
                mtbf_hours: f64::try_get(&r, "", "mtbf").ok(),
                availability: f64::try_get(&r, "", "availability").ok(),
                maintenance_cost: f64::try_get(&r, "", "cost").unwrap_or(0.0),
                cost_available: can_ot,
            });
        }
    }

    Ok(AnalyticsExecutiveDashboard {
        horizon_days: horizon,
        org_node_id: org,
        open_dis,
        open_wos,
        total_assets,
        overdue_items,
        pm_overdue,
        pm_due_today,
        pm_due_next_7d,
        avg_mtbf_hours,
        avg_mttr_hours,
        avg_availability,
        avg_data_quality,
        maintenance_cost,
        reliability_snapshot_count,
        reliability_total_events,
        workload_days,
        workload_quality_badge,
        di_status_segments,
        reliability_series,
        bad_actor_assets,
    })
}
