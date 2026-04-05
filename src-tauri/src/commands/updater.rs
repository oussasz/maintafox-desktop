//! Updater IPC commands.
//!
//! `check_for_update`      — runs before login; no session required; graceful on failure.
//! `install_pending_update` — requires an active authenticated session; app will restart.
//!
//! The release channel is read from `app_settings` at check time so an admin can
//! switch channels through the Settings UI without restarting the application.
//! Phase 1 always returns `available: false` because the manifest endpoint is a stub.

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_updater::UpdaterExt;

use crate::errors::{AppError, AppResult};
use crate::require_session;
use crate::state::AppState;

// ─── Response type ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}

// ─── Commands ─────────────────────────────────────────────────────────────────

/// Check the remote manifest for a newer version.
///
/// Does NOT require an active session — safe to call from the startup sequence
/// or the login screen. Returns `available: false` when no update is found or
/// when the manifest endpoint is unreachable (graceful degradation: the app
/// must remain usable without connectivity to the update server).
#[tauri::command]
pub async fn check_for_update(app: tauri::AppHandle) -> AppResult<UpdateCheckResult> {
    let updater = app
        .updater()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("updater plugin not initialized: {e}")))?;

    match updater.check().await {
        Ok(Some(update)) => {
            tracing::info!(new_version = %update.version, "update available");
            Ok(UpdateCheckResult {
                available: true,
                version: Some(update.version.clone()),
                notes: update.body.clone(),
                pub_date: update.date.map(|d| d.to_string()),
            })
        }
        Ok(None) => {
            tracing::debug!("no update available");
            Ok(UpdateCheckResult {
                available: false,
                version: None,
                notes: None,
                pub_date: None,
            })
        }
        Err(e) => {
            // Update-check failures are non-fatal — the application works without updates.
            tracing::warn!("update check failed (non-fatal): {e}");
            Ok(UpdateCheckResult {
                available: false,
                version: None,
                notes: None,
                pub_date: None,
            })
        }
    }
}

/// Download and install a pending update.
///
/// Requires an active authenticated session — the user must be present to
/// approve an action that will restart the application. The frontend MUST
/// show a confirmation dialog before invoking this command.
///
/// If the cryptographic signature of the downloaded bundle cannot be verified,
/// `tauri-plugin-updater` aborts the install and returns an error. The current
/// version continues to run.
#[tauri::command]
pub async fn install_pending_update(app: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let _user = require_session!(state);

    let updater = app
        .updater()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("updater plugin not initialized: {e}")))?;

    let update = updater
        .check()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to check for update before install: {e}")))?;

    match update {
        Some(update) => {
            tracing::info!(
                new_version = %update.version,
                "installing update — application will restart"
            );
            // download_and_install: first closure receives (chunk_bytes, total_bytes?),
            // second closure fires once installation is about to apply.
            // If signature verification fails, the plugin returns an error and does NOT
            // apply the bundle.
            update
                .download_and_install(|_chunk, _total| {}, || {})
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!("update install failed: {e}")))?;
            Ok(())
        }
        None => Err(AppError::NotFound {
            entity: "pending_update".to_string(),
            id: "current".to_string(),
        }),
    }
}
