pub mod activation;
pub mod activity_feed;
pub mod admin_audit;
pub mod admin_governance;
pub mod admin_permissions;
pub mod admin_stats;
pub mod admin_users;
pub mod advanced_rams;
pub mod analytics_contract;
pub mod analytics_dashboard;
pub mod app;
pub mod archive;
pub mod assets;
pub mod audit_log;
pub mod auth;
pub mod backup;
pub mod computation_jobs;
pub mod dashboard;
pub mod data_integrity;
pub mod database_security;
pub mod di;
pub mod diagnostics;
pub mod entitlements;
pub mod finance;
pub mod fta_rbd_eta;
pub mod home;
pub mod inspection;
pub mod inventory;
pub mod inventory_negative_stock;
pub mod library_documents;
pub mod license;
pub mod locale;
pub mod lookup;
pub mod markov_mc;
pub mod mobile;
pub mod mobile_enrolment;
pub mod notifications;
pub mod org;
pub mod permit;
pub mod personnel;
pub mod planning;
pub mod pm;
pub mod pm_delete;
pub mod product_license;
pub mod profile;
pub mod qualification;
pub mod ram_review;
pub mod rbac;
pub mod reference;
pub mod reliability;
pub mod reliability_study;
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
    /// `ok` when the database answered, `degraded` otherwise.
    pub status: String,
    pub version: String,
    /// The database answered a `SELECT 1` within [`DB_PROBE_TIMEOUT`].
    pub db_connected: bool,
    /// Round trip of that probe, when it answered.
    pub db_latency_ms: Option<u64>,
    /// Why it did not: `DB_TIMEOUT` or `DB_ERROR` (the detail goes to the log).
    pub db_error_code: Option<String>,
    pub locale: String,
}

/// How long the health check waits for the database.
pub const DB_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Outcome of the database probe of the health check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbProbe {
    pub connected: bool,
    pub latency_ms: Option<u64>,
    pub error_code: Option<&'static str>,
}

/// A cheap `SELECT 1` on the database, bounded by `timeout`: never reports "connected" without
/// an answer.
pub async fn probe_database(db: &sea_orm::DatabaseConnection, timeout: std::time::Duration) -> DbProbe {
    use sea_orm::ConnectionTrait;
    let started = std::time::Instant::now();
    let query = db.query_one(sea_orm::Statement::from_string(
        sea_orm::DbBackend::Sqlite,
        "SELECT 1 AS ok".to_string(),
    ));
    match tokio::time::timeout(timeout, query).await {
        Ok(Ok(Some(_))) => DbProbe {
            connected: true,
            latency_ms: Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
            error_code: None,
        },
        Ok(Ok(None)) => {
            tracing::warn!("health_check: SELECT 1 returned no row");
            DbProbe {
                connected: false,
                latency_ms: None,
                error_code: Some("DB_ERROR"),
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "health_check: database probe failed");
            DbProbe {
                connected: false,
                latency_ms: None,
                error_code: Some("DB_ERROR"),
            }
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = timeout.as_millis(),
                "health_check: database probe timed out"
            );
            DbProbe {
                connected: false,
                latency_ms: None,
                error_code: Some("DB_TIMEOUT"),
            }
        }
    }
}

/// Health check command. Returns application status, version, and config info, and really asks
/// the database (`probe_database`), so the status bar can show its state. Answers even when the
/// database does not (status `degraded`): the IPC bridge itself is up.
#[tauri::command]
pub async fn health_check(state: State<'_, AppState>) -> AppResult<HealthCheckResponse> {
    tracing::info!("health_check invoked");
    let probe = probe_database(&state.db, DB_PROBE_TIMEOUT).await;
    let config = state.config.read().await;
    Ok(HealthCheckResponse {
        status: if probe.connected { "ok" } else { "degraded" }.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_connected: probe.connected,
        db_latency_ms: probe.latency_ms,
        db_error_code: probe.error_code.map(str::to_string),
        locale: config.default_locale.clone(),
    })
}

#[cfg(test)]
mod health_tests {
    use super::*;

    #[tokio::test]
    async fn the_probe_answers_from_the_database() {
        let db = crate::db::test_db::empty_memory_db().await;
        let probe = probe_database(&db, DB_PROBE_TIMEOUT).await;
        assert!(probe.connected);
        assert!(probe.latency_ms.is_some());
        assert_eq!(probe.error_code, None);
    }

    #[tokio::test]
    async fn a_closed_database_is_reported_not_connected() {
        let db = crate::db::test_db::empty_memory_db().await;
        db.clone().close().await.expect("close");
        let probe = probe_database(&db, DB_PROBE_TIMEOUT).await;
        assert!(!probe.connected);
        assert_eq!(probe.latency_ms, None);
        assert!(matches!(probe.error_code, Some("DB_ERROR" | "DB_TIMEOUT")));
    }
}

pub use app::get_app_info;
pub use app::get_task_status;
pub use app::shutdown_app;
pub use locale::get_locale_preference;
pub use locale::set_locale_preference;
