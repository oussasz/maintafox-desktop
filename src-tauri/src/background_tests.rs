#[cfg(test)]
mod background_tests {
    use crate::background::{BackgroundTaskSupervisor, TaskStatus};
    use tokio_util::sync::CancellationToken;

    // ── Construction ────────────────────────────────────────────────────

    #[test]
    fn new_supervisor_is_not_shutdown() {
        let sup = BackgroundTaskSupervisor::new();
        let dbg = format!("{sup:?}");
        assert!(dbg.contains("shutdown_cancelled: false"));
    }

    #[test]
    fn default_is_equivalent_to_new() {
        let sup = BackgroundTaskSupervisor::default();
        let dbg = format!("{sup:?}");
        assert!(dbg.contains("BackgroundTaskSupervisor"));
    }

    #[test]
    fn clone_shares_state() {
        let a = BackgroundTaskSupervisor::new();
        let b = a.clone();
        // Both refer to the same inner Arc — debug output should be identical
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }

    // ── Spawn and status ────────────────────────────────────────────────

    #[tokio::test]
    async fn status_is_empty_before_any_spawn() {
        let sup = BackgroundTaskSupervisor::new();
        let entries = sup.status().await;
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn spawn_then_status_shows_running() {
        let sup = BackgroundTaskSupervisor::new();
        sup.spawn("test_task", |token: CancellationToken| async move {
            token.cancelled().await;
        })
        .await;

        let entries = sup.status().await;
        assert_eq!(entries.len(), 1);

        let (id, status) = &entries[0];
        assert_eq!(id, "test_task");
        assert_eq!(*status, TaskStatus::Running);

        // Cleanup
        sup.shutdown(1).await;
    }

    #[tokio::test]
    async fn duplicate_spawn_is_refused() {
        let sup = BackgroundTaskSupervisor::new();

        sup.spawn("dup", |token: CancellationToken| async move {
            token.cancelled().await;
        })
        .await;

        // Second spawn with same id should be silently refused
        sup.spawn("dup", |_token: CancellationToken| async move {
            panic!("should never run");
        })
        .await;

        let entries = sup.status().await;
        assert_eq!(entries.len(), 1, "duplicate spawn should not add a second entry");

        sup.shutdown(1).await;
    }

    // ── Cancel ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn cancel_signals_specific_task() {
        let sup = BackgroundTaskSupervisor::new();
        sup.spawn("cancelme", |token: CancellationToken| async move {
            token.cancelled().await;
        })
        .await;

        sup.cancel("cancelme").await;

        // After cancel, task is removed from the map
        let entries = sup.status().await;
        assert!(entries.is_empty(), "cancelled task should be removed");
    }

    #[tokio::test]
    async fn cancel_unknown_task_does_not_panic() {
        let sup = BackgroundTaskSupervisor::new();
        sup.cancel("nonexistent").await;
        // Should just log a warning, not panic or error
    }

    // ── Finished tasks ──────────────────────────────────────────────────

    #[tokio::test]
    async fn finished_task_shows_finished_status() {
        let sup = BackgroundTaskSupervisor::new();
        sup.spawn("quick", |_token: CancellationToken| async move {
            // Returns immediately
        })
        .await;

        // Give the spawned task a moment to finish
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let entries = sup.status().await;
        assert_eq!(entries.len(), 1);

        let (_, status) = &entries[0];
        assert_eq!(*status, TaskStatus::Finished);

        sup.shutdown(1).await;
    }

    // ── Shutdown ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn shutdown_cancels_all_tasks() {
        let sup = BackgroundTaskSupervisor::new();

        sup.spawn("a", |token: CancellationToken| async move {
            token.cancelled().await;
        })
        .await;

        sup.spawn("b", |token: CancellationToken| async move {
            token.cancelled().await;
        })
        .await;

        sup.shutdown(2).await;

        let entries = sup.status().await;
        assert!(entries.is_empty(), "all tasks should be drained after shutdown");
    }

    #[tokio::test]
    async fn shutdown_respects_timeout_for_stuck_tasks() {
        let sup = BackgroundTaskSupervisor::new();

        sup.spawn("stuck", |_token: CancellationToken| async move {
            // Ignores cancellation, sleeps forever
            tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
        })
        .await;

        let start = tokio::time::Instant::now();
        sup.shutdown(1).await;
        let elapsed = start.elapsed();

        // Timeout is 1s — shutdown should complete within ~1.5s even with a stuck task
        assert!(
            elapsed < tokio::time::Duration::from_secs(3),
            "shutdown took too long: {elapsed:?}"
        );
    }

    // ── TaskStatus serialization ────────────────────────────────────────

    #[test]
    fn task_status_serializes_as_snake_case() {
        let running = serde_json::to_string(&TaskStatus::Running).expect("serialize");
        assert_eq!(running, r#""running""#);

        let cancelled = serde_json::to_string(&TaskStatus::Cancelled).expect("serialize");
        assert_eq!(cancelled, r#""cancelled""#);

        let finished = serde_json::to_string(&TaskStatus::Finished).expect("serialize");
        assert_eq!(finished, r#""finished""#);
    }
}
