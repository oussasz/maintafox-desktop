//! Preventive maintenance IPC commands.

use tauri::State;

use crate::auth::rbac::{self, PermissionScope};
use crate::auth::session_manager::AuthenticatedUser;
use crate::errors::{AppError, AppResult};
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, ExecutePmOccurrenceInput, ExecutePmOccurrenceResult,
    GeneratePmOccurrencesInput, GeneratePmOccurrencesResult, PmDueMetrics, PmExecution, PmExecutionFilter,
    PmFinding, PmGovernanceKpiInput, PmGovernanceKpiReport, PmOccurrence, PmOccurrenceFilter, PmPlan,
    PmPlanFilter, PmPlanVersion, PmPlanningReadinessInput, PmPlanningReadinessProjection,
    PmRecurringFinding, PmRecurringFindingsInput, PublishPmPlanVersionInput, TransitionPmOccurrenceInput,
    TransitionPmPlanLifecycleInput, UpdatePmPlanInput, UpdatePmPlanVersionInput,
};
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
    require_pm_edit_or_manage(&state, &user).await?;
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
    queries::generate_pm_occurrences(&state.db, input).await
}

#[tauri::command]
pub async fn transition_pm_occurrence(
    input: TransitionPmOccurrenceInput,
    state: State<'_, AppState>,
) -> AppResult<PmOccurrence> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
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
    input: ExecutePmOccurrenceInput,
    state: State<'_, AppState>,
) -> AppResult<ExecutePmOccurrenceResult> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    queries::execute_pm_occurrence(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_executions(
    filter: PmExecutionFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmExecution>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
    queries::list_pm_executions(&state.db, filter).await
}

#[tauri::command]
pub async fn list_pm_findings(
    execution_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmFinding>> {
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

async fn enforce_pm_capability(state: &State<'_, AppState>, user: &AuthenticatedUser, perm: &str) -> AppResult<()> {
    crate::entitlements::queries::enforce_capability_for_permission(&state.db, perm).await?;
    crate::license::queries::enforce_permission_matrix(&state.db, user.user_id, perm).await?;
    Ok(())
}

/// Require `pm.create` (canonical). Holders of legacy `pm.manage` are migrated to
/// `pm.create`/`pm.edit`/`pm.delete` by the RBAC normalization migration.
pub(crate) async fn require_pm_create_or_manage(state: &State<'_, AppState>, user: &AuthenticatedUser) -> AppResult<()> {
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

pub(crate) async fn require_pm_delete_or_manage(state: &State<'_, AppState>, user: &AuthenticatedUser) -> AppResult<()> {
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
