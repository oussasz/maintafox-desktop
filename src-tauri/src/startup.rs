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
    /// The installed update could not update the database: the pre-update backup is staged and
    /// the UI offers to reinstall `from_version` (master plan Phase 5.5).
    UpdateRollbackRequired {
        from_version: String,
        to_version: String,
        reason: String,
        /// Set when the pre-update backup could not be verified (missing, no checksum, changed):
        /// nothing was restored and no reinstall is offered.
        backup_refusal: Option<String>,
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
    // A restore staged for a rollback whose installer never ran is cancelled first: only a restore
    // the update journal justifies (or one an administrator staged) is ever applied.
    match crate::updater::reconcile_staged_restore_at_startup(&app, std::path::Path::new(&db_path)) {
        Ok(true) => warn!("startup: the rollback was not installed; its staged restore was cancelled"),
        Ok(false) => {}
        Err(e) => {
            let reason = format!("The staged restore was not applied: the update journal could not be checked: {e}");
            error!("{reason}");
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }
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

    // Phase 1b: the journal of an in-app update (master plan Phase 5.5).
    let (update_journal, update_action) = match crate::updater::startup_action(&app) {
        Ok(found) => found,
        Err(e) => {
            warn!("startup: update journal could not be read, update outcome unknown: {e}");
            (None, crate::updater::journal::StartupAction::Nothing)
        }
    };
    if update_action == crate::updater::journal::StartupAction::RecoveryPending {
        // This version failed to update the database before: the database (the staged pre-update
        // backup, swapped in above) is left untouched until the previous version is reinstalled.
        if let Some(j) = &update_journal {
            warn!(from = %j.from_version, to = %j.to_version, "startup: update recovery pending");
            emit_event(
                &app,
                StartupEvent::UpdateRollbackRequired {
                    from_version: j.from_version.clone(),
                    to_version: j.to_version.clone(),
                    reason: j.failure.clone().unwrap_or_default(),
                    backup_refusal: j.backup_refusal.clone(),
                },
            );
        }
        window.show().ok();
        return Ok(());
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
            // Right after an in-app update: put the pre-update backup back and offer the previous
            // version (forward-only migrations; the older version cannot open a half-migrated file).
            let after_update = update_journal.clone().filter(|j| {
                update_action == crate::updater::journal::StartupAction::VerifyNewVersion
                    && j.kind == crate::updater::journal::UpdateKind::Upgrade
            });
            if let Some(journal) = after_update {
                match crate::updater::on_migration_failed(&app, &app_state.db, journal, &reason).await {
                    Ok(view) => emit_event(
                        &app,
                        StartupEvent::UpdateRollbackRequired {
                            from_version: view.from_version,
                            to_version: view.to_version,
                            reason,
                            backup_refusal: view.backup_refusal,
                        },
                    ),
                    Err(restore_error) => {
                        let reason = format!(
                            "{reason}. The backup taken before the update could not be prepared: {restore_error}"
                        );
                        error!("{reason}");
                        emit_event(&app, StartupEvent::Failed { reason });
                    }
                }
            } else {
                emit_event(&app, StartupEvent::Failed { reason });
            }
            window.show().ok();
            return Err(e);
        }
    }
    if let Err(e) = crate::updater::on_migrations_succeeded(&app, update_journal, &update_action) {
        warn!("startup: update journal could not be closed: {e}");
    }

    // Back on the previous version after a failed update: its database is the pre-update backup.
    let restored = restored || update_action == crate::updater::journal::StartupAction::ReturnedToPrevious;
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

    // Phase 3a: the work-order types with their maintenance natures and the shared DI / OT
    // urgency scale are SYSTEM reference lists the workflow rules depend on.
    info!("startup: enforcing work-order types / natures and urgency scale integrity");
    let catalogs = async {
        crate::wo::types::ensure_work_order_type_catalog(&app_state.db).await?;
        crate::reference::urgency_scale::ensure_urgency_scale(&app_state.db).await
    };
    if let Err(e) = catalogs.await {
        let reason = format!("Work order type / urgency scale integrity check failed: {e}");
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

    // Certification expiry alerts: notify the managers of people whose habilitations have run
    // out or fall inside their renewal notice (deduplicated per week). Never blocks startup.
    match crate::qualification::expiry_alerts::scan_due_certification_alerts(&app_state.db).await {
        Ok(created) => info!("startup: certification expiry scan raised {} alert(s)", created.len()),
        Err(e) => tracing::warn!(error = %e, "startup: certification expiry scan failed"),
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

    // Phase 4a': lists replaced by shared lists (DI.PRIORITY, DI.SYMPTOM, the stock unit lookup
    // inventory.unit_of_measure → MEASURE.UNITS). Migrations retire them
    // with capture suspended; if a replayed sync history brings one back, it is merged / deleted
    // here with capture on, so the change reaches the other devices. A link that cannot be moved
    // keeps the list in place (nothing is lost) and is logged.
    match crate::reference::retirement::retire_superseded_domains(&app_state.db).await {
        Ok(retired) if !retired.is_empty() => info!(?retired, "startup: retired superseded reference lists"),
        Ok(_) => {}
        Err(e) => warn!("startup: retiring superseded reference lists failed (non-fatal): {e}"),
    }

    // Phase 4a'': stock reservations, with capture on so what is written reaches the other devices:
    // reserved stock recomputed from the reservations and their movements
    // (`inventory::reservations`), then the D13 catch-up — reservations still open on
    // closed or cancelled work orders released (audited, system actor). Never blocks the startup;
    // what could not be done is logged and retried at the next start.
    let (upkeep, upkeep_errors) = crate::inventory::reservations::startup_reservation_upkeep(&app_state.db).await;
    info!(
        pairs_recomputed = upkeep.pairs_recomputed,
        recompute_ms = upkeep.recompute_ms,
        work_orders = upkeep.sweep.work_orders,
        reservations_released = upkeep.sweep.reservations,
        "startup: stock reservations upkeep"
    );
    for (wo_id, error) in &upkeep.sweep.failures {
        warn!(wo_id = *wo_id, error = %error, "startup: reservations of an ended work order not released (retried next start)");
    }
    for error in &upkeep_errors {
        warn!("startup: stock reservations upkeep (non-fatal): {error}");
    }

    // Phase 4a (permits): local consequences of received permit ends and lock removals not written
    // before the last stop (`permit::ended_hold::sweep`: running work held, downtime ended), with
    // capture on. Never blocks the startup; a failure is logged and retried by the next sync pass.
    let permit_sweep = crate::permit::ended_hold::sweep(&app_state.db).await;
    info!(
        held = permit_sweep.work_orders_held,
        downtimes_ended = permit_sweep.downtimes_ended,
        failures = permit_sweep.failures,
        "startup: permit-end catch-up"
    );
    // Phase 4a (permit rules): the plant's permit rules on the work orders not started yet that one
    // matches now and that missed it before the last stop (`permit::requirement_rules::catch_up`,
    // audit finding AU-04 R2-S2). Never blocks the startup; a failure is logged and retried by the
    // next sync pass.
    let rules_flagged = crate::permit::requirement_rules::catch_up(&app_state.db).await;
    info!(flagged = rules_flagged, "startup: permit rules catch-up");

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
        let planning_db = app_state.db.clone();
        let planning_sync = app_state.sync_worker.clone();
        tauri::async_runtime::spawn(async move {
            crate::planning::scheduler::start_planning_scheduler(planning_db, planning_sync).await;
        });
        info!("startup: planning scheduler started (finished commitments, weekly auto-freeze)");
    }

    {
        let inspection_db = app_state.db.clone();
        tauri::async_runtime::spawn(async move {
            crate::inspection::scheduler::start_inspection_round_scheduler(inspection_db).await;
        });
        info!("startup: inspection round scheduler started");
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
        Ok(worker) => worker.on_rights_changed(rights_changed_hook(app, state)),
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

/// After a sync run wrote a row the rights are read from (AU-107): every cached permission is
/// dropped (a role or an account may concern any user) and `rbac-changed` makes the window reload
/// its permissions, as an administrator's change made on this PC does (`commands::admin_users`).
fn rights_changed_hook(
    app: &AppHandle,
    state: &crate::state::AppState,
) -> crate::sync::transport::worker::RightsChangedHook {
    let app = app.clone();
    let cache = state.permission_cache.clone();
    std::sync::Arc::new(move || {
        let app = app.clone();
        let cache = cache.clone();
        Box::pin(async move {
            cache.write().await.invalidate_all();
            if let Err(e) = app.emit(
                "rbac-changed",
                crate::rbac::cache::RbacChangedPayload {
                    affected_user_id: None,
                    action: "sync_applied".to_string(),
                },
            ) {
                warn!("startup: cannot tell the window that rights changed by sync: {e}");
            }
        })
    })
}

fn emit_event(app: &AppHandle, event: StartupEvent) {
    if let Err(e) = app.emit("startup_event", event) {
        error!("failed to emit startup event: {e}");
    }
}
