use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info};

use crate::errors::AppResult;

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

/// Run the ordered startup sequence.
///
/// Called once from the `setup` hook in lib.rs after the window is created.
/// On success, emits `StartupEvent::Ready` and calls `window.show()`.
/// On failure, emits `StartupEvent::Failed` and shows the window with a
/// minimal error surface so the user is not left with an invisible process.
pub async fn run_startup_sequence(app: AppHandle) -> AppResult<()> {
    let window = app
        .get_webview_window("main")
        .expect("main window must exist");

    // Phase 1: database integrity and connection
    info!("startup: initialising database");
    let db_path = resolve_db_path(&app)?;
    match crate::db::init_db(&db_path).await {
        Ok(conn) => {
            // Build AppState and inject into managed state
            let state = crate::state::AppState::new(conn);
            app.manage(state);
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
            // run_migrations returns () — emit 0 applied (count not tracked yet)
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

    // Phase 3: entitlement cache (stub for Phase 4 — always succeeds here)
    info!("startup: loading entitlement cache");
    emit_event(&app, StartupEvent::EntitlementCacheLoaded);

    // All phases passed — show the window
    info!("startup: sequence complete, showing window");
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
