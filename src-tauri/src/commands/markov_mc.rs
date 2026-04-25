//! Markov chains, Monte Carlo, RAM guardrails (PRD §6.10).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::reliability::markov_mc::domain::{
    CreateMarkovModelInput, CreateMcModelInput, MarkovModel, MarkovModelsFilter, McModel, McModelsFilter,
    UpdateMarkovModelInput, UpdateMcModelInput,
};
use crate::reliability::markov_mc::queries;
use crate::reliability::markov_mc::GuardrailFlags;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn get_ram_advanced_guardrails(state: State<'_, AppState>) -> AppResult<GuardrailFlags> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::get_ram_advanced_guardrails(&state.db).await
}

#[tauri::command]
pub async fn set_ram_advanced_guardrails(
    flags: GuardrailFlags,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::set_ram_advanced_guardrails(&state.db, &flags).await
}

#[tauri::command]
pub async fn list_mc_models(filter: McModelsFilter, state: State<'_, AppState>) -> AppResult<Vec<McModel>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_mc_models(&state.db, filter).await
}

#[tauri::command]
pub async fn create_mc_model(input: CreateMcModelInput, state: State<'_, AppState>) -> AppResult<McModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_mc_model(&state.db, Some(i64::from(user.user_id)), input).await
}

#[tauri::command]
pub async fn update_mc_model(input: UpdateMcModelInput, state: State<'_, AppState>) -> AppResult<McModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_mc_model(&state.db, input).await
}

#[tauri::command]
pub async fn delete_mc_model(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_mc_model(&state.db, id).await
}

#[tauri::command]
pub async fn evaluate_mc_model(id: i64, state: State<'_, AppState>) -> AppResult<McModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.analyze", PermissionScope::Global);
    queries::evaluate_mc_model(&state.db, id).await
}

#[tauri::command]
pub async fn list_markov_models(
    filter: MarkovModelsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<MarkovModel>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_markov_models(&state.db, filter).await
}

#[tauri::command]
pub async fn create_markov_model(
    input: CreateMarkovModelInput,
    state: State<'_, AppState>,
) -> AppResult<MarkovModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_markov_model(&state.db, Some(i64::from(user.user_id)), input).await
}

#[tauri::command]
pub async fn update_markov_model(
    input: UpdateMarkovModelInput,
    state: State<'_, AppState>,
) -> AppResult<MarkovModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_markov_model(&state.db, input).await
}

#[tauri::command]
pub async fn delete_markov_model(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_markov_model(&state.db, id).await
}

#[tauri::command]
pub async fn evaluate_markov_model(id: i64, state: State<'_, AppState>) -> AppResult<MarkovModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.analyze", PermissionScope::Global);
    queries::evaluate_markov_model(&state.db, id).await
}
