//! Dashboard KPI and workload chart IPC commands.
//!
//! Phase 2 - Sub-phase 00 - File 02 - Sprint S4.
//!
//! Provides aggregated counts for the Dashboard KPI grid and workload chart.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::rbac::{check_permission, PermissionScope};
use crate::errors::{AppError, AppResult};
use crate::kpi_definitions;
use crate::state::AppState;
use crate::require_session;

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
    pub quality_badge: Option<String>,
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
    pub quality_badge: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DashboardLayoutPayload {
    pub layout_json: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveDashboardLayoutInput {
    pub layout_json: String,
}

#[derive(Debug, Serialize)]
pub struct DiStatusSegment {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct DashboardDiStatusChart {
    pub segments: Vec<DiStatusSegment>,
    pub available: bool,
}

#[derive(Debug, Serialize)]
pub struct DashboardReliabilitySnapshotSummary {
    pub available: bool,
    pub snapshot_count: i64,
    pub avg_data_quality_score: Option<f64>,
    pub avg_mtbf_hours: Option<f64>,
    pub total_failure_events: i64,
}

#[derive(Debug, Serialize)]
pub struct KpiSqlSample {
    pub key: String,
    pub value: i64,
    pub sql: String,
    pub sample_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct DashboardKpiValidation {
    pub samples: Vec<KpiSqlSample>,
    pub overdue_items_total: i64,
}

pub const DEFAULT_DASHBOARD_LAYOUT_JSON: &str = r#"{"version":1,"widgets":[{"id":"kpis","order":0,"visible":true},{"id":"workload","order":1,"visible":true},{"id":"di_status","order":2,"visible":true},{"id":"reliability_snapshot","order":3,"visible":true}]}"#;

fn validate_dashboard_layout_json(raw: &str) -> AppResult<()> {
    if raw.len() > 48_000 {
        return Err(AppError::ValidationFailed(vec!["layout_json too large.".into()]));
    }
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| AppError::ValidationFailed(vec![format!("layout_json: {e}")]))?;
    let obj = v
        .as_object()
        .ok_or_else(|| AppError::ValidationFailed(vec!["layout_json must be an object.".into()]))?;
    if obj.get("version").and_then(|x| x.as_u64()) != Some(1) {
        return Err(AppError::ValidationFailed(vec!["layout_json.version must be 1.".into()]));
    }
    let Some(widgets) = obj.get("widgets").and_then(|x| x.as_array()) else {
        return Err(AppError::ValidationFailed(vec!["layout_json.widgets required.".into()]));
    };
    if widgets.is_empty() || widgets.len() > 32 {
        return Err(AppError::ValidationFailed(vec!["layout_json.widgets length invalid.".into()]));
    }
    for w in widgets {
        let wo = w
            .as_object()
            .ok_or_else(|| AppError::ValidationFailed(vec!["each widget must be an object.".into()]))?;
        wo.get("id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .ok_or_else(|| AppError::ValidationFailed(vec!["widget.id invalid.".into()]))?;
        wo.get("order")
            .and_then(|x| x.as_i64())
            .ok_or_else(|| AppError::ValidationFailed(vec!["widget.order invalid.".into()]))?;
        wo.get("visible")
            .and_then(|x| x.as_bool())
            .ok_or_else(|| AppError::ValidationFailed(vec!["widget.visible invalid.".into()]))?;
    }
    Ok(())
}

fn kpi_quality_badge(previous_value: i64, value: i64) -> Option<String> {
    if previous_value == 0 && value != 0 {
        Some(kpi_definitions::quality_badge::INSUFFICIENT_BASELINE.to_string())
    } else {
        None
    }
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

    let open_wos_val = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code NOT IN ('closed','cancelled')",
    )
    .await;
    let open_wos_prev = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code NOT IN ('closed','cancelled') \
         AND wo.created_at <= datetime('now', '-7 days')",
    )
    .await;

    let overdue_val = overdue_count
        + count_scalar(
            db,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences \
             WHERE status NOT IN ('completed','cancelled','missed') \
             AND due_at IS NOT NULL \
             AND due_at < datetime('now')",
        )
        .await;
    let overdue_prev = prev_overdue
        + count_scalar(
            db,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences \
             WHERE status NOT IN ('completed','cancelled','missed') \
             AND due_at IS NOT NULL \
             AND due_at < datetime('now', '-7 days')",
        )
        .await;

    Ok(DashboardKpis {
        open_dis: KpiValue {
            key: kpi_definitions::dashboard::OPEN_DIS.into(),
            value: open_di_count,
            previous_value: prev_di_count,
            available: true,
            quality_badge: kpi_quality_badge(prev_di_count, open_di_count),
        },
        open_wos: KpiValue {
            key: kpi_definitions::dashboard::OPEN_WOS.into(),
            value: open_wos_val,
            previous_value: open_wos_prev,
            available: true,
            quality_badge: kpi_quality_badge(open_wos_prev, open_wos_val),
        },
        total_assets: KpiValue {
            key: kpi_definitions::dashboard::TOTAL_ASSETS.into(),
            value: asset_count,
            previous_value: prev_asset_count,
            available: true,
            quality_badge: kpi_quality_badge(prev_asset_count, asset_count),
        },
        overdue_items: KpiValue {
            key: kpi_definitions::dashboard::OVERDUE_ITEMS.into(),
            value: overdue_val,
            previous_value: overdue_prev,
            available: true,
            quality_badge: kpi_quality_badge(overdue_prev, overdue_val),
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

    let sparse = days
        .iter()
        .all(|d| d.di_created == 0 && d.wo_completed == 0 && d.pm_due == 0);
    let quality_badge = if sparse {
        Some(kpi_definitions::quality_badge::SPARSE_WORKLOAD.to_string())
    } else {
        None
    };

    Ok(DashboardWorkloadChart {
        days,
        period_days: period,
        quality_badge,
    })
}

#[tauri::command]
pub async fn get_dashboard_layout(state: State<'_, AppState>) -> AppResult<DashboardLayoutPayload> {
    let user = require_session!(state);
    let uid = i64::from(user.user_id);
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT layout_json FROM user_dashboard_layouts WHERE user_id = ?",
            [uid.into()],
        ))
        .await?;
    let layout_json = if let Some(r) = row {
        use sea_orm::TryGetable;
        String::try_get(&r, "", "layout_json").unwrap_or_else(|_| DEFAULT_DASHBOARD_LAYOUT_JSON.to_string())
    } else {
        DEFAULT_DASHBOARD_LAYOUT_JSON.to_string()
    };
    if validate_dashboard_layout_json(&layout_json).is_err() {
        return Ok(DashboardLayoutPayload {
            layout_json: DEFAULT_DASHBOARD_LAYOUT_JSON.to_string(),
        });
    }
    Ok(DashboardLayoutPayload { layout_json })
}

#[tauri::command]
pub async fn save_dashboard_layout(
    input: SaveDashboardLayoutInput,
    state: State<'_, AppState>,
) -> AppResult<DashboardLayoutPayload> {
    let user = require_session!(state);
    validate_dashboard_layout_json(&input.layout_json)?;
    let uid = i64::from(user.user_id);
    let now = chrono::Utc::now().to_rfc3339();
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_dashboard_layouts (user_id, layout_json, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET layout_json = excluded.layout_json, updated_at = excluded.updated_at",
            [uid.into(), input.layout_json.clone().into(), now.into()],
        ))
        .await?;
    Ok(DashboardLayoutPayload {
        layout_json: input.layout_json,
    })
}

#[tauri::command]
pub async fn get_dashboard_di_status_chart(state: State<'_, AppState>) -> AppResult<DashboardDiStatusChart> {
    let user = require_session!(state);
    if !check_permission(&state.db, user.user_id, "di.view", &PermissionScope::Global).await? {
        return Ok(DashboardDiStatusChart {
            segments: vec![],
            available: false,
        });
    }
    let rows = state
        .db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT status, COUNT(*) AS cnt FROM intervention_requests GROUP BY status ORDER BY cnt DESC"
                .to_string(),
        ))
        .await?;
    use sea_orm::TryGetable;
    let mut segments = Vec::new();
    for r in rows {
        let status: String = String::try_get(&r, "", "status").unwrap_or_default();
        let count: i64 = i64::try_get(&r, "", "cnt").unwrap_or(0);
        segments.push(DiStatusSegment { status, count });
    }
    Ok(DashboardDiStatusChart {
        segments,
        available: true,
    })
}

#[tauri::command]
pub async fn get_dashboard_reliability_snapshot_summary(
    state: State<'_, AppState>,
) -> AppResult<DashboardReliabilitySnapshotSummary> {
    let user = require_session!(state);
    if !check_permission(&state.db, user.user_id, "rep.view", &PermissionScope::Global).await? {
        return Ok(DashboardReliabilitySnapshotSummary {
            available: false,
            snapshot_count: 0,
            avg_data_quality_score: None,
            avg_mtbf_hours: None,
            total_failure_events: 0,
        });
    }
    let row = state
        .db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            r"SELECT COUNT(*) AS snapshot_count,
                     AVG(data_quality_score) AS avg_dq,
                     AVG(CASE WHEN mtbf IS NOT NULL AND mtbf > 0 THEN mtbf END) AS avg_mtbf,
                     SUM(event_count) AS total_ev
              FROM reliability_kpi_snapshots
              WHERE datetime(period_end) >= datetime('now', '-365 days')"
                .to_string(),
        ))
        .await?;
    let Some(r) = row else {
        return Ok(DashboardReliabilitySnapshotSummary {
            available: true,
            snapshot_count: 0,
            avg_data_quality_score: None,
            avg_mtbf_hours: None,
            total_failure_events: 0,
        });
    };
    use sea_orm::TryGetable;
    let snapshot_count: i64 = i64::try_get(&r, "", "snapshot_count").unwrap_or(0);
    let avg_dq: Option<f64> = f64::try_get(&r, "", "avg_dq").ok();
    let avg_mtbf: Option<f64> = f64::try_get(&r, "", "avg_mtbf").ok();
    let total_ev: i64 = i64::try_get(&r, "", "total_ev").unwrap_or(0);
    Ok(DashboardReliabilitySnapshotSummary {
        available: true,
        snapshot_count,
        avg_data_quality_score: avg_dq,
        avg_mtbf_hours: avg_mtbf,
        total_failure_events: total_ev,
    })
}

/// Same KPI counts as [`get_dashboard_kpis`], plus the exact count SQL and a few row ids per slice for cross-checking in SQLite.
#[tauri::command]
pub async fn get_dashboard_kpi_validation(state: State<'_, AppState>) -> AppResult<DashboardKpiValidation> {
    let db = &state.db;

    const SQL_OPEN_DIS: &str = "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived')";
    const SAMPLE_OPEN_DIS: &str = "SELECT id FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived') ORDER BY id LIMIT 5";

    const SQL_OPEN_WOS: &str = "SELECT COUNT(*) AS cnt FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code NOT IN ('closed','cancelled')";
    const SAMPLE_OPEN_WOS: &str = "SELECT wo.id AS id FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code NOT IN ('closed','cancelled') ORDER BY wo.id LIMIT 5";

    const SQL_ASSETS: &str = "SELECT COUNT(*) AS cnt FROM equipment WHERE deleted_at IS NULL";
    const SAMPLE_ASSETS: &str = "SELECT id FROM equipment WHERE deleted_at IS NULL ORDER BY id LIMIT 5";

    const SQL_OVERDUE_DI: &str = "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived') \
         AND created_at <= datetime('now', '-7 days')";
    const SAMPLE_OVERDUE_DI: &str = "SELECT id FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order',\
         'closed_as_non_executable','archived') \
         AND created_at <= datetime('now', '-7 days') ORDER BY id LIMIT 5";

    const SQL_OVERDUE_PM: &str = "SELECT COUNT(*) AS cnt FROM pm_occurrences \
         WHERE status NOT IN ('completed','cancelled','missed') \
         AND due_at IS NOT NULL \
         AND due_at < datetime('now')";
    const SAMPLE_OVERDUE_PM: &str = "SELECT id FROM pm_occurrences \
         WHERE status NOT IN ('completed','cancelled','missed') \
         AND due_at IS NOT NULL \
         AND due_at < datetime('now') ORDER BY id LIMIT 5";

    let open_dis = count_scalar(db, SQL_OPEN_DIS).await;
    let open_wos = count_scalar(db, SQL_OPEN_WOS).await;
    let total_assets = count_scalar(db, SQL_ASSETS).await;
    let overdue_di = count_scalar(db, SQL_OVERDUE_DI).await;
    let overdue_pm = count_scalar(db, SQL_OVERDUE_PM).await;
    let overdue_items_total = overdue_di + overdue_pm;

    let mut samples = vec![
        KpiSqlSample {
            key: kpi_definitions::dashboard::OPEN_DIS.into(),
            value: open_dis,
            sql: SQL_OPEN_DIS.into(),
            sample_ids: sample_ids(db, SAMPLE_OPEN_DIS).await,
        },
        KpiSqlSample {
            key: kpi_definitions::dashboard::OPEN_WOS.into(),
            value: open_wos,
            sql: SQL_OPEN_WOS.into(),
            sample_ids: sample_ids(db, SAMPLE_OPEN_WOS).await,
        },
        KpiSqlSample {
            key: kpi_definitions::dashboard::TOTAL_ASSETS.into(),
            value: total_assets,
            sql: SQL_ASSETS.into(),
            sample_ids: sample_ids(db, SAMPLE_ASSETS).await,
        },
        KpiSqlSample {
            key: format!("{}.di_stale_window", kpi_definitions::dashboard::OVERDUE_ITEMS),
            value: overdue_di,
            sql: SQL_OVERDUE_DI.into(),
            sample_ids: sample_ids(db, SAMPLE_OVERDUE_DI).await,
        },
        KpiSqlSample {
            key: format!("{}.pm_past_due", kpi_definitions::dashboard::OVERDUE_ITEMS),
            value: overdue_pm,
            sql: SQL_OVERDUE_PM.into(),
            sample_ids: sample_ids(db, SAMPLE_OVERDUE_PM).await,
        },
    ];

    samples.push(KpiSqlSample {
        key: kpi_definitions::dashboard::OVERDUE_ITEMS.into(),
        value: overdue_items_total,
        sql: "overdue_items = di_stale_window + pm_past_due (same SQL as get_dashboard_kpis).".into(),
        sample_ids: vec![],
    });

    Ok(DashboardKpiValidation {
        samples,
        overdue_items_total,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

async fn sample_ids(db: &sea_orm::DatabaseConnection, sql: &str) -> Vec<i64> {
    let Ok(rows) = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
    else {
        return vec![];
    };
    use sea_orm::TryGetable;
    let mut out = Vec::new();
    for r in rows {
        if let Ok(id) = i64::try_get(&r, "", "id") {
            out.push(id);
        }
    }
    out
}

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
