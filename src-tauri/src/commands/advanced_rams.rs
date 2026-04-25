//! FMECA / RCM / Weibull (PRD §6.10.4–6.10.6).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::reliability::advanced_rams::domain::{
    CreateFmecaAnalysisInput, CreateRcmStudyInput, FmecaAnalysesFilter, FmecaAnalysis, FmecaItem,
    FmecaItemWithContext, FmecaItemsEquipmentFilter, FmecaSeverityOccurrenceMatrix, RamIshikawaDiagram,
    RamIshikawaDiagramsFilter,
    ReliabilityRulIndicator, RcmDecision, RcmStudiesFilter, RcmStudy, UpdateFmecaAnalysisInput,
    UpdateRcmStudyInput, UpsertFmecaItemInput, UpsertRamIshikawaDiagramInput, UpsertRcmDecisionInput,
    WeibullFitRecord, WeibullFitRunInput,
};
use crate::reliability::advanced_rams::queries;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn run_weibull_fit(
    input: WeibullFitRunInput,
    state: State<'_, AppState>,
) -> AppResult<WeibullFitRecord> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.analyze", PermissionScope::Global);
    queries::run_and_store_weibull_fit(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn get_latest_weibull_fit_for_equipment(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Option<WeibullFitRecord>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    let _ = user;
    queries::get_latest_weibull_fit_for_equipment(&state.db, equipment_id).await
}

#[tauri::command]
pub async fn get_ram_fmeca_rpn_critical_threshold(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    let _ = user;
    queries::fmeca_rpn_critical_threshold_i64(&state.db).await
}

#[tauri::command]
pub async fn list_fmeca_analyses(
    filter: FmecaAnalysesFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<FmecaAnalysis>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_fmeca_analyses(&state.db, filter).await
}

#[tauri::command]
pub async fn create_fmeca_analysis(
    input: CreateFmecaAnalysisInput,
    state: State<'_, AppState>,
) -> AppResult<FmecaAnalysis> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_fmeca_analysis(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn update_fmeca_analysis(
    input: UpdateFmecaAnalysisInput,
    state: State<'_, AppState>,
) -> AppResult<FmecaAnalysis> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_fmeca_analysis(&state.db, input).await
}

#[tauri::command]
pub async fn delete_fmeca_analysis(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_fmeca_analysis(&state.db, id).await
}

#[tauri::command]
pub async fn list_fmeca_items(analysis_id: i64, state: State<'_, AppState>) -> AppResult<Vec<FmecaItem>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_fmeca_items(&state.db, analysis_id).await
}

#[tauri::command]
pub async fn upsert_fmeca_item(input: UpsertFmecaItemInput, state: State<'_, AppState>) -> AppResult<FmecaItem> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::upsert_fmeca_item(&state.db, input).await
}

#[tauri::command]
pub async fn delete_fmeca_item(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_fmeca_item(&state.db, id).await
}

#[tauri::command]
pub async fn get_fmeca_severity_occurrence_matrix(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<FmecaSeverityOccurrenceMatrix> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    let _ = user;
    queries::get_fmeca_severity_occurrence_matrix(&state.db, equipment_id).await
}

#[tauri::command]
pub async fn list_fmeca_items_for_equipment(
    filter: FmecaItemsEquipmentFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<FmecaItemWithContext>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    let _ = user;
    queries::list_fmeca_items_for_equipment(&state.db, filter).await
}

#[tauri::command]
pub async fn get_reliability_rul_indicator(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ReliabilityRulIndicator> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    let _ = user;
    queries::get_reliability_rul_indicator(&state.db, equipment_id).await
}

#[tauri::command]
pub async fn list_ram_ishikawa_diagrams(
    filter: RamIshikawaDiagramsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<RamIshikawaDiagram>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    let _ = user;
    queries::list_ram_ishikawa_diagrams(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_ram_ishikawa_diagram(
    input: UpsertRamIshikawaDiagramInput,
    state: State<'_, AppState>,
) -> AppResult<RamIshikawaDiagram> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::upsert_ram_ishikawa_diagram(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn delete_ram_ishikawa_diagram(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_ram_ishikawa_diagram(&state.db, id).await
}

#[tauri::command]
pub async fn list_rcm_studies(filter: RcmStudiesFilter, state: State<'_, AppState>) -> AppResult<Vec<RcmStudy>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_rcm_studies(&state.db, filter).await
}

#[tauri::command]
pub async fn create_rcm_study(input: CreateRcmStudyInput, state: State<'_, AppState>) -> AppResult<RcmStudy> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_rcm_study(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn update_rcm_study(input: UpdateRcmStudyInput, state: State<'_, AppState>) -> AppResult<RcmStudy> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_rcm_study(&state.db, input).await
}

#[tauri::command]
pub async fn delete_rcm_study(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_rcm_study(&state.db, id).await
}

#[tauri::command]
pub async fn list_rcm_decisions(study_id: i64, state: State<'_, AppState>) -> AppResult<Vec<RcmDecision>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_rcm_decisions(&state.db, study_id).await
}

#[tauri::command]
pub async fn upsert_rcm_decision(input: UpsertRcmDecisionInput, state: State<'_, AppState>) -> AppResult<RcmDecision> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::upsert_rcm_decision(&state.db, input).await
}

#[tauri::command]
pub async fn delete_rcm_decision(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_rcm_decision(&state.db, id).await
}
