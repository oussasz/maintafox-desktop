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
    /// The account's `password_changed_at` as [`enforce_session_alive`] first read it for this
    /// session (`None`: not read yet; `Some(None)`: the account had no date). A different stored
    /// value later means the password changed after the session started, whatever the clocks say.
    #[serde(skip)]
    pub password_changed_at_seen: Option<Option<String>>,
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

    /// Once the idle deadline has passed the lock is recorded, so it holds until an unlock
    /// (password or PIN) whatever happens to the clock afterwards. The PIN attempts are NOT reset
    /// here: wrong PINs may already have been tried against this lock (the PIN unlock reads the
    /// time-based lock without recording it), and recording the lock later must not give more
    /// tries. They start afresh at each unlock (`SessionManager::unlock_session`).
    fn latch_idle_lock(&mut self) {
        if !self.is_locked && self.is_idle_locked() {
            self.is_locked = true;
        }
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
    /// Minutes without user input before the session locks (session policy at sign-in). The shell
    /// uses it to show the lock screen when the deadline passes; the server stays the authority.
    pub idle_timeout_minutes: Option<i64>,
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

impl RequireSessionFailure {
    /// The refusal sent to the screen: `SESSION_LOCKED` (the lock screen unlocks it) or
    /// `AUTH_ERROR` (sign in again).
    pub fn into_app_error(self) -> crate::errors::AppError {
        match self {
            Self::IdleLocked => crate::errors::AppError::SessionLocked(
                "Session verrouillée pour inactivité. Veuillez vous déverrouiller.".into(),
            ),
            Self::MissingOrExpired => {
                crate::errors::AppError::Auth("Session expirée ou absente. Veuillez vous reconnecter.".into())
            }
        }
    }
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

    /// Validate the in-memory session for a protected IPC command and return a clone of the
    /// authenticated user.
    ///
    /// It does NOT count as activity: timed polls (notifications, presence, statistics, sync)
    /// call protected commands too, and must not keep an unattended session open. Only
    /// [`Self::record_activity`] (the `touch_session` command, sent by the shell on real user
    /// input) moves `last_activity_at`. A session found past its idle deadline is locked here.
    pub fn require_active_user(&mut self) -> Result<AuthenticatedUser, RequireSessionFailure> {
        match self.current.as_mut() {
            None => Err(RequireSessionFailure::MissingOrExpired),
            Some(session) if session.is_expired() => Err(RequireSessionFailure::MissingOrExpired),
            Some(session) if session.is_idle_locked() => {
                session.latch_idle_lock();
                Err(RequireSessionFailure::IdleLocked)
            }
            Some(session) => Ok(session.user.clone()),
        }
    }

    /// Returns a reference to the current authenticated user, if any.
    /// Returns `None` if there is no session, or if the session is expired/locked.
    pub fn current_user(&self) -> Option<&AuthenticatedUser> {
        self.current.as_ref().filter(|s| !s.is_expired()).map(|s| &s.user)
    }

    /// The user worked in the app (keyboard, mouse, touch): `last_activity_at` moves to now.
    ///
    /// A session already past its idle deadline (the PC woke up after a long sleep, the app was
    /// left alone) is not revived: it is locked and stays locked until an unlock (password or
    /// PIN), which is the only other way `last_activity_at` moves.
    pub fn record_activity(&mut self) -> Result<(), RequireSessionFailure> {
        match self.current.as_mut() {
            None => Err(RequireSessionFailure::MissingOrExpired),
            Some(session) if session.is_expired() => Err(RequireSessionFailure::MissingOrExpired),
            Some(session) if session.is_idle_locked() => {
                session.latch_idle_lock();
                Err(RequireSessionFailure::IdleLocked)
            }
            Some(session) => {
                session.last_activity_at = Utc::now();
                Ok(())
            }
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
            password_changed_at_seen: None,
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
                idle_timeout_minutes: None,
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
                idle_timeout_minutes: Some(s.idle_timeout_minutes),
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
    // Tenant-scope assignments in force (the shared rule and clock of `rbac::resolver`).
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(crate::rbac::resolver::in_force_binds(&now));
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT usa.scope_reference
                   FROM user_scope_assignments usa
                  WHERE usa.user_id = ?
                    AND usa.scope_type = 'tenant'
                    AND {}",
                crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
            ),
            values,
        ))
        .await?;

    if rows.is_empty() {
        // A site-only user (roles on org nodes only, step 6) signs in on this activated tenant:
        // org nodes belong to this tenant's database, and every check limits them to their sites.
        if crate::auth::scope::has_node_assignment(db, user_id).await? {
            return Ok(Some(TenantScopeResolution {
                tenant_id: activated_tenant_id.to_string(),
                token_tenant_id: activated_tenant_id.to_string(),
            }));
        }
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

/// Clock tolerance when comparing a password change with a session start, at the session's first
/// check only (before the session has read the account's `password_changed_at`): the change may
/// have been made on another device whose clock runs ahead. After that first check, any change of
/// the stored value ends the session, whatever the clocks say. A forced change (administrator
/// reset) does not rely on it: its flag refuses every command at once.
pub const PASSWORD_CHANGE_CLOCK_TOLERANCE: TimeDelta = TimeDelta::minutes(2);

/// Two readings of `password_changed_at` closer than this are the same date (a value received by
/// sync may come back with another precision or offset). Two real password changes are never
/// this close.
const SAME_PASSWORD_DATE: TimeDelta = TimeDelta::seconds(1);

/// What the stored rows say about an in-memory session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredSessionState {
    /// The account must change its password before anything else.
    pub must_change_password: bool,
}

/// Why [`enforce_session_alive`] ended a session. Logged and written to the audit trail, so a
/// session closed at once (owner test of 0.9.1: "Session expirée ou absente") says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEndReason {
    /// No `app_sessions` row for this session and user (fail closed).
    NoStoredRow,
    /// The row was closed: an administrator, a disable, a reset, a password change elsewhere.
    Revoked,
    AccountInactive,
    AccountDeleted,
    /// The row's `created_at` cannot be read: a session that cannot be dated is not trusted.
    Undatable,
    /// The password changed after the session started (on this device or another one).
    PasswordChanged,
}

impl SessionEndReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NoStoredRow => "no_stored_row",
            Self::Revoked => "revoked",
            Self::AccountInactive => "account_inactive",
            Self::AccountDeleted => "account_deleted",
            Self::Undatable => "undatable",
            Self::PasswordChanged => "password_changed",
        }
    }
}

/// What the account's `password_changed_at` says about a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasswordDateCheck {
    /// No change since the session started.
    Unchanged,
    /// The password changed after the session started.
    Changed,
    /// First check of the session: the date is later than this device's clock (a clock that ran
    /// ahead when it was written, here or on another device). It is not a change made after the
    /// session started; it becomes the session's reference value.
    AheadOfClock,
}

/// Parse a stored RFC 3339 instant (`app_sessions.created_at`, `password_changed_at`).
fn parse_instant(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw).ok().map(|t| t.with_timezone(&Utc))
}

/// Same stored `password_changed_at`: equal dates (within [`SAME_PASSWORD_DATE`]), or the same text
/// when one of them cannot be read as a date.
fn same_password_date(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (parse_instant(a), parse_instant(b)) {
            (Some(x), Some(y)) => (x - y).abs() < SAME_PASSWORD_DATE,
            _ => a == b,
        },
        _ => false,
    }
}

/// Did the password change after the session started?
/// - `seen`: the value this session read at its first check (`None` when not read yet). Once
///   read, any different stored value is a change, whatever the clocks say.
/// - First check: the stored date is compared with the session start (with
///   [`PASSWORD_CHANGE_CLOCK_TOLERANCE`]), except a date later than `now` (beyond the tolerance),
///   which a clock running ahead wrote: closing every new session on it locked the account out
///   for as long as the clock difference lasted.
pub(crate) fn password_date_check(
    seen: Option<Option<&str>>,
    stored: Option<&str>,
    session_created: DateTime<Utc>,
    now: DateTime<Utc>,
) -> PasswordDateCheck {
    if let Some(seen) = seen {
        return if same_password_date(seen, stored) {
            PasswordDateCheck::Unchanged
        } else {
            PasswordDateCheck::Changed
        };
    }
    match stored.and_then(parse_instant) {
        Some(changed) if changed > now + PASSWORD_CHANGE_CLOCK_TOLERANCE => PasswordDateCheck::AheadOfClock,
        Some(changed) if changed > session_created + PASSWORD_CHANGE_CLOCK_TOLERANCE => PasswordDateCheck::Changed,
        _ => PasswordDateCheck::Unchanged,
    }
}

/// The session is still alive in the database, or it is ended here and refused. Ended when:
/// - there is no stored session row for this user (fail closed: no row, no session);
/// - the row was closed (`app_sessions.is_revoked`: an administrator, a disable, a reset);
/// - the account was disabled or deleted (also by a change received from another device);
/// - the password changed after the session started (a reset or change made on any device:
///   `password_changed_at` travels with the account; [`password_date_check`]) — the row is
///   closed too.
///
/// Every end is logged and audited with its [`SessionEndReason`]. A password date later than this
/// device's clock is logged and audited once per session as a clock anomaly, and does not end it.
///
/// Used by `require_session!` (through [`enforce_stored_session`]) and by the change-password
/// command, which must work while a change is required but never on an ended session.
pub async fn enforce_session_alive(
    db: &DatabaseConnection,
    session: &tokio::sync::RwLock<SessionManager>,
    user: &AuthenticatedUser,
    session_db_id: Option<&str>,
) -> crate::errors::AppResult<StoredSessionState> {
    use crate::errors::AppError;
    let row = match session_db_id {
        None => None,
        Some(id) => {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT ua.is_active, ua.deleted_at, ua.force_password_change, ua.password_changed_at, \
                        s.is_revoked, s.created_at AS session_created_at \
                 FROM user_accounts ua \
                 INNER JOIN app_sessions s ON s.id = ? AND s.user_id = CAST(ua.id AS TEXT) \
                 WHERE ua.id = ?",
                [id.into(), user.user_id.into()],
            ))
            .await?
        }
    };
    // What this session read of the password date at its first check (same session only).
    let seen = {
        let guard = session.read().await;
        guard
            .current
            .as_ref()
            .filter(|s| Some(s.session_db_id.as_str()) == session_db_id)
            .and_then(|s| s.password_changed_at_seen.clone())
    };
    let mut password_ahead_of_clock = false;
    let mut stored_password_date: Option<String> = None;
    let end_reason = match &row {
        None => Some(SessionEndReason::NoStoredRow),
        Some(r) => {
            let created = r
                .try_get::<Option<String>>("", "session_created_at")
                .ok()
                .flatten()
                .as_deref()
                .and_then(parse_instant);
            stored_password_date = r.try_get::<Option<String>>("", "password_changed_at").ok().flatten();
            if seen.is_none()
                && stored_password_date
                    .as_deref()
                    .is_some_and(|raw| parse_instant(raw).is_none())
            {
                tracing::warn!(
                    user_id = user.user_id,
                    "unreadable password_changed_at: the session start is not compared with it"
                );
            }
            let password_check = created.map(|created| {
                password_date_check(
                    seen.as_ref().map(Option::as_deref),
                    stored_password_date.as_deref(),
                    created,
                    Utc::now(),
                )
            });
            password_ahead_of_clock = password_check == Some(PasswordDateCheck::AheadOfClock);
            if r.try_get::<i32>("", "is_revoked").unwrap_or(1) == 1 {
                Some(SessionEndReason::Revoked)
            } else if r.try_get::<i32>("", "is_active").unwrap_or(0) != 1 {
                Some(SessionEndReason::AccountInactive)
            } else if r.try_get::<Option<String>>("", "deleted_at").ok().flatten().is_some() {
                Some(SessionEndReason::AccountDeleted)
            } else if created.is_none() {
                Some(SessionEndReason::Undatable)
            } else if password_check == Some(PasswordDateCheck::Changed) {
                Some(SessionEndReason::PasswordChanged)
            } else {
                None
            }
        }
    };
    if let Some(reason) = end_reason {
        if reason == SessionEndReason::PasswordChanged {
            if let Some(id) = session_db_id {
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE app_sessions SET is_revoked = 1 WHERE id = ?",
                    [id.into()],
                ))
                .await?;
            }
        }
        let was_current = {
            let mut guard = session.write().await;
            let current = guard.current.as_ref().map(|s| s.session_db_id.as_str()) == session_db_id;
            if current {
                guard.clear_session();
            }
            current
        };
        tracing::warn!(
            user_id = user.user_id,
            session_id = session_db_id.unwrap_or("-"),
            reason = reason.code(),
            "session ended by the stored-state check"
        );
        if was_current {
            crate::audit::emit(
                db,
                crate::audit::AuditEvent {
                    event_type: crate::audit::event_type::SESSION_CLOSED,
                    actor_id: Some(user.user_id),
                    actor_name: Some(&user.username),
                    entity_type: Some("app_session"),
                    entity_id: session_db_id,
                    summary: "Session closed by the stored-state check",
                    detail_json: Some(serde_json::json!({ "reason": reason.code() }).to_string()),
                    ..Default::default()
                },
            )
            .await;
        }
        return Err(AppError::Auth(
            "Session fermée : le compte a été désactivé, le mot de passe a changé ou la session a été révoquée. \
             Veuillez vous reconnecter."
                .into(),
        ));
    }
    if seen.is_none() {
        // First check of a session that has no reference yet (sign-in records one): this value
        // is its reference from now on. Commands run at once: only the first one sets it.
        let recorded = {
            let mut guard = session.write().await;
            match guard
                .current
                .as_mut()
                .filter(|s| Some(s.session_db_id.as_str()) == session_db_id)
                .filter(|s| s.password_changed_at_seen.is_none())
            {
                Some(current) => {
                    current.password_changed_at_seen = Some(stored_password_date.clone());
                    true
                }
                None => false,
            }
        };
        if recorded && password_ahead_of_clock {
            audit_password_date_ahead(db, user, session_db_id, stored_password_date.as_deref()).await;
        }
    }
    Ok(StoredSessionState {
        must_change_password: row
            .as_ref()
            .and_then(|r| r.try_get::<i32>("", "force_password_change").ok())
            .unwrap_or(0)
            == 1,
    })
}

/// Checks of the stored state behind an in-memory session, run by `require_session!` on every
/// command:
/// - an ended session ([`enforce_session_alive`]) ends here and the command is refused;
/// - the account must change its password (`force_password_change`, set at creation or by an
///   administrator's reset, possibly on another device) → every command is refused with
///   `PASSWORD_CHANGE_REQUIRED`; only the change-password and sign-out commands (which do not
///   use `require_session!`) work. The in-memory flag is raised so the shell shows the
///   change-password screen, and lowered again once the change is no longer required (made on
///   another device).
pub async fn enforce_stored_session(
    db: &DatabaseConnection,
    session: &tokio::sync::RwLock<SessionManager>,
    user: &AuthenticatedUser,
    session_db_id: Option<&str>,
) -> crate::errors::AppResult<()> {
    let state = enforce_session_alive(db, session, user, session_db_id).await?;
    let mut guard = session.write().await;
    let same_session = guard.current.as_ref().map(|s| s.session_db_id.as_str()) == session_db_id;
    if state.must_change_password {
        if let Some(current) = guard.current.as_mut().filter(|_| same_session) {
            current.user.force_password_change = true;
        }
        return Err(crate::errors::AppError::PasswordChangeRequired(format!(
            "{PASSWORD_CHANGE_REQUIRED}: le mot de passe doit être changé avant de continuer."
        )));
    }
    if let Some(current) = guard.current.as_mut().filter(|_| same_session) {
        current.user.force_password_change = false;
    }
    Ok(())
}

/// A password change by the user, in one transaction with the session rotation: the new hash
/// (and, for a forced change, the cleared flag), a new stored row for the current session, and
/// every other stored session of the account closed (`id <> new`, other devices included). The
/// in-memory session is held while this runs and switches to the new row only once committed:
/// no window where the current session is closed, and never "password written but rotation
/// failed". [`enforce_session_alive`] then ends any session older than the password.
pub async fn change_password_and_rotate(
    db: &DatabaseConnection,
    session: &tokio::sync::RwLock<SessionManager>,
    user_id: i32,
    new_hash: &str,
    clear_forced_change: bool,
) -> crate::errors::AppResult<()> {
    use sea_orm::TransactionTrait;
    let mut guard = session.write().await;
    let Some(current) = guard.current.as_mut().filter(|s| s.user.user_id == user_id) else {
        return Err(crate::errors::AppError::Auth(
            "Session expirée ou absente. Veuillez vous reconnecter.".into(),
        ));
    };
    let now = Utc::now().to_rfc3339();
    let changed_at = now.clone();
    let new_id = Uuid::new_v4().to_string();
    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        if clear_forced_change {
            "UPDATE user_accounts SET password_hash = ?, force_password_change = 0, password_changed_at = ?, \
             updated_at = ? WHERE id = ?"
        } else {
            "UPDATE user_accounts SET password_hash = ?, password_changed_at = ?, updated_at = ? WHERE id = ?"
        },
        [new_hash.into(), now.clone().into(), now.clone().into(), user_id.into()],
    ))
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO app_sessions (id, user_id, created_at, expires_at, last_activity_at, is_revoked) \
         VALUES (?, ?, ?, ?, ?, 0)",
        [
            new_id.clone().into(),
            user_id.to_string().into(),
            now.clone().into(),
            current.expires_at.to_rfc3339().into(),
            now.into(),
        ],
    ))
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE app_sessions SET is_revoked = 1 WHERE user_id = CAST(? AS TEXT) AND id <> ? AND is_revoked = 0",
        [user_id.into(), new_id.clone().into()],
    ))
    .await?;
    tx.commit().await?;
    current.session_db_id = new_id;
    // This session made the change: the new date is its reference, not a change after its start.
    current.password_changed_at_seen = Some(Some(changed_at));
    if clear_forced_change {
        current.user.force_password_change = false;
        current.password_expires_in_days = None;
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

/// At sign-in, with the session lock held: record the login and write the session's stored row
/// before the new session is usable. If either write fails, the in-memory session is cleared, so
/// a session without its `app_sessions` row never exists in memory.
pub async fn store_session_or_clear(
    db: &DatabaseConnection,
    manager: &mut SessionManager,
    user_id: i32,
    session_db_id: &str,
    expires_at: &str,
) -> AppResult<()> {
    let stored = async {
        record_successful_login(db, user_id).await?;
        create_session_record(db, session_db_id, user_id, expires_at).await?;
        read_password_changed_at(db, user_id).await
    }
    .await;
    match stored {
        Ok(password_changed_at) => {
            // The session's reference for the stored-state check: any later change of the
            // account's password date ends it, whatever the clocks ([`password_date_check`]).
            let ahead = password_changed_at
                .as_deref()
                .and_then(parse_instant)
                .is_some_and(|changed| changed > Utc::now() + PASSWORD_CHANGE_CLOCK_TOLERANCE);
            let mut signed_in = None;
            if let Some(current) = manager.current.as_mut().filter(|s| s.session_db_id == session_db_id) {
                current.password_changed_at_seen = Some(password_changed_at.clone());
                signed_in = Some(current.user.clone());
            }
            // Written while the caller holds the session lock (sign-in is not usable before it
            // returns); the audit only uses the database, and only when the date is ahead.
            if let (true, Some(user)) = (ahead, signed_in) {
                audit_password_date_ahead(db, &user, Some(session_db_id), password_changed_at.as_deref()).await;
            }
            Ok(())
        }
        Err(e) => {
            manager.clear_session();
            Err(e)
        }
    }
}

/// The account's stored `password_changed_at` (`None` when the account has no date).
async fn read_password_changed_at(db: &DatabaseConnection, user_id: i32) -> AppResult<Option<String>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT password_changed_at FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<String>>("", "password_changed_at").ok().flatten()))
}

/// A password date later than this device's clock: logged and audited once per session (the
/// session is kept; the date is its reference).
async fn audit_password_date_ahead(
    db: &DatabaseConnection,
    user: &AuthenticatedUser,
    session_db_id: Option<&str>,
    password_changed_at: Option<&str>,
) {
    tracing::warn!(
        user_id = user.user_id,
        password_changed_at = password_changed_at.unwrap_or("-"),
        "password_changed_at is later than this device's clock: clock anomaly, the session is kept"
    );
    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::SESSION_CLOCK_ANOMALY,
            actor_id: Some(user.user_id),
            actor_name: Some(&user.username),
            entity_type: Some("app_session"),
            entity_id: session_db_id,
            summary: "Password date later than this device's clock; session kept",
            detail_json: Some(
                serde_json::json!({
                    "password_changed_at": password_changed_at,
                    "device_now": Utc::now().to_rfc3339(),
                })
                .to_string(),
            ),
            ..Default::default()
        },
    )
    .await;
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
    fn password_date_rule_first_check_and_later_checks() {
        let now = Utc::now();
        let created = now - TimeDelta::minutes(1);
        let at = |d: DateTime<Utc>| d.to_rfc3339();
        let past = at(now - TimeDelta::days(30));
        let ahead = at(now + TimeDelta::days(3));
        let after_start = at(created + TimeDelta::minutes(10));
        // First check (nothing read yet).
        assert_eq!(
            password_date_check(None, None, created, now),
            PasswordDateCheck::Unchanged
        );
        assert_eq!(
            password_date_check(None, Some(&past), created, now),
            PasswordDateCheck::Unchanged
        );
        assert_eq!(
            password_date_check(None, Some("not a date"), created, now),
            PasswordDateCheck::Unchanged
        );
        assert_eq!(
            password_date_check(None, Some(&ahead), created, now),
            PasswordDateCheck::AheadOfClock,
            "a date later than this device's clock does not close a new session"
        );
        assert_eq!(
            password_date_check(None, Some(&after_start), created, created + TimeDelta::minutes(11)),
            PasswordDateCheck::Changed,
            "a change after the session start, before its first check, still ends it"
        );
        // Later checks: any different stored value is a change, whatever the clocks.
        assert_eq!(
            password_date_check(Some(Some(&ahead)), Some(&ahead), created, now),
            PasswordDateCheck::Unchanged
        );
        assert_eq!(
            password_date_check(Some(Some(&ahead)), Some(&past), created, now),
            PasswordDateCheck::Changed
        );
        assert_eq!(
            password_date_check(Some(None), Some(&past), created, now),
            PasswordDateCheck::Changed
        );
        assert_eq!(
            password_date_check(Some(Some(&past)), None, created, now),
            PasswordDateCheck::Changed
        );
        assert_eq!(
            password_date_check(
                Some(Some("2026-01-05T08:00:00Z")),
                Some("2026-01-05T09:00:00.000+01:00"),
                created,
                now
            ),
            PasswordDateCheck::Unchanged,
            "the same date in another format"
        );
        assert_eq!(
            password_date_check(Some(Some("garbled")), Some("garbled"), created, now),
            PasswordDateCheck::Unchanged
        );
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

    /// A session whose last user input was `minutes_ago` minutes ago, with a 10-minute idle policy.
    fn session_idle_for(minutes_ago: i64) -> SessionManager {
        let mut mgr = SessionManager::new();
        mgr.create_session_with_timeouts(make_user(), SessionTimeouts::from_policy(10, 600, 15));
        mgr.current.as_mut().unwrap().last_activity_at = Utc::now() - TimeDelta::minutes(minutes_ago);
        mgr
    }

    #[test]
    fn commands_and_polls_do_not_count_as_activity() {
        let mut mgr = session_idle_for(9);
        let before = mgr.current.as_ref().unwrap().last_activity_at;
        // Every protected command (notification, presence, statistics polls) goes through this.
        for _ in 0..20 {
            let user = mgr.require_active_user().expect("still within the idle timeout");
            assert_eq!(user.username, "test_user");
        }
        assert_eq!(
            mgr.current.as_ref().unwrap().last_activity_at,
            before,
            "polls do not move activity"
        );

        // The idle deadline passes with polls still running: the session locks.
        mgr.current.as_mut().unwrap().last_activity_at = Utc::now() - TimeDelta::minutes(11);
        assert_eq!(
            mgr.require_active_user().unwrap_err(),
            RequireSessionFailure::IdleLocked
        );
        assert!(mgr.current.as_ref().unwrap().is_locked, "the lock is recorded");
        assert!(mgr.session_info().is_locked);
    }

    #[test]
    fn user_activity_before_the_deadline_extends_the_session() {
        let mut mgr = session_idle_for(9);
        let before = mgr.current.as_ref().unwrap().last_activity_at;
        mgr.record_activity().expect("within the idle timeout");
        let after = mgr.current.as_ref().unwrap().last_activity_at;
        assert!(after > before);
        assert!(Utc::now() - after < TimeDelta::seconds(5));
        assert!(mgr.is_authenticated());
    }

    #[test]
    fn user_activity_after_the_deadline_does_not_revive_the_session() {
        // The PC woke up after a long sleep: the first input must not reopen the session.
        let mut mgr = session_idle_for(45);
        let before = mgr.current.as_ref().unwrap().last_activity_at;
        assert_eq!(mgr.record_activity().unwrap_err(), RequireSessionFailure::IdleLocked);
        assert_eq!(mgr.current.as_ref().unwrap().last_activity_at, before, "not revived");
        assert!(mgr.current.as_ref().unwrap().is_locked);
        // Still locked at the next input, even if the clock went back meanwhile.
        mgr.current.as_mut().unwrap().last_activity_at = Utc::now();
        assert_eq!(mgr.record_activity().unwrap_err(), RequireSessionFailure::IdleLocked);
        assert_eq!(
            mgr.require_active_user().unwrap_err(),
            RequireSessionFailure::IdleLocked
        );
        assert!(!mgr.is_authenticated());

        // Only an unlock (after the password or PIN check) reopens it, with fresh activity.
        assert!(mgr.unlock_session());
        assert!(mgr.is_authenticated());
        let current = mgr.current.as_ref().unwrap();
        assert!(!current.is_locked);
        assert!(Utc::now() - current.last_activity_at < TimeDelta::seconds(5));
        mgr.record_activity().expect("active again");
    }

    #[test]
    fn record_activity_without_session_is_refused() {
        let mut mgr = SessionManager::new();
        assert_eq!(
            mgr.record_activity().unwrap_err(),
            RequireSessionFailure::MissingOrExpired
        );
        let mut expired = session_idle_for(0);
        expired.current.as_mut().unwrap().expires_at = Utc::now() - TimeDelta::minutes(1);
        assert_eq!(
            expired.record_activity().unwrap_err(),
            RequireSessionFailure::MissingOrExpired
        );
    }

    #[test]
    fn recording_the_idle_lock_keeps_the_pin_attempts() {
        // Wrong PINs tried against the time-based lock before anything recorded it.
        let mut mgr = session_idle_for(11);
        {
            let s = mgr.current.as_mut().unwrap();
            s.pin_failed_attempts = 3;
            s.pin_unlock_disabled = true;
        }
        assert!(mgr.require_active_user().is_err());
        assert!(mgr.record_activity().is_err());
        let s = mgr.current.as_ref().unwrap();
        assert!(s.is_locked);
        assert_eq!(s.pin_failed_attempts, 3, "no new PIN tries");
        assert!(s.pin_unlock_disabled, "PIN unlock stays disabled");
        // The unlock (password) starts them afresh.
        assert!(mgr.unlock_session());
        let s = mgr.current.as_ref().unwrap();
        assert_eq!(s.pin_failed_attempts, 0);
        assert!(!s.pin_unlock_disabled);
    }

    #[test]
    fn session_info_gives_the_idle_timeout() {
        let mgr = session_idle_for(0);
        assert_eq!(mgr.session_info().idle_timeout_minutes, Some(10));
        assert_eq!(SessionManager::new().session_info().idle_timeout_minutes, None);
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
