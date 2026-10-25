// src-tauri/src/commands/locale.rs
//! IPC commands for locale preference management.

use crate::errors::AppResult;
use crate::locale::{self, LocalePreference};
use crate::state::AppState;
use serde::Deserialize;
use tauri::State;

/// Get the resolved locale preference.
/// Does NOT require an active session — the login screen needs this.
#[tauri::command]
pub async fn get_locale_preference(state: State<'_, AppState>) -> AppResult<LocalePreference> {
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
/// User preference → sets `locale.user_language` in `system_config`. Needs no session: the
/// language picked on the sign-in screen is kept for this PC and applied from the first screen
/// (the value is checked against the supported languages, nothing else can be written).
/// Tenant default  → sets `locale.default_language` (requires adm.settings).
#[tauri::command]
pub async fn set_locale_preference(
    payload: SetLocalePayload,
    state: State<'_, AppState>,
) -> AppResult<LocalePreference> {
    let as_tenant_default = payload.as_tenant_default.unwrap_or(false);
    let key = if as_tenant_default {
        let user = crate::require_session!(state);
        // Check adm.settings permission for tenant-wide change
        let allowed = crate::auth::rbac::check_permission(
            &state.db,
            user.user_id,
            crate::rbac::permissions::ADM_SETTINGS,
            &crate::auth::rbac::PermissionScope::Global,
        )
        .await?;
        if !allowed {
            return Err(crate::errors::AppError::PermissionDenied(
                crate::rbac::permissions::ADM_SETTINGS.into(),
            ));
        }
        "locale.default_language"
    } else {
        // From the pre-update backup until the restart, nothing can be changed (updater SF1).
        crate::updater::refuse_while_installing()?;
        "locale.user_language"
    };

    // The follow-Windows value is a personal choice, never a plant default.
    if as_tenant_default && payload.locale == locale::FOLLOW_OS_LOCALE {
        return Err(crate::errors::AppError::ValidationFailed(vec![
            "GATE_CMD_LOCALE_TENANT_DEFAULT: The plant default language must be French or English.".into(),
        ]));
    }

    locale::write_locale_config(&state.db, key, &payload.locale).await?;

    // Tenant-wide Settings change must also clear a stale user override so the
    // admin-selected language actually becomes the active runtime locale.
    if as_tenant_default {
        locale::write_locale_config(&state.db, "locale.user_language", &payload.locale).await?;
    }

    tracing::info!(locale = %payload.locale, key = %key, "locale::preference_updated");

    locale::resolve_locale_preference(&state.db).await
}
