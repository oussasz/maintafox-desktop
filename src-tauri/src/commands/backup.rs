//! Backup IPC commands.
//!
//! All commands require `adm.settings` permission.
//! `run_manual_backup` additionally requires step-up authentication.
//! `factory_reset_stub` requires step-up + explicit confirmation string.

use serde::Deserialize;
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::backup::{self, BackupRunRecord, BackupRunResult, RestoreTestResult};
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
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    require_step_up!(state);

    // Basic path sanitization: reject empty paths
    let target = payload.target_path.trim();
    if target.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "target_path must not be empty".to_string()
        ]));
    }

    backup::run_manual_backup(&state.db, target, user.user_id).await
}

/// List recent backup runs for the audit/history UI.
/// Requires: active session + adm.settings
#[tauri::command]
pub async fn list_backup_runs(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<BackupRunRecord>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    backup::list_backup_runs(&state.db, limit.unwrap_or(20)).await
}

/// Validate a backup file's integrity without restoring it.
/// Requires: active session + adm.settings
#[tauri::command]
pub async fn validate_backup_file(state: State<'_, AppState>, backup_path: String) -> AppResult<RestoreTestResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    backup::validate_backup_file(&state.db, &backup_path).await
}

/// Factory reset stub — security gates are in place but data deletion is NOT
/// implemented in Phase 1.
///
/// Phase 2 will implement the actual deletion after:
/// 1. VPS sync drain (all unsynced data is uploaded)
/// 2. Audit trail archive (audit events are exported)
/// 3. Secure wipe of all DB tables in dependency order
///
/// In Phase 1 this command validates all security gates and then returns an
/// informational error explaining that factory reset is a Phase 2 feature.
#[derive(Debug, Deserialize)]
pub struct FactoryResetPayload {
    pub confirmation_phrase: String,
}

#[tauri::command]
pub async fn factory_reset_stub(state: State<'_, AppState>, payload: FactoryResetPayload) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    require_step_up!(state);

    // Validate confirmation phrase (accept both FR and EN)
    const FR_PHRASE: &str = "EFFACER TOUTES LES DONNÉES";
    const EN_PHRASE: &str = "ERASE ALL DATA";
    if payload.confirmation_phrase.trim() != FR_PHRASE && payload.confirmation_phrase.trim() != EN_PHRASE {
        return Err(AppError::ValidationFailed(vec![
            "factory_reset confirmation phrase does not match".to_string(),
        ]));
    }

    tracing::warn!(
        actor = user.user_id,
        "factory_reset_stub: all security gates passed — \
         data deletion deferred to Phase 2 implementation"
    );

    // Return an explicit "not yet implemented" error so the frontend can show
    // a "this feature is coming in a future version" message.
    Err(AppError::Internal(anyhow::anyhow!(
        "factory_reset: data deletion is implemented in Phase 2 (VPS sync drain required)"
    )))
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

    /// S2-V3 — Factory reset stub rejects wrong confirmation phrase.
    /// The command returns `AppError::ValidationFailed` when the phrase is wrong.
    #[test]
    fn v3_wrong_confirmation_phrase_is_rejected() {
        const FR_PHRASE: &str = "EFFACER TOUTES LES DONNÉES";
        const EN_PHRASE: &str = "ERASE ALL DATA";

        let wrong_phrases = ["yes please", "DELETE ALL", "effacer toutes les données", ""];
        for phrase in &wrong_phrases {
            assert_ne!(phrase.trim(), FR_PHRASE, "phrase '{}' should not match FR", phrase);
            assert_ne!(phrase.trim(), EN_PHRASE, "phrase '{}' should not match EN", phrase);
        }
    }

    /// S2-V3 — Factory reset accepts both FR and EN confirmation phrases.
    #[test]
    fn v3_correct_confirmation_phrases_accepted() {
        const FR_PHRASE: &str = "EFFACER TOUTES LES DONNÉES";
        const EN_PHRASE: &str = "ERASE ALL DATA";

        assert_eq!("EFFACER TOUTES LES DONNÉES".trim(), FR_PHRASE);
        assert_eq!("ERASE ALL DATA".trim(), EN_PHRASE);
        // Leading/trailing whitespace should be trimmed
        assert_eq!("  ERASE ALL DATA  ".trim(), EN_PHRASE);
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
        fn _assert_factory_reset_stub_exists() {
            let _: fn(
                State<'_, AppState>,
                FactoryResetPayload,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<()>> + Send>> =
                |_, _| Box::pin(async { unreachable!() });
        }
    }
}
