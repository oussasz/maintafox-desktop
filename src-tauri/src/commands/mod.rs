pub mod app;
pub mod auth;
pub mod diagnostics;
pub mod lookup;

use tauri::State;

use crate::errors::AppResult;
use crate::state::AppState;

/// Typed response for the `health_check` IPC command.
#[derive(serde::Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub version: String,
    pub db_connected: bool,
    pub locale: String,
}

/// Health check command. Returns application status, version, and config info.
/// Used by the frontend to verify the IPC bridge and managed state are operational.
#[tauri::command]
pub async fn health_check(state: State<'_, AppState>) -> AppResult<HealthCheckResponse> {
    tracing::info!("health_check invoked");
    let config = state.config.read().await;
    Ok(HealthCheckResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_connected: true, // if we got here, the pool is live
        locale: config.default_locale.clone(),
    })
}

pub use app::get_app_info;
pub use app::get_task_status;
pub use app::shutdown_app;
