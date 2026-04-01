use crate::errors::AppResult;

/// Health check command. Returns application status and version.
/// Used by the frontend to verify the IPC bridge is operational.
#[tauri::command]
pub async fn health_check() -> AppResult<serde_json::Value> {
    tracing::info!("health_check invoked");
    Ok(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
