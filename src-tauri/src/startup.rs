use std::time::Instant;
use std::path::PathBuf;

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
    MigrationsComplete { applied: u32 },
    EntitlementCacheLoaded,
    Ready,
    Failed { reason: String },
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
        format!(
            "WARNING: Startup took {elapsed_ms}ms which exceeds the {budget_ms}ms cold-start budget"
        )
    }
}

/// Forces a WAL checkpoint on the local SQLite database.
/// Must be called before any destructive migration is applied.
/// Returns Ok(()) on success or an error if the checkpoint fails.
pub async fn force_wal_checkpoint(db: &sea_orm::DatabaseConnection) -> crate::errors::AppResult<()> {
    use sea_orm::{ConnectionTrait, Statement, DbBackend};
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA wal_checkpoint(FULL);".to_string(),
    ))
    .await
    .map(|_| ())
    .map_err(|e| crate::errors::AppError::Database(
        sea_orm::DbErr::Custom(format!("WAL checkpoint failed: {}", e))
    ))
}

/// Creates a pre-migration backup of the database file.
/// Called when detecting that a pending migration is classified as destructive.
pub fn backup_database(
    db_path: &PathBuf,
    backup_dir: &PathBuf,
) -> crate::errors::AppResult<PathBuf> {
    use std::time::SystemTime;
    use chrono::{DateTime, Utc};

    let timestamp = DateTime::<Utc>::from(SystemTime::now())
        .format("%Y%m%d_%H%M%S")
        .to_string();

    let backup_filename = format!("pre_migration_{}.db", timestamp);
    let backup_path = backup_dir.join(&backup_filename);

    std::fs::create_dir_all(backup_dir)
        .map_err(crate::errors::AppError::Io)?;

    std::fs::copy(db_path, &backup_path)
        .map_err(|e| crate::errors::AppError::Io(
            std::io::Error::new(std::io::ErrorKind::Other, format!("Pre-migration backup failed: {}", e))
        ))?;

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

    let window = app
        .get_webview_window("main")
        .expect("main window must exist");

    // Phase 1: database integrity and connection
    info!("startup: initialising database");
    let db_path = resolve_db_path(&app)?;
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
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }

    // Phase 2: schema migrations
    info!("startup: running migrations");
    let app_state = app.state::<crate::state::AppState>();
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

    // Phase 3: seed system data (idempotent — safe on every startup)
    info!("startup: seeding system data");
    match crate::db::seeder::seed_system_data(&app_state.db).await {
        Ok(()) => {
            info!(
                elapsed_ms = startup_start.elapsed().as_millis() as u64,
                "startup::seed_complete"
            );
        }
        Err(e) => {
            let reason = format!("System seed data failed: {e}");
            error!("{reason}");
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }

    // Phase 4: integrity check
    info!("startup: running integrity check");
    match crate::db::integrity::run_integrity_check(&app_state.db).await {
        Ok(report) => {
            if report.is_healthy {
                info!(
                    elapsed_ms = startup_start.elapsed().as_millis() as u64,
                    domains = report.domain_count,
                    values = report.value_count,
                    "startup::integrity_check_passed"
                );
            } else if report.is_recoverable {
                warn!(
                    issues = report.issues.len(),
                    "startup::integrity_warning \u{2014} recoverable issues found, proceeding"
                );
            } else {
                let reason = report
                    .issues
                    .first()
                    .map_or_else(
                        || "Unknown integrity failure".to_string(),
                        |i| i.description.clone(),
                    );
                error!(
                    issues = report.issues.len(),
                    "startup::integrity_fatal \u{2014} unrecoverable integrity issues"
                );
                emit_event(
                    &app,
                    StartupEvent::Failed {
                        reason: format!("Erreur d'int\u{00e9}grit\u{00e9} : {reason}"),
                    },
                );
                window.show().ok();
                return Err(crate::errors::AppError::Internal(anyhow::anyhow!(
                    "Startup integrity check failed with unrecoverable issues"
                )));
            }
        }
        Err(e) => {
            let reason = format!("Integrity check failed: {e}");
            error!("{reason}");
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }

    // Phase 5: entitlement cache (stub for roadmap Phase 4 \u{2014} always succeeds here)
    info!("startup: loading entitlement cache");
    info!(
        elapsed_ms = startup_start.elapsed().as_millis() as u64,
        "startup::entitlement_cache_loaded"
    );
    emit_event(&app, StartupEvent::EntitlementCacheLoaded);

    // ── Budget check and ready ──────────────────────────────────────────
    let (total_ms, within_budget) =
        validate_startup_duration(startup_start, COLD_START_BUDGET_MS);

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

/// Resolve the database file path inside the Tauri app data directory.
/// Creates the directory if it does not already exist.
fn resolve_db_path(app: &AppHandle) -> AppResult<String> {
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

fn emit_event(app: &AppHandle, event: StartupEvent) {
    if let Err(e) = app.emit("startup_event", event) {
        error!("failed to emit startup event: {e}");
    }
}
