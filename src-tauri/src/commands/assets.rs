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
    characteristics::{self, SaveAssetCharacteristicsPayload},
    criticality,
    decommission::{self, DecommissionAssetPayload},
    documents::{self, UpsertDocumentLinkPayload},
    health,
    hierarchy::{self, LinkAssetPayload},
    history::{self, AssetHistoryQuery},
    identity::{self, CreateAssetPayload, UpdateAssetIdentityPayload},
    import::{self, ApplyPolicy},
    lifecycle::{self, RecordLifecycleEventPayload},
    maintenance_profile,
    meters::{self, CreateAssetMeterPayload, RecordMeterReadingPayload, SetMeterThresholdsPayload},
    photos, registry_metrics,
    repeat_failures::{self, SaveCorrectiveActionPayload, SaveFiveWhyPayload},
    search::{self, AssetPickerSuggestFilters, AssetSearchFilters},
    spare_parts, taxonomy_reference, EquipmentTaxonomyCatalog,
};
use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session};

// ─── Read commands (eq.view) ──────────────────────────────────────────────────

#[tauri::command]
pub async fn get_equipment_taxonomy_catalog(state: State<'_, AppState>) -> AppResult<EquipmentTaxonomyCatalog> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    taxonomy_reference::list_equipment_taxonomy_catalog(&state.db).await
}

#[tauri::command]
pub async fn list_assets(
    status_filter: Option<String>,
    org_node_filter: Option<i64>,
    query: Option<String>,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<identity::Asset>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    identity::list_assets(&state.db, status_filter, org_node_filter, query, limit).await
}

#[tauri::command]
pub async fn get_asset_by_id(asset_id: i64, state: State<'_, AppState>) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    identity::get_asset_by_id(&state.db, asset_id).await
}

#[tauri::command]
pub async fn list_asset_children(
    parent_asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<hierarchy::AssetHierarchyRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    hierarchy::list_asset_children(&state.db, parent_asset_id).await
}

#[tauri::command]
pub async fn list_asset_parents(
    child_asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<hierarchy::AssetHierarchyRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    hierarchy::list_asset_parents(&state.db, child_asset_id).await
}

#[tauri::command]
pub async fn search_assets(
    filters: AssetSearchFilters,
    state: State<'_, AppState>,
) -> AppResult<Vec<search::AssetSearchResult>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    search::search_assets(&state.db, filters).await
}

#[tauri::command]
pub async fn get_equipment_registry_metrics(
    filters: AssetSearchFilters,
    state: State<'_, AppState>,
) -> AppResult<registry_metrics::EquipmentRegistryMetricsPayload> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    registry_metrics::get_equipment_registry_metrics(&state.db, filters).await
}

#[tauri::command]
pub async fn suggest_picker_assets(
    filters: AssetPickerSuggestFilters,
    state: State<'_, AppState>,
) -> AppResult<search::AssetPickerSuggestions> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    search::suggest_picker_assets(&state.db, i64::from(user.user_id), filters).await
}

#[tauri::command]
pub async fn suggest_asset_codes(
    prefix: String,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<search::AssetSuggestion>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    search::suggest_asset_codes(&state.db, &prefix, limit).await
}

#[tauri::command]
pub async fn suggest_asset_names(
    partial: String,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<search::AssetSuggestion>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    search::suggest_asset_names(&state.db, &partial, limit).await
}

// ─── Binding summary (eq.view) ────────────────────────────────────────────────

#[tauri::command]
pub async fn get_asset_binding_summary(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<bindings::AssetBindingSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    bindings::get_asset_binding_summary(&state.db, asset_id).await
}

#[tauri::command]
pub async fn get_asset_health_score(asset_id: i64, state: State<'_, AppState>) -> AppResult<health::AssetHealthScore> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    health::get_asset_health_score(&state.db, asset_id).await
}

#[tauri::command]
pub async fn list_asset_history_events(
    query: AssetHistoryQuery,
    state: State<'_, AppState>,
) -> AppResult<Vec<history::AssetHistoryEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    history::list_asset_history_events(&state.db, query).await
}

#[tauri::command]
pub async fn get_asset_history_summary(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<history::AssetHistorySummary> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    history::get_asset_history_summary(&state.db, asset_id).await
}

// ─── Mutation commands (eq.manage) ────────────────────────────────────────────

#[tauri::command]
pub async fn create_asset(payload: CreateAssetPayload, state: State<'_, AppState>) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    identity::update_asset_identity(&state.db, asset_id, payload, expected_row_version, user.user_id).await
}

#[tauri::command]
pub async fn link_asset_hierarchy(
    payload: LinkAssetPayload,
    state: State<'_, AppState>,
) -> AppResult<hierarchy::AssetHierarchyRow> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    hierarchy::link_asset_hierarchy(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn unlink_asset_hierarchy(
    relation_id: i64,
    effective_to: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<hierarchy::AssetHierarchyRow> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    hierarchy::move_asset_org_node(&state.db, asset_id, new_org_node_id, expected_row_version, user.user_id).await
}

/// "Déplacer le composant" (L2-03): the component goes under another parent (or to stock), from
/// an effective date. `eq.manage` where it is held: the component, its current parent and its new
/// parent must each be in a site the person can act on (checked in the move's transaction).
#[tauri::command]
pub async fn move_asset_component(
    payload: hierarchy::MoveAssetComponentPayload,
    state: State<'_, AppState>,
) -> AppResult<hierarchy::MoveAssetComponentResult> {
    let user = require_session!(state);
    let permission = crate::rbac::permissions::EQ_MANAGE;
    let scope = crate::auth::scope::access_map_cached(&state.db, &state.permission_cache, user.user_id)
        .await?
        .scope_of(permission);
    if !scope.is_anywhere() {
        return Err(AppError::PermissionDenied(format!("Permission requise : {permission}")));
    }
    crate::entitlements::queries::enforce_capability_for_permission(&state.db, permission).await?;
    crate::license::queries::enforce_permission_matrix(&state.db, user.user_id, permission).await?;
    hierarchy::move_asset_component(&state.db, payload, &scope, user.user_id).await
}

/// Where the person holds the transfer right (`eq.manage`), refused when held nowhere, with the
/// entitlement and licence checks of `require_permission!`.
async fn transfer_scope(state: &AppState, user_id: i32) -> AppResult<crate::auth::scope::AccessScope> {
    let permission = crate::assets::transfer::TRANSFER_PERMISSION;
    let scope = crate::auth::scope::access_map_cached(&state.db, &state.permission_cache, user_id)
        .await?
        .scope_of(permission);
    if !scope.is_anywhere() {
        return Err(AppError::PermissionDenied(format!("Permission requise : {permission}")));
    }
    crate::entitlements::queries::enforce_capability_for_permission(&state.db, permission).await?;
    crate::license::queries::enforce_permission_matrix(&state.db, user_id, permission).await?;
    Ok(scope)
}

/// "Transférer" (L2-04), before confirming: what stays attached to the asset and the nodes the
/// person may transfer it to (`eq.manage` where the asset is now).
#[tauri::command]
pub async fn preview_asset_transfer(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::assets::transfer::AssetTransferPreview> {
    let user = require_session!(state);
    let scope = transfer_scope(&state, user.user_id).await?;
    crate::assets::transfer::preview_asset_transfer(&state.db, asset_id, &scope).await
}

/// "Transférer" (L2-04): moves the asset to another org node with reason and effective date.
/// `eq.manage` on both the source and the destination node, checked in the transfer's transaction.
#[tauri::command]
pub async fn transfer_asset(
    payload: crate::assets::transfer::TransferAssetPayload,
    state: State<'_, AppState>,
) -> AppResult<crate::assets::transfer::TransferAssetResult> {
    let user = require_session!(state);
    let scope = transfer_scope(&state, user.user_id).await?;
    crate::assets::transfer::transfer_asset(&state.db, payload, &scope, user.user_id).await
}

// ─── Lifecycle events (eq.view / eq.manage) ───────────────────────────────────

#[tauri::command]
pub async fn list_asset_lifecycle_events(
    asset_id: i64,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<lifecycle::AssetLifecycleEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    lifecycle::list_asset_lifecycle_events(&state.db, asset_id, limit).await
}

#[tauri::command]
pub async fn record_lifecycle_event(
    payload: RecordLifecycleEventPayload,
    state: State<'_, AppState>,
) -> AppResult<lifecycle::AssetLifecycleEvent> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    lifecycle::record_lifecycle_event(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn decommission_asset(
    payload: DecommissionAssetPayload,
    state: State<'_, AppState>,
) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    decommission::decommission_asset(&state.db, payload, user.user_id).await
}

// ─── Meters & readings (eq.view / eq.manage) ─────────────────────────────────

#[tauri::command]
pub async fn list_asset_meters(asset_id: i64, state: State<'_, AppState>) -> AppResult<Vec<meters::AssetMeter>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    meters::list_asset_meters(&state.db, asset_id).await
}

#[tauri::command]
pub async fn create_asset_meter(
    payload: CreateAssetMeterPayload,
    state: State<'_, AppState>,
) -> AppResult<meters::AssetMeter> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    meters::create_asset_meter(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn record_meter_reading(
    payload: RecordMeterReadingPayload,
    confirm_jump: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<meters::MeterReading> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    meters::record_meter_reading_checked(&state.db, payload, confirm_jump.unwrap_or(false), user.user_id).await
}

/// Set or clear the alarm thresholds of a meter (L2-08): a reading that crosses one raises an
/// intervention request.
#[tauri::command]
pub async fn set_meter_thresholds(
    payload: SetMeterThresholdsPayload,
    state: State<'_, AppState>,
) -> AppResult<meters::AssetMeter> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    meters::set_meter_thresholds(&state.db, payload, user.user_id).await
}

// ─── Technical characteristics (eq.view / eq.manage, L2-07) ──────────────────

#[tauri::command]
pub async fn get_asset_characteristics(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<characteristics::AssetCharacteristicSheet> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    characteristics::get_asset_characteristics(&state.db, asset_id).await
}

#[tauri::command]
pub async fn save_asset_characteristics(
    payload: SaveAssetCharacteristicsPayload,
    state: State<'_, AppState>,
) -> AppResult<characteristics::AssetCharacteristicSheet> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    characteristics::save_asset_characteristics(&state.db, payload, user.user_id).await
}

// ─── Repeat failures (eq.view / eq.manage, L2-09) ─────────────────────────────

#[tauri::command]
pub async fn list_repeat_failure_assets(
    state: State<'_, AppState>,
) -> AppResult<repeat_failures::RepeatFailureOverview> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    repeat_failures::list_repeat_failure_assets(&state.db).await
}

#[tauri::command]
pub async fn get_asset_repeat_failure_status(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<repeat_failures::AssetRepeatFailureStatus> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    repeat_failures::asset_repeat_failure_status(&state.db, asset_id).await
}

#[tauri::command]
pub async fn list_asset_five_whys(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<repeat_failures::FiveWhyAnalysis>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    repeat_failures::list_five_whys(&state.db, asset_id).await
}

#[tauri::command]
pub async fn save_asset_five_why(
    payload: SaveFiveWhyPayload,
    state: State<'_, AppState>,
) -> AppResult<repeat_failures::FiveWhyAnalysis> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    repeat_failures::save_five_why(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn list_asset_corrective_actions(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<repeat_failures::CorrectiveAction>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    repeat_failures::list_corrective_actions(&state.db, asset_id).await
}

#[tauri::command]
pub async fn save_asset_corrective_action(
    payload: SaveCorrectiveActionPayload,
    state: State<'_, AppState>,
) -> AppResult<repeat_failures::CorrectiveAction> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    repeat_failures::save_corrective_action(&state.db, payload, user.user_id).await
}

/// Warning a reading would raise (far above the usual use), asked before saving it.
#[tauri::command]
pub async fn assess_meter_reading(
    meter_id: i64,
    reading_value: f64,
    reading_at: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Option<meters::MeterReadingWarning>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    meters::assess_meter_reading(&state.db, meter_id, reading_value, reading_at).await
}

/// Cancel the latest reading of a meter with a reason (the history keeps it, marked cancelled).
#[tauri::command]
pub async fn cancel_meter_reading(
    reading_id: i64,
    reason: String,
    state: State<'_, AppState>,
) -> AppResult<meters::MeterReading> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    meters::cancel_meter_reading(&state.db, reading_id, &reason, user.user_id).await
}

#[tauri::command]
pub async fn get_latest_meter_value(
    meter_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Option<meters::MeterReading>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    meters::get_latest_meter_value(&state.db, meter_id).await
}

#[tauri::command]
pub async fn list_meter_readings(
    meter_id: i64,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<meters::MeterReading>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
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
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    documents::list_asset_document_links(&state.db, asset_id, include_expired.unwrap_or(false)).await
}

#[tauri::command]
pub async fn upsert_asset_document_link(
    payload: UpsertDocumentLinkPayload,
    state: State<'_, AppState>,
) -> AppResult<documents::AssetDocumentLink> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    documents::upsert_asset_document_link(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn expire_asset_document_link(
    link_id: i64,
    valid_to: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<documents::AssetDocumentLink> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
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
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::list_asset_photos(&state.db, &app_data_dir, asset_id).await
}

#[tauri::command]
pub async fn read_asset_photo_preview(
    photo_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<photos::AssetPhotoPreview> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::read_asset_photo_preview(&state.db, &app_data_dir, photo_id).await
}

#[tauri::command]
pub async fn upload_asset_photo(
    app: AppHandle,
    payload: photos::UploadAssetPhotoPayload,
    state: State<'_, AppState>,
) -> AppResult<photos::AssetPhoto> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::upload_asset_photo(&state.db, &app_data_dir, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn delete_asset_photo(photo_id: i64, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_IMPORT,
        PermissionScope::Global
    );

    let batch = import::create_import_batch(&state.db, &filename, &file_sha256, Some(user.user_id as i64)).await?;
    import::parse_and_stage_csv(&state.db, batch.id, &csv_content).await
}

#[tauri::command]
pub async fn validate_asset_import_batch(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<import::ImportBatchSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_IMPORT,
        PermissionScope::Global
    );
    import::validate_import_batch(&state.db, batch_id, Some(user.user_id as i64)).await
}

#[tauri::command]
pub async fn get_asset_import_preview(batch_id: i64, state: State<'_, AppState>) -> AppResult<import::ImportPreview> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_IMPORT,
        PermissionScope::Global
    );
    import::get_import_preview(&state.db, batch_id).await
}

#[tauri::command]
pub async fn apply_asset_import_batch(
    batch_id: i64,
    policy: ApplyPolicy,
    state: State<'_, AppState>,
) -> AppResult<import::ApplyResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_IMPORT,
        PermissionScope::Global
    );
    import::apply_import_batch(&state.db, batch_id, &policy, Some(user.user_id as i64)).await
}

#[tauri::command]
pub async fn list_asset_import_batches(
    status_filter: Option<String>,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<import::ImportBatchSummary>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_IMPORT,
        PermissionScope::Global
    );
    import::list_import_batches(&state.db, status_filter, limit).await
}

// ─── Registry upgrade: paging, commercial data, maintenance profile, spares, criticality ──────

#[tauri::command]
pub async fn search_assets_page(
    filters: AssetSearchFilters,
    state: State<'_, AppState>,
) -> AppResult<search::AssetSearchPage> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    search::search_assets_page(&state.db, filters).await
}

/// One level of the equipment tree (roots, or the children of `parent_id`) read from the
/// hierarchy, within the registry filters. Same permission as the registry list.
#[tauri::command]
pub async fn list_asset_tree_level(
    filters: AssetSearchFilters,
    parent_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<crate::assets::tree::AssetTreeLevel> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    crate::assets::tree::list_asset_tree_level(&state.db, filters, parent_id).await
}

#[tauri::command]
pub async fn update_asset_commercial(
    asset_id: i64,
    input: identity::AssetCommercialInput,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<identity::Asset> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    identity::update_asset_commercial(&state.db, asset_id, input, expected_row_version).await
}

#[tauri::command]
pub async fn get_asset_maintenance_profile(
    asset_id: i64,
    include_descendants: bool,
    state: State<'_, AppState>,
) -> AppResult<maintenance_profile::AssetMaintenanceProfile> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    maintenance_profile::get_asset_maintenance_profile(&state.db, asset_id, include_descendants, chrono::Utc::now())
        .await
}

#[tauri::command]
pub async fn list_asset_spare_parts(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<spare_parts::AssetSparePart>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    spare_parts::list_asset_spare_parts(&state.db, asset_id).await
}

#[tauri::command]
pub async fn list_article_asset_usage(
    article_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<spare_parts::ArticleAssetUsage>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    spare_parts::list_article_asset_usage(&state.db, article_id).await
}

#[tauri::command]
pub async fn add_asset_spare_part(
    payload: spare_parts::AddSparePartPayload,
    state: State<'_, AppState>,
) -> AppResult<spare_parts::AssetSparePart> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    spare_parts::add_asset_spare_part(&state.db, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn update_asset_spare_part(
    id: i64,
    line: spare_parts::SparePartInput,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<spare_parts::AssetSparePart> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    spare_parts::update_asset_spare_part(&state.db, id, line, expected_row_version).await
}

#[tauri::command]
pub async fn remove_asset_spare_part(id: i64, expected_row_version: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    spare_parts::remove_asset_spare_part(&state.db, id, expected_row_version).await
}

#[tauri::command]
pub async fn get_asset_criticality_method(state: State<'_, AppState>) -> AppResult<criticality::CriticalityMethod> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    Ok(criticality::RISK_5X5_V1)
}

#[tauri::command]
pub async fn preview_asset_criticality(
    scores: criticality::CriticalityScores,
    state: State<'_, AppState>,
) -> AppResult<criticality::CriticalityResult> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    criticality::evaluate(&criticality::RISK_5X5_V1, &scores)
}

#[tauri::command]
pub async fn list_asset_criticality_assessments(
    asset_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<criticality::CriticalityAssessment>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::EQ_VIEW, PermissionScope::Global);
    criticality::list_assessments(&state.db, asset_id).await
}

/// Result of a criticality assessment: the stored assessment and the updated asset.
#[derive(serde::Serialize)]
pub struct AssessCriticalityResult {
    pub assessment: criticality::CriticalityAssessment,
    pub asset: identity::Asset,
}

#[tauri::command]
pub async fn assess_asset_criticality(
    payload: criticality::AssessCriticalityPayload,
    state: State<'_, AppState>,
) -> AppResult<AssessCriticalityResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::EQ_MANAGE,
        PermissionScope::Global
    );
    let (assessment, asset) =
        criticality::assess_asset_criticality(&state.db, payload, i64::from(user.user_id)).await?;
    Ok(AssessCriticalityResult { assessment, asset })
}
