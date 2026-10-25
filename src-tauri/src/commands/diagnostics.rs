use crate::auth::rbac::PermissionScope;
use crate::commands::product_license;
use crate::db::rams_presentation_seed::{RamsPresentationSeedInput, RamsPresentationSeedReport};
use crate::db::{integrity, rams_presentation_seed, seeder};
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session};
use sea_orm::TransactionTrait;
use tauri::State;

/// Runs the database integrity check and returns a report.
/// Called by the frontend on startup (pre-auth, read-only) and from the diagnostics panel.
#[tauri::command]
pub async fn run_integrity_check(state: State<'_, AppState>) -> AppResult<integrity::IntegrityReport> {
    integrity::run_integrity_check(&state.db).await
}

/// Re-applies the system seed data and re-runs the integrity check.
/// Used for self-repair when the integrity check found recoverable issues.
/// Safe to call even if seed data is already present (idempotent).
/// It repairs this PC only, exactly as every start does: the seeder runs with sync capture off
/// (as at startup), so the rows it puts back are never sent to the other PCs as unauthored
/// changes. All of it is one transaction: the capture triggers are dropped, the seed applied and
/// the triggers installed again inside it, so no other write of the running app (sync worker,
/// schedulers) can happen while capture is off (the pool has one connection: they wait), and a
/// failure rolls everything back, triggers included.
#[tauri::command]
pub async fn repair_seed_data(state: State<'_, AppState>) -> AppResult<integrity::IntegrityReport> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    crate::require_step_up!(state);
    tracing::info!("diagnostics::repair_seed_data called");
    reseed_without_capture(&state.db).await?;
    integrity::run_integrity_check(&state.db).await
}

/// The seed of [`repair_seed_data`], in one transaction with capture off inside it.
pub(crate) async fn reseed_without_capture(db: &sea_orm::DatabaseConnection) -> AppResult<()> {
    let txn = db.begin().await?;
    crate::sync::capture::suspend_capture(&txn).await?;
    seeder::seed_system_data(&txn).await?;
    crate::sync::capture::install_capture_in(&txn).await?;
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    async fn count(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .and_then(|r| r.try_get::<i64>("", "n").ok())
            .expect("count")
    }

    /// The repair puts a locked role's lost baseline permission back on this PC only: nothing is
    /// queued for the other PCs, and every capture trigger is there again afterwards.
    #[tokio::test]
    async fn a_repair_reseeds_this_pc_only_and_keeps_capture() {
        let db = crate::db::test_db::seeded().await;
        crate::sync::capture::install_capture_in(&db).await.expect("capture");
        // Capture triggers only (`sync::capture`, prefix `sync_cap_`).
        let triggers =
            "SELECT COUNT(*) AS n FROM sqlite_master WHERE type = 'trigger' AND substr(name, 1, 9) = 'sync_cap_'";
        let journal = "SELECT COUNT(*) AS n FROM sync_capture_journal";
        let outbox = "SELECT COUNT(*) AS n FROM sync_outbox";
        let missing = "SELECT COUNT(*) AS n FROM permissions p WHERE p.is_system = 1 AND NOT EXISTS ( \
                         SELECT 1 FROM role_permissions rp JOIN roles r ON r.id = rp.role_id \
                          WHERE rp.permission_id = p.id AND r.name = 'Administrator')";
        let triggers_before = count(&db, triggers).await;
        assert!(triggers_before > 0);
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM role_permissions WHERE role_id = (SELECT id FROM roles WHERE name = 'Administrator') \
               AND permission_id = (SELECT MIN(id) FROM permissions WHERE is_system = 1)"
                .to_string(),
        ))
        .await
        .expect("lose one");
        assert_eq!(count(&db, missing).await, 1);
        let outbox_before = count(&db, outbox).await;
        let journal_before = count(&db, journal).await;

        super::reseed_without_capture(&db).await.expect("repair");

        assert_eq!(count(&db, missing).await, 0, "baseline back");
        assert_eq!(count(&db, journal).await, journal_before, "nothing captured");
        assert_eq!(count(&db, outbox).await, outbox_before, "nothing sent");
        assert_eq!(count(&db, triggers).await, triggers_before, "capture installed again");
    }
}

/// Runs tenant bootstrap from activation claim.
/// - Always ensures root organization exists from activated tenant name.
/// - Seeds generic demo sandbox only when claim.has_demo_data = true.
#[tauri::command]
pub async fn seed_demo_data(state: State<'_, AppState>) -> AppResult<String> {
    if !product_license::is_product_activation_complete(&state.db).await? {
        return Ok("No active activation claim. Bootstrap skipped.".into());
    }
    // Prefer authenticated admin path; allow activation-only bootstrap when no session.
    let has_session = state.session.read().await.current.is_some();
    let actor = if has_session {
        let user = require_session!(state);
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_SETTINGS,
            PermissionScope::Global
        );
        Some(user.user_id)
    } else {
        None
    };
    crate::db::tenant_bootstrap::bootstrap_from_activation_claim(&state.db, actor).await?;
    Ok("Tenant bootstrap completed from activation claim.".into())
}

/// DEMO ONLY — explicit developer command.
/// Direct SQL RAMS simulation backfill (schedule, anchor WO, failure events, exposure logs).
/// When `equipment_id` is omitted, runs the full explicit demo orchestrator (SQL + presentation).
/// Never invoked by startup, equipment create, login, onboarding, or migrations.
#[tauri::command]
pub async fn seed_rams_sql_demo_data(equipment_id: Option<i64>, state: State<'_, AppState>) -> AppResult<String> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
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

/// Send the support bundle to Maintafox support (vendor control API): the same sanitized bundle as
/// `generate_support_bundle`, uploaded to object storage through a presigned URL and verified by
/// the server (size + SHA-256). With a note, the server also opens a support ticket.
/// Requires a session and an activated device; the activation token never reaches the webview.
#[tauri::command]
pub async fn upload_support_bundle(
    note: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::sync::transport::wire::DiagnosticBundleCompleted> {
    let _user = crate::require_session!(state);
    let ticket = crate::diagnostics::upload::ticket_from_note(note.as_deref())
        .map_err(|m| crate::errors::AppError::ValidationFailed(vec![m]))?;
    let token = product_license::sync_activation_bearer_token(&state.db)
        .await?
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| {
            crate::errors::AppError::ValidationFailed(vec![
                "GATE_SUPPORT_NOT_ACTIVATED: This device is not activated: the support bundle cannot be sent."
                    .to_string(),
            ])
        })?;
    let bundle = crate::diagnostics::generate_support_bundle(&app, &state.db).await;
    let bytes = serde_json::to_vec_pretty(&bundle)?;
    let client = crate::sync::transport::client::ControlPlaneClient::new(
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        crate::sync::transport::SyncTiming::production().timeouts,
    )
    .map_err(crate::errors::AppError::SyncError)?;
    let completed = crate::diagnostics::upload::upload_bundle_bytes(
        &client,
        &token,
        bytes,
        &app.package_info().version.to_string(),
        ticket,
    )
    .await
    .map_err(|e| crate::errors::AppError::SyncError(format!("Support bundle upload failed: {}", e.message())))?;
    tracing::info!(bundle_id = %completed.bundle_id, ticket = ?completed.ticket_id, "support bundle sent");
    Ok(completed)
}
