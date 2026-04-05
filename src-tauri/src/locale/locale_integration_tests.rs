// src-tauri/src/locale/locale_integration_tests.rs
//! SP05-F01 Sprint S3 — Integration tests for locale IPC commands.
//!
//! V1: resolve_locale_preference returns valid active_locale + supported_locales
//! V2: write_locale_config persists and resolve picks it up (language switch)
//! V3: persisted value survives re-read (simulates app restart re-read)

#[cfg(test)]
mod tests {
    use crate::locale::{detect_os_locale, read_locale_config, resolve_locale_preference, write_locale_config};

    /// Helper: create an in-memory DB with all migrations applied.
    async fn setup_db() -> sea_orm::DatabaseConnection {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("enable FK");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None).await.expect("migrations");

        db
    }

    /// V1 — Locale detection returns a valid value.
    /// active_locale must be "fr" or "en". supported_locales must contain both.
    #[tokio::test]
    async fn v1_locale_detection_returns_valid_value() {
        let db = setup_db().await;
        let pref = resolve_locale_preference(&db).await.expect("resolve");

        println!("[V1] active_locale      = {:?}", pref.active_locale);
        println!("[V1] user_locale        = {:?}", pref.user_locale);
        println!("[V1] tenant_locale      = {:?}", pref.tenant_locale);
        println!("[V1] os_locale          = {:?}", pref.os_locale);
        println!("[V1] supported_locales  = {:?}", pref.supported_locales);

        // active_locale must be "fr" or "en"
        assert!(
            pref.active_locale == "fr" || pref.active_locale == "en",
            "active_locale '{}' is not fr or en",
            pref.active_locale
        );

        // supported_locales must contain both
        assert!(pref.supported_locales.contains(&"fr".to_string()));
        assert!(pref.supported_locales.contains(&"en".to_string()));

        // No user/tenant preference set yet → should use OS locale or fallback
        assert!(pref.user_locale.is_none(), "user_locale should be None initially");
        assert!(pref.tenant_locale.is_none(), "tenant_locale should be None initially");

        // os_locale should match detect_os_locale()
        assert_eq!(pref.os_locale, detect_os_locale());
    }

    /// V2 — Language switch works: set user locale to "en", verify active changes.
    #[tokio::test]
    async fn v2_language_switch_persists() {
        let db = setup_db().await;

        // Before: no user preference
        let before = resolve_locale_preference(&db).await.expect("resolve before");
        assert!(before.user_locale.is_none());

        // Set user locale to "en"
        write_locale_config(&db, "locale.user_language", "en")
            .await
            .expect("write en");

        // After: user_locale = "en", active_locale = "en" (highest priority)
        let after = resolve_locale_preference(&db).await.expect("resolve after");
        println!(
            "[V2] after set_locale: active={}, user={:?}",
            after.active_locale, after.user_locale
        );
        assert_eq!(after.user_locale, Some("en".to_string()));
        assert_eq!(after.active_locale, "en");

        // Switch back to "fr"
        write_locale_config(&db, "locale.user_language", "fr")
            .await
            .expect("write fr");

        let switched = resolve_locale_preference(&db).await.expect("resolve switched");
        println!(
            "[V2] after switch back: active={}, user={:?}",
            switched.active_locale, switched.user_locale
        );
        assert_eq!(switched.user_locale, Some("fr".to_string()));
        assert_eq!(switched.active_locale, "fr");
    }

    /// V2b — Unsupported locale is rejected by write_locale_config.
    #[tokio::test]
    async fn v2b_unsupported_locale_rejected() {
        let db = setup_db().await;
        let result = write_locale_config(&db, "locale.user_language", "de").await;
        assert!(result.is_err(), "writing unsupported locale 'de' should fail");
        println!(
            "[V2b] Correctly rejected unsupported locale 'de': {:?}",
            result.unwrap_err()
        );
    }

    /// V3 — Locale preference is persisted across re-reads (simulates restart).
    #[tokio::test]
    async fn v3_locale_preference_persisted() {
        let db = setup_db().await;

        // Set to English
        write_locale_config(&db, "locale.user_language", "en")
            .await
            .expect("write en");

        // Re-read (simulates app restart — the DB is the same)
        let pref = resolve_locale_preference(&db).await.expect("resolve");
        println!(
            "[V3] persisted: active={}, user={:?}",
            pref.active_locale, pref.user_locale
        );
        assert_eq!(pref.user_locale, Some("en".to_string()));
        assert_eq!(pref.active_locale, "en");

        // Also verify via direct read_locale_config
        let direct = read_locale_config(&db, "locale.user_language")
            .await
            .expect("direct read");
        println!("[V3] direct DB read: locale.user_language = {:?}", direct);
        assert_eq!(direct, Some("en".to_string()));
    }

    /// V3b — Tenant default is respected when no user preference exists.
    #[tokio::test]
    async fn v3b_tenant_default_used_when_no_user_pref() {
        let db = setup_db().await;

        // Set tenant default to "en", no user preference
        write_locale_config(&db, "locale.default_language", "en")
            .await
            .expect("write tenant default");

        let pref = resolve_locale_preference(&db).await.expect("resolve");
        println!(
            "[V3b] tenant default: active={}, tenant={:?}, user={:?}",
            pref.active_locale, pref.tenant_locale, pref.user_locale
        );
        assert_eq!(pref.tenant_locale, Some("en".to_string()));
        assert!(pref.user_locale.is_none());
        assert_eq!(
            pref.active_locale, "en",
            "tenant default should be used when no user pref"
        );
    }

    /// V3c — User preference overrides tenant default.
    #[tokio::test]
    async fn v3c_user_pref_overrides_tenant_default() {
        let db = setup_db().await;

        // Tenant = en, User = fr → active should be "fr" (user wins)
        write_locale_config(&db, "locale.default_language", "en")
            .await
            .expect("write tenant");
        write_locale_config(&db, "locale.user_language", "fr")
            .await
            .expect("write user");

        let pref = resolve_locale_preference(&db).await.expect("resolve");
        println!(
            "[V3c] precedence: active={}, user={:?}, tenant={:?}",
            pref.active_locale, pref.user_locale, pref.tenant_locale
        );
        assert_eq!(pref.active_locale, "fr", "user preference must override tenant default");
        assert_eq!(pref.user_locale, Some("fr".to_string()));
        assert_eq!(pref.tenant_locale, Some("en".to_string()));
    }
}
