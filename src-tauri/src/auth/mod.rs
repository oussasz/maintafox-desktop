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

pub mod builtin_admin;
pub mod device;
pub mod last_admin;
pub mod lockout;
pub mod password;
pub mod password_policy;
pub mod pin;
pub mod rbac;
pub mod recovery_code;
pub mod scope;
pub(crate) mod second_signer;
pub mod session_manager;

#[cfg(test)]
mod auth_integration_tests;
#[cfg(test)]
pub(crate) mod scope_tests;

/// Short-circuit an IPC command if there is no active authenticated session.
///
/// Distinguishes idle-lock (`SESSION_LOCKED`) from missing/expired (`AUTH_ERROR`).
/// It does not count as user activity (timed polls use it too): only `touch_session`, sent by
/// the shell on real keyboard/mouse input, keeps the session from idle-locking.
///
/// Usage inside an `async` Tauri command:
/// ```ignore
/// let user = require_session!(state);
/// // `user` is an `AuthenticatedUser` (cloned from the session).
/// ```
#[macro_export]
macro_rules! require_session {
    ($state:expr) => {{
        // From the pre-update backup until the restart, nothing can be changed (updater SF1).
        $crate::updater::refuse_while_installing()?;
        let (user, session_db_id) = {
            let mut guard = $state.session.write().await;
            match guard.require_active_user() {
                Ok(user) => {
                    let id = guard.current.as_ref().map(|s| s.session_db_id.clone());
                    (user, id)
                }
                Err(failure) => return Err(failure.into_app_error()),
            }
        };
        // Stored state: revoked session, disabled account, password change required.
        $crate::auth::session_manager::enforce_stored_session(
            &$state.db,
            &$state.session,
            &user,
            session_db_id.as_deref(),
        )
        .await?;
        user
    }};
}

/// Short-circuit an IPC command if the authenticated user does NOT hold
/// the given permission (checked against the database).
///
/// Usage inside an `async` Tauri command:
/// ```ignore
/// let user = require_session!(state);
/// require_permission!(state, &user, crate::rbac::permissions::EQ_DELETE, PermissionScope::Global);
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

/// Short-circuit an IPC command unless the user holds the permission at the record's anchor
/// (`auth::scope`): plant-wide, or on the record's org node or one above it. A record without a
/// site, or a plant-wide object, needs a plant-wide role; the refusal is coded
/// (`SCOPE_OUTSIDE_SITE`, `SCOPE_RECORD_WITHOUT_SITE`, `SCOPE_PLANT_WIDE_ONLY`). Entitlement and
/// licence checks as in [`require_permission!`].
///
/// ```ignore
/// require_permission_at!(state, &user, crate::rbac::permissions::OT_EDIT, Anchor::of(wo.entity_id));
/// ```
#[macro_export]
macro_rules! require_permission_at {
    ($state:expr, $user:expr, $perm:expr, $anchor:expr) => {{
        $crate::auth::scope::require_at(&$state.db, &$state.permission_cache, $user.user_id, $perm, $anchor).await?;
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
            $crate::license::queries::enforce_permission_matrix(&$state.db, $user.user_id, $perm).await?;
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
