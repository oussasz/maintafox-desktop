pub mod admin_audit;
pub mod analytics_contract;
pub mod admin_governance;
pub mod admin_permissions;
pub mod admin_stats;
pub mod admin_users;
pub mod app;
pub mod archive;
pub mod assets;
pub mod audit_log;
pub mod auth;
pub mod backup;
pub mod computation_jobs;
pub mod dashboard;
pub mod data_integrity;
pub mod di;
pub mod diagnostics;
pub mod entitlements;
pub mod finance;
pub mod fta_rbd_eta;
pub mod markov_mc;
pub mod inspection;
pub mod inventory;
pub mod library_documents;
pub mod license;
pub mod locale;
pub mod lookup;
pub mod activity_feed;
pub mod activation;
pub mod advanced_rams;
pub mod notifications;
pub mod org;
pub mod personnel;
pub mod planning;
pub mod permit;
pub mod pm;
pub mod pm_delete;
pub mod product_license;
pub mod profile;
pub mod qualification;
pub mod reliability;
pub mod ram_review;
pub mod rbac;
pub mod reference;
pub mod reports;
pub mod settings;
pub mod sync;
pub mod updater;
pub mod wo;

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
pub use locale::get_locale_preference;
pub use locale::set_locale_preference;
