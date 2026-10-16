//! `SessionManager`: owns the in-memory session, expiry enforcement, and
//! the OS-keyring session token.
//!
//! Rules:
//!   - Exactly one active session at a time per desktop instance.
//!   - The session token is a 32-byte random value stored in OS keyring only.
//!   - The `app_sessions` row is the lifecycle record; expiry is enforced here in memory.
//!   - Every write (login, logout, expire) emits an audit event via the db.

use std::time::Instant;

use chrono::{DateTime, TimeDelta, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use uuid::Uuid;

use crate::errors::AppResult;

/// Default session duration (8 hours) when no session policy is loaded.
/// Login uses the active policy (`absolute_session_minutes`, Settings → Security).
pub const SESSION_DURATION_HOURS: i64 = 8;
/// Default idle timeout (30 minutes) when no session policy is loaded.
/// Login uses the active policy (`idle_timeout_minutes`, Settings → Security).
pub const IDLE_LOCK_MINUTES: i64 = 30;
/// Default step-up window (15 minutes, the policy default) when no session policy is loaded.
/// Login uses the active policy (`step_up_window_minutes`), clamped to 1-30 minutes.
pub const STEP_UP_WINDOW_MINUTES: i64 = 15;
pub const STEP_UP_WINDOW_MIN_MINUTES: i64 = 1;
pub const STEP_UP_WINDOW_MAX_MINUTES: i64 = 30;

/// Session timeouts resolved from the session policy (Settings -> Security).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionTimeouts {
    /// Minutes without activity before the session locks.
    pub idle_minutes: i64,
    /// Minutes after login before the session expires.
    pub absolute_minutes: i64,
    /// Minutes a password re-confirmation (step-up) stays valid.
    pub step_up_minutes: i64,
}

impl Default for SessionTimeouts {
    fn default() -> Self {
        Self {
            idle_minutes: IDLE_LOCK_MINUTES,
            absolute_minutes: SESSION_DURATION_HOURS * 60,
            step_up_minutes: STEP_UP_WINDOW_MINUTES,
        }
    }
}

impl SessionTimeouts {
    /// Policy values; non-positive idle/absolute values fall back to the defaults and the step-up
    /// window is clamped to 1-30 minutes.
    pub fn from_policy(idle_minutes: i64, absolute_minutes: i64, step_up_minutes: i64) -> Self {
        let d = Self::default();
        Self {
            idle_minutes: if idle_minutes > 0 { idle_minutes } else { d.idle_minutes },
            absolute_minutes: if absolute_minutes > 0 {
                absolute_minutes
            } else {
                d.absolute_minutes
            },
            step_up_minutes: step_up_minutes.clamp(STEP_UP_WINDOW_MIN_MINUTES, STEP_UP_WINDOW_MAX_MINUTES),
        }
    }
}

/// The identity of an authenticated user, embedded in the active session.
#[derive(Debug, Clone, Serialize)]
pub struct AuthenticatedUser {
    pub user_id: i32,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub force_password_change: bool,
    pub tenant_id: String,
    pub token_tenant_id: String,
}

/// The full context of an active local session.
#[derive(Debug, Clone, Serialize)]
pub struct LocalSession {
    /// Row id in `app_sessions`
    pub session_db_id: String,
    pub user: AuthenticatedUser,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub is_locked: bool,
    /// Minutes without activity before the session locks (from the session policy at login).
    pub idle_timeout_minutes: i64,
    /// Minutes a step-up verification stays valid (from the session policy at login).
    pub step_up_window_minutes: i64,
    pub password_expires_in_days: Option<i64>,
    pub pin_configured: bool,
    pub pin_failed_attempts: u8,
    pub pin_unlock_disabled: bool,
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

    /// True if the session is idle-locked (no activity for `idle_timeout_minutes`).
    pub fn is_idle_locked(&self) -> bool {
        let idle_deadline = self.last_activity_at + TimeDelta::minutes(self.idle_timeout_minutes);
        self.is_locked || Utc::now() > idle_deadline
    }

    /// True if a step-up verification was completed within `step_up_window_minutes`.
    pub fn is_step_up_valid(&self) -> bool {
        let window_secs = u64::try_from(self.step_up_window_minutes.max(0)).unwrap_or(0) * 60;
        self.step_up_verified_at
            .is_some_and(|t| t.elapsed().as_secs() < window_secs)
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
    pub password_expires_in_days: Option<i64>,
    pub pin_configured: Option<bool>,
    pub expires_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub tenant_id: Option<String>,
    pub token_tenant_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TenantScopeResolution {
    pub tenant_id: String,
    pub token_tenant_id: String,
}

/// The session manager holds the current session in memory.
/// All access is through `RwLock` via `AppState`.
#[derive(Debug, Default)]
pub struct SessionManager {
    /// Current active session; `None` when no user is logged in.
    pub current: Option<LocalSession>,
}

/// Outcome of [`SessionManager::require_active_user`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequireSessionFailure {
    /// No session, or hard expiry passed.
    MissingOrExpired,
    /// Session exists but is idle-locked (or manually locked).
    IdleLocked,
}

impl SessionManager {
    pub const fn new() -> Self {
        Self { current: None }
    }

    /// True if there is a non-expired, non-locked session.
    pub fn is_authenticated(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|s| !s.is_expired() && !s.is_idle_locked())
    }

    /// Validate the in-memory session for a protected IPC command.
    ///
    /// On success, refreshes `last_activity_at` (idle-lock prevention) and returns
    /// a clone of the authenticated user.
    pub fn require_active_user(&mut self) -> Result<AuthenticatedUser, RequireSessionFailure> {
        match &self.current {
            None => return Err(RequireSessionFailure::MissingOrExpired),
            Some(session) if session.is_expired() => {
                return Err(RequireSessionFailure::MissingOrExpired);
            }
            Some(session) if session.is_idle_locked() => {
                return Err(RequireSessionFailure::IdleLocked);
            }
            Some(_) => {}
        }
        self.touch();
        // SAFETY: branches above guarantee a non-expired, non-idle-locked session.
        Ok(self.current.as_ref().unwrap().user.clone())
    }

    /// Returns a reference to the current authenticated user, if any.
    /// Returns `None` if there is no session, or if the session is expired/locked.
    pub fn current_user(&self) -> Option<&AuthenticatedUser> {
        self.current.as_ref().filter(|s| !s.is_expired()).map(|s| &s.user)
    }

    /// Updates the `last_activity_at` timestamp to prevent idle lock.
    /// Call this at the start of any authenticated IPC command.
    pub fn touch(&mut self) {
        if let Some(session) = &mut self.current {
            session.last_activity_at = Utc::now();
        }
    }

    /// Create a new session with the default timeouts (tests and callers without a policy).
    pub fn create_session(&mut self, user: AuthenticatedUser) -> &LocalSession {
        self.create_session_with_timeouts(user, SessionTimeouts::default())
    }

    /// Create a new session after a successful authentication check, with the timeouts of the
    /// active session policy.
    pub fn create_session_with_timeouts(
        &mut self,
        user: AuthenticatedUser,
        timeouts: SessionTimeouts,
    ) -> &LocalSession {
        let now = Utc::now();
        let session = LocalSession {
            session_db_id: Uuid::new_v4().to_string(),
            user,
            created_at: now,
            expires_at: now + TimeDelta::minutes(timeouts.absolute_minutes),
            last_activity_at: now,
            is_locked: false,
            idle_timeout_minutes: timeouts.idle_minutes,
            step_up_window_minutes: timeouts.step_up_minutes,
            password_expires_in_days: None,
            pin_configured: false,
            pin_failed_attempts: 0,
            pin_unlock_disabled: false,
            step_up_verified_at: None,
        };
        self.current = Some(session);
        self.current.as_ref().unwrap()
    }

    /// Lock the current session (e.g., idle timeout or manual lock).
    pub fn lock_session(&mut self) {
        if let Some(session) = &mut self.current {
            session.is_locked = true;
            session.pin_failed_attempts = 0;
            session.pin_unlock_disabled = false;
        }
    }

    /// Unlock an idle-locked session. Returns `true` if the session was
    /// successfully unlocked, `false` if the session is expired or absent.
    /// The caller MUST verify the user's password before calling this.
    pub fn unlock_session(&mut self) -> bool {
        match &mut self.current {
            Some(session) if !session.is_expired() => {
                session.is_locked = false;
                session.last_activity_at = Utc::now();
                session.pin_failed_attempts = 0;
                session.pin_unlock_disabled = false;
                true
            }
            _ => false,
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
        self.current.as_ref().is_some_and(LocalSession::is_step_up_valid)
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
                password_expires_in_days: None,
                pin_configured: None,
                expires_at: None,
                last_activity_at: None,
                tenant_id: None,
                token_tenant_id: None,
            },
            Some(s) => SessionInfo {
                is_authenticated: !s.is_expired() && !s.is_idle_locked(),
                is_locked: s.is_idle_locked(),
                user_id: Some(s.user.user_id),
                username: Some(s.user.username.clone()),
                display_name: s.user.display_name.clone(),
                is_admin: Some(s.user.is_admin),
                force_password_change: Some(s.user.force_password_change),
                password_expires_in_days: s.password_expires_in_days,
                pin_configured: Some(s.pin_configured),
                expires_at: Some(s.expires_at.to_rfc3339()),
                last_activity_at: Some(s.last_activity_at.to_rfc3339()),
                tenant_id: Some(s.user.tenant_id.clone()),
                token_tenant_id: Some(s.user.token_tenant_id.clone()),
            },
        }
    }
}

/// Resolve runtime tenant claims for the current activated tenant.
///
/// Rules:
/// - If active tenant-scoped assignments have explicit `scope_reference` values,
///   the activated tenant must be one of them.
/// - If active tenant-scoped assignments exist but are wildcard (`scope_reference` NULL),
///   the user is treated as allowed on the activated tenant.
/// - If no active tenant-scoped assignment exists, deny access.
pub async fn resolve_tenant_scope_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    activated_tenant_id: &str,
) -> AppResult<Option<TenantScopeResolution>> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT scope_reference
               FROM user_scope_assignments
               WHERE user_id = ?
                 AND scope_type = 'tenant'
                 AND deleted_at IS NULL
                 AND (valid_from IS NULL OR valid_from <= ?)
                 AND (valid_to   IS NULL OR valid_to   >= ?)",
            [user_id.into(), now.clone().into(), now.into()],
        ))
        .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let mut has_wildcard_scope = false;
    let mut explicit_tenant_scope: Vec<String> = Vec::new();
    for row in &rows {
        match row.try_get::<Option<String>>("", "scope_reference").ok().flatten() {
            Some(value) if !value.trim().is_empty() => explicit_tenant_scope.push(value),
            _ => has_wildcard_scope = true,
        }
    }

    if explicit_tenant_scope.is_empty() {
        if has_wildcard_scope {
            return Ok(Some(TenantScopeResolution {
                tenant_id: activated_tenant_id.to_string(),
                token_tenant_id: activated_tenant_id.to_string(),
            }));
        }
        return Ok(None);
    }

    let tenant_matches = explicit_tenant_scope
        .iter()
        .any(|tenant_scope| tenant_scope == activated_tenant_id);
    if !tenant_matches {
        return Ok(None);
    }

    Ok(Some(TenantScopeResolution {
        tenant_id: activated_tenant_id.to_string(),
        token_tenant_id: activated_tenant_id.to_string(),
    }))
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
            r"SELECT id, username, display_name, is_admin, force_password_change, password_hash
               FROM user_accounts
               WHERE LOWER(username) = LOWER(?) AND is_active = 1 AND deleted_at IS NULL",
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

/// Return the user's PIN hash if configured.
pub async fn get_pin_hash_for_user(db: &DatabaseConnection, user_id: i32) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT pin_hash
               FROM user_accounts
               WHERE id = ? AND is_active = 1 AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;

    Ok(row
        .and_then(|r| r.try_get::<Option<String>>("", "pin_hash").ok())
        .flatten())
}

/// Increment `failed_login_attempts` for a user. Locks account at 10 attempts.
pub async fn record_failed_login(db: &DatabaseConnection, user_id: i32) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"UPDATE user_accounts
           SET failed_login_attempts = failed_login_attempts + 1,
               locked_until = CASE
                   WHEN failed_login_attempts + 1 >= 10
                   THEN datetime('now', '+15 minutes')
                   ELSE locked_until
               END,
               updated_at = ?
           WHERE id = ?",
        [now.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}

/// Reset `failed_login_attempts`, `locked_until`, and `consecutive_lockouts` after successful login.
pub async fn record_successful_login(db: &DatabaseConnection, user_id: i32) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"UPDATE user_accounts
           SET failed_login_attempts = 0,
               locked_until = NULL,
               consecutive_lockouts = 0,
               last_login_at = ?,
               last_seen_at = ?,
               updated_at = ?
           WHERE id = ?",
        [now.clone().into(), now.clone().into(), now.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}

/// Error message prefix of a command refused because the user must change their password first.
pub const PASSWORD_CHANGE_REQUIRED: &str = "PASSWORD_CHANGE_REQUIRED";

/// Checks of the stored state behind an in-memory session, run by `require_session!` on every
/// command:
/// - the session row was closed (`app_sessions.is_revoked`, by an administrator, a disable or a
///   password reset) or the account was disabled / deleted, including by a change received from
///   another device → the in-memory session ends and the command is refused;
/// - the account must change its password (`force_password_change`, set at creation or by an
///   administrator's reset) → every command is refused; only the change-password and sign-out
///   commands (which do not use `require_session!`) work. The in-memory flag is raised so the
///   shell shows the change-password screen.
pub async fn enforce_stored_session(
    db: &DatabaseConnection,
    session: &tokio::sync::RwLock<SessionManager>,
    user: &AuthenticatedUser,
    session_db_id: Option<&str>,
) -> crate::errors::AppResult<()> {
    use crate::errors::AppError;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ua.is_active, ua.deleted_at, ua.force_password_change, \
                    (SELECT s.is_revoked FROM app_sessions s WHERE s.id = ?) AS revoked \
             FROM user_accounts ua WHERE ua.id = ?",
            [session_db_id.map(str::to_string).into(), user.user_id.into()],
        ))
        .await?;
    let ended = match &row {
        None => true,
        Some(r) => {
            r.try_get::<i32>("", "is_active").unwrap_or(0) != 1
                || r.try_get::<Option<String>>("", "deleted_at").ok().flatten().is_some()
                || r.try_get::<Option<i32>>("", "revoked").ok().flatten() == Some(1)
        }
    };
    if ended {
        let mut guard = session.write().await;
        if guard.current.as_ref().map(|s| s.session_db_id.as_str()) == session_db_id {
            guard.clear_session();
        }
        return Err(AppError::Auth(
            "Session fermée : le compte a été désactivé ou la session a été révoquée. Veuillez vous reconnecter."
                .into(),
        ));
    }
    let must_change = row
        .as_ref()
        .and_then(|r| r.try_get::<i32>("", "force_password_change").ok())
        .unwrap_or(0)
        == 1;
    if must_change {
        let mut guard = session.write().await;
        if let Some(current) = guard.current.as_mut() {
            current.user.force_password_change = true;
        }
        return Err(AppError::Auth(format!(
            "{PASSWORD_CHANGE_REQUIRED}: le mot de passe doit être changé avant de continuer."
        )));
    }
    Ok(())
}

/// Close every stored session of a user (disable, administrator password reset). The in-memory
/// session of this device, if it is one of them, ends at its next command.
pub async fn revoke_user_sessions<C: ConnectionTrait>(conn: &C, user_id: i64) -> crate::errors::AppResult<u64> {
    Ok(conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE app_sessions SET is_revoked = 1 WHERE user_id = CAST(? AS TEXT) AND is_revoked = 0",
            [user_id.into()],
        ))
        .await?
        .rows_affected())
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
        r"INSERT INTO app_sessions (id, user_id, created_at, expires_at, last_activity_at, is_revoked)
           VALUES (?, ?, ?, ?, ?, 0)",
        [
            session_db_id.into(),
            user_id.to_string().into(),
            now.clone().into(),
            expires_at.into(),
            now.into(),
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
            tenant_id: "tenant-test".into(),
            token_tenant_id: "tenant-test".into(),
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
        assert!(!mgr.is_authenticated(), "Locked session must not be 'authenticated'");
        assert!(mgr.current.as_ref().unwrap().is_locked);
    }

    #[test]
    fn require_active_user_ok_touches_activity() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        let before = mgr.current.as_ref().unwrap().last_activity_at;
        std::thread::sleep(std::time::Duration::from_millis(5));
        let user = mgr.require_active_user().expect("active session");
        assert_eq!(user.username, "test_user");
        let after = mgr.current.as_ref().unwrap().last_activity_at;
        assert!(after >= before);
    }

    #[test]
    fn require_active_user_idle_locked() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        mgr.lock_session();
        assert_eq!(
            mgr.require_active_user().unwrap_err(),
            RequireSessionFailure::IdleLocked
        );
    }

    #[test]
    fn idle_lock_follows_the_session_policy_timeout() {
        let mut mgr = SessionManager::new();
        mgr.create_session_with_timeouts(make_user(), SessionTimeouts::from_policy(10, 120, 15));
        let session = mgr.current.as_mut().unwrap();
        assert_eq!(session.idle_timeout_minutes, 10);
        session.last_activity_at = Utc::now() - TimeDelta::minutes(9);
        assert!(!session.is_idle_locked(), "9 minutes idle < 10-minute policy");
        session.last_activity_at = Utc::now() - TimeDelta::minutes(11);
        assert!(session.is_idle_locked(), "11 minutes idle > 10-minute policy");
        let lifetime = session.expires_at - session.created_at;
        assert_eq!(lifetime.num_minutes(), 120);
    }

    #[test]
    fn invalid_policy_timeouts_fall_back_to_defaults() {
        let mut mgr = SessionManager::new();
        mgr.create_session_with_timeouts(make_user(), SessionTimeouts::from_policy(0, -5, 15));
        let session = mgr.current.as_ref().unwrap();
        assert_eq!(session.idle_timeout_minutes, IDLE_LOCK_MINUTES);
        assert_eq!(
            (session.expires_at - session.created_at).num_hours(),
            SESSION_DURATION_HOURS
        );
    }

    #[test]
    fn step_up_window_follows_policy_and_is_clamped() {
        assert_eq!(SessionTimeouts::from_policy(30, 480, 15).step_up_minutes, 15);
        assert_eq!(SessionTimeouts::from_policy(30, 480, 0).step_up_minutes, 1);
        assert_eq!(SessionTimeouts::from_policy(30, 480, 120).step_up_minutes, 30);
        let mut mgr = SessionManager::new();
        mgr.create_session_with_timeouts(make_user(), SessionTimeouts::from_policy(30, 480, 5));
        let session = mgr.current.as_mut().unwrap();
        assert!(!session.is_step_up_valid(), "no step-up yet");
        session.step_up_verified_at = Instant::now().checked_sub(std::time::Duration::from_secs(4 * 60));
        assert!(session.is_step_up_valid(), "4 minutes < 5-minute window");
        session.step_up_verified_at = Instant::now().checked_sub(std::time::Duration::from_secs(6 * 60));
        assert!(!session.is_step_up_valid(), "6 minutes > 5-minute window");
    }

    #[test]
    fn require_active_user_missing() {
        let mut mgr = SessionManager::new();
        assert_eq!(
            mgr.require_active_user().unwrap_err(),
            RequireSessionFailure::MissingOrExpired
        );
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
