//! RAM expert review sign-off (PRD §6.10).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::reliability::ram_review::domain::{
    CreateRamExpertSignOffInput, RamExpertSignOff, RamExpertSignOffsFilter, SignRamExpertReviewInput,
    UpdateRamExpertSignOffInput,
};
use crate::reliability::ram_review::queries;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_ram_expert_sign_offs(
    filter: RamExpertSignOffsFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<RamExpertSignOff>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_ram_expert_sign_offs(&state.db, filter).await
}

#[tauri::command]
pub async fn create_ram_expert_sign_off(
    input: CreateRamExpertSignOffInput,
    state: State<'_, AppState>,
) -> AppResult<RamExpertSignOff> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    queries::create_ram_expert_sign_off(&state.db, Some(i64::from(user.user_id)), input).await
}

#[tauri::command]
pub async fn update_ram_expert_sign_off(
    input: UpdateRamExpertSignOffInput,
    state: State<'_, AppState>,
) -> AppResult<RamExpertSignOff> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::update_ram_expert_sign_off(&state.db, input).await
}

#[tauri::command]
pub async fn sign_ram_expert_review(
    input: SignRamExpertReviewInput,
    state: State<'_, AppState>,
) -> AppResult<RamExpertSignOff> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::sign_ram_expert_review(&state.db, input).await
}

#[tauri::command]
pub async fn delete_ram_expert_sign_off(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    let _ = user;
    queries::delete_ram_expert_sign_off(&state.db, id).await
}
