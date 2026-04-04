// src-tauri/src/commands/locale.rs
//! IPC commands for locale preference management.

use tauri::State;
use serde::Deserialize;
use crate::state::AppState;
use crate::errors::AppResult;
use crate::locale::{self, LocalePreference};

/// Get the resolved locale preference.
/// Does NOT require an active session — the login screen needs this.
#[tauri::command]
pub async fn get_locale_preference(
    state: State<'_, AppState>,
) -> AppResult<LocalePreference> {
    locale::resolve_locale_preference(&state.db).await
}

/// Payload for setting locale preference.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLocalePayload {
    /// The locale code to set ("fr" | "en").
    pub locale: String,
    /// Whether to save as the tenant default (requires adm.settings permission)
    /// or as a user preference. Defaults to user preference if false/omitted.
    pub as_tenant_default: Option<bool>,
}

/// Set the locale preference.
/// User preference → sets locale.user_language in system_config.
/// Tenant default  → sets locale.default_language (requires adm.settings).
#[tauri::command]
pub async fn set_locale_preference(
    payload: SetLocalePayload,
    state: State<'_, AppState>,
) -> AppResult<LocalePreference> {
    // Require session to change locale preference
    let user = crate::require_session!(state);

    let key = if payload.as_tenant_default.unwrap_or(false) {
        // Check adm.settings permission for tenant-wide change
        let allowed = crate::auth::rbac::check_permission(
            &state.db,
            user.user_id,
            "adm.settings",
            &crate::auth::rbac::PermissionScope::Global,
        )
        .await?;
        if !allowed {
            return Err(crate::errors::AppError::PermissionDenied("adm.settings".into()));
        }
        "locale.default_language"
    } else {
        "locale.user_language"
    };

    locale::write_locale_config(&state.db, key, &payload.locale).await?;

    tracing::info!(
        user_id = %user.user_id,
        locale = %payload.locale,
        key = %key,
        "locale::preference_updated"
    );

    locale::resolve_locale_preference(&state.db).await
}
