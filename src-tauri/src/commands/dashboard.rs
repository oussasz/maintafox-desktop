//! Home page widgets that are not a role panel (`commands::home`): the per-user layout, the
//! activity chart, the open-requests-by-status chart and the indicator strip (`get_overview_kpis`).
//!
//! Every figure fails with an error when its query fails — never a silent 0.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::analytics::layout::{
    get_layout as get_analytics_layout, merge_overview_kpis_into_layout, save_layout as save_analytics_layout,
};
use crate::analytics::overview_kpis::{build_overview_kpis, OverviewKpisPayload};
use crate::auth::rbac::{check_permission, PermissionScope};
use crate::errors::{AppError, AppResult};
use crate::kpi_definitions;
use crate::require_session;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// One day of the activity chart. A series the person may not see is `None` (not 0).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WorkloadDay {
    pub date: String,
    /// Requests submitted that day (`submitted_at`).
    pub di_created: Option<i64>,
    /// Work orders closed that day (status `closed`, `closed_at`).
    pub wo_completed: Option<i64>,
    /// PM occurrences due that day, cancelled ones excluded.
    pub pm_due: Option<i64>,
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

/// Open requests of one status (`di::stats::open_backlog_by_status`).
#[derive(Debug, Serialize)]
pub struct DiStatusSegment {
    pub status: String,
    pub count: i64,
    pub avg_age_days: f64,
}

#[derive(Debug, Serialize)]
pub struct DashboardDiStatusChart {
    pub segments: Vec<DiStatusSegment>,
    pub available: bool,
}

/// Mirrors `DEFAULT_DASHBOARD_LAYOUT` in `shared/dashboard-layout.ts`.
pub const DEFAULT_DASHBOARD_LAYOUT_JSON: &str = r#"{"version":1,"widgets":[{"id":"my_work","order":0,"visible":true},{"id":"supervision","order":1,"visible":true},{"id":"planning","order":2,"visible":true},{"id":"kpis","order":3,"visible":true},{"id":"workload","order":4,"visible":true},{"id":"di_status","order":5,"visible":true}]}"#;

/// Periods of the activity chart (days).
pub const WORKLOAD_PERIODS: [i64; 2] = [7, 30];

fn validate_dashboard_layout_json(raw: &str) -> AppResult<()> {
    if raw.len() > 48_000 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_CMD_DASH_LAYOUT_INVALID: layout_json too large.".into(),
        ]));
    }
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| AppError::ValidationFailed(vec![format!("layout_json: {e}")]))?;
    let obj = v.as_object().ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "GATE_CMD_DASH_LAYOUT_INVALID: layout_json must be an object.".into()
        ])
    })?;
    if obj.get("version").and_then(|x| x.as_u64()) != Some(1) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_CMD_DASH_LAYOUT_INVALID: layout_json.version must be 1.".into(),
        ]));
    }
    let Some(widgets) = obj.get("widgets").and_then(|x| x.as_array()) else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_CMD_DASH_LAYOUT_INVALID: layout_json.widgets required.".into(),
        ]));
    };
    if widgets.is_empty() || widgets.len() > 32 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_CMD_DASH_LAYOUT_INVALID: layout_json.widgets length invalid.".into(),
        ]));
    }
    for w in widgets {
        let wo = w.as_object().ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_CMD_DASH_LAYOUT_INVALID: each widget must be an object.".into()
            ])
        })?;
        wo.get("id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .ok_or_else(|| {
                AppError::ValidationFailed(vec!["GATE_CMD_DASH_LAYOUT_INVALID: widget.id invalid.".into()])
            })?;
        wo.get("order").and_then(|x| x.as_i64()).ok_or_else(|| {
            AppError::ValidationFailed(vec!["GATE_CMD_DASH_LAYOUT_INVALID: widget.order invalid.".into()])
        })?;
        wo.get("visible").and_then(|x| x.as_bool()).ok_or_else(|| {
            AppError::ValidationFailed(vec!["GATE_CMD_DASH_LAYOUT_INVALID: widget.visible invalid.".into()])
        })?;
    }
    if let Some(cards) = obj.get("metric_cards") {
        let arr = cards.as_array().ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_CMD_DASH_LAYOUT_INVALID: layout_json.metric_cards must be an array.".into(),
            ])
        })?;
        if arr.len() > 16 {
            return Err(AppError::ValidationFailed(vec![
                "GATE_CMD_DASH_LAYOUT_INVALID: layout_json.metric_cards length invalid.".into(),
            ]));
        }
        for w in arr {
            let wo = w.as_object().ok_or_else(|| {
                AppError::ValidationFailed(vec![
                    "GATE_CMD_DASH_LAYOUT_INVALID: each metric_card must be an object.".into(),
                ])
            })?;
            wo.get("id")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty() && s.len() <= 64)
                .ok_or_else(|| {
                    AppError::ValidationFailed(vec!["GATE_CMD_DASH_LAYOUT_INVALID: metric_card.id invalid.".into()])
                })?;
            wo.get("order").and_then(|x| x.as_i64()).ok_or_else(|| {
                AppError::ValidationFailed(vec!["GATE_CMD_DASH_LAYOUT_INVALID: metric_card.order invalid.".into()])
            })?;
            wo.get("visible").and_then(|x| x.as_bool()).ok_or_else(|| {
                AppError::ValidationFailed(vec!["GATE_CMD_DASH_LAYOUT_INVALID: metric_card.visible invalid.".into()])
            })?;
        }
    }
    Ok(())
}

pub(crate) fn kpi_quality_badge(previous_value: i64, value: i64) -> Option<String> {
    if previous_value == 0 && value != 0 {
        Some(kpi_definitions::quality_badge::INSUFFICIENT_BASELINE.to_string())
    } else {
        None
    }
}

fn decode<T: sea_orm::TryGetable>(row: &sea_orm::QueryResult, col: &str) -> AppResult<T> {
    row.try_get::<T>("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("dashboard decode '{col}': {e}")))
}

/// Which series of the activity chart the person may see.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkloadSeries {
    pub di: bool,
    pub wo: bool,
    pub pm: bool,
}

/// The activity chart in one query: the last `period` days (today included), each with the
/// counts of the series the person may see.
pub async fn build_workload_chart(
    db: &impl ConnectionTrait,
    period: i64,
    series: WorkloadSeries,
) -> AppResult<DashboardWorkloadChart> {
    if !WORKLOAD_PERIODS.contains(&period) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_CMD_DASH_PERIOD_INVALID: period_days must be one of {WORKLOAD_PERIODS:?}."
        )]));
    }
    let col = |allowed: bool, sql: &str, name: &str| {
        if allowed {
            format!("({sql}) AS {name}")
        } else {
            format!("NULL AS {name}")
        }
    };
    let sql = format!(
        "WITH RECURSIVE days(d) AS ( \
           SELECT date('now', ?) UNION ALL SELECT date(d, '+1 day') FROM days WHERE d < date('now') \
         ) \
         SELECT days.d AS d, {di}, {wo}, {pm} FROM days ORDER BY days.d",
        di = col(
            series.di,
            "SELECT COUNT(*) FROM intervention_requests ir WHERE date(ir.submitted_at) = days.d",
            "di_created"
        ),
        wo = col(
            series.wo,
            "SELECT COUNT(*) FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
             WHERE s.code = 'closed' AND date(wo.closed_at) = days.d",
            "wo_completed"
        ),
        pm = col(
            series.pm,
            "SELECT COUNT(*) FROM pm_occurrences po \
             WHERE po.due_at IS NOT NULL AND date(po.due_at) = days.d AND po.status <> 'cancelled'",
            "pm_due"
        ),
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [format!("-{} days", period - 1).into()],
        ))
        .await?;
    let days = rows
        .iter()
        .map(|r| {
            Ok(WorkloadDay {
                date: decode(r, "d")?,
                di_created: decode(r, "di_created")?,
                wo_completed: decode(r, "wo_completed")?,
                pm_due: decode(r, "pm_due")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let any_series = series.di || series.wo || series.pm;
    let sparse = any_series
        && days
            .iter()
            .all(|d| d.di_created.unwrap_or(0) == 0 && d.wo_completed.unwrap_or(0) == 0 && d.pm_due.unwrap_or(0) == 0);
    Ok(DashboardWorkloadChart {
        days,
        period_days: period,
        quality_badge: sparse.then(|| kpi_definitions::quality_badge::SPARSE_WORKLOAD.to_string()),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Commands
// ═══════════════════════════════════════════════════════════════════════════════

/// Activity per day over `period_days` (7 or 30): requests submitted, work orders closed, PM due.
#[tauri::command]
pub async fn get_dashboard_workload_chart(
    period_days: i64,
    state: State<'_, AppState>,
) -> AppResult<DashboardWorkloadChart> {
    let user = require_session!(state);
    let db = &state.db;
    let series = WorkloadSeries {
        di: check_permission(
            db,
            user.user_id,
            crate::rbac::permissions::DI_VIEW,
            &PermissionScope::Global,
        )
        .await?,
        wo: check_permission(
            db,
            user.user_id,
            crate::rbac::permissions::OT_VIEW,
            &PermissionScope::Global,
        )
        .await?,
        pm: check_permission(
            db,
            user.user_id,
            crate::rbac::permissions::PM_VIEW,
            &PermissionScope::Global,
        )
        .await?,
    };
    build_workload_chart(db, period_days, series).await
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
    let layout_json = match row {
        Some(r) => decode::<String>(&r, "layout_json")?,
        None => DEFAULT_DASHBOARD_LAYOUT_JSON.to_string(),
    };
    // A layout saved by an older version that no longer validates falls back to the default one;
    // the page merges any saved order and visibility it can still read.
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

/// Open requests by status (`di::stats::open_backlog_by_status`, the DI page's open backlog).
#[tauri::command]
pub async fn get_dashboard_di_status_chart(state: State<'_, AppState>) -> AppResult<DashboardDiStatusChart> {
    let user = require_session!(state);
    if !check_permission(
        &state.db,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
        &PermissionScope::Global,
    )
    .await?
    {
        return Ok(DashboardDiStatusChart {
            segments: vec![],
            available: false,
        });
    }
    let backlog = crate::di::stats::open_backlog_by_status(&state.db, None).await?;
    Ok(DashboardDiStatusChart {
        segments: backlog
            .into_iter()
            .map(|b| DiStatusSegment {
                status: b.status,
                count: b.count,
                avg_age_days: b.avg_age_days,
            })
            .collect(),
        available: true,
    })
}

// Overview KPI strip (Vue d'ensemble)

#[derive(Debug, Serialize)]
pub struct OverviewKpiLayoutPayload {
    pub layout_json: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveOverviewKpiLayoutInput {
    pub overview_kpis_json: String,
}

#[tauri::command]
pub async fn get_overview_kpis(state: State<'_, AppState>) -> AppResult<OverviewKpisPayload> {
    let user = require_session!(state);
    let db = &state.db;
    let uid = user.user_id;

    let can_di = check_permission(db, uid, crate::rbac::permissions::DI_VIEW, &PermissionScope::Global).await?;
    let can_ot = check_permission(db, uid, crate::rbac::permissions::OT_VIEW, &PermissionScope::Global).await?;
    let can_eq = check_permission(db, uid, crate::rbac::permissions::EQ_VIEW, &PermissionScope::Global).await?;
    let can_pm = check_permission(db, uid, crate::rbac::permissions::PM_VIEW, &PermissionScope::Global).await?;
    let can_rep = check_permission(db, uid, crate::rbac::permissions::REP_VIEW, &PermissionScope::Global).await?;
    let can_inv = check_permission(db, uid, crate::rbac::permissions::INV_VIEW, &PermissionScope::Global).await?;

    build_overview_kpis(db, can_di, can_ot, can_eq, can_pm, can_rep, can_inv).await
}

#[tauri::command]
pub async fn get_overview_kpi_layout(state: State<'_, AppState>) -> AppResult<OverviewKpiLayoutPayload> {
    let user = require_session!(state);
    let layout_json = get_analytics_layout(&state.db, i64::from(user.user_id)).await?;
    Ok(OverviewKpiLayoutPayload { layout_json })
}

#[tauri::command]
pub async fn save_overview_kpi_layout(input: SaveOverviewKpiLayoutInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    let overview: serde_json::Value = serde_json::from_str(&input.overview_kpis_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("overview_kpis_json: {e}")]))?;
    let existing = get_analytics_layout(&state.db, i64::from(user.user_id)).await?;
    let merged = merge_overview_kpis_into_layout(&existing, overview)?;
    save_analytics_layout(&state.db, i64::from(user.user_id), &merged).await
}
