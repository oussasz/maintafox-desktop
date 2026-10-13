//! Updater IPC commands (master plan Phase 5.5; the logic is in `crate::updater`).
//!
//! `check_for_update`           — no session: safe at startup and on the login screen; never fails
//!                                the app (the result says why there is no update).
//! `install_pending_update`     — signed-in user: pre-update backup, download with progress,
//!                                signature check, install (the app restarts).
//! `reinstall_previous_version` — recovery screen after a failed database update (no session:
//!                                the database could not be opened by this version).
//! `get_update_journal` / `acknowledge_update_notice` — the outcome of the last update.

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::errors::{AppError, AppResult};
use crate::require_session;
use crate::state::AppState;
use crate::updater::{InstallOutcome, UpdateCheckResult, UpdateJournalView, UpdateProgressEvent};

fn database(app: &AppHandle) -> AppResult<sea_orm::DatabaseConnection> {
    app.try_state::<AppState>()
        .map(|s| s.db.clone())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("the database is not open yet")))
}

/// Asks the control API's manifest whether this device should install another version.
#[tauri::command]
pub async fn check_for_update(app: AppHandle, locale: Option<String>) -> AppResult<UpdateCheckResult> {
    let db = database(&app)?;
    Ok(crate::updater::check(&app, &db, locale.as_deref()).await)
}

/// Installs the update the manifest offers now. The frontend asks for confirmation first.
#[tauri::command]
pub async fn install_pending_update(
    app: AppHandle,
    state: State<'_, AppState>,
    locale: Option<String>,
    on_progress: Channel<UpdateProgressEvent>,
) -> AppResult<InstallOutcome> {
    let user = require_session!(state);
    let progress = move |event: UpdateProgressEvent| {
        if let Err(e) = on_progress.send(event) {
            tracing::warn!("update progress could not reach the window: {e}");
        }
    };
    crate::updater::install(&app, &state.db, locale.as_deref(), Some(user.user_id), &progress).await
}

/// Reinstalls the version this device ran before a failed database update.
#[tauri::command]
pub async fn reinstall_previous_version(
    app: AppHandle,
    locale: Option<String>,
    on_progress: Channel<UpdateProgressEvent>,
) -> AppResult<InstallOutcome> {
    let db = database(&app)?;
    let progress = move |event: UpdateProgressEvent| {
        if let Err(e) = on_progress.send(event) {
            tracing::warn!("update progress could not reach the window: {e}");
        }
    };
    crate::updater::reinstall_previous(&app, &db, locale.as_deref(), &progress).await
}

/// The last update's journal (none when this device never updated in the app).
#[tauri::command]
pub fn get_update_journal(app: AppHandle) -> AppResult<Option<UpdateJournalView>> {
    crate::updater::journal_view(&app)
}

/// The user has read the outcome notice of the last update.
#[tauri::command]
pub fn acknowledge_update_notice(app: AppHandle) -> AppResult<()> {
    crate::updater::acknowledge_notice(&app)
}
