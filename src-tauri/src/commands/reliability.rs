//! Failure taxonomy IPC (PRD §6.10.1).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::reliability::domain::{
    CostOfFailureFilter, CostOfFailureRow, DeactivateFailureCodeInput, DismissRamDataQualityIssueInput,
    EquipmentMissingExposureRow, FailureCode, FailureCodeUpsertInput, FailureCodesFilter, FailureEvent,
    FailureEventsFilter, FailureHierarchy, FailureHierarchyUpsertInput, Iso14224DatasetCompleteness,
    RamDataQualityIssue, RamDataQualityIssuesFilter, RamEquipmentQualityBadge, RefreshReliabilityKpiSnapshotInput,
    ReliabilityAnalysisInputEvaluation, ReliabilityKpiSnapshot, ReliabilityKpiSnapshotsFilter, RuntimeExposureLog,
    RuntimeExposureLogsFilter, UpsertFailureEventInput, UpsertRuntimeExposureLogInput, UserDismissal,
    WoMissingFailureModeRow,
};
use crate::reliability::queries;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_failure_hierarchies(state: State<'_, AppState>) -> AppResult<Vec<FailureHierarchy>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_failure_hierarchies(&state.db).await
}

#[tauri::command]
pub async fn upsert_failure_hierarchy(
    input: FailureHierarchyUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<FailureHierarchy> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::upsert_failure_hierarchy(&state.db, input).await
}

#[tauri::command]
pub async fn list_failure_codes(
    filter: FailureCodesFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<FailureCode>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_failure_codes(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_failure_code(
    input: FailureCodeUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<FailureCode> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::upsert_failure_code(&state.db, input).await
}

#[tauri::command]
pub async fn deactivate_failure_code(
    input: DeactivateFailureCodeInput,
    state: State<'_, AppState>,
) -> AppResult<FailureCode> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::deactivate_failure_code(&state.db, input).await
}

#[tauri::command]
pub async fn list_failure_events(
    filter: FailureEventsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<FailureEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_failure_events(&state.db, filter).await
}

#[tauri::command]
pub async fn list_cost_of_failure(
    filter: CostOfFailureFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<CostOfFailureRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_cost_of_failure(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_failure_event(
    input: UpsertFailureEventInput,
    state: State<'_, AppState>,
) -> AppResult<FailureEvent> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::upsert_failure_event(&state.db, input).await
}

#[tauri::command]
pub async fn upsert_runtime_exposure_log(
    input: UpsertRuntimeExposureLogInput,
    state: State<'_, AppState>,
) -> AppResult<RuntimeExposureLog> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::upsert_runtime_exposure_log(&state.db, input).await
}

#[tauri::command]
pub async fn list_runtime_exposure_logs(
    filter: RuntimeExposureLogsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuntimeExposureLog>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_runtime_exposure_logs(&state.db, filter).await
}

#[tauri::command]
pub async fn evaluate_reliability_analysis_input(
    input: RefreshReliabilityKpiSnapshotInput,
    state: State<'_, AppState>,
) -> AppResult<ReliabilityAnalysisInputEvaluation> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::evaluate_reliability_analysis_input(&state.db, input).await
}

#[tauri::command]
pub async fn refresh_reliability_kpi_snapshot(
    input: RefreshReliabilityKpiSnapshotInput,
    state: State<'_, AppState>,
) -> AppResult<ReliabilityKpiSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::refresh_reliability_kpi_snapshot(&state.db, input).await
}

#[tauri::command]
pub async fn list_reliability_kpi_snapshots(
    filter: ReliabilityKpiSnapshotsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<ReliabilityKpiSnapshot>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_reliability_kpi_snapshots(&state.db, filter).await
}

#[tauri::command]
pub async fn get_reliability_kpi_snapshot(id: i64, state: State<'_, AppState>) -> AppResult<ReliabilityKpiSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::get_reliability_kpi_snapshot(&state.db, id).await
}

#[tauri::command]
pub async fn list_ram_data_quality_issues(
    filter: RamDataQualityIssuesFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<RamDataQualityIssue>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_ram_data_quality_issues(&state.db, filter, user.user_id).await
}

#[tauri::command]
pub async fn list_wos_missing_failure_mode(
    equipment_id: Option<i64>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<WoMissingFailureModeRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_wos_missing_failure_mode(&state.db, equipment_id, limit).await
}

#[tauri::command]
pub async fn list_equipment_missing_exposure_90d(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<EquipmentMissingExposureRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_equipment_missing_exposure_90d(&state.db, limit).await
}

#[tauri::command]
pub async fn get_ram_equipment_quality_badge(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<RamEquipmentQualityBadge> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::get_ram_equipment_quality_badge(&state.db, equipment_id, user.user_id).await
}

#[tauri::command]
pub async fn dismiss_ram_data_quality_issue(
    input: DismissRamDataQualityIssueInput,
    state: State<'_, AppState>,
) -> AppResult<UserDismissal> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::dismiss_ram_data_quality_issue(&state.db, user.user_id, input).await
}

#[tauri::command]
pub async fn iso_14224_failure_dataset_completeness(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Iso14224DatasetCompleteness> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::iso_14224_failure_dataset_completeness(&state.db, equipment_id).await
}
