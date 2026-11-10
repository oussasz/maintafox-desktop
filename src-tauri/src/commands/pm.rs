//! Preventive maintenance IPC commands.
//!
//! Site scope (`pm::access`, CMMS Advisor decision on PM plan rights): the plan rights
//! (`pm.create`, `pm.edit`, `pm.approve`, `pm.delete`) are checked at the plan's anchor — its
//! equipment's site, its location's node, plant-wide for a family or criticality-group plan. A plan
//! moved to another site (or made plant-wide) must be a draft, needs `pm.edit` at both ends and is
//! audited.
//!
//! Reading (`pm.view`, CMMS Advisor decision 2026-11-06): the plan list, the occurrence lists, the
//! due figures and the compliance are limited to the plans the reader sees (their sites, and every
//! family / criticality-group plan); a plan and what belongs to it is read with `pm.view` at the
//! plan's anchor. The cross-plan reads without a plan (readiness, governance KPIs, recurring
//! findings, executions) keep the plant-wide check. The occurrence and execution writes, the
//! generation and the meter readings keep their plant-wide checks.

use tauri::State;

use crate::auth::rbac::{self, PermissionScope};
use crate::auth::scope::AccessScope;
use crate::auth::session_manager::AuthenticatedUser;
use crate::errors::{AppError, AppResult};
use crate::pm::access as pm_access;
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
    let scope = view_scope(&state, &user).await?;
    queries::list_pm_plans_scoped(&state.db, filter, &scope).await
}

#[tauri::command]
pub async fn get_pm_plan(plan_id: i64, state: State<'_, AppState>) -> AppResult<PmPlan> {
    let user = require_session!(state);
    require_view_plan(&state, &user, plan_id).await?;
    queries::get_pm_plan(&state.db, plan_id).await
}

#[tauri::command]
pub async fn create_pm_plan(input: CreatePmPlanInput, state: State<'_, AppState>) -> AppResult<PmPlan> {
    let user = require_session!(state);
    // `pm.create` at the new plan's target (its equipment's site, its location; a family or a
    // criticality group: plant-wide roles only).
    pm_access::require_create_at(
        &state.db,
        &state.permission_cache,
        user.user_id,
        &input.asset_scope_type,
        input.asset_scope_id,
    )
    .await?;
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
    // `pm.edit` at the plan's anchor; a change of anchor (another site, or plant-wide) only on a
    // draft, with `pm.edit` at the new anchor too, recorded with the update.
    let site_move = pm_access::require_plan_update(
        &state.db,
        &state.permission_cache,
        user.user_id,
        plan_id,
        expected_row_version,
        input.asset_scope_type.as_deref(),
        input.asset_scope_id,
    )
    .await?;
    let plan_ref = plan_id.to_string();
    let event = site_move.as_ref().map(|m| crate::audit::AuditEvent {
        event_type: pm_access::AUDIT_ACTION_SITE_CHANGED,
        actor_id: Some(user.user_id),
        entity_type: Some("pm_plan"),
        entity_id: Some(plan_ref.as_str()),
        summary: "PM plan moved to another site",
        detail_json: Some(m.details_json()),
        ..Default::default()
    });
    queries::update_pm_plan_recorded(&state.db, plan_id, expected_row_version, input, event).await
}

#[tauri::command]
pub async fn transition_pm_plan_lifecycle(
    input: TransitionPmPlanLifecycleInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlan> {
    let user = require_session!(state);
    let next = input.next_status.trim().to_lowercase();
    // Approval: `pm.approve` at the plan's anchor; the other steps: `pm.edit` there.
    let permission = if next == "approved" {
        crate::rbac::permissions::PM_APPROVE
    } else {
        crate::rbac::permissions::PM_EDIT
    };
    require_on_plan(&state, &user, permission, input.plan_id).await?;
    queries::transition_pm_plan_lifecycle(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_plan_versions(pm_plan_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PmPlanVersion>> {
    let user = require_session!(state);
    require_view_plan(&state, &user, pm_plan_id).await?;
    queries::list_pm_plan_versions(&state.db, pm_plan_id).await
}

#[tauri::command]
pub async fn create_pm_plan_version(
    pm_plan_id: i64,
    input: CreatePmPlanVersionInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlanVersion> {
    let user = require_session!(state);
    require_on_plan(&state, &user, crate::rbac::permissions::PM_CREATE, pm_plan_id).await?;
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
    require_on_version(&state, &user, crate::rbac::permissions::PM_EDIT, version_id).await?;
    queries::update_pm_plan_version(&state.db, version_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn publish_pm_plan_version(
    input: PublishPmPlanVersionInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlanVersion> {
    let user = require_session!(state);
    require_on_version(&state, &user, crate::rbac::permissions::PM_EDIT, input.version_id).await?;
    queries::publish_pm_plan_version(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_occurrences(
    filter: PmOccurrenceFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmOccurrence>> {
    let user = require_session!(state);
    let scope = view_scope(&state, &user).await?;
    if let Some(plan_id) = filter.pm_plan_id {
        require_view_plan(&state, &user, plan_id).await?;
    }
    queries::list_pm_occurrences_scoped(&state.db, filter, &scope).await
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
    // The auto-pilot status is plant-wide (no site's records): any `pm.view` holder reads it.
    view_scope(&state, &user).await?;
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

/// The acting user of a PM occurrence write (transition, execution) is always the session user:
/// an actor id sent by the screen is ignored, as in the WO commands, so the audit trail and the
/// creator of a generated WO can never name someone else.
fn bind_session_actor(actor_id: &mut Option<i64>, session_user_id: impl Into<i64>) {
    *actor_id = Some(session_user_id.into());
}

#[tauri::command]
pub async fn transition_pm_occurrence(
    mut input: TransitionPmOccurrenceInput,
    state: State<'_, AppState>,
) -> AppResult<PmOccurrence> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    bind_session_actor(&mut input.actor_id, user.user_id);
    queries::transition_pm_occurrence(&state.db, input).await
}

#[tauri::command]
pub async fn get_pm_due_metrics(state: State<'_, AppState>) -> AppResult<PmDueMetrics> {
    let user = require_session!(state);
    let scope = view_scope(&state, &user).await?;
    queries::get_pm_due_metrics_scoped(&state.db, &scope).await
}

#[tauri::command]
pub async fn list_pm_planning_readiness(
    input: PmPlanningReadinessInput,
    state: State<'_, AppState>,
) -> AppResult<PmPlanningReadinessProjection> {
    let user = require_session!(state);
    require_view_plan_or_plant(&state, &user, input.pm_plan_id).await?;
    queries::list_pm_planning_readiness(&state.db, input).await
}

#[tauri::command]
pub async fn get_pm_governance_kpi_report(
    input: PmGovernanceKpiInput,
    state: State<'_, AppState>,
) -> AppResult<PmGovernanceKpiReport> {
    let user = require_session!(state);
    require_view_plan_or_plant(&state, &user, input.pm_plan_id).await?;
    queries::get_pm_governance_kpi_report(&state.db, input).await
}
/// Reasons (deferral / miss / cancel) and finding severities of the PM dialogs, read with the PM
/// view right (no reference-data permission needed).
#[tauri::command]
pub async fn list_pm_execution_options(
    state: State<'_, AppState>,
) -> AppResult<crate::pm::reason_catalog::PmExecutionOptions> {
    let user = require_session!(state);
    view_scope(&state, &user).await?;
    crate::pm::reason_catalog::list_pm_execution_options(&state.db).await
}

#[tauri::command]
pub async fn execute_pm_occurrence(
    mut input: ExecutePmOccurrenceInput,
    state: State<'_, AppState>,
) -> AppResult<ExecutePmOccurrenceResult> {
    let user = require_session!(state);
    require_pm_edit_or_manage(&state, &user).await?;
    bind_session_actor(&mut input.actor_id, user.user_id);
    queries::execute_pm_occurrence(&state.db, input).await
}

#[tauri::command]
pub async fn list_pm_executions(filter: PmExecutionFilter, state: State<'_, AppState>) -> AppResult<Vec<PmExecution>> {
    let user = require_session!(state);
    let plan_id = match (filter.pm_plan_id, filter.occurrence_id) {
        (Some(plan_id), _) => Some(plan_id),
        (None, Some(occurrence_id)) => Some(pm_access::occurrence_plan(&state.db, occurrence_id).await?),
        (None, None) => None,
    };
    require_view_plan_or_plant(&state, &user, plan_id).await?;
    if let (Some(plan_id), Some(occurrence_id)) = (filter.pm_plan_id, filter.occurrence_id) {
        // Both given: the occurrence's own plan is read too (never another plan's rows by its id).
        let occurrence_plan = pm_access::occurrence_plan(&state.db, occurrence_id).await?;
        if occurrence_plan != plan_id {
            require_view_plan(&state, &user, occurrence_plan).await?;
        }
    }
    queries::list_pm_executions(&state.db, filter).await
}

#[tauri::command]
pub async fn list_pm_findings(execution_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PmFinding>> {
    let user = require_session!(state);
    let plan_id = pm_access::execution_plan(&state.db, execution_id).await?;
    require_view_plan(&state, &user, plan_id).await?;
    queries::list_pm_findings(&state.db, execution_id).await
}

#[tauri::command]
pub async fn list_pm_recurring_findings(
    input: PmRecurringFindingsInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmRecurringFinding>> {
    let user = require_session!(state);
    require_view_plan_or_plant(&state, &user, input.pm_plan_id).await?;
    queries::list_pm_recurring_findings(&state.db, input).await
}

#[tauri::command]
pub async fn get_pm_occurrence(occurrence_id: i64, state: State<'_, AppState>) -> AppResult<PmOccurrence> {
    let user = require_session!(state);
    let plan_id = pm_access::occurrence_plan(&state.db, occurrence_id).await?;
    require_view_plan(&state, &user, plan_id).await?;
    queries::get_pm_occurrence_by_id(&state.db, occurrence_id).await
}

/// PM compliance % (SMRP 5.4.1 style) for a period; also read by analytics.
#[tauri::command]
pub async fn get_pm_compliance(input: PmComplianceInput, state: State<'_, AppState>) -> AppResult<PmComplianceReport> {
    let user = require_session!(state);
    let scope = view_scope(&state, &user).await?;
    if let Some(plan_id) = input.pm_plan_id {
        require_view_plan(&state, &user, plan_id).await?;
    }
    let visible = pm_access::occurrence_view_filter(&scope);
    crate::pm::compliance::get_pm_compliance_visible(&state.db, input, &visible).await
}

#[tauri::command]
pub async fn list_pm_scope_options(scope_type: String, state: State<'_, AppState>) -> AppResult<Vec<PmScopeOption>> {
    let user = require_session!(state);
    // Locations: those where the reader holds `pm.view`; families and criticality groups: all.
    let scope = view_scope(&state, &user).await?;
    crate::pm::plan_tools::list_pm_scope_options(&state.db, &scope_type, &scope).await
}

#[tauri::command]
pub async fn preview_pm_scope_assets(
    scope_type: String,
    scope_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PmScopeAsset>> {
    let user = require_session!(state);
    pm_access::require_view_target(
        &state.db,
        &state.permission_cache,
        user.user_id,
        &scope_type,
        Some(scope_id),
    )
    .await?;
    crate::pm::plan_tools::preview_pm_scope_assets(&state.db, &scope_type, scope_id).await
}

#[tauri::command]
pub async fn list_pm_asset_meters(equipment_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PmAssetMeter>> {
    let user = require_session!(state);
    pm_access::require_view_target(
        &state.db,
        &state.permission_cache,
        user.user_id,
        "equipment",
        Some(equipment_id),
    )
    .await?;
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
    let plan_id = pm_access::finding_plan(&state.db, finding_id).await?;
    require_on_plan(&state, &user, crate::rbac::permissions::PM_CREATE, plan_id).await?;
    crate::pm::plan_tools::create_pm_revision_from_finding(&state.db, finding_id).await
}

/// Where the reader holds `pm.view` (held nowhere: refused as before); filters the lists.
async fn view_scope(state: &State<'_, AppState>, user: &AuthenticatedUser) -> AppResult<AccessScope> {
    pm_access::require_view_anywhere(&state.db, &state.permission_cache, user.user_id).await
}

/// `pm.view` on the plan `plan_id` and what belongs to it (`pm::access::check_view_plan`).
async fn require_view_plan(state: &State<'_, AppState>, user: &AuthenticatedUser, plan_id: i64) -> AppResult<()> {
    pm_access::require_view_plan(&state.db, &state.permission_cache, user.user_id, plan_id).await
}

/// A read about one plan (`pm.view` at it), or across plans: then the plant-wide right, as before
/// (these cross-plan figures are not limited to the reader's sites yet).
async fn require_view_plan_or_plant(
    state: &State<'_, AppState>,
    user: &AuthenticatedUser,
    plan_id: Option<i64>,
) -> AppResult<()> {
    match plan_id {
        Some(plan_id) => require_view_plan(state, user, plan_id).await,
        None => {
            require_permission!(state, user, crate::rbac::permissions::PM_VIEW, PermissionScope::Global);
            Ok(())
        }
    }
}

/// `permission` at the anchor of the plan `plan_id` (`pm::access`).
pub(crate) async fn require_on_plan(
    state: &State<'_, AppState>,
    user: &AuthenticatedUser,
    permission: &str,
    plan_id: i64,
) -> AppResult<()> {
    pm_access::require_on_plan(&state.db, &state.permission_cache, user.user_id, permission, plan_id).await
}

/// `permission` at the anchor of the plan of the version `version_id` (`pm::access`).
pub(crate) async fn require_on_version(
    state: &State<'_, AppState>,
    user: &AuthenticatedUser,
    permission: &str,
    version_id: i64,
) -> AppResult<()> {
    pm_access::require_on_version(&state.db, &state.permission_cache, user.user_id, permission, version_id).await
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

#[cfg(test)]
mod session_actor_tests {
    use super::bind_session_actor;

    #[test]
    fn a_caller_sent_actor_is_replaced_by_the_session_user() {
        let mut actor = Some(999_i64);
        bind_session_actor(&mut actor, 5_i32);
        assert_eq!(actor, Some(5));
    }

    #[test]
    fn a_missing_actor_is_the_session_user() {
        let mut actor = None;
        bind_session_actor(&mut actor, 7_i32);
        assert_eq!(actor, Some(7));
    }
}
