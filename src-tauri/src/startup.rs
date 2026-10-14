use std::path::PathBuf;
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

use crate::errors::AppResult;

/// Cold-start budget from PRD §14.1 (milliseconds).
const COLD_START_BUDGET_MS: u64 = 4_000;

/// Events emitted to the frontend during startup.
/// Corresponding TypeScript types live in shared/ipc-types.ts.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum StartupEvent {
    DbReady,
    MigrationsComplete {
        applied: u32,
    },
    EntitlementCacheLoaded,
    Ready,
    Failed {
        reason: String,
    },
    /// Encrypted database cannot be opened (key missing/wrong): UI asks for the recovery key.
    DatabaseKeyRequired {
        reason: String,
    },
}

// ── Pure helpers (unit-testable without Tauri runtime) ──────────────────────

/// Validates that the startup duration is within the given budget.
///
/// Returns `(elapsed_ms, within_budget)`.
/// Cold start budget: PRD §14.1 = 4 000 ms.
#[allow(clippy::cast_possible_truncation)]
pub fn validate_startup_duration(start: Instant, budget_ms: u64) -> (u64, bool) {
    let elapsed_ms = start.elapsed().as_millis() as u64;
    (elapsed_ms, elapsed_ms <= budget_ms)
}

/// Builds a human-readable startup diagnostic message for tracing output.
pub fn format_startup_message(elapsed_ms: u64, within_budget: bool, budget_ms: u64) -> String {
    if within_budget {
        format!("Startup complete in {elapsed_ms}ms (within {budget_ms}ms budget)")
    } else {
        format!("WARNING: Startup took {elapsed_ms}ms which exceeds the {budget_ms}ms cold-start budget")
    }
}

/// Forces a WAL checkpoint on the local SQLite database.
/// Must be called before any destructive migration is applied.
/// Returns Ok(()) on success or an error if the checkpoint fails.
pub async fn force_wal_checkpoint(db: &sea_orm::DatabaseConnection) -> crate::errors::AppResult<()> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA wal_checkpoint(FULL);".to_string(),
    ))
    .await
    .map(|_| ())
    .map_err(|e| crate::errors::AppError::Database(sea_orm::DbErr::Custom(format!("WAL checkpoint failed: {}", e))))
}

/// Creates a pre-migration backup of the database file.
/// Called when detecting that a pending migration is classified as destructive.
pub fn backup_database(db_path: &PathBuf, backup_dir: &PathBuf) -> crate::errors::AppResult<PathBuf> {
    use chrono::{DateTime, Utc};
    use std::time::SystemTime;

    let timestamp = DateTime::<Utc>::from(SystemTime::now())
        .format("%Y%m%d_%H%M%S")
        .to_string();

    let backup_filename = format!("pre_migration_{}.db", timestamp);
    let backup_path = backup_dir.join(&backup_filename);

    std::fs::create_dir_all(backup_dir).map_err(crate::errors::AppError::Io)?;

    std::fs::copy(db_path, &backup_path).map_err(|e| {
        crate::errors::AppError::Io(std::io::Error::other(format!("Pre-migration backup failed: {}", e)))
    })?;

    tracing::info!(
        backup_path = %backup_path.display(),
        "startup::pre_migration_backup_complete"
    );

    Ok(backup_path)
}

// ── Startup sequence ────────────────────────────────────────────────────────

/// Run the ordered startup sequence.
///
/// Called once from the `setup` hook in lib.rs after the window is created.
/// On success, emits `StartupEvent::Ready` and calls `window.show()`.
/// On failure, emits `StartupEvent::Failed` and shows the window with a
/// minimal error surface so the user is not left with an invisible process.
#[allow(clippy::cast_possible_truncation)]
pub async fn run_startup_sequence(app: AppHandle) -> AppResult<()> {
    let startup_start = Instant::now();

    let window = app.get_webview_window("main").expect("main window must exist");

    // Phase 1: database integrity and connection
    info!("startup: initialising database");
    let db_path = resolve_db_path(&app)?;
    // A restore staged by `restore_from_backup` is swapped in before the database is opened.
    let restored = match crate::backup::restore::apply_pending_restore(std::path::Path::new(&db_path)) {
        Ok(restored) => restored,
        Err(e) => {
            let reason = format!("Restore from backup failed: {e}");
            error!("{reason}");
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    };
    match crate::db::init_db(&db_path).await {
        Ok(conn) => {
            let state = crate::state::AppState::new(conn);
            app.manage(state);
            info!(
                elapsed_ms = startup_start.elapsed().as_millis() as u64,
                "startup::db_ready"
            );
            emit_event(&app, StartupEvent::DbReady);
        }
        Err(e) => {
            let reason = format!("Database initialisation failed: {e}");
            error!("{reason}");
            if matches!(e, crate::errors::AppError::DatabaseKeyUnavailable(_)) {
                emit_event(&app, StartupEvent::DatabaseKeyRequired { reason });
            } else {
                emit_event(&app, StartupEvent::Failed { reason });
            }
            window.show().ok();
            return Err(e);
        }
    }

    // Phase 2: schema migrations
    info!("startup: running migrations");
    let app_state = app.state::<crate::state::AppState>();
    // Sync capture is off while migrations and the startup seeders run: rows every device seeds
    // itself are never sent. It is installed again before the sync worker starts (Phase 4b).
    if let Err(e) = crate::sync::capture::suspend_capture(&app_state.db).await {
        let reason = format!("Sync capture could not be suspended before migrations: {e}");
        error!("{reason}");
        emit_event(&app, StartupEvent::Failed { reason });
        window.show().ok();
        return Err(e);
    }
    match crate::db::run_migrations(&app_state.db).await {
        Ok(()) => {
            info!(
                elapsed_ms = startup_start.elapsed().as_millis() as u64,
                "startup::migrations_complete"
            );
            emit_event(&app, StartupEvent::MigrationsComplete { applied: 0 });
        }
        Err(e) => {
            let reason = format!("Migration failed: {e}");
            error!("{reason}");
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }

    if restored {
        // The restored database resumes sync from a full snapshot (`forget_server_position`).
        if let Err(e) = crate::sync::transport::store::forget_server_position(&app_state.db).await {
            let reason = format!("Sync position could not be reset after the restore: {e}");
            error!("{reason}");
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
        info!("startup: restored backup will resynchronise from a snapshot");
    }

    // Phase 3: seed system data (idempotent — INSERT OR IGNORE)
    info!("startup: seeding system data");
    if let Err(e) = crate::db::seeder::seed_system_data(&app_state.db).await {
        warn!("startup: system seed returned error (non-fatal): {e}");
    }

    // Phase 3a: enforce mandatory WO type contract to prevent cross-environment drift.
    info!("startup: enforcing work_order_types integrity");
    if let Err(e) = crate::wo::types::ensure_required_work_order_types(&app_state.db).await {
        let reason = format!("Work order type integrity check failed: {e}");
        error!("{reason}");
        emit_event(&app, StartupEvent::Failed { reason: reason.clone() });
        window.show().ok();
        return Err(crate::errors::AppError::Internal(anyhow::anyhow!(reason)));
    }

    info!("startup: enforcing urgency_levels (work order priorities) integrity");
    if let Err(e) = crate::wo::priorities::ensure_required_urgency_levels(&app_state.db).await {
        let reason = format!("Urgency levels integrity check failed: {e}");
        error!("{reason}");
        emit_event(&app, StartupEvent::Failed { reason: reason.clone() });
        window.show().ok();
        return Err(crate::errors::AppError::Internal(anyhow::anyhow!(reason)));
    }

    info!("startup: enforcing work_order_statuses (OT lifecycle) integrity");
    if let Err(e) = crate::wo::statuses::ensure_required_work_order_statuses(&app_state.db).await {
        let reason = format!("Work order statuses integrity check failed: {e}");
        error!("{reason}");
        emit_event(&app, StartupEvent::Failed { reason: reason.clone() });
        window.show().ok();
        return Err(crate::errors::AppError::Internal(anyhow::anyhow!(reason)));
    }

    info!("startup: enforcing system reference catalog integrity (EQUIPMENT / DI / PERSONNEL / WORK)");
    if let Err(e) =
        crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&app_state.db).await
    {
        let reason = format!("System reference catalog integrity check failed: {e}");
        error!("{reason}");
        emit_event(&app, StartupEvent::Failed { reason: reason.clone() });
        window.show().ok();
        return Err(crate::errors::AppError::Internal(anyhow::anyhow!(reason)));
    }

    // Phase 3b: tenant bootstrap from activation claim.
    // Clean-by-default policy:
    // - always ensure a single root organization from activated tenant name
    // - only seed neutral demo sandbox when claim.has_demo_data = true
    if let Err(e) = crate::db::tenant_bootstrap::bootstrap_from_activation_claim(&app_state.db, None).await {
        warn!("startup: tenant bootstrap returned error (non-fatal): {e}");
    }
    // Idempotent heal + fail-closed check for DBs that still have NULL-scoped org nodes.
    if let Err(e) = crate::org::model_scope::heal_null_structure_model_ids_onto_active(&app_state.db).await {
        warn!("startup: org structure_model_id heal returned error (non-fatal): {e}");
    } else if let Err(e) = crate::org::model_scope::assert_no_null_structure_model_ids(&app_state.db).await {
        warn!("startup: org structure_model_id invariant violated after heal (non-fatal): {e}");
    }

    // Phase 3c: developer demo plant (debug builds only, opt-in with MAINTAFOX_SEED_DEMO=1).
    // Never part of the production flow: release builds do not compile it.
    // Each seeder no-ops once a product licence is activated (tenant data isolation).
    #[cfg(debug_assertions)]
    if std::env::var("MAINTAFOX_SEED_DEMO").as_deref() == Ok("1") {
        seed_developer_demo_plant(&app_state.db).await;
    }

    // RAMS demo seed is DEMO-ONLY and must never run in the production app flow
    // (startup, equipment create, login, onboarding, migrations).
    // Explicit developer entry points only (plus the debug-only opt-in above):
    //   - seed_rams_sql_demo_data
    //   - seed_rams_presentation_data

    // Phase 4: entitlement cache and offline-safe fallback state.
    info!("startup: ensuring licensing trust keys");
    if let Err(err) = crate::license::security::ensure_default_licensing_trust_keys(&app_state.db).await {
        warn!("startup: licensing trust key seed failed (non-fatal): {err}");
    }
    info!("startup: loading entitlement cache");
    if let Err(err) = crate::entitlements::queries::get_entitlement_summary(&app_state.db).await {
        warn!("startup: entitlement cache warmup failed (non-fatal): {err}");
    }
    info!(
        elapsed_ms = startup_start.elapsed().as_millis() as u64,
        "startup::entitlement_cache_loaded"
    );
    emit_event(&app, StartupEvent::EntitlementCacheLoaded);

    // Phase 4a: change capture for every synchronised table. Without it local changes would not
    // reach the outbox, so a failure stops the startup.
    match crate::sync::capture::install_capture(&app_state.db).await {
        Ok(report) => info!(
            entity_types = report.entity_types,
            baseline_rows = report.baseline_rows,
            "startup: sync capture installed"
        ),
        Err(e) => {
            let reason = format!("Sync capture could not be installed: {e}");
            error!("{reason}");
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }

    // Phase 4b: sync worker (protocol v2), background task `sync`. Closing the main window only
    // hides it in the tray (window.rs), so the process and the worker keep running.
    start_sync_worker(&app, &app_state).await;

    // Phase 5: notification scheduler background loop (non-fatal)
    {
        let scheduler_db = app_state.db.clone();
        tauri::async_runtime::spawn(async move {
            crate::notifications::scheduler::start_notification_scheduler(scheduler_db).await;
        });
        info!("startup: notification scheduler started");
    }

    // Phase 5a: notification emails through the control API (non-fatal; `notifications::email`)
    {
        let email_db = app_state.db.clone();
        tauri::async_runtime::spawn(async move {
            crate::notifications::email::start_email_dispatcher(
                email_db,
                env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL").to_string(),
            )
            .await;
        });
        info!("startup: notification email dispatcher started");
    }

    // Daily digest of notifications (`notifications::digest`), supervised background task.
    {
        let digest_db = app_state.db.clone();
        app_state
            .tasks
            .spawn("notification_digest", move |cancel| {
                crate::notifications::digest::run_digest_job(digest_db, cancel)
            })
            .await;
        info!("startup: notification daily digest started");
    }

    // Phase 5b: DI SLA breach poller (non-fatal)
    {
        let sla_db = app_state.db.clone();
        tauri::async_runtime::spawn(async move {
            crate::di::sla_poller::start_di_sla_poller(sla_db).await;
        });
        info!("startup: DI SLA poller started");
    }

    {
        let report_db = app_state.db.clone();
        let report_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
        tauri::async_runtime::spawn(async move {
            crate::reports::scheduler::start_report_scheduler(report_db, report_dir).await;
        });
        info!("startup: report scheduler started");
    }

    {
        let pm_db = app_state.db.clone();
        tauri::async_runtime::spawn(async move {
            crate::pm::scheduler::start_pm_generation_scheduler(pm_db).await;
        });
        info!("startup: PM generation scheduler started");
    }

    {
        let backup_db = app_state.db.clone();
        let backup_dir = crate::commands::backup::app_backup_dir(&app)?;
        tauri::async_runtime::spawn(async move {
            crate::backup::scheduler::start_backup_scheduler(backup_db, backup_dir).await;
        });
        info!("startup: backup scheduler started");
    }

    // ── Budget check and ready ──────────────────────────────────────────
    let (total_ms, within_budget) = validate_startup_duration(startup_start, COLD_START_BUDGET_MS);

    if within_budget {
        info!(elapsed_ms = total_ms, "startup::complete");
    } else {
        warn!(
            elapsed_ms = total_ms,
            budget_ms = COLD_START_BUDGET_MS,
            "startup::COLD_START_BUDGET_EXCEEDED — review DB init and migration time"
        );
    }

    emit_event(&app, StartupEvent::Ready);
    window
        .show()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;

    Ok(())
}

/// Developer demo plant: org tree, equipment, reference data, requests, work orders, test
/// users and RAMS failure history. Debug builds only; every seeder is idempotent and no-ops on
/// an activated (tenant) database. Failures are logged and never stop the startup.
#[cfg(debug_assertions)]
async fn seed_developer_demo_plant(db: &sea_orm::DatabaseConnection) {
    use crate::db::demo_seeder;
    info!("startup: seeding developer demo plant (MAINTAFOX_SEED_DEMO=1)");
    if let Err(e) = demo_seeder::seed_demo_data(db).await {
        warn!("startup: demo seed returned error (non-fatal): {e}");
    }
    if let Err(e) = demo_seeder::seed_reference_demo_data(db).await {
        warn!("startup: reference demo seed returned error (non-fatal): {e}");
    }
    if let Err(e) = demo_seeder::seed_test_viewer_user(db).await {
        warn!("startup: viewer user seed returned error (non-fatal): {e}");
    }
    if let Err(e) = demo_seeder::seed_technicien_user(db).await {
        warn!("startup: technician user seed returned error (non-fatal): {e}");
    }
    if let Err(e) = demo_seeder::seed_demo_work_orders(db).await {
        warn!("startup: work order demo seed returned error (non-fatal): {e}");
    }
    match crate::commands::product_license::is_product_activation_complete(db).await {
        Ok(false) => {
            if let Err(e) = crate::db::rams_presentation_seed::run_explicit_rams_demo_seed(db).await {
                warn!("startup: RAMS demo seed returned error (non-fatal): {e}");
            }
        }
        Ok(true) => info!("startup: RAMS demo seed skipped (licence activated)"),
        Err(e) => warn!("startup: licence state unreadable, RAMS demo seed skipped: {e}"),
    }
}

/// Resolve the database file path inside the Tauri app data directory.
/// Creates the directory if it does not already exist.
pub(crate) fn resolve_db_path(app: &AppHandle) -> AppResult<String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;
    std::fs::create_dir_all(&dir).map_err(crate::errors::AppError::Io)?;
    let db_file = dir.join("maintafox.db");
    db_file
        .to_str()
        .map(String::from)
        .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("Non-UTF8 app data path")))
}

/// Start the sync worker as background task `sync` and forward its status to the webview.
async fn start_sync_worker(app: &AppHandle, state: &crate::state::AppState) {
    use crate::sync::transport::{
        LicenseSyncAccess, SyncTiming, SyncWorker, SyncWorkerConfig, SYNC_STATUS_EVENT, SYNC_TASK_ID,
    };

    let handle = state.sync_worker.clone();
    {
        let mut status = handle.subscribe();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while status.changed().await.is_ok() {
                let current = status.borrow_and_update().clone();
                if let Err(e) = app.emit(SYNC_STATUS_EVENT, current) {
                    warn!("startup: cannot forward sync status to the window: {e}");
                }
            }
        });
    }

    let media_root = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            let message = format!("App data directory unavailable: {e}");
            error!("startup: sync worker not started: {message}");
            handle.publish_not_configured(&message);
            return;
        }
    };
    match crate::sync::media::backfill_library_document_media(&state.db, &media_root).await {
        Ok(0) => {}
        Ok(added) => tracing::info!(added, "startup: library documents queued for file sync"),
        Err(e) => warn!("startup: library documents could not be queued for file sync: {e}"),
    }
    let config = SyncWorkerConfig {
        api_base_url: env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL").to_string(),
        media_root,
        timing: SyncTiming::production(),
    };
    let worker = match SyncWorker::new(
        state.db.clone(),
        handle.clone(),
        config,
        std::sync::Arc::new(LicenseSyncAccess),
    ) {
        Ok(worker) => worker,
        Err(message) => {
            error!("startup: sync worker not started: {message}");
            handle.publish_not_configured(&message);
            return;
        }
    };
    let Some(commands) = handle.take_receiver() else {
        warn!("startup: sync worker already started");
        return;
    };
    state
        .tasks
        .spawn(SYNC_TASK_ID, move |cancel| worker.run(commands, cancel))
        .await;
    info!("startup: sync worker started");
}

fn emit_event(app: &AppHandle, event: StartupEvent) {
    if let Err(e) = app.emit("startup_event", event) {
        error!("failed to emit startup event: {e}");
    }
}
