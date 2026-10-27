//! Data integrity workbench (gap 06 sprint 02).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::data_integrity::{
    apply_data_integrity_repair, list_open_findings, list_waiver_approvers, run_data_integrity_detectors,
    waive_data_integrity_finding, ApplyDataIntegrityRepairInput, DataIntegrityFindingRow,
    WaiveDataIntegrityFindingInput, WaiverApproverOption,
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INTEGRITY_REPAIR,
        PermissionScope::Global
    );
    list_open_findings(&state.db, limit.unwrap_or(100)).await
}

/// The people who may be named as second person on a waiver (the same check as the waiver itself),
/// without the caller. Same rights as the findings list.
#[tauri::command]
pub async fn list_integrity_waiver_approvers(state: State<'_, AppState>) -> AppResult<Vec<WaiverApproverOption>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INTEGRITY_REPAIR,
        PermissionScope::Global
    );
    list_waiver_approvers(&state.db, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn run_data_integrity_detectors_cmd(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INTEGRITY_REPAIR,
        PermissionScope::Global
    );
    require_step_up!(state);
    run_data_integrity_detectors(&state.db).await
}

#[tauri::command]
pub async fn waive_data_integrity_finding_cmd(
    input: WaiveDataIntegrityFindingInput,
    state: State<'_, AppState>,
) -> AppResult<DataIntegrityFindingRow> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INTEGRITY_REPAIR,
        PermissionScope::Global
    );
    require_step_up!(state);
    waive_data_integrity_finding(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn apply_data_integrity_repair_cmd(
    input: ApplyDataIntegrityRepairInput,
    state: State<'_, AppState>,
) -> AppResult<DataIntegrityFindingRow> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INTEGRITY_REPAIR,
        PermissionScope::Global
    );
    require_step_up!(state);
    apply_data_integrity_repair(&state.db, input, i64::from(user.user_id)).await
}
