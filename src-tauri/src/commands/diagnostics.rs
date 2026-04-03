use tauri::State;
use crate::state::AppState;
use crate::errors::AppResult;
use crate::db::{integrity, seeder};

/// Runs the database integrity check and returns a report.
/// Called by the frontend on startup and from the diagnostics panel.
#[tauri::command]
pub async fn run_integrity_check(
    state: State<'_, AppState>,
) -> AppResult<integrity::IntegrityReport> {
    integrity::run_integrity_check(&state.db).await
}

/// Re-applies the system seed data and re-runs the integrity check.
/// Used for self-repair when the integrity check found recoverable issues.
/// Safe to call even if seed data is already present (idempotent).
#[tauri::command]
pub async fn repair_seed_data(
    state: State<'_, AppState>,
) -> AppResult<integrity::IntegrityReport> {
    tracing::info!("diagnostics::repair_seed_data called");
    seeder::seed_system_data(&state.db).await?;
    integrity::run_integrity_check(&state.db).await
}
