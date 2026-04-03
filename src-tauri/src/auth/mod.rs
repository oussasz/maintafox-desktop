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

