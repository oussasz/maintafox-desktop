//! FTA / RBD / event tree (PRD §6.10).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::reliability::fta_rbd_eta::domain::{
    CreateEventTreeModelInput, CreateFtaModelInput, CreateRbdModelInput, EventTreeModel,
    EventTreeModelsFilter, FtaModel, FtaModelsFilter, RbdModel, RbdModelsFilter, UpdateEventTreeModelInput,
    UpdateFtaModelInput, UpdateRbdModelInput,
};
use crate::reliability::fta_rbd_eta::queries;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_fta_models(filter: FtaModelsFilter, state: State<'_, AppState>) -> AppResult<Vec<FtaModel>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_fta_models(&state.db, filter).await
}

#[tauri::command]
pub async fn create_fta_model(input: CreateFtaModelInput, state: State<'_, AppState>) -> AppResult<FtaModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_fta_model(&state.db, Some(i64::from(user.user_id)), input).await
}

#[tauri::command]
pub async fn update_fta_model(input: UpdateFtaModelInput, state: State<'_, AppState>) -> AppResult<FtaModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_fta_model(&state.db, input).await
}

#[tauri::command]
pub async fn delete_fta_model(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_fta_model(&state.db, id).await
}

#[tauri::command]
pub async fn evaluate_fta_model(id: i64, state: State<'_, AppState>) -> AppResult<FtaModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.analyze", PermissionScope::Global);
    queries::evaluate_fta_model(&state.db, id).await
}

#[tauri::command]
pub async fn list_rbd_models(filter: RbdModelsFilter, state: State<'_, AppState>) -> AppResult<Vec<RbdModel>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_rbd_models(&state.db, filter).await
}

#[tauri::command]
pub async fn create_rbd_model(input: CreateRbdModelInput, state: State<'_, AppState>) -> AppResult<RbdModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_rbd_model(&state.db, Some(i64::from(user.user_id)), input).await
}

#[tauri::command]
pub async fn update_rbd_model(input: UpdateRbdModelInput, state: State<'_, AppState>) -> AppResult<RbdModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_rbd_model(&state.db, input).await
}

#[tauri::command]
pub async fn delete_rbd_model(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_rbd_model(&state.db, id).await
}

#[tauri::command]
pub async fn evaluate_rbd_model(id: i64, state: State<'_, AppState>) -> AppResult<RbdModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.analyze", PermissionScope::Global);
    queries::evaluate_rbd_model(&state.db, id).await
}

#[tauri::command]
pub async fn list_event_tree_models(
    filter: EventTreeModelsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<EventTreeModel>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_event_tree_models(&state.db, filter).await
}

#[tauri::command]
pub async fn create_event_tree_model(
    input: CreateEventTreeModelInput,
    state: State<'_, AppState>,
) -> AppResult<EventTreeModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_event_tree_model(&state.db, Some(i64::from(user.user_id)), input).await
}

#[tauri::command]
pub async fn update_event_tree_model(
    input: UpdateEventTreeModelInput,
    state: State<'_, AppState>,
) -> AppResult<EventTreeModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_event_tree_model(&state.db, input).await
}

#[tauri::command]
pub async fn delete_event_tree_model(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_event_tree_model(&state.db, id).await
}

#[tauri::command]
pub async fn evaluate_event_tree_model(id: i64, state: State<'_, AppState>) -> AppResult<EventTreeModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.analyze", PermissionScope::Global);
    queries::evaluate_event_tree_model(&state.db, id).await
}
