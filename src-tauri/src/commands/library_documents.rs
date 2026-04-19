//! IPC commands for tenant document library (PRD §6.15).

use tauri::Manager;
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::library_documents::{
    self, LibraryDocumentRow, LibraryDocumentUploadInput, UpdateLibraryDocumentInput,
};
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_library_documents(
    category: Option<String>,
    equipment_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<LibraryDocumentRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "doc.view", PermissionScope::Global);
    library_documents::list_library_documents(&state.db, category, equipment_id).await
}

#[tauri::command]
pub async fn upload_library_document(
    app: tauri::AppHandle,
    category: String,
    equipment_id: Option<i64>,
    title: String,
    file_name: String,
    file_bytes: Vec<u8>,
    mime_type: String,
    notes: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<LibraryDocumentRow> {
    let user = require_session!(state);
    require_permission!(state, &user, "doc.manage", PermissionScope::Global);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;

    let input = LibraryDocumentUploadInput {
        category,
        equipment_id,
        title,
        file_name,
        file_bytes,
        mime_type,
        notes,
        uploaded_by_id: i64::from(user.user_id),
    };

    library_documents::save_library_document(&state.db, &app_data_dir, input).await
}

#[tauri::command]
pub async fn get_library_document_file(
    app: tauri::AppHandle,
    id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<u8>> {
    let user = require_session!(state);
    require_permission!(state, &user, "doc.view", PermissionScope::Global);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;

    library_documents::read_library_document_file(&state.db, &app_data_dir, id).await
}

#[tauri::command]
pub async fn delete_library_document(
    id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "doc.manage", PermissionScope::Global);
    library_documents::delete_library_document_record(&state.db, id).await
}

#[tauri::command]
pub async fn update_library_document(
    input: UpdateLibraryDocumentInput,
    state: State<'_, AppState>,
) -> AppResult<LibraryDocumentRow> {
    let user = require_session!(state);
    require_permission!(state, &user, "doc.manage", PermissionScope::Global);
    library_documents::update_library_document_metadata(&state.db, input).await
}
