//! Preventive maintenance IPC commands.

use tauri::State;

use crate::auth::rbac::{self, PermissionScope};
use crate::auth::session_manager::AuthenticatedUser;
use crate::errors::{AppError, AppResult};
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, ExecutePmOccurrenceInput, ExecutePmOccurrenceResult,
    GeneratePmOccurrencesInput, GeneratePmOccurrencesResult, PmDueMetrics, PmExecution, PmExecutionFilter, PmFinding,
    PmGenerationStatus, PmGovernanceKpiInput, PmGovernanceKpiReport, PmOccurrence, PmOccurrenceFilter, PmPlan,
    PmPlanFilter, PmPlanVersion, PmPlanningReadinessInput, PmPlanningReadinessProjection, PmRecurringFinding,
    PmRecurringFindingsInput, PublishPmPlanVersionInput, TransitionPmOccurrenceInput, TransitionPmPlanLifecycleInput,
    UpdatePmGenerationSettingsInput, UpdatePmPlanInput, UpdatePmPlanVersionInput,
};
use crate::pm::domain::{
    PmAssetMeter, PmComplianceInput, PmComplianceReport, PmScopeAsset, PmScopeOption, RecordPmMeterReadingInput,
};
use crate::pm::generation_control;
use crate::pm::queries;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_pm_plans(filter: PmPlanFilter, state: State<'_, AppState>) -> AppResult<Vec<PmPlan>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_plans(&state.db, filter).await
}

#[tauri::command]
pub async fn get_pm_plan(plan_id: i64, state: State<'_, AppState>) -> AppResult<PmPlan> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::get_pm_plan(&state.db, plan_id).await
}

#[tauri::command]
pub async fn create_pm_plan(input: CreatePmPlanInput, state: State<'_, AppState>) -> AppResult<PmPlan> {
    let user = require_session!(state);
    require_pm_create_or_manage(&state, &user).await?;
    queries::create_pm_plan(&state.db, input).await
}

#[tauri::command]
pub async fn update_pm_plan(
    plan_id: i64,
    expected_row_version: i64,
    input: UpdatePmPlanInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlan> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    queries::update_pm_plan(&state.db, plan_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn transition_pm_plan_lifecycle(
    input: TransitionPmPlanLifecycleInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlan> {
    let user = require_session!(state);
    let next = input.next_status.trim().to_lowercase();
    if next == "approved" {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::PM_APPROVE,
            PermissionScope::Global
        );
        enforce_pm_capability(&state, &user, crate::rbac::permissions::PM_APPROVE).await?;
    } else {
        require_pm_edit_or_manage(&state, &user).await?;
    }
    queries::transition_pm_plan_lifecycle(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_plan_versions(pm_plan_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PmPlanVersion>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_plan_versions(&state.db, pm_plan_id).await
}

#[tauri::command]
pub async fn create_pm_plan_version(
    pm_plan_id: i64,
    input: CreatePmPlanVersionInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlanVersion> {
    let user = require_session!(state);
    require_pm_create_or_manage(&state, &user).await?;
    queries::create_pm_plan_version(&state.db, pm_plan_id, input).await
}

#[tauri::command]
pub async fn update_pm_plan_version(
    version_id: i64,
    expected_row_version: i64,
    input: UpdatePmPlanVersionInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlanVersion> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    queries::update_pm_plan_version(&state.db, version_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn publish_pm_plan_version(
    input: PublishPmPlanVersionInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlanVersion> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    queries::publish_pm_plan_version(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_occurrences(
    filter: PmOccurrenceFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmOccurrence>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_occurrences(&state.db, filter).await
}

#[tauri::command]
pub async fn generate_pm_occurrences(
    input: GeneratePmOccurrencesInput,
    state: State<'_, AppState>,
) -> AppResult<GeneratePmOccurrencesResult> {
    let user = require_session!(state);
    require_pm_create_or_manage(&state, &user).await?;
    generation_control::run_generation_with_audit(&state.db, input, "manual", false).await
}

#[tauri::command]
pub async fn get_pm_generation_status(state: State<'_, AppState>) -> AppResult<PmGenerationStatus> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    generation_control::get_pm_generation_status(&state.db).await
}

#[tauri::command]
pub async fn update_pm_generation_settings(
    input: UpdatePmGenerationSettingsInput,
    state: State<'_, AppState>,
) -> AppResult<PmGenerationStatus> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    let _ = generation_control::update_pm_generation_settings(&state.db, input).await?;
    generation_control::get_pm_generation_status(&state.db).await
}

#[tauri::command]
pub async fn transition_pm_occurrence(
    mut input: TransitionPmOccurrenceInput,
    state: State<'_, AppState>,
) -> AppResult<PmOccurrence> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    if input.actor_id.is_none() {
        input.actor_id = Some(i64::from(user.user_id));
    }
    queries::transition_pm_occurrence(&state.db, input).await
}

#[tauri::command]
pub async fn get_pm_due_metrics(state: State<'_, AppState>) -> AppResult<PmDueMetrics> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::get_pm_due_metrics(&state.db).await
}

#[tauri::command]
pub async fn list_pm_planning_readiness(
    input: PmPlanningReadinessInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlanningReadinessProjection> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_planning_readiness(&state.db, input).await
}

#[tauri::command]
pub async fn get_pm_governance_kpi_report(
    input: PmGovernanceKpiInput,
    state: State<'_, AppState>,
) -> AppResult<PmGovernanceKpiReport> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::get_pm_governance_kpi_report(&state.db, input).await
}
#[tauri::command]
pub async fn execute_pm_occurrence(
    mut input: ExecutePmOccurrenceInput,
    state: State<'_, AppState>,
) -> AppResult<ExecutePmOccurrenceResult> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    if input.actor_id.is_none() {
        input.actor_id = Some(i64::from(user.user_id));
    }
    queries::execute_pm_occurrence(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_executions(filter: PmExecutionFilter, state: State<'_, AppState>) -> AppResult<Vec<PmExecution>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_executions(&state.db, filter).await
}

#[tauri::command]
pub async fn list_pm_findings(execution_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PmFinding>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_findings(&state.db, execution_id).await
}

#[tauri::command]
pub async fn list_pm_recurring_findings(
    input: PmRecurringFindingsInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmRecurringFinding>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_recurring_findings(&state.db, input).await
}

#[tauri::command]
pub async fn get_pm_occurrence(occurrence_id: i64, state: State<'_, AppState>) -> AppResult<PmOccurrence> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::get_pm_occurrence_by_id(&state.db, occurrence_id).await
}

/// PM compliance % (SMRP 5.4.1 style) for a period; also read by analytics.
#[tauri::command]
pub async fn get_pm_compliance(input: PmComplianceInput, state: State<'_, AppState>) -> AppResult<PmComplianceReport> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    crate::pm::compliance::get_pm_compliance(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_scope_options(scope_type: String, state: State<'_, AppState>) -> AppResult<Vec<PmScopeOption>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    crate::pm::plan_tools::list_pm_scope_options(&state.db, &scope_type).await
}

#[tauri::command]
pub async fn preview_pm_scope_assets(
    scope_type: String,
    scope_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmScopeAsset>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    crate::pm::plan_tools::preview_pm_scope_assets(&state.db, &scope_type, scope_id).await
}

#[tauri::command]
pub async fn list_pm_asset_meters(equipment_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PmAssetMeter>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    crate::pm::plan_tools::list_pm_asset_meters(&state.db, equipment_id).await
}

/// Meter reading entered from the PM workspace (rollover / replacement stated by the user).
#[tauri::command]
pub async fn record_pm_meter_reading(
    input: RecordPmMeterReadingInput,
    state: State<'_, AppState>,
) -> AppResult<PmAssetMeter> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    crate::pm::plan_tools::record_pm_meter_reading(&state.db, input, user.user_id).await
}

/// "Revise the plan" from a finding: draft version with the finding as change reason.
#[tauri::command]
pub async fn create_pm_revision_from_finding(finding_id: i64, state: State<'_, AppState>) -> AppResult<PmPlanVersion> {
    let user = require_session!(state);
    require_pm_create_or_manage(&state, &user).await?;
    crate::pm::plan_tools::create_pm_revision_from_finding(&state.db, finding_id).await
}

async fn enforce_pm_capability(state: &State<'_, AppState>, user: &AuthenticatedUser, perm: &str) -> AppResult<()> {
    crate::entitlements::queries::enforce_capability_for_permission(&state.db, perm).await?;
    crate::license::queries::enforce_permission_matrix(&state.db, user.user_id, perm).await?;
    Ok(())
}

/// Require `pm.create` (canonical). Holders of legacy `pm.manage` are migrated to
/// `pm.create`/`pm.edit`/`pm.delete` by the RBAC normalization migration.
pub(crate) async fn require_pm_create_or_manage(
    state: &State<'_, AppState>,
    user: &AuthenticatedUser,
) -> AppResult<()> {
    let scope = PermissionScope::Global;
    if rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::PM_CREATE,
        &scope,
    )
    .await?
    {
        return enforce_pm_capability(state, user, crate::rbac::permissions::PM_CREATE).await;
    }
    Err(AppError::PermissionDenied(format!(
        "Permission requise : {}",
        crate::rbac::permissions::PM_CREATE
    )))
}

pub(crate) async fn require_pm_edit_or_manage(state: &State<'_, AppState>, user: &AuthenticatedUser) -> AppResult<()> {
    let scope = PermissionScope::Global;
    if rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::PM_EDIT,
        &scope,
    )
    .await?
    {
        return enforce_pm_capability(state, user, crate::rbac::permissions::PM_EDIT).await;
    }
    Err(AppError::PermissionDenied(format!(
        "Permission requise : {}",
        crate::rbac::permissions::PM_EDIT
    )))
}

pub(crate) async fn require_pm_delete_or_manage(
    state: &State<'_, AppState>,
    user: &AuthenticatedUser,
) -> AppResult<()> {
    let scope = PermissionScope::Global;
    if rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::PM_DELETE,
        &scope,
    )
    .await?
    {
        return enforce_pm_capability(state, user, crate::rbac::permissions::PM_DELETE).await;
    }
    Err(AppError::PermissionDenied(format!(
        "Permission requise : {}",
        crate::rbac::permissions::PM_DELETE
    )))
}
