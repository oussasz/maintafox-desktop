//! Reliability (RAMS) module IPC — fleet view, studies, life analysis, decision log.

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::rbac::permissions::{OT_CREATE, PM_EDIT, RAM_ANALYZE, RAM_MANAGE, RAM_VIEW};
use crate::reliability::study::domain::{
    ApproverOption, CreateDecisionInput, CreateStudyInput, DecideDecisionInput, FleetView, FleetViewFilter,
    LifeAnalysis, LifeAnalysisRequest, ReliabilityStudy, SaveStudyCostsInput, SetStudyRecordInput, StudyDataset,
    StudyDecision, UpdateStudyInput, DECISION_PM_INTERVAL,
};
use crate::reliability::study::{analysis, decisions, fleet_view, records, store};
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn get_reliability_fleet_view(filter: FleetViewFilter, state: State<'_, AppState>) -> AppResult<FleetView> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    fleet_view::fleet_view(&state.db, filter).await
}

#[tauri::command]
pub async fn list_reliability_studies(
    include_archived: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ReliabilityStudy>> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    store::list_studies(&state.db, include_archived.unwrap_or(false)).await
}

#[tauri::command]
pub async fn create_reliability_study(
    input: CreateStudyInput,
    state: State<'_, AppState>,
) -> AppResult<ReliabilityStudy> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    store::create_study(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn update_reliability_study(
    input: UpdateStudyInput,
    state: State<'_, AppState>,
) -> AppResult<ReliabilityStudy> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    store::update_study(&state.db, input).await
}

/// Cost inputs of the age-replacement model, optionally kept as the equipment class default.
#[tauri::command]
pub async fn save_reliability_study_costs(
    input: SaveStudyCostsInput,
    state: State<'_, AppState>,
) -> AppResult<ReliabilityStudy> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    if input.save_as_class_default_for.is_some() {
        require_permission!(state, &user, RAM_MANAGE, PermissionScope::Global);
    }
    store::save_costs(&state.db, input, i64::from(user.user_id)).await
}

/// Step 1: failure and suspension records of the study with ages, choices and data checks.
#[tauri::command]
pub async fn get_reliability_study_dataset(study_id: i64, state: State<'_, AppState>) -> AppResult<StudyDataset> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    Ok(records::build_dataset(&state.db, study_id).await?.dataset)
}

/// Include / exclude a record (with its reason) or take its failure instant from the request.
#[tauri::command]
pub async fn set_reliability_study_record(
    input: SetStudyRecordInput,
    state: State<'_, AppState>,
) -> AppResult<StudyDataset> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    store::append_record_event(&state.db, &input, i64::from(user.user_id)).await?;
    Ok(records::build_dataset(&state.db, input.study_id).await?.dataset)
}

/// Step 2: Weibull fit with 90 % bounds, life metrics, plots and the replacement-interval costs.
#[tauri::command]
pub async fn get_reliability_study_life_analysis(
    input: LifeAnalysisRequest,
    state: State<'_, AppState>,
) -> AppResult<LifeAnalysis> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    Ok(analysis::life_analysis(&state.db, input).await?.0)
}

#[tauri::command]
pub async fn list_reliability_study_decisions(
    study_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<StudyDecision>> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    decisions::list_decisions(&state.db, study_id).await
}

/// People who can approve a reliability decision (`ram.manage`).
#[tauri::command]
pub async fn list_reliability_decision_approvers(state: State<'_, AppState>) -> AppResult<Vec<ApproverOption>> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    decisions::list_approvers(&state.db).await
}

#[tauri::command]
pub async fn create_reliability_study_decision(
    input: CreateDecisionInput,
    state: State<'_, AppState>,
) -> AppResult<StudyDecision> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    decisions::create_decision(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn approve_reliability_study_decision(
    input: DecideDecisionInput,
    state: State<'_, AppState>,
) -> AppResult<StudyDecision> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_MANAGE, PermissionScope::Global);
    decisions::approve_decision(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn reject_reliability_study_decision(
    input: DecideDecisionInput,
    state: State<'_, AppState>,
) -> AppResult<StudyDecision> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_MANAGE, PermissionScope::Global);
    decisions::reject_decision(&state.db, input, i64::from(user.user_id)).await
}

/// Create the PM plan draft version or the work order of an approved decision.
#[tauri::command]
pub async fn apply_reliability_study_decision(
    input: DecideDecisionInput,
    state: State<'_, AppState>,
) -> AppResult<StudyDecision> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_MANAGE, PermissionScope::Global);
    let current = decisions::get_decision(&state.db, input.decision_id).await?;
    if current.action_kind == DECISION_PM_INTERVAL {
        require_permission!(state, &user, PM_EDIT, PermissionScope::Global);
    } else {
        require_permission!(state, &user, OT_CREATE, PermissionScope::Global);
    }
    decisions::apply_decision(&state.db, input, i64::from(user.user_id)).await
}

// ── Part B: FMECA, RCM, system model, advanced tools, data quality ────────────────────────────

use crate::reliability::study::advanced::{
    EventTreeInput, EventTreeResult, MarkovInput, MarkovResult, SimulationInput, SimulationResult,
};
use crate::reliability::study::data_quality::DataQualityView;
use crate::reliability::study::fmeca::{FmecaWorksheet, SaveFmecaItemInput};
use crate::reliability::study::rcm::{RcmWorksheet, SaveRcmInput};
use crate::reliability::study::scales::RatingScales;
use crate::reliability::study::system_model::{
    EvaluateFtInput, EvaluateRbdInput, FtResult, RbdResult, SaveSystemModelInput, SystemModel,
};
use crate::reliability::study::{advanced, data_quality, fmeca, rcm, scales, system_model};

fn tool_disabled() -> crate::errors::AppError {
    crate::errors::AppError::ValidationFailed(vec!["ADVANCED_TOOL_DISABLED".into()])
}

/// Step 3: FMECA worksheet with occurrence proposals and the criticality matrix (no write).
#[tauri::command]
pub async fn get_reliability_study_fmeca(study_id: i64, state: State<'_, AppState>) -> AppResult<FmecaWorksheet> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    fmeca::get_worksheet(&state.db, study_id).await
}

#[tauri::command]
pub async fn save_reliability_study_fmeca_item(
    input: SaveFmecaItemInput,
    state: State<'_, AppState>,
) -> AppResult<FmecaWorksheet> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    fmeca::save_item(&state.db, input, user.user_id).await
}

#[tauri::command]
pub async fn delete_reliability_study_fmeca_item(
    study_id: i64,
    item_id: i64,
    state: State<'_, AppState>,
) -> AppResult<FmecaWorksheet> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    fmeca::delete_item(&state.db, study_id, item_id).await
}

#[tauri::command]
pub async fn get_reliability_rating_scales(state: State<'_, AppState>) -> AppResult<RatingScales> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    scales::load_scales(&state.db).await
}

/// Plant FMECA rating grids (severity / occurrence / detection, matrix zones).
#[tauri::command]
pub async fn save_reliability_rating_scales(
    scales: RatingScales,
    state: State<'_, AppState>,
) -> AppResult<RatingScales> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_MANAGE, PermissionScope::Global);
    scales::save_scales(&state.db, &scales, user.user_id).await
}

/// Step 4: RCM rows with pre-filled answers and their basis (no write).
#[tauri::command]
pub async fn get_reliability_study_rcm(study_id: i64, state: State<'_, AppState>) -> AppResult<RcmWorksheet> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    rcm::get_worksheet(&state.db, study_id).await
}

#[tauri::command]
pub async fn save_reliability_study_rcm_decision(
    input: SaveRcmInput,
    state: State<'_, AppState>,
) -> AppResult<RcmWorksheet> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    rcm::save_decision(&state.db, input, user.user_id).await
}

/// Step 5: saved RBD / fault tree of the study and the values its sources can use (no write).
#[tauri::command]
pub async fn get_reliability_study_system_model(study_id: i64, state: State<'_, AppState>) -> AppResult<SystemModel> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    system_model::get_system_model(&state.db, study_id).await
}

#[tauri::command]
pub async fn evaluate_reliability_study_rbd(
    input: EvaluateRbdInput,
    state: State<'_, AppState>,
) -> AppResult<RbdResult> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    system_model::evaluate_rbd_for_study(&state.db, input).await
}

#[tauri::command]
pub async fn evaluate_reliability_study_fault_tree(
    input: EvaluateFtInput,
    state: State<'_, AppState>,
) -> AppResult<FtResult> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    system_model::evaluate_ft_for_study(&state.db, input).await
}

#[tauri::command]
pub async fn save_reliability_study_system_model(
    input: SaveSystemModelInput,
    state: State<'_, AppState>,
) -> AppResult<SystemModel> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_ANALYZE, PermissionScope::Global);
    system_model::save_system_model(&state.db, input, i64::from(user.user_id)).await
}

/// Markov steady state of a repairable system (states and rates typed in a table).
#[tauri::command]
pub async fn evaluate_reliability_markov(input: MarkovInput, state: State<'_, AppState>) -> AppResult<MarkovResult> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    let flags = crate::reliability::markov_mc::guardrails::load_guardrails(&state.db).await?;
    if !flags.markov_enabled {
        return Err(tool_disabled());
    }
    advanced::evaluate_markov(&input, flags.markov_max_states.max(2) as usize)
}

#[tauri::command]
pub async fn evaluate_reliability_event_tree(
    input: EventTreeInput,
    state: State<'_, AppState>,
) -> AppResult<EventTreeResult> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    advanced::evaluate_event_tree(&input)
}

/// Monte Carlo simulation of the study block diagram over the mission.
#[tauri::command]
pub async fn simulate_reliability_study_system(
    input: SimulationInput,
    state: State<'_, AppState>,
) -> AppResult<SimulationResult> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    let flags = crate::reliability::markov_mc::guardrails::load_guardrails(&state.db).await?;
    if !flags.monte_carlo_enabled {
        return Err(tool_disabled());
    }
    let study = store::get_study(&state.db, input.study_id).await?;
    let (data, _) = system_model::load_source_data(&state.db, &study).await?;
    let max_trials = flags.mc_max_trials.max(1);
    // CPU-bound: off the async runtime.
    tokio::task::spawn_blocking(move || advanced::simulate(&input, &data, max_trials))
        .await
        .map_err(|e| crate::errors::AppError::SyncError(format!("simulation task: {e}")))?
}

/// Data-quality worklist of the reliability module.
#[tauri::command]
pub async fn get_reliability_data_quality(
    months: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<DataQualityView> {
    let user = require_session!(state);
    require_permission!(state, &user, RAM_VIEW, PermissionScope::Global);
    data_quality::data_quality(&state.db, months.unwrap_or(24), user.user_id).await
}
