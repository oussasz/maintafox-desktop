//! Authentication IPC commands.
//!
//! Security rules:
//!   - `login()` never returns a useful error on bad credentials — always generic.
//!   - `login()` never reveals whether a username exists or not.
//!   - All auth errors are logged at WARN level with the username (not password).
//!   - The session token is not returned in any IPC response.

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::warn;

use crate::auth::{password, session_manager};
use crate::errors::{AppError, AppResult};
use crate::state::AppState;

/// Input for the login command. Received from the React login form.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Response returned on successful login.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub session_info: session_manager::SessionInfo,
}

/// Attempt to authenticate with a local username and password.
///
/// Returns an opaque error on any failure — does not reveal whether the
/// username exists, whether the password was wrong, or whether the account
/// is locked. Details are logged at WARN level on the Rust side only.
#[tauri::command]
pub async fn login(
    payload: LoginRequest,
    state: State<'_, AppState>,
) -> AppResult<LoginResponse> {
    let username = payload.username.trim().to_string();

    if username.is_empty() || payload.password.is_empty() {
        return Err(AppError::Auth(
            "Identifiant ou mot de passe invalide.".into(),
        ));
    }

    // Look up user
    let user_record = session_manager::find_active_user(&state.db, &username).await?;
    let (user_id, db_username, display_name, is_admin, force_pw_change, pw_hash) =
        match user_record {
            None => {
                // User not found — run a dummy hash to consume constant time
                let _ = password::hash_password("timing_sink_unused");
                warn!(username = %username, "login::user_not_found");
                return Err(AppError::Auth(
                    "Identifiant ou mot de passe invalide.".into(),
                ));
            }
            Some(r) => r,
        };

    // Verify password
    let stored_hash = match pw_hash {
        None => {
            warn!(username = %username, "login::no_password_hash_sso_only");
            return Err(AppError::Auth(
                "Identifiant ou mot de passe invalide.".into(),
            ));
        }
        Some(h) => h,
    };

    let password_ok = password::verify_password(&payload.password, &stored_hash)?;
    if !password_ok {
        session_manager::record_failed_login(&state.db, user_id).await?;
        warn!(username = %username, "login::wrong_password");
        return Err(AppError::Auth(
            "Identifiant ou mot de passe invalide.".into(),
        ));
    }

    // Password correct — create session
    let auth_user = session_manager::AuthenticatedUser {
        user_id,
        username: db_username,
        display_name,
        is_admin,
        force_password_change: force_pw_change,
    };

    let mut session_guard = state.session.write().await;
    let session = session_guard.create_session(auth_user);

    // Capture data before dropping the write lock
    let session_id = session.session_db_id.clone();
    let expires_rfc3339 = session.expires_at.to_rfc3339();
    drop(session_guard);

    // Record in DB for audit purposes
    session_manager::record_successful_login(&state.db, user_id).await?;
    session_manager::create_session_record(&state.db, &session_id, user_id, &expires_rfc3339)
        .await?;

    let info = state.session.read().await.session_info();
    Ok(LoginResponse { session_info: info })
}

/// Log the current user out and clear the active session.
#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> AppResult<()> {
    let mut session_guard = state.session.write().await;
    if let Some(session_id) = session_guard.clear_session() {
        tracing::info!(session_id = %session_id, "auth::logout");
    }
    Ok(())
}

/// Returns the current session state without requiring authentication.
/// Called by the React shell to decide which screen to show on startup.
#[tauri::command]
pub async fn get_session_info(
    state: State<'_, AppState>,
) -> AppResult<session_manager::SessionInfo> {
    Ok(state.session.read().await.session_info())
}
