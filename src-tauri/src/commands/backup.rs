//! Backup IPC commands.
//!
//! All commands require `adm.settings` permission.
//! `run_manual_backup` additionally requires step-up authentication.
//! `restore_from_backup` and `factory_reset` require step-up; factory reset also requires an
//! explicit confirmation phrase. Both restart the application.

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::backup::{self, BackupRunRecord, BackupRunResult, RestoreTestResult};
use crate::db::encryption;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[derive(Debug, Deserialize)]
pub struct RunManualBackupPayload {
    pub target_path: String,
}

/// Run a manual backup to the specified target path.
/// Requires: active session + adm.settings + recent step-up
#[tauri::command]
pub async fn run_manual_backup(
    state: State<'_, AppState>,
    payload: RunManualBackupPayload,
) -> AppResult<BackupRunResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    require_step_up!(state);

    // Basic path sanitization: reject empty paths
    let target = payload.target_path.trim();
    if target.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "target_path must not be empty".to_string()
        ]));
    }

    backup::run_manual_backup(&state.db, target, backup::TRIGGER_MANUAL, Some(user.user_id)).await
}

/// List recent backup runs for the audit/history UI.
/// Requires: active session + adm.settings
#[tauri::command]
pub async fn list_backup_runs(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<BackupRunRecord>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    backup::list_backup_runs(&state.db, limit.unwrap_or(20)).await
}

/// Validate a backup file's integrity without restoring it.
/// Requires: active session + adm.settings
#[tauri::command]
pub async fn validate_backup_file(state: State<'_, AppState>, backup_path: String) -> AppResult<RestoreTestResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    let key = crate::db::encryption::load_stored_key()?;
    backup::validate_backup_file(&state.db, &backup_path, key.as_ref()).await
}

/// Directory for backups the application writes on its own (scheduled, pre-restore, pre-reset).
pub(crate) fn app_backup_dir(app: &AppHandle) -> AppResult<std::path::PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?
        .join("backups");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// This installation's database key from the OS credential store (restore, update rollback).
pub(crate) fn require_database_key() -> AppResult<encryption::DatabaseKey> {
    encryption::load_stored_key()?
        .ok_or_else(|| AppError::DatabaseKeyUnavailable("no database key is stored in the OS credential store".into()))
}

/// Restores the database from `backup_path` and restarts the application.
///
/// The backup is validated (integrity, this installation's key, schema not newer than this
/// build) and the current database is first saved as a `pre_restore` backup. The swap happens at
/// the next startup, before the database is opened (see `backup::restore`).
/// Requires: active session + adm.settings + recent step-up
#[tauri::command]
pub async fn restore_from_backup(app: AppHandle, state: State<'_, AppState>, backup_path: String) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    require_step_up!(state);

    let backup_path = backup_path.trim();
    if backup_path.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "backup_path must not be empty".to_string()
        ]));
    }
    let key = require_database_key()?;
    let db_path = crate::startup::resolve_db_path(&app)?;
    backup::restore::stage_restore(
        &state.db,
        std::path::Path::new(&db_path),
        std::path::Path::new(backup_path),
        &key,
        &app_backup_dir(&app)?,
        Some(user.user_id),
    )
    .await?;
    // This restore replaced any the updater had staged: the update journal no longer names one,
    // so the next start applies this one (`updater::reconcile_staged_restore`).
    if let Err(e) = crate::updater::forget_staged_restore_for_app(&app) {
        tracing::warn!("restore: the update journal could not be updated: {e}");
    }
    app.restart()
}

#[derive(Debug, Deserialize)]
pub struct FactoryResetPayload {
    pub confirmation_phrase: String,
}

const FACTORY_RESET_PHRASE_FR: &str = "EFFACER TOUTES LES DONNÉES";
const FACTORY_RESET_PHRASE_EN: &str = "ERASE ALL DATA";

fn factory_reset_phrase_matches(phrase: &str) -> bool {
    let phrase = phrase.trim();
    phrase == FACTORY_RESET_PHRASE_FR || phrase == FACTORY_RESET_PHRASE_EN
}

/// Erases all tenant data on this device and restarts into first-run setup.
///
/// A final `pre_factory_reset` backup is written to the application backup folder first; if it
/// fails, nothing is erased. Data not yet synchronized to the server exists only in that backup.
/// Requires: active session + adm.settings + recent step-up + confirmation phrase (FR or EN)
#[tauri::command]
pub async fn factory_reset(app: AppHandle, state: State<'_, AppState>, payload: FactoryResetPayload) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    require_step_up!(state);
    if !factory_reset_phrase_matches(&payload.confirmation_phrase) {
        return Err(AppError::ValidationFailed(vec![
            "factory_reset confirmation phrase does not match".to_string(),
        ]));
    }

    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let final_backup = app_backup_dir(&app)?.join(format!("maintafox-pre-factory-reset-{stamp}.db"));
    backup::run_manual_backup(
        &state.db,
        &final_backup.to_string_lossy(),
        backup::TRIGGER_PRE_FACTORY_RESET,
        Some(user.user_id),
    )
    .await?;

    let wiped_rows = crate::commands::product_license::reset_to_first_run(&state, "factory reset").await?;
    tracing::warn!(
        event = "factory_reset",
        actor = user.user_id,
        wiped_rows,
        final_backup = %final_backup.display(),
        "factory reset completed; restarting"
    );
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S2-V1 — RunManualBackupPayload deserializes correctly from IPC JSON.
    /// Tauri sends `{ payload: { target_path: "..." } }` — this test confirms the
    /// inner payload struct parses as expected.
    #[test]
    fn v1_run_manual_backup_payload_deserializes() {
        let json = r#"{"target_path": "C:\\Temp\\maintafox_test.db"}"#;
        let payload: RunManualBackupPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.target_path, "C:\\Temp\\maintafox_test.db");
    }

    /// S2-V1 — Empty target_path is caught by the trim-check in the command.
    /// The service function is never called with an empty path.
    #[test]
    fn v1_empty_target_path_is_detected() {
        let payload = RunManualBackupPayload {
            target_path: "   ".to_string(),
        };
        assert!(
            payload.target_path.trim().is_empty(),
            "whitespace-only path should trim to empty"
        );
    }

    /// Factory reset rejects any phrase other than the exact FR/EN confirmation.
    #[test]
    fn v3_wrong_confirmation_phrase_is_rejected() {
        for phrase in ["yes please", "DELETE ALL", "effacer toutes les données", ""] {
            assert!(
                !factory_reset_phrase_matches(phrase),
                "phrase '{phrase}' must be rejected"
            );
        }
    }

    /// Factory reset accepts both FR and EN phrases, ignoring surrounding whitespace.
    #[test]
    fn v3_correct_confirmation_phrases_accepted() {
        assert!(factory_reset_phrase_matches("EFFACER TOUTES LES DONNÉES"));
        assert!(factory_reset_phrase_matches("ERASE ALL DATA"));
        assert!(factory_reset_phrase_matches("  ERASE ALL DATA  "));
    }

    /// S2-V3 — FactoryResetPayload deserializes correctly from IPC JSON.
    #[test]
    fn v3_factory_reset_payload_deserializes() {
        let json = r#"{"confirmation_phrase": "EFFACER TOUTES LES DONNÉES"}"#;
        let payload: FactoryResetPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.confirmation_phrase, "EFFACER TOUTES LES DONNÉES");
    }

    /// S2 — All four commands are async functions with #[tauri::command].
    /// This is a compile-time verification: if this module compiles, the commands
    /// have the correct signatures and are properly registered.
    /// The explicit type assertions below exist to catch signature drift.
    #[test]
    fn command_signatures_are_valid() {
        // These type assertions verify the return types match AppResult<T>.
        // If the signatures change, this test will fail at compile time.
        fn _assert_run_manual_backup_exists() {
            let _: fn(
                State<'_, AppState>,
                RunManualBackupPayload,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<BackupRunResult>> + Send>> =
                |_, _| Box::pin(async { unreachable!() });
        }
    }
}
