//! Database encryption key commands (Phase 1.2).
//!
//! - `get_database_recovery_key`: admin reveals the recovery key to store it offline.
//! - `recover_database_key`: startup recovery when the OS credential store lost the key.

use tauri::{AppHandle, State};

use crate::db::encryption;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session};

/// Returns the human-readable recovery key (`XXXX-XXXX-…`).
///
/// Requires `adm.settings` and a fresh step-up: whoever holds this key can decrypt a copied
/// database file.
#[tauri::command]
pub async fn get_database_recovery_key(state: State<'_, AppState>) -> AppResult<String> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        crate::auth::rbac::PermissionScope::Global
    );
    crate::require_step_up!(state);
    let key = encryption::load_stored_key()?.ok_or_else(|| {
        AppError::DatabaseKeyUnavailable("no database key is stored in the OS credential store".into())
    })?;
    tracing::warn!(
        event = "database_recovery_key_revealed",
        user_id = user.user_id,
        "Database recovery key revealed to an administrator"
    );
    Ok(key.recovery_string())
}

/// Startup recovery: stores `recovery_key` in the OS credential store after proving it opens the
/// existing encrypted database, then restarts the app.
///
/// Callable before any session exists (the database cannot be opened yet). It changes nothing
/// unless the caller already holds the exact 256-bit key of this database.
#[tauri::command]
pub async fn recover_database_key(app: AppHandle, recovery_key: String) -> AppResult<()> {
    let db_path = crate::startup::resolve_db_path(&app)?;
    encryption::recover_with_recovery_key(std::path::Path::new(&db_path), &recovery_key).await?;
    tracing::info!(
        event = "database_key_recovered",
        "Database key restored from recovery key; restarting"
    );
    app.restart()
}
