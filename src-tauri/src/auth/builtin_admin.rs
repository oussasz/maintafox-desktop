//! The built-in "admin" account (review E-1).
//!
//! Older versions seeded, on every install, an active administrator "admin" with a password fixed
//! in the source (`force_password_change = 1`: whoever signed in first chose the password). Now:
//! - **Release builds never create it.** The dev account and its password exist only in builds
//!   with `debug_assertions` (`cargo test`, `tauri dev`): a compile-time switch, so a release binary
//!   carries neither the seeding code nor the password ([`BuildProfile`], [`DEV_ADMIN_PASSWORD`]).
//!   On a new PC the only way in is the first-start "create the first administrator" screen.
//! - **The first-start screen closes for good** once an administrator has been created
//!   ([`first_admin_marker_closes`]): disabling the last administrator later does not reopen it.
//!   The marker is a `system_config` row naming the administrator's account; it stays valid as long
//!   as that account row exists (disabled or deleted included), so a local data reset (which wipes
//!   the accounts) reopens the screen, and nothing else does.
//! - **Existing PCs**: a built-in "admin" whose password was never changed (its forced change is
//!   still pending) is disabled, with an audit entry, as soon as another active administrator
//!   exists ([`retire_builtin_admin_if_superseded`]): at every start (`seeder::seed_system_data`),
//!   before every sign-in, when an administrator is created or given a role, and when the account
//!   arrives by synchronisation. In builds
//!   with `debug_assertions` it is deleted (soft delete) instead. While it is the only
//!   administrator it stays usable and the shell shows a warning to create a named administrator
//!   ([`get_builtin_admin_warning`]).
//! - **Synchronisation**: the account arriving from another PC is disabled on arrival when this PC
//!   has another administrator, or when this PC did not have it (a PC that never had the account
//!   does not gain a default-password administrator) ([`after_inbound_user_account`]).
//!
//! The account is recognised by its seeded identity (`sync::identity::seeded_entity_sync_id` of
//! `user_accounts` / `admin`) and its username, never by the username alone: a first administrator
//! someone names "admin" on the first-start screen is an ordinary account.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Serialize;
use tauri::State;

use crate::errors::{AppError, AppResult};
use crate::state::AppState;

/// Username of the built-in account.
pub const BUILTIN_ADMIN_USERNAME: &str = "admin";

/// Password of the dev account. Compiled only with `debug_assertions` (never in a release binary).
#[cfg(debug_assertions)]
pub const DEV_ADMIN_PASSWORD: &str = "Admin#2026!";

/// `system_config` key of the "an administrator has been created on this PC" marker.
pub const FIRST_ADMIN_MARKER_KEY: &str = "first_administrator_created";

/// Which kind of build is running: decides whether the dev account is seeded, and whether a
/// superseded built-in account is deleted (dev) or only disabled (release).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildProfile {
    /// `debug_assertions` on: `cargo test`, `tauri dev`.
    Development,
    /// `debug_assertions` off: `tauri build` (the release profile).
    Release,
}

impl BuildProfile {
    /// The profile of this binary (compile time).
    pub const fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Development
        } else {
            Self::Release
        }
    }

    /// Whether the system seed creates the dev "admin" account.
    pub const fn seeds_builtin_admin(self) -> bool {
        matches!(self, Self::Development)
    }

    /// Whether a superseded built-in account is deleted (soft delete) rather than only disabled.
    pub const fn deletes_superseded_builtin_admin(self) -> bool {
        matches!(self, Self::Development)
    }
}

/// Identity of the built-in account (same on every PC: `sync::seeded_identity`).
pub fn builtin_admin_entity_sync_id() -> String {
    crate::sync::identity::seeded_entity_sync_id("user_accounts", BUILTIN_ADMIN_USERNAME)
}

/// The built-in account as stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinAdminRow {
    pub user_id: i64,
    pub is_active: bool,
    pub deleted: bool,
    /// The forced first password change is still pending and the password was never changed
    /// (same rule as `product_license::count_tenant_administrators`): nobody chose its password.
    pub password_never_changed: bool,
}

impl BuiltinAdminRow {
    /// Active, not deleted, password never changed: the account to retire once superseded.
    pub fn is_default_password_account(&self) -> bool {
        self.is_active && !self.deleted && self.password_never_changed
    }
}

/// The built-in account of this PC, if it exists (any state).
pub async fn builtin_admin_row(conn: &impl ConnectionTrait) -> AppResult<Option<BuiltinAdminRow>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, is_active, deleted_at IS NOT NULL AS deleted,
                    (force_password_change = 1 AND password_changed_at IS NULL) AS never_changed
               FROM user_accounts WHERE username = ? AND entity_sync_id = ?",
            [BUILTIN_ADMIN_USERNAME.into(), builtin_admin_entity_sync_id().into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(BuiltinAdminRow {
        user_id: row.try_get("", "id")?,
        is_active: row.try_get::<i64>("", "is_active")? == 1,
        deleted: row.try_get::<i64>("", "deleted")? == 1,
        password_never_changed: row.try_get::<i64>("", "never_changed")? == 1,
    }))
}

/// Active accounts (not deleted, not disabled) other than `excluding_user_id` that hold
/// `adm.settings` through an assignment in force, whatever its scope: the same notion of
/// administrator as the first-start rule (`product_license::count_tenant_administrators`), by the
/// permission model (any role carrying the right), never a role name.
pub async fn other_active_administrator_exists(
    conn: &impl ConnectionTrait,
    excluding_user_id: Option<i64>,
) -> AppResult<bool> {
    Ok(first_active_administrator(conn, excluding_user_id).await?.is_some())
}

/// `entity_sync_id` of the oldest active administrator other than `excluding_user_id`.
async fn first_active_administrator(
    conn: &impl ConnectionTrait,
    excluding_user_id: Option<i64>,
) -> AppResult<Option<String>> {
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = crate::rbac::resolver::in_force_binds(&now).to_vec();
    values.push(crate::rbac::permissions::ADM_SETTINGS.into());
    values.push(excluding_user_id.unwrap_or(-1).into());
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ua.entity_sync_id AS sync_id
                   FROM user_accounts ua
                   JOIN user_scope_assignments usa ON usa.user_id = ua.id
                   JOIN role_permissions rp ON rp.role_id = usa.role_id
                   JOIN permissions p ON p.id = rp.permission_id
                  WHERE ua.deleted_at IS NULL AND ua.is_active = 1
                    AND {}
                    AND p.name = ?
                    AND ua.id <> ?
                    AND ua.entity_sync_id IS NOT NULL
                  ORDER BY ua.id
                  LIMIT 1",
                crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
            ),
            values,
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<String>("", "sync_id")).transpose()?)
}

/// Why the built-in account was retired (audit detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetireOrigin {
    /// Start of the app (also after an upgrade and after a local data reset).
    Startup,
    /// An administrator was just created (first-start screen, user administration).
    AdministratorCreated,
    /// The account arrived from another PC.
    SyncArrival,
    /// Someone is signing in (`commands::auth::login`, before the account is looked up).
    SignIn,
}

impl RetireOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::AdministratorCreated => "administrator_created",
            Self::SyncArrival => "sync_arrival",
            Self::SignIn => "sign_in",
        }
    }
}

/// What was done to the built-in account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetiredBuiltinAdmin {
    pub user_id: i64,
    /// Deleted (soft delete, dev builds) rather than only disabled.
    pub deleted: bool,
}

/// Audit action of a retirement (the account deactivation event of user administration).
const RETIRE_ACTION: &str = crate::audit::event_type::USER_DEACTIVATED;

fn retire_audit(
    retired: RetiredBuiltinAdmin,
    origin: RetireOrigin,
    reason: &str,
) -> crate::audit::writer::AuditEventInput {
    crate::audit::writer::AuditEventInput {
        action_code: RETIRE_ACTION.to_string(),
        target_type: Some("user_account".to_string()),
        target_id: Some(retired.user_id.to_string()),
        actor_id: None,
        auth_context: "system".to_string(),
        result: "success".to_string(),
        before_hash: None,
        after_hash: None,
        retention_class: "standard".to_string(),
        details_json: Some(serde_json::json!({
            "summary": "Built-in administrator account retired",
            "username": BUILTIN_ADMIN_USERNAME,
            "reason": reason,
            "origin": origin.as_str(),
            "deleted": retired.deleted,
        })),
    }
}

/// The account update itself (capture handling and audit are the caller's).
async fn write_retirement(
    conn: &impl ConnectionTrait,
    row: &BuiltinAdminRow,
    profile: BuildProfile,
) -> AppResult<RetiredBuiltinAdmin> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let deleted = profile.deletes_superseded_builtin_admin();
    // A deleted account keeps its deletion instant; a disabled one leaves `deleted_at` as it is.
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts
            SET is_active = 0, updated_at = ?, deleted_at = CASE WHEN ? = 1 THEN ? ELSE deleted_at END
          WHERE id = ?",
        [
            now.clone().into(),
            i32::from(deleted).into(),
            now.into(),
            row.user_id.into(),
        ],
    ))
    .await?;
    crate::auth::session_manager::revoke_user_sessions(conn, row.user_id).await?;
    Ok(RetiredBuiltinAdmin {
        user_id: row.user_id,
        deleted,
    })
}

const REASON_SUPERSEDED: &str = "another_administrator_exists";
const REASON_NOT_ON_THIS_PC: &str = "received_on_a_pc_without_it";

/// Disables (release) or deletes (dev) the built-in account when its password was never changed
/// and another active administrator exists; otherwise changes nothing (`None`). The change is a
/// local decision of this PC: written with synchronisation capture suppressed, so each PC applies
/// the rule itself and a PC whose only administrator is that account never loses it because
/// another PC has one. The audit entry is written once the change is committed.
pub async fn retire_builtin_admin_if_superseded(
    db: &DatabaseConnection,
    profile: BuildProfile,
    origin: RetireOrigin,
) -> AppResult<Option<RetiredBuiltinAdmin>> {
    let txn = db.begin().await?;
    let Some(row) = builtin_admin_row(&txn).await? else {
        return Ok(None);
    };
    if !row.is_default_password_account() || !other_active_administrator_exists(&txn, Some(row.user_id)).await? {
        return Ok(None);
    }
    crate::sync::capture::begin_suppression(&txn).await?;
    let retired = write_retirement(&txn, &row, profile).await;
    crate::sync::capture::end_suppression(&txn).await?;
    let retired = retired?;
    txn.commit().await?;
    crate::audit::writer::write_audit_event(db, retire_audit(retired, origin, REASON_SUPERSEDED)).await?;
    tracing::warn!(
        user_id = retired.user_id,
        deleted = retired.deleted,
        origin = origin.as_str(),
        "builtin_admin::retired"
    );
    Ok(Some(retired))
}

/// Before a sign-in is checked (`commands::auth::login`): a superseded built-in account is retired
/// first, so it can never open a session once another administrator exists, whichever way that
/// administrator arrived (synchronisation included). Fail-closed: an error refuses the sign-in.
pub async fn retire_before_sign_in(db: &DatabaseConnection) -> AppResult<()> {
    retire_builtin_admin_if_superseded(db, BuildProfile::current(), RetireOrigin::SignIn).await?;
    Ok(())
}

/// The account a sign-in checks (`commands::auth::login`): [`retire_before_sign_in`], then the
/// active account of that username (`session_manager::find_active_user`). A superseded built-in
/// account is therefore never found, so never signed in.
pub async fn find_sign_in_account(
    db: &DatabaseConnection,
    username: &str,
) -> AppResult<Option<(i32, String, Option<String>, bool, bool, Option<String>)>> {
    retire_before_sign_in(db).await?;
    crate::auth::session_manager::find_active_user(db, username).await
}

/// After an administrator was created or given a role (first-start screen, user administration),
/// once that change is committed: retires a superseded built-in account. The change itself
/// succeeded, so a failure here is logged and not returned; the account is retired at the next
/// sign-in attempt ([`retire_before_sign_in`]) or start ([`reconcile_at_startup`]).
pub async fn retire_after_administration_change(db: &DatabaseConnection) {
    if let Err(error) =
        retire_builtin_admin_if_superseded(db, BuildProfile::current(), RetireOrigin::AdministratorCreated).await
    {
        tracing::error!(%error, "builtin_admin::retire_after_administration_change_failed");
    }
}

/// Synchronisation hook (`sync::apply_hooks::after_upsert`, `user_accounts`), inside the apply
/// transaction and its capture suppression: when the row just applied is the built-in account with
/// its password never changed and still active, it is disabled (deleted in dev builds) if this PC
/// has another active administrator, or if the row was inserted (this PC did not have the account:
/// it does not gain an administrator whose password is known to everyone). A PC whose only
/// administrator is that very account keeps it (warning state, as at startup).
pub(crate) async fn after_inbound_user_account(
    conn: &impl ConnectionTrait,
    user_id: i64,
    inserted: bool,
    profile: BuildProfile,
) -> AppResult<()> {
    let Some(row) = builtin_admin_row(conn).await? else {
        return Ok(());
    };
    if row.user_id != user_id || !row.is_default_password_account() {
        return Ok(());
    }
    let reason = if other_active_administrator_exists(conn, Some(row.user_id)).await? {
        REASON_SUPERSEDED
    } else if inserted {
        REASON_NOT_ON_THIS_PC
    } else {
        return Ok(());
    };
    let retired = write_retirement(conn, &row, profile).await?;
    crate::audit::writer::write_audit_event_in(conn, retire_audit(retired, RetireOrigin::SyncArrival, reason)).await?;
    tracing::warn!(user_id, reason, "builtin_admin::retired_on_arrival");
    Ok(())
}

// ── "First administrator created" marker ─────────────────────────────────────────────────────────

/// Records, in the caller's transaction, that `admin_entity_sync_id` is an administrator created on
/// this PC: the first-start screen is closed from then on ([`first_admin_marker_closes`]). Keeps a
/// marker that is still valid; replaces one whose account no longer exists (local data reset).
pub async fn record_first_administrator_in(conn: &impl ConnectionTrait, admin_entity_sync_id: &str) -> AppResult<()> {
    if admin_entity_sync_id.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "The first administrator has no synchronisation identity.".into(),
        ]));
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO system_config (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
          WHERE NOT EXISTS (SELECT 1 FROM user_accounts u WHERE u.entity_sync_id = system_config.value)",
        [FIRST_ADMIN_MARKER_KEY.into(), admin_entity_sync_id.into(), now.into()],
    ))
    .await?;
    Ok(())
}

/// True once an administrator has been created on this PC: the marker exists and the account it
/// names is still stored (active, disabled or deleted alike). Disabling or deleting every
/// administrator does not reopen the first-start screen; a local data reset, which wipes the
/// accounts, does.
pub async fn first_admin_marker_closes(conn: &impl ConnectionTrait) -> AppResult<bool> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT EXISTS (
                 SELECT 1 FROM system_config c
                   JOIN user_accounts u ON u.entity_sync_id = c.value
                  WHERE c.key = ?
             ) AS closed",
            [FIRST_ADMIN_MARKER_KEY.into()],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "closed")).transpose()?.unwrap_or(0) == 1)
}

/// PCs that had administrators before the marker existed: records the oldest active administrator
/// as the marker's account. The built-in account counts only once its password has been changed
/// (an account with a password nobody chose is not an administrator someone created).
pub async fn backfill_first_admin_marker(conn: &impl ConnectionTrait) -> AppResult<bool> {
    if first_admin_marker_closes(conn).await? {
        return Ok(false);
    }
    let builtin = builtin_admin_row(conn).await?;
    let exclude = builtin
        .as_ref()
        .filter(|row| row.password_never_changed)
        .map(|row| row.user_id);
    let Some(sync_id) = first_active_administrator(conn, exclude).await? else {
        return Ok(false);
    };
    record_first_administrator_in(conn, &sync_id).await?;
    Ok(true)
}

/// At every start (`seeder::seed_system_data`): marker of PCs administered before it existed, then
/// retirement of a superseded built-in account.
pub async fn reconcile_at_startup(db: &DatabaseConnection, profile: BuildProfile) -> AppResult<()> {
    backfill_first_admin_marker(db).await?;
    retire_builtin_admin_if_superseded(db, profile, RetireOrigin::Startup).await?;
    Ok(())
}

/// A password chosen for the first administrator meets what every password set by an administrator
/// meets: the fixed minimum (`admin_users::validate_password_strength`) and the plant password
/// policy (Settings › Security).
pub async fn validate_first_admin_password(db: &DatabaseConnection, password: &str) -> AppResult<()> {
    crate::commands::admin_users::validate_password_strength(password)?;
    let policy = crate::auth::password_policy::PasswordPolicy::load(db).await;
    crate::auth::password_policy::validate_password_strength(password, &policy).map_err(AppError::ValidationFailed)
}

// ── Warning shown to the built-in account ────────────────────────────────────────────────────────

/// What the shell shows the signed-in user about the built-in account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BuiltinAdminWarning {
    /// The session in progress is the built-in account's.
    pub signed_in_as_builtin_admin: bool,
    /// Another active administrator exists on this PC.
    pub other_administrator_exists: bool,
    /// The built-in account's password was never changed.
    pub password_change_pending: bool,
}

pub(crate) async fn builtin_admin_warning_for(
    db: &DatabaseConnection,
    session_user_id: Option<i64>,
) -> AppResult<BuiltinAdminWarning> {
    let none = BuiltinAdminWarning {
        signed_in_as_builtin_admin: false,
        other_administrator_exists: false,
        password_change_pending: false,
    };
    let (Some(user_id), Some(row)) = (session_user_id, builtin_admin_row(db).await?) else {
        return Ok(none);
    };
    if row.user_id != user_id || row.deleted || !row.is_active {
        return Ok(none);
    }
    Ok(BuiltinAdminWarning {
        signed_in_as_builtin_admin: true,
        other_administrator_exists: other_active_administrator_exists(db, Some(row.user_id)).await?,
        password_change_pending: row.password_never_changed,
    })
}

/// The warning for the session of this PC. Answers also while the forced password change is
/// pending (it reads the in-memory session, not `require_session!`, which refuses that state), so
/// the change-password screen can show it; it says nothing about any other account.
#[tauri::command]
pub async fn get_builtin_admin_warning(state: State<'_, AppState>) -> AppResult<BuiltinAdminWarning> {
    let user_id = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .map(|session| i64::from(session.user.user_id));
    builtin_admin_warning_for(&state.db, user_id).await
}

#[cfg(test)]
#[path = "builtin_admin_tests.rs"]
mod tests;
