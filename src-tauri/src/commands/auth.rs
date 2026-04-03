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
use crate::require_session;
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
                crate::audit::emit(&state.db, crate::audit::AuditEvent {
                    event_type: crate::audit::event_type::LOGIN_FAILURE,
                    summary:    "Failed login attempt — user not found",
                    detail_json: Some(format!(r#"{{"username_provided":true}}"#)),
                    ..Default::default()
                }).await;
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
            crate::audit::emit(&state.db, crate::audit::AuditEvent {
                event_type: crate::audit::event_type::LOGIN_FAILURE,
                actor_id:   Some(user_id),
                summary:    "Failed login attempt — no local password configured",
                ..Default::default()
            }).await;
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
        crate::audit::emit(&state.db, crate::audit::AuditEvent {
            event_type: crate::audit::event_type::LOGIN_FAILURE,
            actor_id:   Some(user_id),
            summary:    "Failed login attempt — wrong password",
            detail_json: Some(format!(r#"{{"username_provided":true}}"#)),
            ..Default::default()
        }).await;
        return Err(AppError::Auth(
            "Identifiant ou mot de passe invalide.".into(),
        ));
    }

    // ── Device trust enforcement ─────────────────────────────────────────
    let fingerprint = crate::auth::device::derive_device_fingerprint()
        .unwrap_or_else(|_| "unknown-fingerprint".to_string());

    let is_online = crate::auth::device::is_network_available();

    // Get existing trust record for this user+device
    let trust = crate::auth::device::get_device_trust(&state.db, user_id, &fingerprint).await?;

    match (&trust, is_online) {
        // First login: device not yet trusted. Requires connectivity.
        (None, false) => {
            warn!(username = %username, "login::first_login_requires_online");
            return Err(AppError::Auth(
                "La premi\u{00e8}re connexion sur cet appareil n\u{00e9}cessite une connexion r\u{00e9}seau.".into(),
            ));
        }
        // First login: register trust now (online)
        (None, true) => {
            crate::auth::device::register_device_trust(
                &state.db,
                user_id,
                &fingerprint,
                None,
            )
            .await?;
            crate::audit::emit(&state.db, crate::audit::AuditEvent {
                event_type:  crate::audit::event_type::DEVICE_TRUST_REGISTERED,
                actor_id:    Some(user_id),
                entity_type: Some("trusted_device"),
                entity_id:   Some(&fingerprint),
                summary:     "Device trust registered on first online login",
                ..Default::default()
            }).await;
            tracing::info!(username = %username, "login::device_trust_registered");
        }
        // Known device but revoked: allow online login only
        (Some(t), _) if t.is_revoked => {
            if !is_online {
                return Err(AppError::Auth(
                    "Cet appareil a \u{00e9}t\u{00e9} r\u{00e9}voqu\u{00e9}. Connexion en ligne requise.".into(),
                ));
            }
            // Online + revoked: allow login but do not re-register trust
        }
        // Known device, offline: check grace window
        (Some(_), false) => {
            let (allowed, _) = crate::auth::device::check_offline_access(
                &state.db,
                user_id,
                &fingerprint,
            )
            .await?;
            if !allowed {
                return Err(AppError::Auth(
                    "Fen\u{00ea}tre de connexion hors ligne expir\u{00e9}e. Connexion en ligne requise.".into(),
                ));
            }
            tracing::info!(username = %username, "login::offline_access_granted");
        }
        // Known device, online: update last_seen_at
        (Some(_), true) => {
            crate::auth::device::register_device_trust(
                &state.db,
                user_id,
                &fingerprint,
                None,
            )
            .await?;
        }
    }

    // ── Create session ────────────────────────────────────────────────────
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

    // ── Audit: successful login ───────────────────────────────────────
    crate::audit::emit(&state.db, crate::audit::AuditEvent {
        event_type:  crate::audit::event_type::LOGIN_SUCCESS,
        actor_id:    Some(user_id),
        actor_name:  Some(&username),
        summary:     "Successful login",
        detail_json: Some(format!(r#"{{"offline":{}}}"#, !is_online)),
        device_id:   Some(&fingerprint),
        ..Default::default()
    }).await;

    let info = state.session.read().await.session_info();
    Ok(LoginResponse { session_info: info })
}

/// Log the current user out and clear the active session.
#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> AppResult<()> {
    // Capture actor before clearing the session
    let actor_id = state.session.read().await.current.as_ref().map(|s| s.user.user_id);

    let mut session_guard = state.session.write().await;
    if let Some(session_id) = session_guard.clear_session() {
        tracing::info!(session_id = %session_id, "auth::logout");
    }
    drop(session_guard);

    crate::audit::emit(&state.db, crate::audit::AuditEvent {
        event_type: crate::audit::event_type::LOGOUT,
        actor_id,
        summary:    "User logged out",
        ..Default::default()
    }).await;

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

// ── Device trust IPC commands ─────────────────────────────────────────────────

use crate::auth::device;

/// Get the trust status of the current device for the currently logged-in user.
/// Requires an active session.
#[tauri::command]
pub async fn get_device_trust_status(
    state: State<'_, AppState>,
) -> AppResult<device::DeviceTrustStatus> {
    let user = require_session!(state);

    let fingerprint = device::derive_device_fingerprint()
        .unwrap_or_else(|_| "unknown".to_string());

    let trust = device::get_device_trust(&state.db, user.user_id, &fingerprint).await?;
    let (offline_allowed, offline_hours) = device::check_offline_access(
        &state.db,
        user.user_id,
        &fingerprint,
    )
    .await?;

    Ok(device::DeviceTrustStatus {
        device_fingerprint: fingerprint,
        is_trusted: trust.is_some() && !trust.as_ref().map(|t| t.is_revoked).unwrap_or(false),
        is_revoked: trust.as_ref().map(|t| t.is_revoked).unwrap_or(false),
        offline_allowed,
        offline_hours_remaining: offline_hours,
        device_label: trust.as_ref().and_then(|t| t.device_label.clone()),
        trusted_at: trust.as_ref().map(|t| t.trusted_at.clone()),
    })
}

/// Revoke trust for a specific trusted device by row id.
/// Use this to remove offline access for a lost or stolen device.
/// Requires admin permissions (enforced in SP04-F03).
#[tauri::command]
pub async fn revoke_device_trust(
    device_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    device::revoke_device_trust(&state.db, &device_id, user.user_id).await?;

    crate::audit::emit(&state.db, crate::audit::AuditEvent {
        event_type:  crate::audit::event_type::DEVICE_TRUST_REVOKED,
        actor_id:    Some(user.user_id),
        entity_type: Some("trusted_device"),
        entity_id:   Some(&device_id),
        summary:     "Device trust revoked",
        ..Default::default()
    }).await;

    Ok(())
}
