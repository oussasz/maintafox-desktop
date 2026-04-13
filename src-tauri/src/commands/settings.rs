//! IPC commands for the settings module.
//!
//! Write operations require `adm.settings`. High-risk settings additionally
//! require an active step-up verification.

use serde::Deserialize;
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::settings::{self, AppSetting, PolicySnapshot, SessionPolicy, SettingsChangeEvent};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

/// List all settings. Requires `adm.settings`.
#[tauri::command]
pub async fn list_all_settings(state: State<'_, AppState>) -> AppResult<Vec<AppSetting>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    settings::list_all_settings(&state.db).await
}

/// List settings for a given category. Requires `adm.settings`.
#[tauri::command]
pub async fn list_settings_by_category(
    category: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<AppSetting>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    settings::list_settings_by_category(&state.db, &category).await
}

/// List distinct setting categories. Requires `adm.settings`.
#[tauri::command]
pub async fn list_settings_categories(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
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
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);

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
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    settings::list_change_events(&state.db, limit.unwrap_or(50)).await
}
