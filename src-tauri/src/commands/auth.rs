//! Authentication IPC commands.
//!
//! Security rules:
//!   - `login()` never returns a useful error on bad credentials — always generic.
//!   - `login()` never reveals whether a username exists or not.
//!   - All auth errors are logged at WARN level with the username (not password).
//!   - The session token is not returned in any IPC response.

use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::warn;

use crate::auth::{lockout, password, password_policy, pin, session_manager};
use crate::errors::{AppError, AppResult};
use crate::require_session;
use crate::state::AppState;

fn tenant_mismatch_recovery_message(activated_tenant: &str) -> String {
    format!(
        "Ce compte n'est pas autorisé pour le tenant activé ({activated_tenant}). \
        Connectez-vous avec un compte autorisé pour ce tenant ou réactivez l'appareil avec la clé correcte."
    )
}

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
pub async fn login(payload: LoginRequest, state: State<'_, AppState>) -> AppResult<LoginResponse> {
    let username = payload.username.trim().to_string();

    if username.is_empty() || payload.password.is_empty() {
        return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
    }

    // Look up user
    let user_record = session_manager::find_active_user(&state.db, &username).await?;
    let (user_id, db_username, display_name, is_admin, force_pw_change, pw_hash) = match user_record {
        None => {
            // User not found — run a dummy hash to consume constant time
            let _ = password::hash_password("timing_sink_unused");
            warn!(username = %username, "login::user_not_found");
            crate::audit::emit(
                &state.db,
                crate::audit::AuditEvent {
                    event_type: crate::audit::event_type::LOGIN_FAILURE,
                    summary: "Failed login attempt — user not found",
                    detail_json: Some(r#"{"username_provided":true}"#.to_string()),
                    ..Default::default()
                },
            )
            .await;
            return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
        }
        Some(r) => r,
    };

    // ── Account lockout check (OWASP brute-force protection) ─────────────
    lockout::check_lockout(&state.db, user_id).await?;

    // Load lockout policy for recording failed attempts later
    let lockout_policy = lockout::LockoutPolicy::load(&state.db).await;

    // Verify password
    let stored_hash = match pw_hash {
        None => {
            warn!(username = %username, "login::no_password_hash_sso_only");
            crate::audit::emit(
                &state.db,
                crate::audit::AuditEvent {
                    event_type: crate::audit::event_type::LOGIN_FAILURE,
                    actor_id: Some(user_id),
                    summary: "Failed login attempt — no local password configured",
                    ..Default::default()
                },
            )
            .await;
            return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
        }
        Some(h) => h,
    };

    let password_ok = password::verify_password(&payload.password, &stored_hash)?;
    if !password_ok {
        lockout::record_failed_attempt(&state.db, user_id, &lockout_policy).await?;
        warn!(username = %username, "login::wrong_password");
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::LOGIN_FAILURE,
                actor_id: Some(user_id),
                summary: "Failed login attempt — wrong password",
                detail_json: Some(r#"{"username_provided":true}"#.to_string()),
                ..Default::default()
            },
        )
        .await;
        return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
    }

    // ── Device trust enforcement ─────────────────────────────────────────
    let fingerprint =
        crate::auth::device::derive_device_fingerprint().unwrap_or_else(|_| "unknown-fingerprint".to_string());

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
            crate::auth::device::register_device_trust(&state.db, user_id, &fingerprint, None).await?;
            crate::audit::emit(
                &state.db,
                crate::audit::AuditEvent {
                    event_type: crate::audit::event_type::DEVICE_TRUST_REGISTERED,
                    actor_id: Some(user_id),
                    entity_type: Some("trusted_device"),
                    entity_id: Some(&fingerprint),
                    summary: "Device trust registered on first online login",
                    ..Default::default()
                },
            )
            .await;
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
        // Known device, offline: apply strict activation/offline eligibility checks.
        (Some(_), false) => {
            let decision =
                crate::activation::queries::evaluate_offline_activation_policy(&state.db, user_id, &fingerprint)
                    .await?;
            if !decision.allowed {
                return Err(AppError::Auth(
                    decision.denial_message.unwrap_or_else(|| {
                        "Connexion hors ligne refus\u{00e9}e par la politique d'activation.".to_string()
                    }),
                ));
            }
            tracing::info!(username = %username, "login::offline_access_granted");
        }
        // Known device, online: apply reconnect revocation and refresh trust heartbeat.
        (Some(_), true) => {
            if let Some(reason) = crate::activation::queries::process_reconnect_revocation(&state.db).await? {
                return Err(AppError::Auth(format!(
                    "Connexion refus\u{00e9}e: r\u{00e9}vocation d'activation appliqu\u{00e9}e \u{00e0} la reconnexion ({reason})."
                )));
            }
            crate::auth::device::register_device_trust(&state.db, user_id, &fingerprint, None).await?;
        }
    }

    let activated_tenant_id = crate::commands::product_license::get_activation_claim_tenant_id(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::SessionClaimInvalid(
                "Activation tenant claim is missing on this device. Re-enter the activation key to continue.".into(),
            )
        })?;

    let tenant_scope = session_manager::resolve_tenant_scope_for_user(&state.db, user_id, &activated_tenant_id)
        .await?
        .ok_or_else(|| AppError::TenantScopeViolation(tenant_mismatch_recovery_message(&activated_tenant_id)))?;

    // ── Password expiry enforcement ────────────────────────────────────
    let policy = password_policy::PasswordPolicy::load(&state.db).await;
    let mut effective_force_pw_change = force_pw_change;
    let mut password_expires_in_days: Option<i64> = None;
    let pin_configured = session_manager::get_pin_hash_for_user(&state.db, user_id)
        .await?
        .is_some();

    match password_policy::check_password_expiry(&state.db, user_id, &policy).await? {
        password_policy::PasswordExpiryStatus::Valid => {}
        password_policy::PasswordExpiryStatus::ExpiringSoon { days_remaining } => {
            password_expires_in_days = Some(days_remaining.max(0));
        }
        password_policy::PasswordExpiryStatus::Expired => {
            effective_force_pw_change = true;
            let now = chrono::Utc::now().to_rfc3339();
            state
                .db
                .execute(sea_orm::Statement::from_sql_and_values(
                    sea_orm::DbBackend::Sqlite,
                    r"UPDATE user_accounts
                       SET force_password_change = 1,
                           updated_at = ?
                       WHERE id = ?",
                    [now.into(), user_id.into()],
                ))
                .await?;

            crate::audit::emit(
                &state.db,
                crate::audit::AuditEvent {
                    event_type: crate::audit::event_type::FORCE_CHANGE_SET,
                    actor_id: Some(user_id),
                    summary: "Force password change set due to password expiry",
                    detail_json: Some(r#"{"reason":"expired"}"#.to_string()),
                    ..Default::default()
                },
            )
            .await;
        }
        password_policy::PasswordExpiryStatus::NeverSet => {
            effective_force_pw_change = true;
            let now = chrono::Utc::now().to_rfc3339();
            state
                .db
                .execute(sea_orm::Statement::from_sql_and_values(
                    sea_orm::DbBackend::Sqlite,
                    r"UPDATE user_accounts
                       SET force_password_change = 1,
                           updated_at = ?
                       WHERE id = ?",
                    [now.into(), user_id.into()],
                ))
                .await?;

            crate::audit::emit(
                &state.db,
                crate::audit::AuditEvent {
                    event_type: crate::audit::event_type::FORCE_CHANGE_SET,
                    actor_id: Some(user_id),
                    summary: "Force password change set due to missing password_changed_at",
                    detail_json: Some(r#"{"reason":"never_set"}"#.to_string()),
                    ..Default::default()
                },
            )
            .await;
        }
    }

    // ── Create session ────────────────────────────────────────────────────
    // Password correct — create session
    let auth_user = session_manager::AuthenticatedUser {
        user_id,
        username: db_username,
        display_name,
        is_admin,
        force_password_change: effective_force_pw_change,
        tenant_id: tenant_scope.tenant_id,
        token_tenant_id: tenant_scope.token_tenant_id,
    };

    let mut session_guard = state.session.write().await;
    session_guard.create_session(auth_user);

    if let Some(current) = &mut session_guard.current {
        current.password_expires_in_days = password_expires_in_days;
        current.pin_configured = pin_configured;
    }

    // Capture data before further mutation
    let (session_id, expires_rfc3339) = match session_guard.current.as_ref() {
        Some(s) => (s.session_db_id.clone(), s.expires_at.to_rfc3339()),
        None => {
            return Err(AppError::Auth(
                "Impossible de créer la session utilisateur.".into(),
            ));
        }
    };

    // A successful password login is equivalent to a step-up verification.
    session_guard.record_step_up();
    drop(session_guard);

    // Record in DB for audit purposes
    session_manager::record_successful_login(&state.db, user_id).await?;
    session_manager::create_session_record(&state.db, &session_id, user_id, &expires_rfc3339).await?;

    // ── Audit: successful login ───────────────────────────────────────
    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::LOGIN_SUCCESS,
            actor_id: Some(user_id),
            actor_name: Some(&username),
            summary: "Successful login",
            detail_json: Some(format!(r#"{{"offline":{}}}"#, !is_online)),
            device_id: Some(&fingerprint),
            ..Default::default()
        },
    )
    .await;

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

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::LOGOUT,
            actor_id,
            summary: "User logged out",
            ..Default::default()
        },
    )
    .await;

    Ok(())
}

/// Returns the current session state without requiring authentication.
/// Called by the React shell to decide which screen to show on startup.
#[tauri::command]
pub async fn get_session_info(state: State<'_, AppState>) -> AppResult<session_manager::SessionInfo> {
    let maybe_session = state.session.read().await.current.clone();
    let Some(current_session) = maybe_session else {
        return Ok(state.session.read().await.session_info());
    };
    if current_session.is_expired() {
        return Ok(state.session.read().await.session_info());
    }

    let activated_tenant_id = crate::commands::product_license::get_activation_claim_tenant_id(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::SessionClaimInvalid(
                "Activation tenant claim is missing on this device. Re-enter the activation key to continue.".into(),
            )
        })?;
    if current_session.user.tenant_id != activated_tenant_id {
        {
            let mut session_guard = state.session.write().await;
            session_guard.clear_session();
        }
        return Err(AppError::SessionClaimInvalid(
            "Session tenant claim is stale. Please sign in again for the currently activated tenant.".into(),
        ));
    }

    Ok(state.session.read().await.session_info())
}

/// Heartbeat: update in-memory + DB `last_activity_at` for the current session.
/// The frontend calls this periodically so the presence query sees fresh activity.
#[tauri::command]
pub async fn touch_session(state: State<'_, AppState>) -> AppResult<()> {
    let session_db_id = {
        let mut guard = state.session.write().await;
        guard.touch();
        match &guard.current {
            Some(s) => s.session_db_id.clone(),
            None => return Ok(()),
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    let _ = state
        .db
        .execute(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "UPDATE app_sessions SET last_activity_at = ? WHERE id = ?",
            [now.into(), session_db_id.into()],
        ))
        .await;

    Ok(())
}

// ── Device trust IPC commands ─────────────────────────────────────────────────

use crate::auth::device;

/// Get the trust status of the current device for the currently logged-in user.
/// Requires an active session.
#[tauri::command]
pub async fn get_device_trust_status(state: State<'_, AppState>) -> AppResult<device::DeviceTrustStatus> {
    let user = require_session!(state);

    let fingerprint = device::derive_device_fingerprint().unwrap_or_else(|_| "unknown".to_string());

    let trust = device::get_device_trust(&state.db, user.user_id, &fingerprint).await?;
    let (offline_allowed, offline_hours) = device::check_offline_access(&state.db, user.user_id, &fingerprint).await?;
    let decision =
        crate::activation::queries::evaluate_offline_activation_policy(&state.db, user.user_id, &fingerprint).await?;

    Ok(device::DeviceTrustStatus {
        device_fingerprint: fingerprint,
        is_trusted: trust.is_some() && !trust.as_ref().is_some_and(|t| t.is_revoked),
        is_revoked: trust.as_ref().is_some_and(|t| t.is_revoked),
        offline_allowed,
        offline_hours_remaining: offline_hours,
        device_label: trust.as_ref().and_then(|t| t.device_label.clone()),
        trusted_at: trust.as_ref().map(|t| t.trusted_at.clone()),
        offline_denial_code: decision.denial_code,
        offline_denial_message: decision.denial_message,
    })
}

/// Revoke trust for a specific trusted device by row id.
/// Use this to remove offline access for a lost or stolen device.
/// Requires admin permissions (enforced in SP04-F03).
#[tauri::command]
pub async fn revoke_device_trust(device_id: String, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    device::revoke_device_trust(&state.db, &device_id, user.user_id).await?;

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::DEVICE_TRUST_REVOKED,
            actor_id: Some(user.user_id),
            entity_type: Some("trusted_device"),
            entity_id: Some(&device_id),
            summary: "Device trust revoked",
            ..Default::default()
        },
    )
    .await;

    Ok(())
}

// ── Unlock Session ────────────────────────────────────────────────────────────

/// Input for the `unlock_session` command.
#[derive(Debug, Deserialize)]
pub struct UnlockSessionRequest {
    pub password: String,
}

/// Unlock an idle-locked session by verifying the user's password.
///
/// The session must exist and not be expired. If the session has expired,
/// the user is told to log in again. If the password is wrong, an opaque
/// auth error is returned.
#[tauri::command]
pub async fn unlock_session(
    payload: UnlockSessionRequest,
    state: State<'_, AppState>,
) -> AppResult<session_manager::SessionInfo> {
    // Read session to get the user — even locked sessions have a user
    let user = {
        let sm = state.session.read().await;
        match &sm.current {
            Some(s) if !s.is_expired() => s.user.clone(),
            _ => {
                return Err(AppError::Auth(
                    "Session expir\u{00e9}e. Veuillez vous reconnecter.".into(),
                ));
            }
        }
    };

    // Verify password against the database
    let user_record = session_manager::find_active_user(&state.db, &user.username).await?;

    let pw_hash = match user_record.and_then(|r| r.5) {
        Some(h) => h,
        None => {
            return Err(AppError::Auth("Mot de passe incorrect.".into()));
        }
    };

    let valid = password::verify_password(&payload.password, &pw_hash)?;
    if !valid {
        warn!(username = %user.username, "unlock_session::wrong_password");
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::STEP_UP_FAILURE,
                actor_id: Some(user.user_id),
                summary: "Unlock failed: wrong password",
                ..Default::default()
            },
        )
        .await;
        return Err(AppError::Auth("Mot de passe incorrect.".into()));
    }

    // Unlock the session
    let mut sm = state.session.write().await;
    if !sm.unlock_session() {
        return Err(AppError::Auth(
            "Session expir\u{00e9}e. Veuillez vous reconnecter.".into(),
        ));
    }

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::LOGIN_SUCCESS,
            actor_id: Some(user.user_id),
            summary: "Session unlocked after idle lock",
            ..Default::default()
        },
    )
    .await;

    let info = sm.session_info();
    Ok(info)
}

// ── PIN Commands ─────────────────────────────────────────────────────────────

/// Input for the `set_pin` command.
#[derive(Debug, Deserialize)]
pub struct SetPinInput {
    pub current_password: String,
    pub new_pin: String,
}

/// Input for the `clear_pin` command.
#[derive(Debug, Deserialize)]
pub struct ClearPinInput {
    pub current_password: String,
}

/// Input for the `unlock_session_with_pin` command.
#[derive(Debug, Deserialize)]
pub struct PinUnlockInput {
    pub pin: String,
}

/// Configure or update quick-unlock PIN.
/// Requires current password verification.
#[tauri::command]
pub async fn set_pin(payload: SetPinInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);

    let user_record = session_manager::find_active_user(&state.db, &user.username).await?;
    let pw_hash = match user_record.and_then(|r| r.5) {
        Some(h) => h,
        None => {
            return Err(AppError::Auth(
                "Aucun mot de passe local configuré pour cet utilisateur.".into(),
            ));
        }
    };

    let password_ok = password::verify_password(&payload.current_password, &pw_hash)?;
    if !password_ok {
        return Err(AppError::Auth("Mot de passe incorrect.".into()));
    }

    pin::validate_pin_format(payload.new_pin.trim())?;
    let pin_hash = pin::hash_pin(payload.new_pin.trim())?;

    let now = chrono::Utc::now().to_rfc3339();
    state
        .db
        .execute(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r"UPDATE user_accounts
               SET pin_hash = ?,
                   updated_at = ?
               WHERE id = ?",
            [pin_hash.into(), now.into(), user.user_id.into()],
        ))
        .await?;

    let mut sm = state.session.write().await;
    if let Some(current) = &mut sm.current {
        current.pin_configured = true;
        current.pin_failed_attempts = 0;
        current.pin_unlock_disabled = false;
    }
    drop(sm);

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::PIN_SET,
            actor_id: Some(user.user_id),
            summary: "Quick unlock PIN configured",
            ..Default::default()
        },
    )
    .await;

    Ok(())
}

/// Clear quick-unlock PIN.
/// Requires current password verification.
#[tauri::command]
pub async fn clear_pin(payload: ClearPinInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);

    let user_record = session_manager::find_active_user(&state.db, &user.username).await?;
    let pw_hash = match user_record.and_then(|r| r.5) {
        Some(h) => h,
        None => {
            return Err(AppError::Auth(
                "Aucun mot de passe local configuré pour cet utilisateur.".into(),
            ));
        }
    };

    let password_ok = password::verify_password(&payload.current_password, &pw_hash)?;
    if !password_ok {
        return Err(AppError::Auth("Mot de passe incorrect.".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    state
        .db
        .execute(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r"UPDATE user_accounts
               SET pin_hash = NULL,
                   updated_at = ?
               WHERE id = ?",
            [now.into(), user.user_id.into()],
        ))
        .await?;

    let mut sm = state.session.write().await;
    if let Some(current) = &mut sm.current {
        current.pin_configured = false;
        current.pin_failed_attempts = 0;
        current.pin_unlock_disabled = false;
    }
    drop(sm);

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::PIN_CLEARED,
            actor_id: Some(user.user_id),
            summary: "Quick unlock PIN removed",
            ..Default::default()
        },
    )
    .await;

    Ok(())
}

/// Internal unlock routine shared by test code.
#[cfg(test)]
pub(crate) async fn unlock_session_with_pin_internal(
    db: &sea_orm::DatabaseConnection,
    sm: &mut session_manager::SessionManager,
    pin_input: &str,
) -> AppResult<session_manager::SessionInfo> {
    let pin_value = pin_input.trim();
    pin::validate_pin_format(pin_value)?;

    let user_id = match &sm.current {
        Some(s) if !s.is_expired() && s.is_idle_locked() => {
            if s.pin_unlock_disabled {
                return Err(AppError::Auth(
                    "PIN désactivé, utilisez le mot de passe.".into(),
                ));
            }
            s.user.user_id
        }
        Some(_) => {
            return Err(AppError::Auth(
                "La session n'est pas verrouillée. Utilisez le flux normal.".into(),
            ));
        }
        None => {
            return Err(AppError::Auth(
                "Session expirée. Veuillez vous reconnecter.".into(),
            ));
        }
    };

    let pin_hash = session_manager::get_pin_hash_for_user(db, user_id)
        .await?
        .ok_or_else(|| AppError::Auth("Aucun PIN configuré pour cet utilisateur.".into()))?;

    let valid = pin::verify_pin(pin_value, &pin_hash)?;
    if !valid {
        if let Some(s) = &mut sm.current {
            s.pin_failed_attempts = s.pin_failed_attempts.saturating_add(1);
            if s.pin_failed_attempts >= 3 {
                s.pin_unlock_disabled = true;
                return Err(AppError::Auth(
                    "PIN désactivé, utilisez le mot de passe.".into(),
                ));
            }
        }

        return Err(AppError::Auth("PIN incorrect.".into()));
    }

    if let Some(s) = &mut sm.current {
        s.pin_failed_attempts = 0;
        s.pin_unlock_disabled = false;
    }

    if !sm.unlock_session() {
        return Err(AppError::Auth(
            "Session expirée. Veuillez vous reconnecter.".into(),
        ));
    }

    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::SESSION_UNLOCKED_WITH_PIN,
            actor_id: Some(user_id),
            summary: "Session unlocked with PIN",
            ..Default::default()
        },
    )
    .await;

    Ok(sm.session_info())
}

/// Unlock an idle-locked session using a 4-6 digit PIN.
#[tauri::command]
pub async fn unlock_session_with_pin(
    payload: PinUnlockInput,
    state: State<'_, AppState>,
) -> AppResult<session_manager::SessionInfo> {
    let pin_value = payload.pin.trim().to_string();
    pin::validate_pin_format(&pin_value)?;

    let user_id = {
        let sm = state.session.read().await;
        match &sm.current {
            Some(s) if !s.is_expired() && s.is_idle_locked() => {
                if s.pin_unlock_disabled {
                    return Err(AppError::Auth(
                        "PIN désactivé, utilisez le mot de passe.".into(),
                    ));
                }
                s.user.user_id
            }
            Some(_) => {
                return Err(AppError::Auth(
                    "La session n'est pas verrouillée. Utilisez le flux normal.".into(),
                ));
            }
            None => {
                return Err(AppError::Auth(
                    "Session expirée. Veuillez vous reconnecter.".into(),
                ));
            }
        }
    };

    let pin_hash = session_manager::get_pin_hash_for_user(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::Auth("Aucun PIN configuré pour cet utilisateur.".into()))?;

    let valid = pin::verify_pin(&pin_value, &pin_hash)?;

    let info = {
        let mut sm = state.session.write().await;
        match &sm.current {
            Some(s) if !s.is_expired() && s.is_idle_locked() => {
                if s.pin_unlock_disabled {
                    return Err(AppError::Auth(
                        "PIN désactivé, utilisez le mot de passe.".into(),
                    ));
                }
            }
            Some(_) => {
                return Err(AppError::Auth(
                    "La session n'est pas verrouillée. Utilisez le flux normal.".into(),
                ));
            }
            None => {
                return Err(AppError::Auth(
                    "Session expirée. Veuillez vous reconnecter.".into(),
                ));
            }
        }

        if !valid {
            if let Some(s) = &mut sm.current {
                s.pin_failed_attempts = s.pin_failed_attempts.saturating_add(1);
                if s.pin_failed_attempts >= 3 {
                    s.pin_unlock_disabled = true;
                    return Err(AppError::Auth(
                        "PIN désactivé, utilisez le mot de passe.".into(),
                    ));
                }
            }

            return Err(AppError::Auth("PIN incorrect.".into()));
        }

        if let Some(s) = &mut sm.current {
            s.pin_failed_attempts = 0;
            s.pin_unlock_disabled = false;
        }

        if !sm.unlock_session() {
            return Err(AppError::Auth(
                "Session expirée. Veuillez vous reconnecter.".into(),
            ));
        }

        sm.session_info()
    };

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::SESSION_UNLOCKED_WITH_PIN,
            actor_id: Some(user_id),
            summary: "Session unlocked with PIN",
            ..Default::default()
        },
    )
    .await;

    Ok(info)
}

// ── Force Change Password ─────────────────────────────────────────────────────

/// Input for the `force_change_password` command.
#[derive(Debug, Deserialize)]
pub struct ForceChangePasswordRequest {
    pub new_password: String,
}

/// Response returned after a successful force password change.
#[derive(Debug, Serialize)]
pub struct ForceChangePasswordResponse {
    pub session_info: session_manager::SessionInfo,
}

/// Change the password for a user who has `force_password_change = true`.
///
/// This command is only callable when the current session has
/// `force_password_change` set. It hashes the new password with argon2id,
/// updates the database, clears the flag, and returns the updated session.
#[tauri::command]
pub async fn force_change_password(
    payload: ForceChangePasswordRequest,
    state: State<'_, AppState>,
) -> AppResult<ForceChangePasswordResponse> {
    // Read current session — must be authenticated with force_password_change
    let user = {
        let sm = state.session.read().await;
        match &sm.current {
            Some(s) if !s.is_expired() && s.user.force_password_change => s.user.clone(),
            Some(_) => {
                return Err(AppError::Auth("Le changement de mot de passe n'est pas requis.".into()));
            }
            None => {
                return Err(AppError::Auth("Non authentifi\u{00e9}.".into()));
            }
        }
    };

    // Validate password strength (minimum 8 characters)
    let new_password = payload.new_password.trim();
    let policy = password_policy::PasswordPolicy::load(&state.db).await;
    password_policy::validate_password_strength(new_password, &policy)
        .map_err(AppError::ValidationFailed)?;

    // Hash the new password with argon2id
    let new_hash = password::hash_password(new_password)?;

    // Update user_accounts in the database
    let now = chrono::Utc::now().to_rfc3339();
    state
        .db
        .execute(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r"UPDATE user_accounts
               SET password_hash = ?,
                   force_password_change = 0,
                   password_changed_at = ?,
                   updated_at = ?
               WHERE id = ?",
            [
                new_hash.into(),
                now.clone().into(),
                now.clone().into(),
                user.user_id.into(),
            ],
        ))
        .await?;

    // Update the in-memory session
    let mut sm = state.session.write().await;
    if let Some(session) = &mut sm.current {
        session.user.force_password_change = false;
        session.password_expires_in_days = None;
    }

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::FORCE_CHANGE_SET,
            actor_id: Some(user.user_id),
            summary: "Password changed via force-change flow",
            ..Default::default()
        },
    )
    .await;

    tracing::info!(user_id = user.user_id, "force_change_password completed");

    let info = sm.session_info();
    Ok(ForceChangePasswordResponse { session_info: info })
}
