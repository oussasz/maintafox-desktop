//! The last active administrator (CMMS Advisor decision 2026-10-19, option (c)).
//!
//! The built-in "admin" account with a known password is gone (review E-1) and the first-start
//! "create the first administrator" screen closes for good once an administrator exists
//! (`auth::builtin_admin::first_admin_marker_closes`). So a plant whose administrators are all
//! disabled would be locked out. The rule:
//! - **Commands**: disabling an account, withdrawing or replacing a role assignment, or any change
//!   that would leave no active administrator is refused (coded [`LAST_ADMINISTRATOR_REQUIRED`]);
//!   the change and its ledger entries are rolled back ([`ensure_an_admin_remains`], run inside the
//!   change's transaction by `commands::admin_users`).
//! - **Synchronisation**: a received `user_accounts`, `user_scope_assignments` or `roles` row (or a
//!   delete of one) that would leave this PC without an active administrator is a sync conflict and
//!   is not applied ([`inbound_change_check`], from `sync::apply_hooks`). It is judged by the same
//!   count as the commands: the received change is written inside a savepoint, counted, and undone.
//! - **Lockout**: failed sign-ins lock the last administrator for at most
//!   [`LAST_ADMIN_MAX_LOCKOUT_MINUTES`]; a longer or unreadable stored lock is brought back to that
//!   bound ([`last_admin_lockout_minutes`], [`clamp_last_admin_lock`], used by `auth::lockout`).
//! - **Warning**: while the signed-in administrator is the only one, the shell asks for a second
//!   one ([`get_single_administrator_warning`]); two administrators can reset each other's password.
//!
//! "Active administrator" is the plant-administrator rule of `auth::rbac` (elevated admin access:
//! an active, not deleted account that is the bootstrap account or holds an administrator role
//! plant-wide through a permanent assignment in force — what `commands::admin_users` requires to
//! manage administrator accounts), minus the built-in "admin" account whose password was never
//! changed (`auth::builtin_admin::BuiltinAdminRow::password_never_changed`, the rule by which the
//! first-start code does not count it either): an account whose password nobody chose is not an
//! administrator someone can be trusted to be, so the last named administrator is never disabled in
//! its favour. An emergency elevation expires by itself and does not count.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;
use tauri::State;

use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::sync::materialize::ApplyOutcome;
use crate::sync::row_layout::EntityLayout;

/// Coded refusal / sync-conflict kind (translated under `settings:errors` and the sync conflict
/// kinds of the shell).
pub const LAST_ADMINISTRATOR_REQUIRED: &str = "LAST_ADMINISTRATOR_REQUIRED";

/// Longest failed-sign-in lock of the last active administrator, in minutes.
pub const LAST_ADMIN_MAX_LOCKOUT_MINUTES: i64 = 60;

/// Synchronised tables whose rows decide who is an administrator ([`inbound_change_check`]).
const GUARDED_ENTITY_TYPES: [&str; 3] = ["user_accounts", "user_scope_assignments", "roles"];

/// Accounts that are active administrators (see the module rule), by id.
pub async fn active_administrator_ids(conn: &impl ConnectionTrait) -> AppResult<Vec<i64>> {
    let builtin_never_changed = crate::auth::builtin_admin::builtin_admin_row(conn)
        .await?
        .filter(|row| row.password_never_changed)
        .map(|row| row.user_id);
    let mut ids = crate::auth::rbac::elevated_admin_user_ids(conn).await?;
    ids.retain(|id| Some(*id) != builtin_never_changed);
    Ok(ids)
}

/// Number of active administrators ([`active_administrator_ids`]).
pub async fn active_admin_count(conn: &impl ConnectionTrait) -> AppResult<i64> {
    Ok(i64::try_from(active_administrator_ids(conn).await?.len()).unwrap_or(i64::MAX))
}

/// True when `user_id` is the one and only active administrator.
pub async fn is_sole_active_administrator(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<bool> {
    Ok(active_administrator_ids(conn).await? == [user_id])
}

/// After a change, in its transaction: it took away the last active administrator (there were
/// `before` of them). A plant that had none before is not judged here (first start, or only the
/// built-in account whose password was never changed).
pub async fn leaves_no_administrator(conn: &impl ConnectionTrait, before: i64) -> AppResult<bool> {
    Ok(before > 0 && active_admin_count(conn).await? == 0)
}

const REFUSAL_MESSAGE: &str = "This change would leave the plant without an active administrator: create or \
                               enable a second administrator first.";

/// The refusal of a change that would leave no active administrator (coded).
pub fn last_administrator_refusal() -> AppError {
    crate::errors::org_validation_failed(LAST_ADMINISTRATOR_REQUIRED, REFUSAL_MESSAGE, HashMap::new())
}

/// Run inside the transaction of a change, after it: refuses (the caller's transaction is then
/// dropped, i.e. rolled back) when the change took away the last active administrator.
pub async fn ensure_an_admin_remains(conn: &impl ConnectionTrait, before: i64) -> AppResult<()> {
    if leaves_no_administrator(conn, before).await? {
        return Err(last_administrator_refusal());
    }
    Ok(())
}

// ── Synchronisation ──────────────────────────────────────────────────────────────────────────────

tokio::task_local! {
    /// Set while a received change is being tried out ([`inbound_change_check`]): the nested apply
    /// does not try it out again.
    static PROBING_INBOUND_CHANGE: ();
}

const PROBE_SAVEPOINT: &str = "last_admin_probe";

/// A received change of a guarded row ([`inbound_change_check`]).
#[derive(Debug, Clone, Copy)]
pub(crate) enum InboundChange<'a> {
    /// Insert or update, with the received row (`payload_json`).
    Upsert(&'a str),
    /// Delete of the row.
    Delete,
}

/// Received change of a `user_accounts`, `user_scope_assignments` or `roles` row: `Some(conflict)`
/// when applying it would leave this PC without an active administrator while it has one. The
/// change is written inside a savepoint (an upsert by the normal apply, `sync::row_codec::apply_row`;
/// a delete as that apply deletes, by `entity_sync_id`), counted with [`leaves_no_administrator`],
/// and always undone; the caller then applies it for real, or records the conflict. A row this PC
/// does not have cannot take an administrator away and is not tried out. A change that the apply
/// itself refuses or fails is left to the real apply, which refuses or fails the same way.
pub(crate) async fn inbound_change_check(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    change: InboundChange<'_>,
) -> AppResult<Option<ApplyOutcome>> {
    if !GUARDED_ENTITY_TYPES.contains(&layout.def.entity_type) || PROBING_INBOUND_CHANGE.try_with(|_| ()).is_ok() {
        return Ok(None);
    }
    let before = active_admin_count(conn).await?;
    if before == 0 || !stored_row_exists(conn, layout.table(), entity_sync_id).await? {
        return Ok(None);
    }
    conn.execute_unprepared(&format!("SAVEPOINT {PROBE_SAVEPOINT}")).await?;
    let written = match change {
        InboundChange::Upsert(payload_json) => PROBING_INBOUND_CHANGE
            .scope(
                (),
                Box::pin(crate::sync::row_codec::apply_row(
                    conn,
                    layout,
                    entity_sync_id,
                    "upsert",
                    payload_json,
                )),
            )
            .await
            .is_ok_and(|outcome| matches!(outcome, ApplyOutcome::Applied)),
        InboundChange::Delete => conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("DELETE FROM \"{}\" WHERE entity_sync_id = ?", layout.table()),
                [entity_sync_id.into()],
            ))
            .await
            .is_ok_and(|done| done.rows_affected() > 0),
    };
    let verdict = if written {
        leaves_no_administrator(conn, before).await
    } else {
        Ok(false)
    };
    conn.execute_unprepared(&format!("ROLLBACK TO SAVEPOINT {PROBE_SAVEPOINT}"))
        .await?;
    conn.execute_unprepared(&format!("RELEASE SAVEPOINT {PROBE_SAVEPOINT}"))
        .await?;
    // References resolved during the trial may name rows the rollback removed.
    crate::sync::row_codec::clear_ref_cache();
    if !verdict? {
        return Ok(None);
    }
    tracing::warn!(
        entity_type = layout.def.entity_type,
        entity_sync_id,
        "last_admin::inbound_change_refused"
    );
    Ok(Some(ApplyOutcome::Conflict {
        code: LAST_ADMINISTRATOR_REQUIRED.into(),
        message: format!(
            "{}: this change would leave this PC without an active administrator; it is not applied.",
            layout.def.entity_type
        ),
    }))
}

async fn stored_row_exists(conn: &impl ConnectionTrait, table: &str, entity_sync_id: &str) -> AppResult<bool> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT EXISTS (SELECT 1 FROM \"{table}\" WHERE entity_sync_id = ?) AS found"),
            [entity_sync_id.into()],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "found")).transpose()?.unwrap_or(0) == 1)
}

// ── Lockout ──────────────────────────────────────────────────────────────────────────────────────

/// Lock duration for `user_id` after failed sign-ins: the policy's `policy_minutes`, at most
/// [`LAST_ADMIN_MAX_LOCKOUT_MINUTES`] for the last active administrator.
pub async fn last_admin_lockout_minutes(
    conn: &impl ConnectionTrait,
    user_id: i64,
    policy_minutes: i64,
) -> AppResult<i64> {
    if is_sole_active_administrator(conn, user_id).await? {
        return Ok(policy_minutes.clamp(1, LAST_ADMIN_MAX_LOCKOUT_MINUTES));
    }
    Ok(policy_minutes)
}

/// A running lock of `user_id` ending at `locked_until`: for the last active administrator, a lock
/// ending later than [`LAST_ADMIN_MAX_LOCKOUT_MINUTES`] from now, or unreadable, is brought back to
/// that bound (stored); returns the end that applies. Any other account keeps its lock as stored.
pub async fn clamp_last_admin_lock(conn: &impl ConnectionTrait, user_id: i64, locked_until: &str) -> AppResult<String> {
    let bound = chrono::Utc::now() + chrono::Duration::minutes(LAST_ADMIN_MAX_LOCKOUT_MINUTES);
    let within_bound = stored_instant(locked_until).is_some_and(|until| until <= bound);
    if within_bound || !is_sole_active_administrator(conn, user_id).await? {
        return Ok(locked_until.to_string());
    }
    let bounded = bound.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET locked_until = ?, updated_at = ? WHERE id = ?",
        [
            bounded.clone().into(),
            chrono::Utc::now().to_rfc3339().into(),
            user_id.into(),
        ],
    ))
    .await?;
    tracing::warn!(user_id, "last_admin::lock_bounded");
    Ok(bounded)
}

/// A stored lock end: RFC 3339 (`auth::lockout`), or SQLite `datetime()` text in UTC
/// (`session_manager::record_failed_login`).
fn stored_instant(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = raw.trim();
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|t| t.and_utc())
        })
}

// ── Warning ──────────────────────────────────────────────────────────────────────────────────────

/// What the shell shows the signed-in user about the number of administrators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SingleAdministratorWarning {
    /// The signed-in account is the only active administrator: a second one should be created.
    pub sole_active_administrator: bool,
}

pub(crate) async fn single_administrator_warning_for(
    conn: &impl ConnectionTrait,
    session_user_id: Option<i64>,
) -> AppResult<SingleAdministratorWarning> {
    let sole_active_administrator = match session_user_id {
        Some(user_id) => is_sole_active_administrator(conn, user_id).await?,
        None => false,
    };
    Ok(SingleAdministratorWarning {
        sole_active_administrator,
    })
}

/// The warning for the session of this PC (read like `builtin_admin::get_builtin_admin_warning`:
/// also while a forced password change is pending); it says nothing about any other account.
#[tauri::command]
pub async fn get_single_administrator_warning(state: State<'_, AppState>) -> AppResult<SingleAdministratorWarning> {
    let user_id = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .map(|session| i64::from(session.user.user_id));
    single_administrator_warning_for(&state.db, user_id).await
}

#[cfg(test)]
#[path = "last_admin_tests.rs"]
mod tests;
