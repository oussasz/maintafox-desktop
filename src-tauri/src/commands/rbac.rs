//! RBAC IPC commands.
//!
//! Exposes permission queries and step-up verification to the frontend.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::rbac;
use crate::auth::{password, session_manager};
use crate::errors::{AppError, AppResult};
use crate::require_session;
use crate::state::AppState;

/// Return the effective permission set for the currently authenticated user.
///
/// The frontend calls this after login to pre-load permissions into the
/// `usePermissions` hook so that `<PermissionGate>` can work without
/// round-tripping to the backend for every check.
#[tauri::command]
pub async fn get_my_permissions(
    state: State<'_, AppState>,
) -> AppResult<Vec<rbac::PermissionRecord>> {
    let user = require_session!(state);
    rbac::get_user_permissions(&state.db, user.user_id).await
}

/// Input for the `verify_step_up` command.
#[derive(Debug, Deserialize)]
pub struct StepUpRequest {
    pub password: String,
}

/// Response from a successful step-up verification.
#[derive(Debug, Serialize)]
pub struct StepUpResponse {
    pub success: bool,
    pub expires_at: String,
}

/// Verify the user's password for a dangerous-action step-up.
///
/// On success, records the verification timestamp in the in-memory session.
/// The step-up window (`STEP_UP_DURATION_SECS`) starts from this moment.
#[tauri::command]
pub async fn verify_step_up(
    payload: StepUpRequest,
    state: State<'_, AppState>,
) -> AppResult<StepUpResponse> {
    let user = require_session!(state);

    // Look up the current password hash from the database
    let user_record =
        session_manager::find_active_user(&state.db, &user.username).await?;

    let pw_hash = match user_record {
        Some(row) => match row.5 {
            Some(hash) => hash,
            None => {
                return Err(AppError::Auth(
                    "Aucun mot de passe configuré.".into(),
                ));
            }
        },
        None => {
            return Err(AppError::Auth(
                "Utilisateur introuvable.".into(),
            ));
        }
    };

    // Verify the provided password against the stored hash
    let valid = password::verify_password(&payload.password, &pw_hash)?;
    if !valid {
        crate::audit::emit(&state.db, crate::audit::AuditEvent {
            event_type: crate::audit::event_type::STEP_UP_FAILURE,
            actor_id:   Some(user.user_id),
            summary:    "Step-up reauthentication failed: wrong password",
            ..Default::default()
        }).await;
        return Err(AppError::Auth(
            "Mot de passe incorrect.".into(),
        ));
    }

    // Record the step-up timestamp in the session
    let mut sm = state.session.write().await;
    sm.record_step_up();

    crate::audit::emit(&state.db, crate::audit::AuditEvent {
        event_type: crate::audit::event_type::STEP_UP_SUCCESS,
        actor_id:   Some(user.user_id),
        summary:    "Step-up reauthentication verified",
        ..Default::default()
    }).await;

    let expires_at = (chrono::Utc::now()
        + chrono::TimeDelta::seconds(session_manager::STEP_UP_DURATION_SECS as i64))
        .to_rfc3339();

    Ok(StepUpResponse {
        success: true,
        expires_at,
    })
}
