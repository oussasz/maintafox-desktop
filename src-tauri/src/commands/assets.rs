//! Asset module IPC commands.
//!
//! Permission gates:
//!   eq.view   — list assets, get asset by id, list children/parents,
//!               list lifecycle events, list meters/readings, list document links
//!   eq.manage — create asset, update identity, link/unlink hierarchy, move org node,
//!               record lifecycle events, create meters, record readings,
//!               upsert/expire document links

use tauri::{AppHandle, Manager, State};

use crate::assets::{
    bindings,
    documents::{self, UpsertDocumentLinkPayload},
    health,
    hierarchy::{self, LinkAssetPayload},
    identity::{self, CreateAssetPayload, UpdateAssetIdentityPayload},
    import::{self, ApplyPolicy},
    lifecycle::{self, RecordLifecycleEventPayload},
    meters::{self, CreateAssetMeterPayload, RecordMeterReadingPayload},
    photos,
    search::{self, AssetSearchFilters},
};
use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session};

// ─── Read commands (eq.view) ──────────────────────────────────────────────────

#[tauri::command]
pub async fn list_assets(
    status_filter: Option<String>,
    org_node_filter: Option<i64>,
    query: Option<String>,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<identity::Asset>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    identity::list_assets(&state.db, status_filter, org_node_filter, query, limit).await
}

#[tauri::command]
pub async fn get_asset_by_id(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    identity::get_asset_by_id(&state.db, asset_id).await
}

#[tauri::command]
pub async fn list_asset_children(
    parent_asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<hierarchy::AssetHierarchyRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    hierarchy::list_asset_children(&state.db, parent_asset_id).await
}

#[tauri::command]
pub async fn list_asset_parents(
    child_asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<hierarchy::AssetHierarchyRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    hierarchy::list_asset_parents(&state.db, child_asset_id).await
}

#[tauri::command]
pub async fn search_assets(
    filters: AssetSearchFilters,
    state: State<'_, AppState>,
) -> AppResult<Vec<search::AssetSearchResult>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    search::search_assets(&state.db, filters).await
}

#[tauri::command]
pub async fn suggest_asset_codes(
    prefix: String,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<search::AssetSuggestion>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    search::suggest_asset_codes(&state.db, &prefix, limit).await
}

#[tauri::command]
pub async fn suggest_asset_names(
    partial: String,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<search::AssetSuggestion>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    search::suggest_asset_names(&state.db, &partial, limit).await
}

// ─── Binding summary (eq.view) ────────────────────────────────────────────────

#[tauri::command]
pub async fn get_asset_binding_summary(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<bindings::AssetBindingSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    bindings::get_asset_binding_summary(&state.db, asset_id).await
}

#[tauri::command]
pub async fn get_asset_health_score(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<health::AssetHealthScore> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    health::get_asset_health_score(&state.db, asset_id).await
}

// ─── Mutation commands (eq.manage) ────────────────────────────────────────────

#[tauri::command]
pub async fn create_asset(
    payload: CreateAssetPayload,
    state: State<'_, AppState>,
) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    identity::create_asset(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn update_asset_identity(
    asset_id: i64,
    payload: UpdateAssetIdentityPayload,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    identity::update_asset_identity(&state.db, asset_id, payload, expected_row_version, user.user_id)
        .await
}

#[tauri::command]
pub async fn link_asset_hierarchy(
    payload: LinkAssetPayload,
    state: State<'_, AppState>,
) -> AppResult<hierarchy::AssetHierarchyRow> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    hierarchy::link_asset_hierarchy(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn unlink_asset_hierarchy(
    relation_id: i64,
    effective_to: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<hierarchy::AssetHierarchyRow> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    hierarchy::unlink_asset_hierarchy(&state.db, relation_id, effective_to, user.user_id).await
}

#[tauri::command]
pub async fn move_asset_org_node(
    asset_id: i64,
    new_org_node_id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    hierarchy::move_asset_org_node(
        &state.db,
        asset_id,
        new_org_node_id,
        expected_row_version,
        user.user_id,
    )
    .await
}

// ─── Lifecycle events (eq.view / eq.manage) ───────────────────────────────────

#[tauri::command]
pub async fn list_asset_lifecycle_events(
    asset_id: i64,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<lifecycle::AssetLifecycleEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    lifecycle::list_asset_lifecycle_events(&state.db, asset_id, limit).await
}

#[tauri::command]
pub async fn record_lifecycle_event(
    payload: RecordLifecycleEventPayload,
    state: State<'_, AppState>,
) -> AppResult<lifecycle::AssetLifecycleEvent> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    lifecycle::record_lifecycle_event(&state.db, payload, user.user_id).await
}

// ─── Meters & readings (eq.view / eq.manage) ─────────────────────────────────

#[tauri::command]
pub async fn list_asset_meters(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<meters::AssetMeter>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    meters::list_asset_meters(&state.db, asset_id).await
}

#[tauri::command]
pub async fn create_asset_meter(
    payload: CreateAssetMeterPayload,
    state: State<'_, AppState>,
) -> AppResult<meters::AssetMeter> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    meters::create_asset_meter(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn record_meter_reading(
    payload: RecordMeterReadingPayload,
    state: State<'_, AppState>,
) -> AppResult<meters::MeterReading> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    meters::record_meter_reading(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn get_latest_meter_value(
    meter_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Option<meters::MeterReading>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    meters::get_latest_meter_value(&state.db, meter_id).await
}

#[tauri::command]
pub async fn list_meter_readings(
    meter_id: i64,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<meters::MeterReading>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    meters::list_meter_readings(&state.db, meter_id, limit).await
}

// ─── Document links (eq.view / eq.manage) ─────────────────────────────────────

#[tauri::command]
pub async fn list_asset_document_links(
    asset_id: i64,
    include_expired: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<documents::AssetDocumentLink>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    documents::list_asset_document_links(&state.db, asset_id, include_expired.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn upsert_asset_document_link(
    payload: UpsertDocumentLinkPayload,
    state: State<'_, AppState>,
) -> AppResult<documents::AssetDocumentLink> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    documents::upsert_asset_document_link(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn expire_asset_document_link(
    link_id: i64,
    valid_to: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<documents::AssetDocumentLink> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    documents::expire_asset_document_link(&state.db, link_id, valid_to, user.user_id).await
}

// ─── Photos (eq.view / eq.manage) ──────────────────────────────────────────────

#[tauri::command]
pub async fn list_asset_photos(
    asset_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<photos::AssetPhoto>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.view", PermissionScope::Global);
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::list_asset_photos(&state.db, &app_data_dir, asset_id).await
}

#[tauri::command]
pub async fn upload_asset_photo(
    app: AppHandle,
    payload: photos::UploadAssetPhotoPayload,
    state: State<'_, AppState>,
) -> AppResult<photos::AssetPhoto> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::upload_asset_photo(&state.db, &app_data_dir, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn delete_asset_photo(
    photo_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.manage", PermissionScope::Global);
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::delete_asset_photo(&state.db, &app_data_dir, photo_id).await
}

// ─── Import commands (eq.import) ──────────────────────────────────────────────

#[tauri::command]
pub async fn create_asset_import_batch(
    filename: String,
    file_sha256: String,
    csv_content: Vec<u8>,
    state: State<'_, AppState>,
) -> AppResult<import::ImportBatchSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.import", PermissionScope::Global);

    let batch = import::create_import_batch(
        &state.db,
        &filename,
        &file_sha256,
        Some(user.user_id as i64),
    )
    .await?;
    import::parse_and_stage_csv(&state.db, batch.id, &csv_content).await
}

#[tauri::command]
pub async fn validate_asset_import_batch(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<import::ImportBatchSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.import", PermissionScope::Global);
    import::validate_import_batch(&state.db, batch_id, Some(user.user_id as i64)).await
}

#[tauri::command]
pub async fn get_asset_import_preview(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<import::ImportPreview> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.import", PermissionScope::Global);
    import::get_import_preview(&state.db, batch_id).await
}

#[tauri::command]
pub async fn apply_asset_import_batch(
    batch_id: i64,
    policy: ApplyPolicy,
    state: State<'_, AppState>,
) -> AppResult<import::ApplyResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.import", PermissionScope::Global);
    import::apply_import_batch(&state.db, batch_id, &policy, Some(user.user_id as i64)).await
}

#[tauri::command]
pub async fn list_asset_import_batches(
    status_filter: Option<String>,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<import::ImportBatchSummary>> {
    let user = require_session!(state);
    require_permission!(state, &user, "eq.import", PermissionScope::Global);
    import::list_import_batches(&state.db, status_filter, limit).await
}
