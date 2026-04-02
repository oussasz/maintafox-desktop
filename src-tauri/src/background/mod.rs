//! Background task supervisor.
//!
//! Rules:
//!   - All long-running background work is spawned through this supervisor.
//!   - Each task receives a CancellationToken it must poll regularly.
//!   - Graceful shutdown broadcasts cancellation and joins all handles with a timeout.
//!   - Task identifiers are stable strings (e.g. "sync", "updater", "analytics").

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub type TaskId = &'static str;

/// Status of a tracked background task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Cancelled,
    Finished,
}

/// A tracked background task entry.
struct TaskEntry {
    handle: JoinHandle<()>,
    token: CancellationToken,
}

/// Top-level supervisor that owns all background task handles.
///
/// Clone is cheap; the inner state is Arc-wrapped.
#[derive(Clone)]
pub struct BackgroundTaskSupervisor {
    tasks: Arc<Mutex<HashMap<&'static str, TaskEntry>>>,
    /// Parent token — cancelling this cancels all child tokens.
    shutdown_token: CancellationToken,
}

impl std::fmt::Debug for BackgroundTaskSupervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundTaskSupervisor")
            .field("shutdown_cancelled", &self.shutdown_token.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl BackgroundTaskSupervisor {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            shutdown_token: CancellationToken::new(),
        }
    }

    /// Spawn a new background task identified by `id`.
    ///
    /// If a task with the same id is already running, the spawn is refused and
    /// a warning is logged. The future receives a child cancellation token.
    pub async fn spawn<F, Fut>(&self, id: TaskId, factory: F)
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut tasks = self.tasks.lock().await;
        if tasks.contains_key(id) {
            warn!("background: task '{id}' is already running; new spawn refused");
            return;
        }

        let child_token = self.shutdown_token.child_token();
        let token_for_task = child_token.clone();
        let handle = tokio::spawn(async move {
            info!("background task started: {id}");
            factory(token_for_task).await;
            info!("background task finished: {id}");
        });

        tasks.insert(id, TaskEntry { handle, token: child_token });
        info!("background: spawned task '{id}'");
    }

    /// Cancel a specific task by id.
    pub async fn cancel(&self, id: TaskId) {
        let mut tasks = self.tasks.lock().await;
        if let Some(entry) = tasks.get(id) {
            entry.token.cancel();
            info!("background: cancellation signalled for task '{id}'");
        } else {
            warn!("background: cancel requested for unknown task '{id}'");
        }
        tasks.remove(id);
    }

    /// Return a list of (id, status) for all known tasks.
    pub async fn status(&self) -> Vec<(String, TaskStatus)> {
        let tasks = self.tasks.lock().await;
        tasks
            .iter()
            .map(|(id, entry)| {
                let status = if entry.handle.is_finished() {
                    TaskStatus::Finished
                } else if entry.token.is_cancelled() {
                    TaskStatus::Cancelled
                } else {
                    TaskStatus::Running
                };
                (id.to_string(), status)
            })
            .collect()
    }

    /// Graceful shutdown: cancel all tasks and await them with a timeout.
    ///
    /// Called from the Tauri `on_window_event` Destroyed handler.
    pub async fn shutdown(&self, timeout_secs: u64) {
        info!("background: initiating graceful shutdown (timeout={timeout_secs}s)");
        self.shutdown_token.cancel();

        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

        let mut tasks = self.tasks.lock().await;
        for (id, entry) in tasks.drain() {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, entry.handle).await {
                Ok(Ok(())) => info!("background: task '{id}' shutdown cleanly"),
                Ok(Err(e)) => error!("background: task '{id}' panicked: {e}"),
                Err(_) => warn!("background: task '{id}' did not finish within timeout"),
            }
        }

        info!("background: shutdown complete");
    }
}

impl Default for BackgroundTaskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
