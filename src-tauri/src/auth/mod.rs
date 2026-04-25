//! Authentication and identity domain.
//!
//! Module layout:
//!   `auth::password`         — argon2id hash/verify
//!   `auth::session_manager`  — `SessionManager`, `AuthenticatedUser`, `LocalSession`
//!
//! Architecture rules:
//!   - No session token is ever stored in `SQLite` in plaintext.
//!   - The `SessionManager` is the single authoritative source of who is logged in.
//!   - All IPC commands that need auth call `require_session!(&state)`.

pub mod device;
pub mod lockout;
pub mod password;
pub mod password_policy;
pub mod pin;
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
        let has = $crate::auth::rbac::check_permission_cached(
            &$state.db,
            &$state.permission_cache,
            $user.user_id,
            $perm,
            &$scope,
        )
        .await?;

        if !has {
            return Err($crate::errors::AppError::PermissionDenied(format!(
                "Permission requise : {}",
                $perm
            )));
        }

        $crate::entitlements::queries::enforce_capability_for_permission(&$state.db, $perm).await?;
        $crate::license::queries::enforce_permission_matrix(&$state.db, $user.user_id, $perm).await?;

        // NOTE: Step-up enforcement is NOT done here. Use the explicit
        // `require_step_up!(state)` macro in commands that perform
        // dangerous write operations.  Read-only commands that merely
        // check a permission should never demand step-up.
    }};
}

/// Like [`require_permission!`], but allows accounts with `user_accounts.is_admin = 1` (bootstrap /
/// full admin) to pass the RBAC matrix check. Entitlement and license enforcement still apply.
///
/// Use for operator-facing surfaces where the Administrator role matrix can drift after new
/// permissions are added (e.g. sync.*).
#[macro_export]
macro_rules! require_permission_allowing_system_admin {
    ($state:expr, $user:expr, $perm:expr, $scope:expr) => {{
        if !$user.is_admin {
            $crate::require_permission!($state, $user, $perm, $scope);
        } else {
            $crate::entitlements::queries::enforce_capability_for_permission(&$state.db, $perm).await?;
            $crate::license::queries::enforce_permission_matrix(&$state.db, $user.user_id, $perm)
                .await?;
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
