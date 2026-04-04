// src-tauri/src/locale/mod.rs
//! Locale preference detection and persistence.
//!
//! Locale precedence (highest to lowest):
//!   1. User preference in system_config (key: locale.user_language)
//!   2. Tenant default in system_config (key: locale.default_language)
//!   3. OS locale (detected at startup via sys_locale)
//!   4. Hard fallback: "fr"

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use crate::errors::{AppError, AppResult};

#[cfg(test)]
mod locale_integration_tests;

/// Supported locale codes. Only lowercase base codes — no region variants.
pub const SUPPORTED_LOCALES: &[&str] = &["fr", "en"];
pub const DEFAULT_LOCALE:    &str = "fr";
pub const FALLBACK_LOCALE:   &str = "fr";

#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalePreference {
    /// The resolved locale to use right now (may be user, tenant, or OS).
    pub active_locale:    String,
    /// The user's explicit preference, if set.
    pub user_locale:      Option<String>,
    /// The tenant-level default, if set.
    pub tenant_locale:    Option<String>,
    /// The OS locale at startup (informational).
    pub os_locale:        Option<String>,
    /// All supported locale codes.
    pub supported_locales: Vec<String>,
}

/// Detect the OS locale and return the base language code (e.g., "fr" from "fr-DZ").
pub fn detect_os_locale() -> Option<String> {
    sys_locale::get_locale()
        .map(|loc| {
            let base = loc.split('-').next().unwrap_or("fr").to_lowercase();
            if SUPPORTED_LOCALES.contains(&base.as_str()) {
                base
            } else {
                FALLBACK_LOCALE.to_string()
            }
        })
}

/// Read a locale value from system_config by key.
pub async fn read_locale_config(db: &DatabaseConnection, key: &str) -> AppResult<Option<String>> {
    let row = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT value FROM system_config WHERE key = ?",
        [key.into()],
    ))
    .await?;

    Ok(row.and_then(|r| r.try_get::<String>("", "value").ok()))
}

/// Write a locale value to system_config (upsert).
pub async fn write_locale_config(
    db: &DatabaseConnection,
    key: &str,
    value: &str,
) -> AppResult<()> {
    if !SUPPORTED_LOCALES.contains(&value) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Unsupported locale '{value}'. Supported: {:?}",
            SUPPORTED_LOCALES
        )]));
    }
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT INTO system_config (key, value, updated_at)
           VALUES (?, ?, ?)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#,
        [key.into(), value.into(), now.into()],
    ))
    .await?;
    Ok(())
}

/// Resolve the full locale preference according to the precedence chain.
pub async fn resolve_locale_preference(db: &DatabaseConnection) -> AppResult<LocalePreference> {
    let user_locale   = read_locale_config(db, "locale.user_language").await?;
    let tenant_locale = read_locale_config(db, "locale.default_language").await?;
    let os_locale     = detect_os_locale();

    let active_locale = user_locale
        .clone()
        .or_else(|| tenant_locale.clone())
        .or_else(|| os_locale.clone())
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string());

    Ok(LocalePreference {
        active_locale,
        user_locale,
        tenant_locale,
        os_locale,
        supported_locales: SUPPORTED_LOCALES.iter().map(|s| s.to_string()).collect(),
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
    fn os_locale_returns_base_code() {
        let _locale = detect_os_locale();
    }

    #[test]
    fn unsupported_locale_rejected() {
        assert!(!SUPPORTED_LOCALES.contains(&"de"));
    }
}
