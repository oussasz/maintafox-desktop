#[cfg(test)]
mod startup_tests {
    use crate::startup::{format_startup_message, validate_startup_duration, StartupEvent};
    use std::time::Instant;

    // ── StartupEvent serialization ──────────────────────────────────────

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

    // ── Startup timing helpers ──────────────────────────────────────────

    #[test]
    fn validate_startup_within_budget_returns_true() {
        let start = Instant::now();
        let (elapsed, within) = validate_startup_duration(start, 4_000);
        assert!(
            elapsed < 4_000,
            "Test itself took longer than the budget — CI machine is too slow"
        );
        assert!(within, "Instant startup should always be within 4000ms budget");
    }

    #[test]
    fn format_startup_message_within_budget() {
        let msg = format_startup_message(350, true, 4_000);
        assert!(msg.contains("350ms"), "Message must include elapsed time");
        assert!(msg.contains("within"), "Message must say 'within' for under-budget");
        assert!(msg.contains("4000ms"), "Message must include the budget value");
    }

    #[test]
    fn format_startup_message_over_budget() {
        let msg = format_startup_message(5_200, false, 4_000);
        assert!(msg.contains("5200ms"), "Message must include elapsed time");
        assert!(
            msg.to_lowercase().contains("warning"),
            "Message must contain a warning indicator"
        );
        assert!(msg.contains("4000ms"), "Message must include the budget value");
    }
}
