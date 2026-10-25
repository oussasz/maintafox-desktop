//! IPC commands for the settings module.
//!
//! Write operations require `adm.settings`. High-risk settings additionally
//! require an active step-up verification.

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
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
    ensure_readable_by_any_session(&key)?;
    settings::get_setting(&state.db, &key, &resolved_scope).await
}

/// The reason recorded with a setting change. A high-risk change (step-up) must carry the
/// person's reason: it is the audit trail's "why" next to who, when and old → new.
fn change_reason(high_risk: bool, summary: Option<&str>) -> AppResult<Option<String>> {
    let reason = summary.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if high_risk
        && reason
            .as_ref()
            .is_none_or(|r| r.chars().count() < settings::catalog::MIN_REASON_CHARS)
    {
        return Err(AppError::ValidationFailed(vec!["SETTING_REASON_REQUIRED".to_string()]));
    }
    Ok(reason)
}

/// A per-site rule at every planning site. Requires `adm.settings`.
#[tauri::command]
pub async fn list_site_settings(
    key: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<settings::site::SiteSettingView>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::site::list_site_settings(&state.db, &key).await
}

/// Sets a per-site rule. Requires `adm.settings` and a fresh step-up; the reason is audited.
#[tauri::command]
pub async fn set_site_setting(input: settings::site::SetSiteSettingInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    require_step_up!(state);
    settings::site::set_site_setting(&state.db, &input, user.user_id, true).await
}

/// The audited changes of a per-site rule at one site, newest first. Requires `adm.settings`.
#[tauri::command]
pub async fn list_site_setting_events(
    site_node_id: i64,
    key: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<settings::site::SiteSettingEvent>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::site::list_site_setting_events(&state.db, site_node_id, &key, 20).await
}

/// Any session may read the plant parameters it needs to display (currency, language, header,
/// reopen window). Licence onboarding (activation token, machine fingerprint), bootstrap state
/// and module-managed rows are read only through their own commands.
fn ensure_readable_by_any_session(key: &str) -> AppResult<()> {
    if settings::catalog::is_plant_parameter(key) {
        Ok(())
    } else {
        Err(AppError::PermissionDenied(format!(
            "Setting '{key}' is not readable through the generic settings command."
        )))
    }
}

#[derive(Debug, Deserialize)]
pub struct SetSettingPayload {
    pub key: String,
    pub scope: Option<String>,
    pub value_json: String,
    pub change_summary: Option<String>,
}

/// Write a setting value. Requires `adm.settings`.
/// Only settings of the catalog are accepted, each validated against its kind; settings with
/// `setting_risk = high` also require step-up.
#[tauri::command]
pub async fn set_setting(payload: SetSettingPayload, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );

    let Some(def) = settings::catalog::definition(&payload.key) else {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_SETTING_UNKNOWN: Unknown setting '{}'.",
            payload.key
        )]));
    };
    let scope = payload.scope.unwrap_or_else(|| def.scope.to_string());
    if scope != def.scope {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_SETTING_SCOPE_MISMATCH: Setting '{}' is stored at {} scope, not '{scope}'.",
            def.key, def.scope
        )]));
    }

    let existing = settings::get_setting(&state.db, &payload.key, &scope).await?;
    let high_risk = def.risk == "high" || existing.as_ref().is_some_and(|s| s.setting_risk == "high");
    if high_risk {
        require_step_up!(state);
    }

    let mut value_json = settings::catalog::validate_value(def, &payload.value_json)?;
    if def.kind == settings::catalog::SettingKind::DocumentHeader {
        value_json = settings::branding::keep_stored_logo(&state.db, &value_json).await?;
    }
    if def.kind == settings::catalog::SettingKind::CurrencyCode {
        // The tenant currency is an active currency of Données de référence › Devises.
        let code: String = serde_json::from_str(&value_json)?;
        crate::reference::currencies::ensure_active_currency(&state.db, &code).await?;
    }

    let summary = change_reason(high_risk, payload.change_summary.as_deref())?
        .unwrap_or_else(|| format!("Setting '{}' updated via IPC", payload.key));

    settings::set_setting_recorded(
        &state.db,
        def.key,
        &scope,
        &value_json,
        Some(user.user_id),
        &summary,
        high_risk,
    )
    .await
}

/// The currencies of the published list (Données de référence › Devises): code, names, symbol,
/// minor units, active flag. Any session (pickers and amount display read it).
#[tauri::command]
pub async fn list_reference_currencies(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::reference::currencies::CurrencyOption>> {
    let _user = require_session!(state);
    crate::reference::currencies::published_currencies(&state.db).await
}

/// Settings in use with their shipped and current values, lock state, last change and the roles
/// allowed to change them. Requires `adm.settings`.
#[tauri::command]
pub async fn get_settings_overview(state: State<'_, AppState>) -> AppResult<settings::catalog::SettingsOverview> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::catalog::overview(&state.db).await
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
    setting_key: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SettingsChangeEvent>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    settings::list_change_events(&state.db, limit.unwrap_or(50), setting_key.as_deref()).await
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

#[cfg(test)]
mod tests {
    use super::{change_reason, ensure_readable_by_any_session};

    #[test]
    fn a_high_risk_change_needs_a_reason() {
        let refused = change_reason(true, Some("  ok  "));
        assert!(
            matches!(refused, Err(crate::errors::AppError::ValidationFailed(m)) if m[0] == "SETTING_REASON_REQUIRED")
        );
        assert!(change_reason(true, None).is_err());
        assert_eq!(
            change_reason(true, Some(" Single engineer on site "))
                .unwrap()
                .as_deref(),
            Some("Single engineer on site")
        );
        assert_eq!(change_reason(false, None).unwrap(), None);
    }

    #[test]
    fn only_plant_parameters_are_readable_by_any_session() {
        assert!(ensure_readable_by_any_session("locale.currency_code").is_ok());
        assert!(ensure_readable_by_any_session("documents.default_header").is_ok());
        assert!(ensure_readable_by_any_session("wo.reopen_window_days").is_ok());
        assert!(ensure_readable_by_any_session("product.license_onboarding").is_err());
        assert!(ensure_readable_by_any_session("tenant.bootstrap_state").is_err());
    }
}

/// The letterhead prints use, checked with the Settings panel's rules: `valid` (with its HTML),
/// `absent`, or `invalid` (`DOC_HEADER_INVALID`) so the print can warn. Any session (prints).
#[tauri::command]
pub async fn get_print_document_header(
    state: State<'_, AppState>,
) -> AppResult<settings::branding::PrintDocumentHeader> {
    let _user = require_session!(state);
    settings::branding::print_document_header(&state.db).await
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
