//! Authentication and identity domain.
//!
//! Module layout:
//!   auth::password         — argon2id hash/verify
//!   auth::session_manager  — SessionManager, AuthenticatedUser, LocalSession
//!
//! Architecture rules:
//!   - No session token is ever stored in SQLite in plaintext.
//!   - The SessionManager is the single authoritative source of who is logged in.
//!   - All IPC commands that need auth call `require_session!(&state)`.

pub mod device;
pub mod password;
pub mod rbac;
pub mod session_manager;

#[cfg(test)]
mod auth_integration_tests;

/// Short-circuit an IPC command if there is no active authenticated session.
///
/// Usage inside an `async` Tauri command:
/// ```ignore
/// let user = require_session!(state);
/// // `user` is an `AuthenticatedUser` (cloned from the session).
/// ```
#[macro_export]
macro_rules! require_session {
    ($state:expr) => {{
        let guard = $state.session.read().await;
        if !guard.is_authenticated() {
            return Err($crate::errors::AppError::Auth(
                "Session expirée ou absente. Veuillez vous reconnecter.".into(),
            ));
        }
        // SAFETY: is_authenticated() guarantees current is Some and non-expired
        guard.current.as_ref().unwrap().user.clone()
    }};
}

/// Short-circuit an IPC command if the authenticated user does NOT hold
/// the given permission (checked against the database).
///
/// Usage inside an `async` Tauri command:
/// ```ignore
/// let user = require_session!(state);
/// require_permission!(state, &user, "eq.delete", PermissionScope::Global);
/// ```
#[macro_export]
macro_rules! require_permission {
    ($state:expr, $user:expr, $perm:expr, $scope:expr) => {{
        let has = $crate::auth::rbac::check_permission(&$state.db, $user.user_id, $perm, &$scope).await?;

        if !has {
            return Err($crate::errors::AppError::PermissionDenied(format!(
                "Permission requise : {}",
                $perm
            )));
        }

        // If the permission requires step-up, verify it
        let guard = $state.session.read().await;
        let needs_step_up = $crate::auth::rbac::permission_requires_step_up(&$state.db, $perm)
            .await
            .unwrap_or(false);

        if needs_step_up && !guard.is_step_up_valid() {
            return Err($crate::errors::AppError::StepUpRequired);
        }
    }};
}

/// Short-circuit an IPC command if no valid step-up verification exists.
///
/// Usage inside an `async` Tauri command:
/// ```ignore
/// let user = require_session!(state);
/// require_step_up!(state);
/// ```
#[macro_export]
macro_rules! require_step_up {
    ($state:expr) => {{
        let guard = $state.session.read().await;
        if !guard.is_step_up_valid() {
            return Err($crate::errors::AppError::StepUpRequired);
        }
    }};
}
