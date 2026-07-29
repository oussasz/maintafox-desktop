use crate::auth::rbac::PermissionScope;
use crate::commands::product_license;
use crate::db::{integrity, rams_presentation_seed, seeder};
use crate::db::rams_presentation_seed::{RamsPresentationSeedInput, RamsPresentationSeedReport};
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session};
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

/// Runs tenant bootstrap from activation claim.
/// - Always ensures root organization exists from activated tenant name.
/// - Seeds generic demo sandbox only when claim.has_demo_data = true.
#[tauri::command]
pub async fn seed_demo_data(state: State<'_, AppState>) -> AppResult<String> {
    if !product_license::is_product_activation_complete(&state.db).await? {
        return Ok("No active activation claim. Bootstrap skipped.".into());
    }
    crate::db::tenant_bootstrap::bootstrap_from_activation_claim(&state.db, 0).await?;
    Ok("Tenant bootstrap completed from activation claim.".into())
}

/// DEMO ONLY — explicit developer command.
/// Direct SQL RAMS simulation backfill (schedule, anchor WO, failure events, exposure logs).
/// When `equipment_id` is omitted, runs the full explicit demo orchestrator (SQL + presentation).
/// Never invoked by startup, equipment create, login, onboarding, or migrations.
#[tauri::command]
pub async fn seed_rams_sql_demo_data(
    equipment_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    if let Some(id) = equipment_id {
        crate::db::rams_sql_demo_seed::ensure_equipment_rams_simulation(&state.db, id).await?;
        Ok(format!("RAMS SQL demo seed completed for equipment {id}."))
    } else {
        rams_presentation_seed::run_explicit_rams_demo_seed(&state.db).await?;
        Ok("RAMS demo seed completed for eligible equipment (explicit).".into())
    }
}

/// DEMO ONLY — explicit developer command.
/// Seeds governed RAMS presentation data on an existing equipment via production WO close-out paths.
/// Invoked only from the manual "Generate Demo RAMS Data" UI or diagnostics IPC — never automatically.
#[tauri::command]
pub async fn seed_rams_presentation_data(
    input: RamsPresentationSeedInput,
    state: State<'_, AppState>,
) -> AppResult<RamsPresentationSeedReport> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    rams_presentation_seed::seed_rams_presentation_data(&state.db, input).await
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
