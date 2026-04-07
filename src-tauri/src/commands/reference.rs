//! Reference data governance IPC commands.
//!
//! Phase 2 – Sub-phase 03 – File 01 – Sprint S3.
//!
//! Permission gates:
//!   ref.view    — list/get domains, sets, values
//!   ref.manage  — create/update domains, create draft sets, create/update/deactivate/move values
//!   ref.publish — validate, publish, and run merge/migrate governance actions

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::reference::{aliases, domains, imports as ref_imports, migrations as ref_migrations, publish as ref_publish, search as ref_search, sets, values};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

// ─── Domain commands (ref.view / ref.manage) ─────────────────────────────────

#[tauri::command]
pub async fn list_reference_domains(
    state: State<'_, AppState>,
) -> AppResult<Vec<domains::ReferenceDomain>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    domains::list_reference_domains(&state.db).await
}

#[tauri::command]
pub async fn get_reference_domain(
    domain_id: i64,
    state: State<'_, AppState>,
) -> AppResult<domains::ReferenceDomain> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    domains::get_reference_domain(&state.db, domain_id).await
}

#[tauri::command]
pub async fn create_reference_domain(
    payload: domains::CreateReferenceDomainPayload,
    state: State<'_, AppState>,
) -> AppResult<domains::ReferenceDomain> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    domains::create_reference_domain(&state.db, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn update_reference_domain(
    domain_id: i64,
    payload: domains::UpdateReferenceDomainPayload,
    state: State<'_, AppState>,
) -> AppResult<domains::ReferenceDomain> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    domains::update_reference_domain(&state.db, domain_id, payload, i64::from(user.user_id)).await
}

// ─── Set commands (ref.view / ref.manage / ref.publish) ──────────────────────

#[tauri::command]
pub async fn list_reference_sets(
    domain_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<sets::ReferenceSet>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    sets::list_sets_for_domain(&state.db, domain_id).await
}

#[tauri::command]
pub async fn get_reference_set(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<sets::ReferenceSet> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    sets::get_reference_set(&state.db, set_id).await
}

#[tauri::command]
pub async fn create_draft_reference_set(
    domain_id: i64,
    state: State<'_, AppState>,
) -> AppResult<sets::ReferenceSet> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    sets::create_draft_set(&state.db, domain_id, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn validate_reference_set(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<sets::ReferenceSet> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    sets::validate_set(&state.db, set_id, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn publish_reference_set(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<sets::ReferenceSet> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    sets::publish_set(&state.db, set_id, i64::from(user.user_id)).await
}

// ─── Value commands (ref.view / ref.manage) ──────────────────────────────────

#[tauri::command]
pub async fn list_reference_values(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<values::ReferenceValue>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    values::list_values(&state.db, set_id).await
}

#[tauri::command]
pub async fn get_reference_value(
    value_id: i64,
    state: State<'_, AppState>,
) -> AppResult<values::ReferenceValue> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    values::get_value(&state.db, value_id).await
}

#[tauri::command]
pub async fn create_reference_value(
    payload: values::CreateReferenceValuePayload,
    state: State<'_, AppState>,
) -> AppResult<values::ReferenceValue> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    values::create_value(&state.db, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn update_reference_value(
    value_id: i64,
    payload: values::UpdateReferenceValuePayload,
    state: State<'_, AppState>,
) -> AppResult<values::ReferenceValue> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    values::update_value(&state.db, value_id, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn deactivate_reference_value(
    value_id: i64,
    state: State<'_, AppState>,
) -> AppResult<values::ReferenceValue> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    values::deactivate_value(&state.db, value_id, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn move_reference_value_parent(
    value_id: i64,
    new_parent_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<values::ReferenceValue> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    values::move_value_parent(&state.db, value_id, new_parent_id, i64::from(user.user_id)).await
}

// ─── Migration commands (ref.publish + step-up) ─────────────────────────────

#[tauri::command]
pub async fn merge_reference_values(
    domain_id: i64,
    from_value_id: i64,
    to_value_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_migrations::ReferenceUsageMigrationResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    require_step_up!(state);
    ref_migrations::merge_reference_values(
        &state.db,
        domain_id,
        from_value_id,
        to_value_id,
        i64::from(user.user_id),
    )
    .await
}

#[tauri::command]
pub async fn migrate_reference_usage(
    domain_id: i64,
    from_value_id: i64,
    to_value_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_migrations::ReferenceUsageMigrationResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    require_step_up!(state);
    ref_migrations::migrate_reference_usage(
        &state.db,
        domain_id,
        from_value_id,
        to_value_id,
        i64::from(user.user_id),
    )
    .await
}

#[tauri::command]
pub async fn list_reference_migrations(
    domain_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ref_migrations::ReferenceValueMigration>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    ref_migrations::list_reference_migrations(&state.db, domain_id, limit.unwrap_or(50)).await
}

// ─── Alias commands (ref.view / ref.manage) ──────────────────────────────────

#[tauri::command]
pub async fn list_reference_aliases(
    reference_value_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<aliases::ReferenceAlias>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    aliases::list_aliases(&state.db, reference_value_id).await
}

#[tauri::command]
pub async fn get_reference_alias(
    alias_id: i64,
    state: State<'_, AppState>,
) -> AppResult<aliases::ReferenceAlias> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    aliases::get_alias(&state.db, alias_id).await
}

#[tauri::command]
pub async fn create_reference_alias(
    payload: aliases::CreateReferenceAliasPayload,
    state: State<'_, AppState>,
) -> AppResult<aliases::ReferenceAlias> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    aliases::create_alias(&state.db, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn update_reference_alias(
    alias_id: i64,
    payload: aliases::UpdateReferenceAliasPayload,
    state: State<'_, AppState>,
) -> AppResult<aliases::ReferenceAlias> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    aliases::update_alias(&state.db, alias_id, payload, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn delete_reference_alias(
    alias_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    aliases::delete_alias(&state.db, alias_id, i64::from(user.user_id)).await
}

// ─── Import / Export commands (ref.manage / ref.publish) ─────────────────────

#[tauri::command]
pub async fn create_ref_import_batch(
    domain_id: i64,
    source_filename: String,
    source_sha256: String,
    state: State<'_, AppState>,
) -> AppResult<ref_imports::RefImportBatchSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    ref_imports::create_import_batch(
        &state.db,
        domain_id,
        &source_filename,
        &source_sha256,
        Some(i64::from(user.user_id)),
    )
    .await
}

#[tauri::command]
pub async fn stage_ref_import_rows(
    batch_id: i64,
    rows: Vec<ref_imports::ImportRowInput>,
    state: State<'_, AppState>,
) -> AppResult<ref_imports::RefImportBatchSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    ref_imports::stage_import_rows(&state.db, batch_id, rows).await
}

#[tauri::command]
pub async fn validate_ref_import_batch(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_imports::RefImportBatchSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    ref_imports::validate_import_batch(
        &state.db,
        batch_id,
        Some(i64::from(user.user_id)),
    )
    .await
}

#[tauri::command]
pub async fn apply_ref_import_batch(
    batch_id: i64,
    policy: ref_imports::RefImportApplyPolicy,
    state: State<'_, AppState>,
) -> AppResult<ref_imports::RefImportApplyResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    require_step_up!(state);
    ref_imports::apply_import_batch(&state.db, batch_id, policy, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn get_ref_import_preview(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_imports::RefImportPreview> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    ref_imports::get_import_preview(&state.db, batch_id).await
}

#[tauri::command]
pub async fn export_ref_domain_set(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_imports::RefExportResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    ref_imports::export_domain_set(&state.db, set_id).await
}

#[tauri::command]
pub async fn list_ref_import_batches(
    domain_id: i64,
    status_filter: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ref_imports::RefImportBatchSummary>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    ref_imports::list_import_batches(&state.db, domain_id, status_filter, limit).await
}

// ─── Search commands (ref.view) ─────────────────────────────────────────────

#[tauri::command]
pub async fn search_reference_values(
    domain_code: String,
    query: String,
    locale: String,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ref_search::ReferenceSearchHit>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.view", PermissionScope::Global);
    ref_search::search_reference_values(
        &state.db,
        &domain_code,
        &query,
        &locale,
        limit.unwrap_or(50),
    )
    .await
}

// -- Reference publish governance commands (SP03-F04-S1) ---------------------

#[tauri::command]
pub async fn compute_ref_publish_readiness(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_publish::ReferencePublishReadiness> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    ref_publish::compute_publish_readiness(&state.db, set_id).await
}

#[tauri::command]
pub async fn preview_ref_publish_impact(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_publish::ReferenceImpactSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    ref_publish::preview_publish_impact(&state.db, set_id).await
}

#[tauri::command]
pub async fn governed_publish_reference_set(
    set_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ref_publish::ReferencePublishResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.publish", PermissionScope::Global);
    require_step_up!(state);
    ref_publish::publish_reference_set(&state.db, set_id, i64::from(user.user_id)).await
}
