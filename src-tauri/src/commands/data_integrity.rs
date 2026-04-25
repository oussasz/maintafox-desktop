//! Data integrity workbench (gap 06 sprint 02).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::data_integrity::{
    apply_data_integrity_repair, list_open_findings, run_data_integrity_detectors,
    waive_data_integrity_finding, ApplyDataIntegrityRepairInput, DataIntegrityFindingRow,
    WaiveDataIntegrityFindingInput,
};
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[tauri::command]
pub async fn list_data_integrity_findings(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<DataIntegrityFindingRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.manage", PermissionScope::Global);
    require_permission!(state, &user, "integrity.repair", PermissionScope::Global);
    list_open_findings(&state.db, limit.unwrap_or(100)).await
}

#[tauri::command]
pub async fn run_data_integrity_detectors_cmd(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.manage", PermissionScope::Global);
    require_permission!(state, &user, "integrity.repair", PermissionScope::Global);
    require_step_up!(state);
    run_data_integrity_detectors(&state.db).await
}

#[tauri::command]
pub async fn waive_data_integrity_finding_cmd(
    input: WaiveDataIntegrityFindingInput,
    state: State<'_, AppState>,
) -> AppResult<DataIntegrityFindingRow> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.manage", PermissionScope::Global);
    require_permission!(state, &user, "integrity.repair", PermissionScope::Global);
    require_step_up!(state);
    waive_data_integrity_finding(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn apply_data_integrity_repair_cmd(
    input: ApplyDataIntegrityRepairInput,
    state: State<'_, AppState>,
) -> AppResult<DataIntegrityFindingRow> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.manage", PermissionScope::Global);
    require_permission!(state, &user, "integrity.repair", PermissionScope::Global);
    require_step_up!(state);
    apply_data_integrity_repair(&state.db, input, i64::from(user.user_id)).await
}
