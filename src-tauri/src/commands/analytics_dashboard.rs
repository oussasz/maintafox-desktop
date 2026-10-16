//! Executive analytics dashboard IPC (RAMS & Analytics hub).

use tauri::State;

use crate::analytics::layout::{get_layout, save_layout};
use crate::analytics::projection::{build_executive_dashboard, AnalyticsExecutiveDashboard, AnalyticsExecutiveFilter};
use crate::auth::rbac::{check_permission, PermissionScope};
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct AnalyticsDashboardLayoutPayload {
    pub layout_json: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveAnalyticsDashboardLayoutInput {
    pub layout_json: String,
}

#[tauri::command]
pub async fn get_analytics_executive_dashboard(
    filter: AnalyticsExecutiveFilter,
    state: State<'_, AppState>,
) -> AppResult<AnalyticsExecutiveDashboard> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_VIEW,
        PermissionScope::Global
    );
    let db = &state.db;
    let uid = user.user_id;
    let can_di = check_permission(db, uid, crate::rbac::permissions::DI_VIEW, &PermissionScope::Global).await?;
    let can_ot = check_permission(db, uid, crate::rbac::permissions::OT_VIEW, &PermissionScope::Global).await?;
    let can_eq = check_permission(db, uid, crate::rbac::permissions::EQ_VIEW, &PermissionScope::Global).await?;
    let can_pm = check_permission(db, uid, crate::rbac::permissions::PM_VIEW, &PermissionScope::Global).await?;
    let can_rep = check_permission(db, uid, crate::rbac::permissions::REP_VIEW, &PermissionScope::Global).await?;
    let can_fin = check_permission(db, uid, crate::rbac::permissions::FIN_VIEW, &PermissionScope::Global).await?;

    build_executive_dashboard(db, &filter, can_di, can_ot, can_eq, can_pm, can_rep, can_fin).await
}

#[tauri::command]
pub async fn get_analytics_dashboard_layout(state: State<'_, AppState>) -> AppResult<AnalyticsDashboardLayoutPayload> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_VIEW,
        PermissionScope::Global
    );
    let layout_json = get_layout(&state.db, i64::from(user.user_id)).await?;
    Ok(AnalyticsDashboardLayoutPayload { layout_json })
}

#[tauri::command]
pub async fn save_analytics_dashboard_layout(
    input: SaveAnalyticsDashboardLayoutInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_VIEW,
        PermissionScope::Global
    );
    save_layout(&state.db, i64::from(user.user_id), &input.layout_json).await
}
