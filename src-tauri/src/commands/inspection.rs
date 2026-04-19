//! Inspection rounds & templates (PRD §6.25).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Deserialize;
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::inspection::domain::{
    AddInspectionEvidenceInput, CreateInspectionTemplateInput, EnqueueInspectionOfflineInput,
    InspectionAnomaliesFilter, InspectionAnomaly, InspectionCheckpoint, InspectionCheckpointsFilter, InspectionEvidence,
    InspectionEvidenceFilter, InspectionOfflineQueueItem, InspectionReliabilitySignal, InspectionReliabilitySignalsFilter,
    InspectionResult, InspectionResultsFilter, InspectionRound, InspectionTemplate, InspectionTemplateVersion,
    InspectionTemplateVersionsFilter, PublishInspectionTemplateVersionInput, RecordInspectionResultInput,
    RefreshInspectionReliabilitySignalsInput, ScheduleInspectionRoundInput, UpdateInspectionAnomalyInput,
};
use crate::inspection::queries;
use crate::inspection::results;
use crate::inspection::routing;
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
    pub type_id: i64,
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
    Ok(row.and_then(|r| r.try_get("", "personnel_id").ok()).flatten())
}

#[tauri::command]
pub async fn list_inspection_templates(state: State<'_, AppState>) -> AppResult<Vec<InspectionTemplate>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    queries::list_inspection_templates(&state.db).await
}

#[tauri::command]
pub async fn list_inspection_template_versions(
    filter: InspectionTemplateVersionsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionTemplateVersion>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    queries::list_inspection_template_versions(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_checkpoints(
    filter: InspectionCheckpointsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionCheckpoint>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    queries::list_inspection_checkpoints(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_rounds(state: State<'_, AppState>) -> AppResult<Vec<InspectionRound>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    queries::list_inspection_rounds(&state.db).await
}

#[tauri::command]
pub async fn create_inspection_template(
    input: CreateInspectionTemplateInput,
    state: State<'_, AppState>,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.admin", PermissionScope::Global);
    queries::create_inspection_template(&state.db, input).await
}

#[tauri::command]
pub async fn publish_inspection_template_version(
    input: PublishInspectionTemplateVersionInput,
    state: State<'_, AppState>,
) -> AppResult<(InspectionTemplate, InspectionTemplateVersion, Vec<InspectionCheckpoint>)> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.admin", PermissionScope::Global);
    queries::publish_inspection_template_version(&state.db, input).await
}

#[tauri::command]
pub async fn schedule_inspection_round(
    input: ScheduleInspectionRoundInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionRound> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.admin", PermissionScope::Global);
    queries::schedule_inspection_round(&state.db, input).await
}

#[tauri::command]
pub async fn list_inspection_results(
    filter: InspectionResultsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionResult>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    results::list_inspection_results(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_evidence(
    filter: InspectionEvidenceFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionEvidence>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    results::list_inspection_evidence(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inspection_anomalies(
    filter: InspectionAnomaliesFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionAnomaly>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    results::list_inspection_anomalies(&state.db, filter).await
}

#[tauri::command]
pub async fn record_inspection_result(
    input: RecordInspectionResultInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
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
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
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
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
    results::update_inspection_anomaly(&state.db, input).await
}

#[tauri::command]
pub async fn enqueue_inspection_offline(
    input: EnqueueInspectionOfflineInput,
    state: State<'_, AppState>,
) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
    results::enqueue_inspection_offline(&state.db, input).await
}

#[tauri::command]
pub async fn list_inspection_offline_queue(state: State<'_, AppState>) -> AppResult<Vec<InspectionOfflineQueueItem>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    results::list_inspection_offline_queue(&state.db).await
}

#[tauri::command]
pub async fn mark_inspection_offline_synced(queue_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
    results::mark_inspection_offline_synced(&state.db, queue_id).await
}

#[tauri::command]
pub async fn route_inspection_anomaly_to_di(
    input: RouteInspectionAnomalyToDiInput,
    state: State<'_, AppState>,
) -> AppResult<(crate::di::domain::InterventionRequest, InspectionAnomaly)> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
    let has_global = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "di.create",
        &PermissionScope::Global,
    )
    .await?;
    if !has_global {
        require_permission!(state, &user, "di.create.own", PermissionScope::Global);
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
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
    require_permission!(state, &user, "ot.create", PermissionScope::Global);
    let (wo, anomaly) = routing::route_inspection_anomaly_to_wo(
        &state.db,
        input.anomaly_id,
        input.expected_row_version,
        i64::from(user.user_id),
        input.type_id,
        input.title,
    )
    .await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "created".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order created from inspection anomaly".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    })
    .await;
    Ok((wo, anomaly))
}

#[tauri::command]
pub async fn defer_inspection_anomaly(
    input: DeferInspectionAnomalyInput,
    state: State<'_, AppState>,
) -> AppResult<InspectionAnomaly> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.execute", PermissionScope::Global);
    routing::defer_inspection_anomaly(&state.db, input.anomaly_id, input.expected_row_version).await
}

#[tauri::command]
pub async fn list_inspection_reliability_signals(
    filter: InspectionReliabilitySignalsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionReliabilitySignal>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.view", PermissionScope::Global);
    signals::list_inspection_reliability_signals(&state.db, filter).await
}

#[tauri::command]
pub async fn refresh_inspection_reliability_signals(
    input: RefreshInspectionReliabilitySignalsInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<InspectionReliabilitySignal>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ins.admin", PermissionScope::Global);
    signals::refresh_inspection_reliability_signals(&state.db, input).await
}
