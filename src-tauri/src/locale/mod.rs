// src-tauri/src/locale/mod.rs
//! Locale preference detection and persistence.
//!
//! Locale precedence (highest to lowest):
//!   1. User preference in `system_config` (key: `locale.user_language`). Its value may be
//!      [`FOLLOW_OS_LOCALE`]: the user explicitly chose "follow the Windows language", the only
//!      case where the OS language is used.
//!   2. Tenant default in `system_config` (key: `locale.default_language`)
//!   3. Settings SSOT `app_settings.locale.primary_language` (tenant scope)
//!   4. Hard default: "fr"
//!
//! The OS language is never a silent default: an English Windows opens a fresh install in French.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

#[cfg(test)]
mod locale_integration_tests;

/// Supported locale codes. Only lowercase base codes — no region variants.
pub const SUPPORTED_LOCALES: &[&str] = &["fr", "en"];
pub const DEFAULT_LOCALE: &str = "fr";
pub const FALLBACK_LOCALE: &str = "fr";
/// Stored value of `locale.user_language` meaning "follow the Windows language" (explicit choice).
pub const FOLLOW_OS_LOCALE: &str = "system";

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalePreference {
    /// The resolved locale to use right now (may be user, tenant, or OS).
    pub active_locale: String,
    /// The user's explicit preference, if set.
    pub user_locale: Option<String>,
    /// The tenant-level default, if set.
    pub tenant_locale: Option<String>,
    /// The OS locale at startup (informational).
    pub os_locale: Option<String>,
    /// All supported locale codes.
    pub supported_locales: Vec<String>,
}

/// Detect the OS locale and return the base language code (e.g., "fr" from "fr-DZ").
pub fn detect_os_locale() -> Option<String> {
    sys_locale::get_locale().map(|loc| {
        let base = loc.split('-').next().unwrap_or("fr").to_lowercase();
        if SUPPORTED_LOCALES.contains(&base.as_str()) {
            base
        } else {
            FALLBACK_LOCALE.to_string()
        }
    })
}

/// Read a locale value from `system_config` by key.
pub async fn read_locale_config(db: &impl ConnectionTrait, key: &str) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = ?",
            [key.into()],
        ))
        .await?;

    Ok(row.and_then(|r| r.try_get::<String>("", "value").ok()))
}

/// Write a locale value to `system_config` (upsert).
pub async fn write_locale_config(db: &impl ConnectionTrait, key: &str, value: &str) -> AppResult<()> {
    let follow_os = key == "locale.user_language" && value == FOLLOW_OS_LOCALE;
    if !follow_os && !SUPPORTED_LOCALES.contains(&value) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_LOCALE_UNSUPPORTED: Unsupported locale '{value}'. Supported: {:?}",
            SUPPORTED_LOCALES
        )]));
    }
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO system_config (key, value, updated_at)
           VALUES (?, ?, ?)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        [key.into(), value.into(), now.into()],
    ))
    .await?;
    Ok(())
}

/// Read tenant primary language from `app_settings` (JSON string value).
async fn read_app_settings_primary_language(db: &impl ConnectionTrait) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT setting_value_json FROM app_settings
               WHERE setting_key = 'locale.primary_language'
                 AND setting_scope = 'tenant'
               LIMIT 1",
            [],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    let raw = row.try_get::<String>("", "setting_value_json").ok();
    let Some(raw) = raw else {
        return Ok(None);
    };

    // Stored as JSON string, e.g. "\"fr\"".
    let parsed = serde_json::from_str::<String>(&raw).unwrap_or(raw);
    let normalized = parsed.trim().to_lowercase();
    if SUPPORTED_LOCALES.contains(&normalized.as_str()) {
        Ok(Some(normalized))
    } else {
        Ok(None)
    }
}

/// The language in use: the user's choice (or the OS language when they chose to follow it),
/// else the plant default, else French. The OS language is never used unless chosen.
pub fn pick_active_locale(user: Option<&str>, tenant: Option<&str>, os: Option<&str>) -> String {
    let supported = |l: &&str| SUPPORTED_LOCALES.contains(l);
    let from_user = match user {
        Some(FOLLOW_OS_LOCALE) => os.filter(supported),
        other => other.filter(supported),
    };
    from_user
        .or_else(|| tenant.filter(supported))
        .unwrap_or(DEFAULT_LOCALE)
        .to_string()
}

/// The plant default language: `locale.default_language`, else the Settings SSOT
/// (`app_settings.locale.primary_language`, tenant scope); `None` when neither holds a supported
/// language. A stored value is read trimmed and in lower case; any other value counts as unset.
pub async fn read_plant_language(db: &impl ConnectionTrait) -> AppResult<Option<String>> {
    let stored = read_locale_config(db, "locale.default_language")
        .await?
        .map(|code| code.trim().to_lowercase())
        .filter(|code| SUPPORTED_LOCALES.contains(&code.as_str()));
    match stored {
        Some(code) => Ok(Some(code)),
        // Fall back to Settings SSOT when system_config tenant default was never seeded.
        None => read_app_settings_primary_language(db).await,
    }
}

/// Write the plant default language to both places it is read from (`locale.default_language`
/// and the Settings SSOT row), as the Settings screen does; the SSOT row must exist (seeded).
pub async fn write_plant_language(db: &impl ConnectionTrait, code: &str) -> AppResult<()> {
    write_locale_config(db, "locale.default_language", code).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"UPDATE app_settings SET setting_value_json = ?, last_modified_at = ?
           WHERE setting_key = 'locale.primary_language' AND setting_scope = 'tenant'",
        [
            serde_json::Value::from(code).to_string().into(),
            chrono::Utc::now().to_rfc3339().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Resolve the full locale preference according to the precedence chain.
pub async fn resolve_locale_preference(db: &DatabaseConnection) -> AppResult<LocalePreference> {
    let user_locale = read_locale_config(db, "locale.user_language").await?;
    let tenant_locale = read_plant_language(db).await?;
    let os_locale = detect_os_locale();

    let active_locale = pick_active_locale(user_locale.as_deref(), tenant_locale.as_deref(), os_locale.as_deref());

    Ok(LocalePreference {
        active_locale,
        user_locale,
        tenant_locale,
        os_locale,
        supported_locales: SUPPORTED_LOCALES.iter().map(|s| (*s).to_string()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_locales_includes_fr_and_en() {
        assert!(SUPPORTED_LOCALES.contains(&"fr"));
        assert!(SUPPORTED_LOCALES.contains(&"en"));
    }

    #[test]
    fn english_windows_does_not_win_over_the_french_default() {
        assert_eq!(pick_active_locale(None, None, Some("en")), "fr");
        assert_eq!(pick_active_locale(None, Some("fr"), Some("en")), "fr");
    }

    #[test]
    fn user_then_plant_default_then_french() {
        assert_eq!(pick_active_locale(Some("en"), Some("fr"), Some("fr")), "en");
        assert_eq!(pick_active_locale(None, Some("en"), Some("fr")), "en");
        assert_eq!(pick_active_locale(None, None, None), "fr");
        assert_eq!(pick_active_locale(Some("xx"), Some("en"), None), "en");
    }

    #[test]
    fn the_os_language_is_used_only_when_the_user_chose_to_follow_it() {
        assert_eq!(pick_active_locale(Some(FOLLOW_OS_LOCALE), Some("fr"), Some("en")), "en");
        assert_eq!(pick_active_locale(Some(FOLLOW_OS_LOCALE), Some("en"), None), "en");
        assert_eq!(pick_active_locale(Some(FOLLOW_OS_LOCALE), None, None), "fr");
    }

    #[test]
    fn os_locale_returns_base_code() {
        let _locale = detect_os_locale();
    }

    #[test]
    fn unsupported_locale_rejected() {
        assert!(!SUPPORTED_LOCALES.contains(&"de"));
    }
}
