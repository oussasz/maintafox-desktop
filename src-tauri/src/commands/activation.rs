use tauri::State;

use crate::activation::domain::{
    ApplyMachineActivationInput, MachineActivationApplyResult, MachineActivationDiagnostics, MachineActivationStatus,
    OfflineActivationDecision, RebindMachineActivationInput, RebindMachineActivationResult, RotateActivationSecretInput,
    RotateActivationSecretResult,
};
use crate::activation::queries;
use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[tauri::command]
pub async fn apply_machine_activation_contract(
    input: ApplyMachineActivationInput,
    state: State<'_, AppState>,
) -> AppResult<MachineActivationApplyResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "act.manage", PermissionScope::Global);
    require_step_up!(state);
    queries::apply_machine_activation(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn get_machine_activation_status(state: State<'_, AppState>) -> AppResult<MachineActivationStatus> {
    let user = require_session!(state);
    require_permission!(state, &user, "act.view", PermissionScope::Global);
    queries::get_machine_activation_status(&state.db).await
}

#[tauri::command]
pub async fn evaluate_offline_activation_policy(state: State<'_, AppState>) -> AppResult<OfflineActivationDecision> {
    let user = require_session!(state);
    let fingerprint = crate::auth::device::derive_device_fingerprint().unwrap_or_else(|_| "unknown".to_string());
    queries::evaluate_offline_activation_policy(&state.db, user.user_id, &fingerprint).await
}

#[tauri::command]
pub async fn rotate_activation_binding_secret(
    input: RotateActivationSecretInput,
    state: State<'_, AppState>,
) -> AppResult<RotateActivationSecretResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "act.manage", PermissionScope::Global);
    require_step_up!(state);
    queries::rotate_activation_binding_secret(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn get_machine_activation_diagnostics(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<MachineActivationDiagnostics> {
    let user = require_session!(state);
    require_permission!(state, &user, "act.view", PermissionScope::Global);
    queries::get_machine_activation_diagnostics(&state.db, limit).await
}

#[tauri::command]
pub async fn request_machine_activation_rebind(
    input: RebindMachineActivationInput,
    state: State<'_, AppState>,
) -> AppResult<RebindMachineActivationResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "act.manage", PermissionScope::Global);
    require_step_up!(state);
    queries::request_machine_rebind(&state.db, input, Some(i64::from(user.user_id))).await
}
