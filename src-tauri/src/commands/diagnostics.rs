use crate::db::{integrity, seeder};
use crate::errors::AppResult;
use crate::state::AppState;
use tauri::State;

/// Runs the database integrity check and returns a report.
/// Called by the frontend on startup and from the diagnostics panel.
#[tauri::command]
pub async fn run_integrity_check(state: State<'_, AppState>) -> AppResult<integrity::IntegrityReport> {
    integrity::run_integrity_check(&state.db).await
}

/// Re-applies the system seed data and re-runs the integrity check.
/// Used for self-repair when the integrity check found recoverable issues.
/// Safe to call even if seed data is already present (idempotent).
#[tauri::command]
pub async fn repair_seed_data(state: State<'_, AppState>) -> AppResult<integrity::IntegrityReport> {
    tracing::info!("diagnostics::repair_seed_data called");
    seeder::seed_system_data(&state.db).await?;
    integrity::run_integrity_check(&state.db).await
}

// ─── SP06-F03 commands ────────────────────────────────────────────────────────

/// Return rich application info for the diagnostics panel.
///
/// Richer than the pre-auth `get_app_info` from `commands::app`: includes
/// DB schema version, active locale from `app_settings`, and process uptime.
/// Requires an active authenticated session.
#[tauri::command]
pub async fn get_diagnostics_info(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::diagnostics::DiagnosticsAppInfo> {
    let _user = crate::require_session!(state);
    Ok(crate::diagnostics::collect_diagnostics_app_info(&app, &state.db).await)
}

/// Generate and return a sanitized support bundle.
///
/// Captures the last 500 log lines (sanitized), application metadata, and
/// any non-fatal collection warnings. Read-only: no state changes, no network calls.
/// Requires an active authenticated session (limits accidental IPC exposure).
///
/// NOTE: Bundle generation reads the rolling log file from disk. On slow hardware
/// with a full 500-line day this may take ~100 ms. The frontend should show a
/// loading indicator while waiting.
#[tauri::command]
pub async fn generate_support_bundle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::diagnostics::SupportBundle> {
    let _user = crate::require_session!(state);
    Ok(crate::diagnostics::generate_support_bundle(&app, &state.db).await)
}
