//! FMECA / RCM / Weibull (PRD §6.10.4–6.10.6).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::reliability::advanced_rams::domain::{
    CreateFmecaAnalysisInput, CreateRcmStudyInput, FmecaAnalysesFilter, FmecaAnalysis, FmecaItem, FmecaItemWithContext,
    FmecaItemsEquipmentFilter, FmecaSeverityOccurrenceMatrix, RamIshikawaDiagram, RamIshikawaDiagramsFilter,
    RcmDecision, RcmStudiesFilter, RcmStudy, ReliabilityRulIndicator, SuggestedPartForFailure,
    SuggestedPartsForFailureInput, UpdateFmecaAnalysisInput, UpdateRcmStudyInput, UpsertFmecaItemInput,
    UpsertRamIshikawaDiagramInput, UpsertRcmDecisionInput, WeibullDashboardInput, WeibullDashboardPayload,
    WeibullFitRecord, WeibullFitRunInput,
};
use crate::reliability::advanced_rams::queries;
use crate::state::AppState;
use crate::{require_permission, require_permission_allowing_system_admin, require_session};

#[tauri::command]
pub async fn run_weibull_fit(input: WeibullFitRunInput, state: State<'_, AppState>) -> AppResult<WeibullFitRecord> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::RAM_ANALYZE,
        PermissionScope::Global
    );
    queries::run_and_store_weibull_fit(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn get_latest_weibull_fit_for_equipment(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Option<WeibullFitRecord>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::get_latest_weibull_fit_for_equipment(&state.db, equipment_id).await
}

#[tauri::command]
pub async fn get_weibull_dashboard_payload(
    input: WeibullDashboardInput,
    state: State<'_, AppState>,
) -> AppResult<WeibullDashboardPayload> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::get_weibull_dashboard_payload(&state.db, input).await
}

#[tauri::command]
pub async fn get_ram_fmeca_rpn_critical_threshold(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::fmeca_rpn_critical_threshold_i64(&state.db).await
}

#[tauri::command]
pub async fn list_fmeca_analyses(
    filter: FmecaAnalysesFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<FmecaAnalysis>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    queries::list_fmeca_analyses(&state.db, filter).await
}

#[tauri::command]
pub async fn create_fmeca_analysis(
    input: CreateFmecaAnalysisInput,
    state: State<'_, AppState>,
) -> AppResult<FmecaAnalysis> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    queries::create_fmeca_analysis(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn update_fmeca_analysis(
    input: UpdateFmecaAnalysisInput,
    state: State<'_, AppState>,
) -> AppResult<FmecaAnalysis> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::update_fmeca_analysis(&state.db, input).await
}

#[tauri::command]
pub async fn delete_fmeca_analysis(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::delete_fmeca_analysis(&state.db, id).await
}

#[tauri::command]
pub async fn list_fmeca_items(analysis_id: i64, state: State<'_, AppState>) -> AppResult<Vec<FmecaItem>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    queries::list_fmeca_items(&state.db, analysis_id).await
}

#[tauri::command]
pub async fn upsert_fmeca_item(input: UpsertFmecaItemInput, state: State<'_, AppState>) -> AppResult<FmecaItem> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::upsert_fmeca_item(&state.db, input).await
}

#[tauri::command]
pub async fn delete_fmeca_item(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::delete_fmeca_item(&state.db, id).await
}

#[tauri::command]
pub async fn get_fmeca_severity_occurrence_matrix(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<FmecaSeverityOccurrenceMatrix> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::get_fmeca_severity_occurrence_matrix(&state.db, equipment_id).await
}

#[tauri::command]
pub async fn list_fmeca_items_for_equipment(
    filter: FmecaItemsEquipmentFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<FmecaItemWithContext>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::list_fmeca_items_for_equipment(&state.db, filter).await
}

#[tauri::command]
pub async fn get_suggested_parts_for_failure(
    input: SuggestedPartsForFailureInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<SuggestedPartForFailure>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::get_suggested_parts_for_failure(&state.db, input).await
}

#[tauri::command]
pub async fn get_reliability_rul_indicator(
    equipment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ReliabilityRulIndicator> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::get_reliability_rul_indicator(&state.db, equipment_id).await
}

#[tauri::command]
pub async fn list_ram_ishikawa_diagrams(
    filter: RamIshikawaDiagramsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<RamIshikawaDiagram>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    let _ = user;
    queries::list_ram_ishikawa_diagrams(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_ram_ishikawa_diagram(
    input: UpsertRamIshikawaDiagramInput,
    state: State<'_, AppState>,
) -> AppResult<RamIshikawaDiagram> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    queries::upsert_ram_ishikawa_diagram(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn delete_ram_ishikawa_diagram(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::delete_ram_ishikawa_diagram(&state.db, id).await
}

#[tauri::command]
pub async fn list_rcm_studies(filter: RcmStudiesFilter, state: State<'_, AppState>) -> AppResult<Vec<RcmStudy>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    queries::list_rcm_studies(&state.db, filter).await
}

#[tauri::command]
pub async fn create_rcm_study(input: CreateRcmStudyInput, state: State<'_, AppState>) -> AppResult<RcmStudy> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    queries::create_rcm_study(&state.db, Some(user.user_id), input).await
}

#[tauri::command]
pub async fn update_rcm_study(input: UpdateRcmStudyInput, state: State<'_, AppState>) -> AppResult<RcmStudy> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::update_rcm_study(&state.db, input).await
}

#[tauri::command]
pub async fn delete_rcm_study(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::delete_rcm_study(&state.db, id).await
}

#[tauri::command]
pub async fn list_rcm_decisions(study_id: i64, state: State<'_, AppState>) -> AppResult<Vec<RcmDecision>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    queries::list_rcm_decisions(&state.db, study_id).await
}

#[tauri::command]
pub async fn upsert_rcm_decision(input: UpsertRcmDecisionInput, state: State<'_, AppState>) -> AppResult<RcmDecision> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::upsert_rcm_decision(&state.db, input).await
}

#[tauri::command]
pub async fn delete_rcm_decision(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    let _ = user;
    queries::delete_rcm_decision(&state.db, id).await
}

/// RCM decision logic: records the answers, the recommended tactic and the decision path.
#[tauri::command]
pub async fn apply_rcm_decision_logic(
    input: crate::reliability::rcm_loop::ApplyRcmLogicInput,
    state: State<'_, AppState>,
) -> AppResult<crate::reliability::rcm_loop::RcmDecisionLogic> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    crate::reliability::rcm_loop::apply_rcm_decision_logic(&state.db, input).await
}

/// Decision path, recommendation and linked PM version / WO of every decision of a study.
#[tauri::command]
pub async fn list_rcm_decision_logic(
    study_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::reliability::rcm_loop::RcmDecisionLogic>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    crate::reliability::rcm_loop::list_rcm_decision_logic(&state.db, study_id).await
}

/// Draft PM plan version from an FMECA item or an RCM decision, linked both ways.
#[tauri::command]
pub async fn create_pm_revision_from_rams_source(
    input: crate::reliability::rcm_loop::PmRevisionFromSourceInput,
    state: State<'_, AppState>,
) -> AppResult<crate::reliability::rcm_loop::SourceAction> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    crate::reliability::rcm_loop::create_pm_revision_from_source(&state.db, input).await
}

/// Failure mode ratio and failure rate observed for an FMECA item (criticality inputs).
#[tauri::command]
pub async fn get_fmeca_criticality_evidence(
    item_id: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::reliability::rcm_loop::CriticalityEvidence> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    crate::reliability::rcm_loop::fmeca_criticality_evidence(&state.db, item_id).await
}

/// MIL-STD-1629A mode criticality number of an FMECA item.
#[tauri::command]
pub async fn compute_fmeca_criticality(
    input: crate::reliability::rcm_loop::FmecaCriticalityInput,
    state: State<'_, AppState>,
) -> AppResult<crate::reliability::rcm_loop::FmecaCriticality> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    crate::reliability::rcm_loop::compute_fmeca_criticality(&state.db, input).await
}

/// Work order from an FMECA item or an RCM decision (redesign, corrective action), linked back.
#[tauri::command]
pub async fn create_wo_from_rams_source(
    input: crate::reliability::rcm_loop::WoFromSourceInput,
    state: State<'_, AppState>,
) -> AppResult<crate::reliability::rcm_loop::SourceAction> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_MANAGE,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::OT_CREATE,
        PermissionScope::Global
    );
    crate::reliability::rcm_loop::create_wo_from_source(&state.db, input, i64::from(user.user_id)).await
}

/// Preview of the RCM decision logic for a set of answers (nothing is saved).
#[tauri::command]
pub async fn preview_rcm_recommendation(
    answers: crate::reliability::rcm_loop::RcmAnswers,
    state: State<'_, AppState>,
) -> AppResult<crate::reliability::rcm_loop::RcmRecommendation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    Ok(crate::reliability::rcm_loop::recommend_tactic(&answers))
}

/// Pareto of unplanned failures by failure mode, ranked by count, cost or downtime.
#[tauri::command]
pub async fn get_failure_mode_pareto(
    input: crate::reliability::pareto::FailureModeParetoInput,
    state: State<'_, AppState>,
) -> AppResult<crate::reliability::pareto::FailureModePareto> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    crate::reliability::pareto::get_failure_mode_pareto(&state.db, input).await
}

/// Reliability report of one equipment (KPIs, Weibull, FMECA, Pareto) as PDF or Excel.
#[tauri::command]
pub async fn export_reliability_report(
    equipment_id: i64,
    export_format: String,
    state: State<'_, AppState>,
) -> AppResult<crate::planning::domain::ExportedBinaryDocument> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::RAM_VIEW,
        PermissionScope::Global
    );
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REP_EXPORT,
        PermissionScope::Global
    );
    crate::reports::reliability_export::export_reliability_report(&state.db, equipment_id, &export_format).await
}
