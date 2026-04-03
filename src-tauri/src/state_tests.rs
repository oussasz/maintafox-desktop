#[cfg(test)]
mod state_tests {
    use crate::auth::session_manager::SessionManager;
    use crate::state::AppConfig;

    // ── AppConfig defaults ──────────────────────────────────────────────

    #[test]
    fn app_config_default_app_name() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.app_name, "Maintafox");
    }

    #[test]
    fn app_config_default_locale_is_french() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.default_locale, "fr");
    }

    #[test]
    fn app_config_default_offline_grace_hours() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.max_offline_grace_hours, 72);
    }

    #[test]
    fn app_config_clone_is_independent() {
        let original = AppConfig::default();
        let mut cloned = original.clone();
        cloned.app_name = "Changed".to_string();
        assert_eq!(original.app_name, "Maintafox");
        assert_eq!(cloned.app_name, "Changed");
    }

    // ── SessionManager (replaces SessionManagerStub from SP02) ──────────

    #[test]
    fn session_manager_default_has_no_active_session() {
        let mgr = SessionManager::new();
        assert!(!mgr.is_authenticated());
    }

    #[test]
    fn session_manager_current_is_none_by_default() {
        let mgr = SessionManager::new();
        assert!(mgr.current.is_none());
        assert!(mgr.current_user().is_none());
    }
}
