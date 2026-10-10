//! IPC commands for the settings module.
//!
//! Write operations require `adm.settings`. High-risk settings additionally
//! require an active step-up verification.

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::settings::{self, AppSetting, PolicySnapshot, SessionPolicy, SettingsChangeEvent};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

/// List all settings. Requires `adm.settings`.
#[tauri::command]
pub async fn list_all_settings(state: State<'_, AppState>) -> AppResult<Vec<AppSetting>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::list_all_settings(&state.db).await
}

/// List settings for a given category. Requires `adm.settings`.
#[tauri::command]
pub async fn list_settings_by_category(category: String, state: State<'_, AppState>) -> AppResult<Vec<AppSetting>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::list_settings_by_category(&state.db, &category).await
}

/// List distinct setting categories. Requires `adm.settings`.
#[tauri::command]
pub async fn list_settings_categories(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::list_settings_categories(&state.db).await
}

/// Read a single setting by key and optional scope.
#[tauri::command]
pub async fn get_setting(
    key: String,
    scope: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Option<AppSetting>> {
    let _user = require_session!(state);
    let resolved_scope = scope.unwrap_or_else(|| "tenant".to_string());
    settings::get_setting(&state.db, &key, &resolved_scope).await
}

#[derive(Debug, Deserialize)]
pub struct SetSettingPayload {
    pub key: String,
    pub scope: Option<String>,
    pub value_json: String,
    pub change_summary: Option<String>,
}

/// Write a setting value. Requires `adm.settings`.
/// Existing settings with `setting_risk = high` also require step-up.
#[tauri::command]
pub async fn set_setting(payload: SetSettingPayload, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );

    let scope = payload.scope.unwrap_or_else(|| "tenant".to_string());

    let existing = settings::get_setting(&state.db, &payload.key, &scope).await?;
    if let Some(ref s) = existing {
        if s.setting_risk == "high" {
            require_step_up!(state);
        }
    }

    let summary = payload
        .change_summary
        .unwrap_or_else(|| format!("Setting '{}' updated via IPC", payload.key));

    settings::set_setting(
        &state.db,
        &payload.key,
        &scope,
        &payload.value_json,
        user.user_id,
        &summary,
    )
    .await
}

/// Return the active policy snapshot for a domain.
#[tauri::command]
pub async fn get_policy_snapshot(domain: String, state: State<'_, AppState>) -> AppResult<Option<PolicySnapshot>> {
    let _user = require_session!(state);
    settings::get_active_policy(&state.db, &domain).await
}

/// Return the resolved session policy (active snapshot or safe defaults).
/// This command is intentionally unauthenticated for login-screen initialization.
#[tauri::command]
pub async fn get_session_policy(state: State<'_, AppState>) -> AppResult<SessionPolicy> {
    Ok(settings::load_session_policy(&state.db).await)
}

/// Return the most recent settings audit events.
#[tauri::command]
pub async fn list_setting_change_events(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SettingsChangeEvent>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::list_change_events(&state.db, limit.unwrap_or(50)).await
}

#[derive(Debug, Deserialize)]
pub struct SavePolicyDraftPayload {
    pub domain: String,
    pub snapshot_json: String,
}

#[derive(Debug, Deserialize)]
pub struct ActivatePolicyPayload {
    pub domain: String,
    pub snapshot_id: i64,
}

/// List all policy snapshots for a domain (active + drafts + history).
#[tauri::command]
pub async fn list_policy_snapshots(domain: String, state: State<'_, AppState>) -> AppResult<Vec<PolicySnapshot>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::list_policy_snapshots(&state.db, &domain).await
}

/// Create or update the draft policy snapshot for a domain.
#[tauri::command]
pub async fn save_policy_draft(
    payload: SavePolicyDraftPayload,
    state: State<'_, AppState>,
) -> AppResult<PolicySnapshot> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::save_policy_draft(&state.db, &payload.domain, &payload.snapshot_json, user.user_id).await
}

/// Run validation rules against the current draft.
#[tauri::command]
pub async fn test_policy_draft(
    domain: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<settings::PolicyTestResult>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::test_policy_draft(&state.db, &domain).await
}

/// Promote a draft snapshot to active. Security domains require step-up.
#[tauri::command]
pub async fn activate_policy(payload: ActivatePolicyPayload, state: State<'_, AppState>) -> AppResult<PolicySnapshot> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    if matches!(payload.domain.as_str(), "session" | "password") {
        require_step_up!(state);
    }
    settings::activate_policy(&state.db, &payload.domain, payload.snapshot_id, user.user_id).await
}

/// Discard the current draft for a domain.
#[tauri::command]
pub async fn discard_policy_draft(domain: String, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::discard_policy_draft(&state.db, &domain, user.user_id).await
}

/// List connection profiles (read-only) for the Integrations tab.
#[tauri::command]
pub async fn list_connection_profiles(state: State<'_, AppState>) -> AppResult<Vec<settings::ConnectionProfile>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::list_connection_profiles(&state.db).await
}

/// Ensure baseline `app_settings` rows and default policy snapshots exist.
/// Idempotent — safe when Settings loads and finds an empty catalog.
#[tauri::command]
pub async fn ensure_settings_defaults(state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    crate::db::seeder::seed_default_settings(&state.db).await?;
    settings::ensure_default_policies(&state.db).await?;
    Ok(())
}

/// Upload / replace the tenant document branding logo. Requires `adm.settings`.
#[tauri::command]
pub async fn upload_document_branding_logo(
    app: AppHandle,
    source_path: String,
    state: State<'_, AppState>,
) -> AppResult<settings::branding::DocumentBrandingLogoUploadResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    settings::branding::upload_document_branding_logo(&state.db, &app_data_dir, &source_path, user.user_id).await
}

/// Clear the tenant document branding logo. Requires `adm.settings`.
#[tauri::command]
pub async fn clear_document_branding_logo(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    settings::branding::clear_document_branding_logo(&state.db, &app_data_dir, user.user_id).await
}

/// Read the tenant document branding logo preview (`data:` URL payload).
/// Any authenticated session may read (print consumers inject the header).
#[tauri::command]
pub async fn read_document_branding_logo_preview(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<settings::branding::DocumentBrandingLogoPreview>> {
    let _user = require_session!(state);
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    settings::branding::read_document_branding_logo_preview(&state.db, &app_data_dir).await
}
