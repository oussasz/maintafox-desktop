//! Planning IPC commands.

use tauri::State;

use crate::auth::rbac::{check_permission, PermissionScope};
use crate::errors::{AppError, AppResult};
use crate::planning::domain::{
    CapacityRule, CapacityRuleFilter, CreateCapacityRuleInput, CreatePlanningWindowInput,
    CreateScheduleBreakInInput, CreateScheduleCommitmentInput, ExportPlanningGanttPdfInput,
    ExportedBinaryDocument, FreezeSchedulePeriodInput, NotifyTeamsInput, NotifyTeamsResult,
    PlanningGanttFilter, PlanningGanttSnapshot, PlanningWindow, PlanningWindowFilter,
    RefreshScheduleCandidatesInput, RefreshScheduleCandidatesResult, RescheduleCommitmentInput,
    ScheduleBacklogSnapshot, ScheduleBreakIn, ScheduleBreakInFilter, ScheduleCandidate,
    ScheduleCandidateFilter, ScheduleChangeLogEntry, ScheduleCommitment, ScheduleCommitmentFilter,
    SchedulingConflict, TeamCapacityLoad, UpdateCapacityRuleInput, UpdatePlanningWindowInput,
};
use crate::planning::queries;
use crate::planning::scheduling;
use crate::state::AppState;
use crate::{require_permission, require_session};

async fn require_plan_edit_or_legacy_manage(state: &State<'_, AppState>, user_id: i32) -> AppResult<()> {
    let has_plan_edit = check_permission(&state.db, user_id, "plan.edit", &PermissionScope::Global).await?;
    if has_plan_edit {
        return Ok(());
    }

    let has_legacy_manage = check_permission(&state.db, user_id, "plan.manage", &PermissionScope::Global).await?;
    if has_legacy_manage {
        return Ok(());
    }

    Err(AppError::PermissionDenied(
        "Required permission: plan.edit (or legacy plan.manage compatibility).".to_string(),
    ))
}

async fn require_plan_windows(state: &State<'_, AppState>, user_id: i32) -> AppResult<()> {
    let has = check_permission(&state.db, user_id, "plan.windows", &PermissionScope::Global).await?;
    if has {
        return Ok(());
    }
    Err(AppError::PermissionDenied(
        "Required permission: plan.windows.".to_string(),
    ))
}

async fn require_plan_confirm(state: &State<'_, AppState>, user_id: i32) -> AppResult<()> {
    let has = check_permission(&state.db, user_id, "plan.confirm", &PermissionScope::Global).await?;
    if has {
        return Ok(());
    }
    Err(AppError::PermissionDenied(
        "Required permission: plan.confirm.".to_string(),
    ))
}

#[tauri::command]
pub async fn list_schedule_candidates(
    filter: ScheduleCandidateFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<ScheduleCandidate>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    queries::list_schedule_candidates(&state.db, filter).await
}

#[tauri::command]
pub async fn list_scheduling_conflicts(
    candidate_id: Option<i64>,
    include_resolved: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SchedulingConflict>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    queries::list_scheduling_conflicts(&state.db, candidate_id, include_resolved).await
}

#[tauri::command]
pub async fn refresh_schedule_candidates(
    input: RefreshScheduleCandidatesInput,
    state: State<'_, AppState>,
) -> AppResult<RefreshScheduleCandidatesResult> {
    let user = require_session!(state);
    require_plan_edit_or_legacy_manage(&state, user.user_id).await?;
    queries::refresh_schedule_candidates(&state.db, input).await
}

#[tauri::command]
pub async fn get_schedule_backlog_snapshot(
    filter: ScheduleCandidateFilter,
    state: State<'_, AppState>,
) -> AppResult<ScheduleBacklogSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    queries::get_schedule_backlog_snapshot(&state.db, filter).await
}

#[tauri::command]
pub async fn list_capacity_rules(
    filter: CapacityRuleFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<CapacityRule>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::list_capacity_rules(&state.db, filter).await
}

#[tauri::command]
pub async fn create_capacity_rule(
    input: CreateCapacityRuleInput,
    state: State<'_, AppState>,
) -> AppResult<CapacityRule> {
    let user = require_session!(state);
    require_plan_windows(&state, user.user_id).await?;
    scheduling::create_capacity_rule(&state.db, input).await
}

#[tauri::command]
pub async fn update_capacity_rule(
    rule_id: i64,
    expected_row_version: i64,
    input: UpdateCapacityRuleInput,
    state: State<'_, AppState>,
) -> AppResult<CapacityRule> {
    let user = require_session!(state);
    require_plan_windows(&state, user.user_id).await?;
    scheduling::update_capacity_rule(&state.db, rule_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_planning_windows(
    filter: PlanningWindowFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PlanningWindow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::list_planning_windows(&state.db, filter).await
}

#[tauri::command]
pub async fn create_planning_window(
    input: CreatePlanningWindowInput,
    state: State<'_, AppState>,
) -> AppResult<PlanningWindow> {
    let user = require_session!(state);
    require_plan_windows(&state, user.user_id).await?;
    scheduling::create_planning_window(&state.db, input).await
}

#[tauri::command]
pub async fn update_planning_window(
    window_id: i64,
    expected_row_version: i64,
    input: UpdatePlanningWindowInput,
    state: State<'_, AppState>,
) -> AppResult<PlanningWindow> {
    let user = require_session!(state);
    require_plan_windows(&state, user.user_id).await?;
    scheduling::update_planning_window(&state.db, window_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_schedule_commitments(
    filter: ScheduleCommitmentFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<ScheduleCommitment>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::list_schedule_commitments(&state.db, filter).await
}

#[tauri::command]
pub async fn list_schedule_change_log(
    commitment_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ScheduleChangeLogEntry>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::list_schedule_change_log(&state.db, commitment_id).await
}

#[tauri::command]
pub async fn list_schedule_break_ins(
    filter: ScheduleBreakInFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<ScheduleBreakIn>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::list_schedule_break_ins(&state.db, filter).await
}

#[tauri::command]
pub async fn create_schedule_commitment(
    input: CreateScheduleCommitmentInput,
    state: State<'_, AppState>,
) -> AppResult<ScheduleCommitment> {
    let user = require_session!(state);
    require_plan_confirm(&state, user.user_id).await?;
    scheduling::create_schedule_commitment(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn reschedule_schedule_commitment(
    input: RescheduleCommitmentInput,
    state: State<'_, AppState>,
) -> AppResult<ScheduleCommitment> {
    let user = require_session!(state);
    require_plan_confirm(&state, user.user_id).await?;
    scheduling::reschedule_schedule_commitment(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn create_schedule_break_in(
    input: CreateScheduleBreakInInput,
    state: State<'_, AppState>,
) -> AppResult<ScheduleBreakIn> {
    let user = require_session!(state);
    require_plan_confirm(&state, user.user_id).await?;
    scheduling::create_schedule_break_in(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn freeze_schedule_period(
    input: FreezeSchedulePeriodInput,
    state: State<'_, AppState>,
) -> AppResult<i64> {
    let user = require_session!(state);
    require_plan_confirm(&state, user.user_id).await?;
    scheduling::freeze_schedule_period(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn get_planning_gantt_snapshot(
    filter: PlanningGanttFilter,
    state: State<'_, AppState>,
) -> AppResult<PlanningGanttSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::get_planning_gantt_snapshot(&state.db, filter).await
}

#[tauri::command]
pub async fn list_team_capacity_load(
    period_start: String,
    period_end: String,
    team_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<TeamCapacityLoad>> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::list_team_capacity_load(&state.db, period_start, period_end, team_id).await
}

#[tauri::command]
pub async fn notify_schedule_teams(
    input: NotifyTeamsInput,
    state: State<'_, AppState>,
) -> AppResult<NotifyTeamsResult> {
    let user = require_session!(state);
    require_plan_confirm(&state, user.user_id).await?;
    scheduling::notify_schedule_teams(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn export_planning_gantt_pdf(
    input: ExportPlanningGanttPdfInput,
    state: State<'_, AppState>,
) -> AppResult<ExportedBinaryDocument> {
    let user = require_session!(state);
    require_permission!(state, &user, "plan.view", PermissionScope::Global);
    scheduling::export_planning_gantt_pdf(&state.db, input).await
}

