//! Application-level IPC commands.
//!
//! All responses are typed structs that serialize to JSON for the TypeScript
//! layer. Command names must match entries in `docs/IPC_COMMAND_REGISTRY.md`.

use tauri::State;

use crate::errors::AppResult;
use crate::state::AppState;

/// Platform and build information returned by `get_app_info`.
#[derive(serde::Serialize)]
pub struct AppInfoResponse {
    pub version: String,
    pub build_mode: String,
    pub os: String,
    pub arch: String,
    pub app_name: String,
    pub default_locale: String,
}

/// Returns static build metadata and runtime environment info.
/// This command is always callable even before session authentication.
#[tauri::command]
pub async fn get_app_info(state: State<'_, AppState>) -> AppResult<AppInfoResponse> {
    let config = state.config.read().await;
    Ok(AppInfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_mode: if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "release".to_string()
        },
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        app_name: config.app_name.clone(),
        default_locale: config.default_locale.clone(),
    })
}

/// One entry in the background task status list.
#[derive(serde::Serialize)]
pub struct TaskStatusEntry {
    pub id: String,
    pub status: crate::background::TaskStatus,
}

/// Returns the current status of all tracked background tasks.
/// Supervisor returns empty list before any tasks are spawned (Phase 1 normal state).
#[tauri::command]
pub async fn get_task_status(
    state: State<'_, AppState>,
) -> AppResult<Vec<TaskStatusEntry>> {
    let entries = state.tasks.status().await;
    Ok(entries
        .into_iter()
        .map(|(id, status)| TaskStatusEntry { id, status })
        .collect())
}
