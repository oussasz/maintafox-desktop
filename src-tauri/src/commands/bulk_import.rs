//! "Imports de données" IPC: one wizard for the plant data that has no dedicated importer.
//!
//! Each import type names the permission it needs (see `bulk_import::EntitySpec`). Reading the
//! catalogue only needs a session; creating a batch, previewing, exporting and applying need the
//! type's permission, and applying also needs step-up verification.

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::bulk_import::{self, ApplyResult, BatchSummary, CreateInput, EntityInfo, ExportTable, Preview};
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

async fn entity_of_batch(state: &State<'_, AppState>, batch_id: i64) -> AppResult<String> {
    bulk_import::entity_of(&state.db, batch_id).await
}

#[tauri::command]
pub async fn list_bulk_import_entities(state: State<'_, AppState>) -> AppResult<Vec<EntityInfo>> {
    let _user = require_session!(state);
    Ok(bulk_import::list_entities())
}

#[tauri::command]
pub async fn create_bulk_import_batch(input: CreateInput, state: State<'_, AppState>) -> AppResult<BatchSummary> {
    let user = require_session!(state);
    let spec = bulk_import::spec(&input.entity)?;
    require_permission!(state, &user, spec.permission, PermissionScope::Global);
    bulk_import::create_batch(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn get_bulk_import_preview(batch_id: i64, state: State<'_, AppState>) -> AppResult<Preview> {
    let user = require_session!(state);
    let entity = entity_of_batch(&state, batch_id).await?;
    let spec = bulk_import::spec(&entity)?;
    require_permission!(state, &user, spec.permission, PermissionScope::Global);
    bulk_import::get_preview(&state.db, batch_id).await
}

#[tauri::command]
pub async fn apply_bulk_import_batch(batch_id: i64, state: State<'_, AppState>) -> AppResult<ApplyResult> {
    let user = require_session!(state);
    let entity = entity_of_batch(&state, batch_id).await?;
    let spec = bulk_import::spec(&entity)?;
    require_permission!(state, &user, spec.permission, PermissionScope::Global);
    if let Some(extra) = bulk_import::extra_permission(&state.db, batch_id).await? {
        require_permission!(state, &user, extra, PermissionScope::Global);
    }
    require_step_up!(state);
    bulk_import::apply_batch(&state.db, batch_id, user.user_id).await
}

#[tauri::command]
pub async fn get_bulk_import_template(entity: String, state: State<'_, AppState>) -> AppResult<ExportTable> {
    let _user = require_session!(state);
    bulk_import::template(&entity)
}

#[tauri::command]
pub async fn export_bulk_import_data(entity: String, state: State<'_, AppState>) -> AppResult<ExportTable> {
    let user = require_session!(state);
    let spec = bulk_import::spec(&entity)?;
    require_permission!(state, &user, spec.permission, PermissionScope::Global);
    bulk_import::export(&state.db, &entity).await
}
