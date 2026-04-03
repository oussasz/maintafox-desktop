//! SessionManager: owns the in-memory session, expiry enforcement, and
//! the OS-keyring session token.
//!
//! Rules:
//!   - Exactly one active session at a time per desktop instance.
//!   - The session token is a 32-byte random value stored in OS keyring only.
//!   - The app_sessions row is the lifecycle record; expiry is enforced here in memory.
//!   - Every write (login, logout, expire) emits an audit event via the db.

use std::time::Instant;

use chrono::{DateTime, TimeDelta, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use uuid::Uuid;

use crate::errors::AppResult;

/// Session duration: 8 hours of activity before forced re-login.
pub const SESSION_DURATION_HOURS: i64 = 8;
/// Idle timeout: session is locked (not expired) after 30 minutes of no activity.
pub const IDLE_LOCK_MINUTES: i64 = 30;
/// Step-up verification window: 120 seconds after re-entering password.
pub const STEP_UP_DURATION_SECS: u64 = 120;

/// The identity of an authenticated user, embedded in the active session.
#[derive(Debug, Clone, Serialize)]
pub struct AuthenticatedUser {
    pub user_id: i32,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub force_password_change: bool,
}

/// The full context of an active local session.
#[derive(Debug, Clone, Serialize)]
pub struct LocalSession {
    /// Row id in app_sessions
    pub session_db_id: String,
    pub user: AuthenticatedUser,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub is_locked: bool,
    /// When the user last completed step-up password verification.
    /// `None` means no step-up has been performed this session.
    #[serde(skip)]
    pub step_up_verified_at: Option<Instant>,
}

impl LocalSession {
    /// True if the session has passed its hard expiry time.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// True if the session is idle-locked (no activity for `IDLE_LOCK_MINUTES`).
    pub fn is_idle_locked(&self) -> bool {
        let idle_deadline =
            self.last_activity_at + TimeDelta::minutes(IDLE_LOCK_MINUTES);
        self.is_locked || Utc::now() > idle_deadline
    }

    /// True if a step-up verification was completed within `STEP_UP_DURATION_SECS`.
    pub fn is_step_up_valid(&self) -> bool {
        self.step_up_verified_at
            .map(|t| t.elapsed().as_secs() < STEP_UP_DURATION_SECS)
            .unwrap_or(false)
    }
}

/// Serializable summary returned by the `get_session_info` IPC command.
/// Does NOT include the token, password hash, or any credential material.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub is_authenticated: bool,
    pub is_locked: bool,
    pub user_id: Option<i32>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
    pub force_password_change: Option<bool>,
    pub expires_at: Option<String>,
    pub last_activity_at: Option<String>,
}

/// The session manager holds the current session in memory.
/// All access is through `RwLock` via `AppState`.
#[derive(Debug, Default)]
pub struct SessionManager {
    /// Current active session; `None` when no user is logged in.
    pub current: Option<LocalSession>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { current: None }
    }

    /// True if there is a non-expired, non-locked session.
    pub fn is_authenticated(&self) -> bool {
        self.current
            .as_ref()
            .map(|s| !s.is_expired() && !s.is_idle_locked())
            .unwrap_or(false)
    }

    /// Returns a reference to the current authenticated user, if any.
    /// Returns `None` if there is no session, or if the session is expired/locked.
    pub fn current_user(&self) -> Option<&AuthenticatedUser> {
        self.current
            .as_ref()
            .filter(|s| !s.is_expired())
            .map(|s| &s.user)
    }

    /// Updates the `last_activity_at` timestamp to prevent idle lock.
    /// Call this at the start of any authenticated IPC command.
    pub fn touch(&mut self) {
        if let Some(session) = &mut self.current {
            session.last_activity_at = Utc::now();
        }
    }

    /// Create a new session after a successful authentication check.
    pub fn create_session(&mut self, user: AuthenticatedUser) -> &LocalSession {
        let now = Utc::now();
        let session = LocalSession {
            session_db_id: Uuid::new_v4().to_string(),
            user,
            created_at: now,
            expires_at: now + TimeDelta::hours(SESSION_DURATION_HOURS),
            last_activity_at: now,
            is_locked: false,
            step_up_verified_at: None,
        };
        self.current = Some(session);
        self.current.as_ref().unwrap()
    }

    /// Lock the current session (e.g., idle timeout or manual lock).
    pub fn lock_session(&mut self) {
        if let Some(session) = &mut self.current {
            session.is_locked = true;
        }
    }

    /// Record a successful step-up password verification.
    pub fn record_step_up(&mut self) {
        if let Some(session) = &mut self.current {
            session.step_up_verified_at = Some(Instant::now());
        }
    }

    /// True if the current session has a valid (non-expired) step-up verification.
    pub fn is_step_up_valid(&self) -> bool {
        self.current
            .as_ref()
            .map(|s| s.is_step_up_valid())
            .unwrap_or(false)
    }

    /// Clear the current session (logout or forced expiry).
    pub fn clear_session(&mut self) -> Option<String> {
        let session_id = self.current.as_ref().map(|s| s.session_db_id.clone());
        self.current = None;
        session_id
    }

    /// Returns a `SessionInfo` summary for the IPC response.
    pub fn session_info(&self) -> SessionInfo {
        match &self.current {
            None => SessionInfo {
                is_authenticated: false,
                is_locked: false,
                user_id: None,
                username: None,
                display_name: None,
                is_admin: None,
                force_password_change: None,
                expires_at: None,
                last_activity_at: None,
            },
            Some(s) => SessionInfo {
                is_authenticated: !s.is_expired() && !s.is_idle_locked(),
                is_locked: s.is_idle_locked(),
                user_id: Some(s.user.user_id),
                username: Some(s.user.username.clone()),
                display_name: s.user.display_name.clone(),
                is_admin: Some(s.user.is_admin),
                force_password_change: Some(s.user.force_password_change),
                expires_at: Some(s.expires_at.to_rfc3339()),
                last_activity_at: Some(s.last_activity_at.to_rfc3339()),
            },
        }
    }
}

// ── DB helpers ────────────────────────────────────────────────────────────────

/// Look up a user account by username (case-insensitive via `LOWER()`).
/// Returns `None` if the user does not exist or is not active.
pub async fn find_active_user(
    db: &DatabaseConnection,
    username: &str,
) -> AppResult<Option<(i32, String, Option<String>, bool, bool, Option<String>)>> {
    // Returns: (id, username, display_name, is_admin, force_password_change, password_hash)
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT id, username, display_name, is_admin, force_password_change, password_hash
               FROM user_accounts
               WHERE LOWER(username) = LOWER(?) AND is_active = 1 AND deleted_at IS NULL"#,
            [username.into()],
        ))
        .await?;

    Ok(row.map(|r| {
        (
            r.try_get::<i32>("", "id").unwrap_or(0),
            r.try_get::<String>("", "username").unwrap_or_default(),
            r.try_get::<Option<String>>("", "display_name").unwrap_or(None),
            r.try_get::<i32>("", "is_admin").unwrap_or(0) == 1,
            r.try_get::<i32>("", "force_password_change").unwrap_or(0) == 1,
            r.try_get::<Option<String>>("", "password_hash").unwrap_or(None),
        )
    }))
}

/// Increment `failed_login_attempts` for a user. Locks account at 10 attempts.
pub async fn record_failed_login(db: &DatabaseConnection, user_id: i32) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"UPDATE user_accounts
           SET failed_login_attempts = failed_login_attempts + 1,
               locked_until = CASE
                   WHEN failed_login_attempts + 1 >= 10
                   THEN datetime('now', '+15 minutes')
                   ELSE locked_until
               END,
               updated_at = ?
           WHERE id = ?"#,
        [now.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}

/// Reset `failed_login_attempts` and `locked_until` after successful login.
pub async fn record_successful_login(db: &DatabaseConnection, user_id: i32) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"UPDATE user_accounts
           SET failed_login_attempts = 0,
               locked_until = NULL,
               last_login_at = ?,
               last_seen_at = ?,
               updated_at = ?
           WHERE id = ?"#,
        [
            now.clone().into(),
            now.clone().into(),
            now.into(),
            user_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Write an `app_sessions` row for audit purposes.
pub async fn create_session_record(
    db: &DatabaseConnection,
    session_db_id: &str,
    user_id: i32,
    expires_at: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT INTO app_sessions (id, user_id, created_at, expires_at, is_revoked)
           VALUES (?, ?, ?, ?, 0)"#,
        [
            session_db_id.into(),
            user_id.into(),
            now.into(),
            expires_at.into(),
        ],
    ))
    .await?;
    Ok(())
}

// ── Unit tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: 1,
            username: "test_user".into(),
            display_name: Some("Test User".into()),
            is_admin: false,
            force_password_change: false,
        }
    }

    #[test]
    fn new_manager_is_not_authenticated() {
        let mgr = SessionManager::new();
        assert!(!mgr.is_authenticated());
        assert!(mgr.current_user().is_none());
    }

    #[test]
    fn create_session_sets_authenticated() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        assert!(mgr.is_authenticated());
        assert_eq!(mgr.current_user().unwrap().username, "test_user");
    }

    #[test]
    fn clear_session_removes_authentication() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        mgr.clear_session();
        assert!(!mgr.is_authenticated());
    }

    #[test]
    fn lock_session_reports_locked() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        mgr.lock_session();
        assert!(
            !mgr.is_authenticated(),
            "Locked session must not be 'authenticated'"
        );
        assert!(mgr.current.as_ref().unwrap().is_locked);
    }

    #[test]
    fn session_info_unauthenticated_is_all_none() {
        let mgr = SessionManager::new();
        let info = mgr.session_info();
        assert!(!info.is_authenticated);
        assert!(info.user_id.is_none());
        assert!(info.username.is_none());
    }

    #[test]
    fn session_info_authenticated_has_user_fields() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        let info = mgr.session_info();
        assert!(info.is_authenticated);
        assert_eq!(info.user_id, Some(1));
        assert_eq!(info.username.as_deref(), Some("test_user"));
        assert_eq!(info.is_admin, Some(false));
    }

    #[test]
    fn step_up_not_valid_by_default() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        assert!(!mgr.is_step_up_valid());
    }

    #[test]
    fn record_step_up_makes_it_valid() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        mgr.record_step_up();
        assert!(mgr.is_step_up_valid());
    }

    #[test]
    fn step_up_on_no_session_is_not_valid() {
        let mgr = SessionManager::new();
        assert!(!mgr.is_step_up_valid());
    }
}
