//! Inspection rounds & templates (PRD §6.25).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Deserialize;
use tauri::{Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::inspection::domain::{
    AddInspectionEvidenceInput, AddInspectionPhotoInput, CreateInspectionTemplateInput, InspectionAnomaliesFilter,
    InspectionAnomaly, InspectionCheckpoint, InspectionCheckpointsFilter, InspectionEvidence, InspectionEvidenceFilter,
    InspectionReliabilitySignal, InspectionReliabilitySignalsFilter, InspectionResult, InspectionResultsFilter,
    InspectionRound, InspectionRoundActionInput, InspectionRoundRunner, InspectionRoundSummary,
    InspectionRoundSummaryFilter, InspectionTemplate, InspectionTemplateVersion, InspectionTemplateVersionsFilter,
    PublishInspectionTemplateVersionInput, RecordInspectionResultInput, RefreshInspectionReliabilitySignalsInput,
    ScheduleInspectionRoundInput, UpdateInspectionAnomalyInput,
};
use crate::inspection::queries;
use crate::inspection::results;
use crate::inspection::routing;
use crate::inspection::runner;
use crate::inspection::signals;
use crate::state::AppState;
use crate::wo::audit;
use crate::{require_permission, require_session};

#[derive(Debug, Deserialize)]
pub struct RouteInspectionAnomalyToDiInput {
    pub anomaly_id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RouteInspectionAnomalyToWoInput {
    pub anomaly_id: i64,
    pub expected_row_version: i64,
    pub type_code: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeferInspectionAnomalyInput {
    pub anomaly_id: i64,
    pub expected_row_version: i64,
}

async fn actor_personnel_id(state: &State<'_, AppState>, user_id: i32) -> AppResult<Option<i64>> {
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;
    row.map(|r| r.try_get::<Option<i64>>("", "personnel_id"))
        .transpose()
        .map(Option::flatten)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("personnel_id decode: {e}")))
}

#[tauri::command]
pub async fn list_inspection_templates(state: State<'_, AppState>) -> AppResult<Vec<InspectionTemplate>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    queries::list_inspection_templates(&state.db).await
}

#[tauri::command]
pub async fn list_inspection_template_versions(
    filter: InspectionTemplateVersionsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionTemplateVersion>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    queries::list_inspection_template_versions(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_checkpoints(
    filter: InspectionCheckpointsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionCheckpoint>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    queries::list_inspection_checkpoints(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_rounds(state: State<'_, AppState>) -> AppResult<Vec<InspectionRound>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    queries::list_inspection_rounds(&state.db).await
}

#[tauri::command]
pub async fn create_inspection_template(
    input: CreateInspectionTemplateInput,
    state: State<'_, AppState>,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_ADMIN,
        PermissionScope::Global
    );
    queries::create_inspection_template(&state.db, input).await
}

#[tauri::command]
pub async fn publish_inspection_template_version(
    input: PublishInspectionTemplateVersionInput,
    state: State<'_, AppState>,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_ADMIN,
        PermissionScope::Global
    );
    queries::publish_inspection_template_version(&state.db, input).await
}

#[tauri::command]
pub async fn schedule_inspection_round(
    input: ScheduleInspectionRoundInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionRound> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_ADMIN,
        PermissionScope::Global
    );
    queries::schedule_inspection_round(&state.db, input).await
}

#[tauri::command]
pub async fn list_inspection_results(
    filter: InspectionResultsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionResult>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    results::list_inspection_results(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_evidence(
    filter: InspectionEvidenceFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionEvidence>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    results::list_inspection_evidence(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_anomalies(
    filter: InspectionAnomaliesFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionAnomaly>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    results::list_inspection_anomalies(&state.db, filter).await
}

#[tauri::command]
pub async fn record_inspection_result(
    input: RecordInspectionResultInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    let pid = actor_personnel_id(&state, user.user_id)
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["User has no personnel_id.".into()]))?;
    results::record_inspection_result(&state.db, input, pid).await
}

#[tauri::command]
pub async fn add_inspection_evidence(
    input: AddInspectionEvidenceInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionEvidence> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    let pid = actor_personnel_id(&state, user.user_id)
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["User has no personnel_id.".into()]))?;
    results::add_inspection_evidence(&state.db, input, pid).await
}

#[tauri::command]
pub async fn update_inspection_anomaly(
    input: UpdateInspectionAnomalyInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionAnomaly> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    results::update_inspection_anomaly(&state.db, input).await
}

#[tauri::command]
pub async fn list_inspection_round_summaries(
    filter: InspectionRoundSummaryFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionRoundSummary>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    runner::list_inspection_round_summaries(&state.db, filter).await
}

#[tauri::command]
pub async fn get_inspection_round_runner(
    round_id: i64,
    state: State<'_, AppState>,
) -> AppResult<InspectionRoundRunner> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    runner::get_inspection_round_runner(&state.db, round_id).await
}

#[tauri::command]
pub async fn start_inspection_round(
    input: InspectionRoundActionInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionRoundSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    runner::start_inspection_round(&state.db, input).await
}

#[tauri::command]
pub async fn complete_inspection_round(
    input: InspectionRoundActionInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionRoundSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    runner::complete_inspection_round(&state.db, input).await
}

/// Supervisor review of a round with findings (every anomaly must have a decision).
#[tauri::command]
pub async fn review_inspection_round(
    input: InspectionRoundActionInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionRoundSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_ADMIN,
        PermissionScope::Global
    );
    runner::review_inspection_round(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn add_inspection_photo_evidence(
    app: tauri::AppHandle,
    input: AddInspectionPhotoInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionEvidence> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    runner::add_inspection_photo_evidence(&state.db, &app_data_dir, input).await
}

#[tauri::command]
pub async fn route_inspection_anomaly_to_di(
    input: RouteInspectionAnomalyToDiInput,
    state: State<'_, AppState>,
) -> AppResult<(crate::di::domain::InterventionRequest, InspectionAnomaly)> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    let has_global = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        crate::rbac::permissions::DI_CREATE,
        &PermissionScope::Global,
    )
    .await?;
    if has_global {
        crate::entitlements::queries::enforce_capability_for_permission(&state.db, crate::rbac::permissions::DI_CREATE)
            .await?;
        crate::license::queries::enforce_permission_matrix(
            &state.db,
            user.user_id,
            crate::rbac::permissions::DI_CREATE,
        )
        .await?;
    } else {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::DI_CREATE_OWN,
            PermissionScope::Global
        );
    }
    routing::route_inspection_anomaly_to_di(
        &state.db,
        input.anomaly_id,
        input.expected_row_version,
        i64::from(user.user_id),
        input.title,
        input.description,
    )
    .await
}

#[tauri::command]
pub async fn route_inspection_anomaly_to_wo(
    input: RouteInspectionAnomalyToWoInput,
    state: State<'_, AppState>,
) -> AppResult<(crate::wo::domain::WorkOrder, InspectionAnomaly)> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::OT_CREATE,
        PermissionScope::Global
    );
    let (wo, anomaly) = routing::route_inspection_anomaly_to_wo(
        &state.db,
        input.anomaly_id,
        input.expected_row_version,
        i64::from(user.user_id),
        input.type_code,
        input.title,
    )
    .await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "created".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order created from inspection anomaly".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok((wo, anomaly))
}

#[tauri::command]
pub async fn defer_inspection_anomaly(
    input: DeferInspectionAnomalyInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionAnomaly> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_EXECUTE,
        PermissionScope::Global
    );
    routing::defer_inspection_anomaly(&state.db, input.anomaly_id, input.expected_row_version).await
}

#[tauri::command]
pub async fn list_inspection_reliability_signals(
    filter: InspectionReliabilitySignalsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionReliabilitySignal>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_VIEW,
        PermissionScope::Global
    );
    signals::list_inspection_reliability_signals(&state.db, filter).await
}

#[tauri::command]
pub async fn refresh_inspection_reliability_signals(
    input: RefreshInspectionReliabilitySignalsInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionReliabilitySignal>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INS_ADMIN,
        PermissionScope::Global
    );
    signals::refresh_inspection_reliability_signals(&state.db, input).await
}
