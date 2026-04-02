#[cfg(test)]
mod state_tests {
    use crate::state::{AppConfig, SessionManagerStub};

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

    // ── SessionManagerStub ──────────────────────────────────────────────

    #[test]
    fn session_stub_default_has_no_active_session() {
        let stub = SessionManagerStub::default();
        assert!(!stub.has_active_session);
    }

    #[test]
    fn session_stub_can_be_toggled() {
        let mut stub = SessionManagerStub::default();
        stub.has_active_session = true;
        assert!(stub.has_active_session);
    }
}
