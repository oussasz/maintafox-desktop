#[cfg(test)]
mod startup_tests {
    use crate::startup::StartupEvent;

    #[test]
    fn db_ready_event_serializes_correctly() {
        let event = StartupEvent::DbReady;
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"db_ready""#));
    }

    #[test]
    fn migrations_complete_event_includes_applied_count() {
        let event = StartupEvent::MigrationsComplete { applied: 3 };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"migrations_complete""#));
        assert!(json.contains(r#""applied":3"#));
    }

    #[test]
    fn failed_event_includes_reason() {
        let event = StartupEvent::Failed {
            reason: "DB locked".to_string(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"failed""#));
        assert!(json.contains(r#""reason":"DB locked""#));
    }

    #[test]
    fn ready_event_serializes_as_snake_case_stage() {
        let event = StartupEvent::Ready;
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"ready""#));
    }
}
