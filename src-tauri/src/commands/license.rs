use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::license::domain::{
    ApplyAdminLicenseActionInput, ApplyAdminLicenseActionResult, ApplyLicensingCompromiseResponseInput,
    ApplyLicensingCompromiseResponseResult, LicenseStatusView, LicenseTraceEvent,
};
use crate::license::queries;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[tauri::command]
pub async fn get_license_enforcement_status(state: State<'_, AppState>) -> AppResult<LicenseStatusView> {
    let user = require_session!(state);
    require_permission!(state, &user, "lic.view", PermissionScope::Global);
    queries::get_license_status_view(&state.db, user.user_id).await
}

#[tauri::command]
pub async fn apply_admin_license_action(
    input: ApplyAdminLicenseActionInput,
    state: State<'_, AppState>,
) -> AppResult<ApplyAdminLicenseActionResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "lic.manage", PermissionScope::Global);
    require_step_up!(state);
    queries::apply_admin_license_action(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn list_license_trace_events(
    limit: Option<i64>,
    correlation_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<LicenseTraceEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "lic.view", PermissionScope::Global);
    queries::list_license_trace_events(&state.db, limit, correlation_id).await
}

#[tauri::command]
pub async fn apply_licensing_compromise_response(
    input: ApplyLicensingCompromiseResponseInput,
    state: State<'_, AppState>,
) -> AppResult<ApplyLicensingCompromiseResponseResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "lic.manage", PermissionScope::Global);
    require_step_up!(state);
    queries::apply_licensing_compromise_response(&state.db, input, Some(i64::from(user.user_id))).await
}
