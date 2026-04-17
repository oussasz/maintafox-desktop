//! Dashboard KPI and workload chart IPC commands.
//!
//! Phase 2 - Sub-phase 00 - File 02 - Sprint S4.
//!
//! Provides aggregated counts for the Dashboard KPI grid and workload chart.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;
use tauri::State;

use crate::errors::AppResult;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Single KPI card data.
#[derive(Debug, Serialize)]
pub struct KpiValue {
    pub key: String,
    pub value: i64,
    pub previous_value: i64,
    pub available: bool,
}

/// Top-level response for `get_dashboard_kpis`.
#[derive(Debug, Serialize)]
pub struct DashboardKpis {
    pub open_dis: KpiValue,
    pub open_wos: KpiValue,
    pub total_assets: KpiValue,
    pub overdue_items: KpiValue,
}

/// Single day data for the workload chart.
#[derive(Debug, Serialize)]
pub struct WorkloadDay {
    pub date: String,
    pub di_created: i64,
    pub wo_completed: i64,
    pub pm_due: i64,
}

/// Response for `get_dashboard_workload_chart`.
#[derive(Debug, Serialize)]
pub struct DashboardWorkloadChart {
    pub days: Vec<WorkloadDay>,
    pub period_days: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Commands
// ═══════════════════════════════════════════════════════════════════════════════

/// Returns KPI counts for the dashboard card grid.
///
/// - Open DIs: count of intervention_requests NOT in terminal states
/// - Open WOs: count of WOs not in terminal status
/// - Total Assets: count of equipment rows
/// - Overdue Items: DIs + PM occurrences currently overdue
///
/// Each KPI also returns a `previous_value` (same window shifted back)
/// so the frontend can compute the trend delta.
#[tauri::command]
pub async fn get_dashboard_kpis(state: State<'_, AppState>) -> AppResult<DashboardKpis> {
    let db = &state.db;

    // ── Open DIs (current) ────────────────────────────────────────────
    let open_di_count = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived')",
    )
    .await;

    // ── Previous-period open DIs (snapshot: created before 7 days ago) ──
    let prev_di_count = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived') \
         AND created_at <= datetime('now', '-7 days')",
    )
    .await;

    // ── Total assets (current) ────────────────────────────────────────
    let asset_count = count_scalar(db, "SELECT COUNT(*) AS cnt FROM equipment WHERE deleted_at IS NULL").await;
    let prev_asset_count = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM equipment WHERE deleted_at IS NULL \
         AND created_at <= datetime('now', '-7 days')",
    )
    .await;

    // ── Overdue DIs: in non-terminal state for > 7 days ───────────────
    let overdue_count = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived') \
         AND created_at <= datetime('now', '-7 days')",
    )
    .await;

    let prev_overdue = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived') \
         AND created_at <= datetime('now', '-14 days')",
    )
    .await;

    Ok(DashboardKpis {
        open_dis: KpiValue {
            key: "open_dis".into(),
            value: open_di_count,
            previous_value: prev_di_count,
            available: true,
        },
        open_wos: KpiValue {
            key: "open_wos".into(),
            value: count_scalar(
                db,
                "SELECT COUNT(*) AS cnt FROM work_orders wo \
                 JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE s.code NOT IN ('closed','cancelled')",
            )
            .await,
            previous_value: count_scalar(
                db,
                "SELECT COUNT(*) AS cnt FROM work_orders wo \
                 JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE s.code NOT IN ('closed','cancelled') \
                 AND wo.created_at <= datetime('now', '-7 days')",
            )
            .await,
            available: true,
        },
        total_assets: KpiValue {
            key: "total_assets".into(),
            value: asset_count,
            previous_value: prev_asset_count,
            available: true,
        },
        overdue_items: KpiValue {
            key: "overdue_items".into(),
            value: overdue_count
                + count_scalar(
                    db,
                    "SELECT COUNT(*) AS cnt FROM pm_occurrences \
                     WHERE status NOT IN ('completed','cancelled','missed') \
                     AND due_at IS NOT NULL \
                     AND due_at < datetime('now')",
                )
                .await,
            previous_value: prev_overdue
                + count_scalar(
                    db,
                    "SELECT COUNT(*) AS cnt FROM pm_occurrences \
                     WHERE status NOT IN ('completed','cancelled','missed') \
                     AND due_at IS NOT NULL \
                     AND due_at < datetime('now', '-7 days')",
                )
                .await,
            available: true,
        },
    })
}

/// Returns per-day workload data for the chart.
///
/// `period_days`: 7 or 30 (validated, defaults to 7 if invalid).
///
/// For each day in the period, counts:
/// - DIs created on that date
/// - WOs completed
/// - PM due
#[tauri::command]
pub async fn get_dashboard_workload_chart(
    period_days: i64,
    state: State<'_, AppState>,
) -> AppResult<DashboardWorkloadChart> {
    let db = &state.db;
    let period = if period_days == 30 { 30 } else { 7 };

    let mut days = Vec::with_capacity(period as usize);

    for i in (0..period).rev() {
        let date_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT date('now', ? || ' days') AS d",
                [(-i).into()],
            ))
            .await?;

        let date_str: String = date_row
            .as_ref()
            .and_then(|r| {
                use sea_orm::TryGetable;
                String::try_get(r, "", "d").ok()
            })
            .unwrap_or_default();

        // DI created on this date
        let di_created = count_scalar(
            db,
            &format!(
                "SELECT COUNT(*) AS cnt FROM intervention_requests \
                 WHERE date(created_at) = '{date_str}'"
            ),
        )
        .await;

        let wo_completed = count_scalar(
            db,
            &format!(
                "SELECT COUNT(*) AS cnt FROM work_orders wo \
                 JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE s.code = 'closed' AND date(wo.closed_at) = '{date_str}'"
            ),
        )
        .await;

        let pm_due = count_scalar(
            db,
            &format!(
                "SELECT COUNT(*) AS cnt FROM pm_occurrences \
                 WHERE due_at IS NOT NULL \
                 AND date(due_at) = '{date_str}' \
                 AND status NOT IN ('completed','cancelled','missed')"
            ),
        )
        .await;

        days.push(WorkloadDay {
            date: date_str,
            di_created,
            wo_completed,
            pm_due,
        });
    }

    Ok(DashboardWorkloadChart {
        days,
        period_days: period,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute a `SELECT COUNT(*) AS cnt` query and return the count.
/// Returns 0 if the table doesn't exist or the query fails.
async fn count_scalar(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
    let result = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await;

    match result {
        Ok(Some(row)) => {
            use sea_orm::TryGetable;
            i64::try_get(&row, "", "cnt").unwrap_or(0)
        }
        _ => 0,
    }
}
