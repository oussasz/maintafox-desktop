//! Access grants received by synchronisation are re-checked against the person who made them
//! (security item S3).
//!
//! An administrator whose right to manage users or roles was withdrawn on PC B can still, on a PC A
//! that has not received the withdrawal, give roles or permissions. Before a received grant is
//! written ([`inbound_grant_check`], from `sync::apply_hooks::before_upsert_checked`, before the
//! last-administrator probe), the person the row names as its author must hold the right to make
//! it **here, now** (the rule of `settings::site::holds_tenant_right`: an active account, through a
//! permanent plant-wide role in force, emergency grants excluded, or a plant administrator):
//! - `user_scope_assignments` new here, or changed in role, scope or validity (widened), made
//!   permanent from an emergency, or restored (`deleted_at` cleared): `assigned_by_id` holds
//!   `adm.users`; an administrator role (Superadmin / Administrator) also needs a plant
//!   administrator, and nobody grants a role to their own account (`commands::admin_users`,
//!   `ensure_may_grant_role`);
//! - `role_permissions` new here: `granted_by_id` holds `adm.roles` (a plant administrator for an
//!   administrator role);
//! - `user_accounts` (the row never names who changed it, [`account_check`]): the bootstrap flag
//!   (`is_admin`) set on an account held here, a new account with it while this PC has its first
//!   administrator, or an account holding a right to give access re-enabled or restored: refused.
//!   A password change and a re-enable of any other account name their author and are judged by
//!   [`author_check`] too (`sync::credential_check`, which holds back only those keys).
//!
//! An author who does not hold the right now (lead decision on review BLOCKING 1; role permissions
//! are deleted, so a past set of rights cannot be rebuilt, and the row's times come from the
//! sender's clock):
//! - when this PC knows when the author lost it ([`loss_time`]: the end of the assignment that gave
//!   it, the account disabled or deleted), a grant dated before that time minus
//!   `settings::site::MAX_CLOCK_SKEW_MINUTES` is applied (it was made while the author held it); a
//!   grant dated after, or whose date cannot be read, is a [`GRANT_BY_UNAUTHORISED_ADMIN`] conflict;
//! - when this PC cannot tell (typically the first pull of a new PC, or a role permission deleted),
//!   the grant is applied and listed in ONE review notice to the administrators for the run
//!   ([`with_review_collection`]): never blocked, never applied silently.
//!
//! A conflict is decided by an administrator in the Sync centre; "take the received version" goes
//! through this check again (it never bypasses it). An author not applied here yet makes the row
//! wait for that account, as for any missing parent; so do rights of the author still waiting in
//! the inbox (the precedent of `qualification::sync_check`,
//! `permit::sync_check::rights_may_still_arrive`). Those waits end with the next complete pull
//! ([`release_after_complete_pull`]): nothing more can arrive, the grant is judged as above.
//!
//! Rows without an author that this check accepts: an assignment to an account holding the
//! bootstrap flag (the first administrator, `product_license`; the dev seeder's built-in account),
//! which the flag makes a plant administrator anyway; the Administrator role given by a vendor
//! recovery code (`auth::recovery_code`, its `admin_recovery.used` audit record here; waiting while
//! that record is in the inbox); a permission of a system role (migrations and the seeder grant
//! them on every PC). Any other new grant without an author is a conflict. The recovery-code
//! exception trusts a synchronised audit row: the trust model of sync is that activated PCs are
//! honest replicas (this check stops a demoted administrator using the app, not a forged peer).
//!
//! Revocations and deletes are never checked: they only take access away.
//!
//! **Convergence on the PC that made the grant** ([`after_rights_change`]): when a received change
//! takes `adm.users` or `adm.roles` away from someone here (a role permission removed, an
//! assignment ended, an account disabled or deleted), the grants this database holds that this
//! person made from the time of the withdrawal (as the received row records it, minus
//! `settings::site::MAX_CLOCK_SKEW_MINUTES`; every live grant of theirs when the row gives no time,
//! a delete) are listed in ONE review notice to the administrators
//! (`settings::received_notice::send_grant_review_notice`). Nothing is ended or deleted silently.
//!
//! A delegation policy (`delegated_admin_policies`) created or widened elsewhere names who did it
//! (`changed_by_id`, migration 262) and is judged like a role permission, against `adm.roles`
//! ([`delegation_policy_check`]). One that names nobody (made before 0.9.2) is applied and listed in
//! the review notice while this PC has not finished its first complete pull (the server already
//! held it), and is a conflict for an administrator afterwards. A change that only narrows a held
//! policy (domains removed, step-up required) is applied.

use std::collections::BTreeSet;

use chrono::Utc;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};
use crate::permit::sync_check::{field, Payload};
use crate::rbac::permissions::{ADM_ROLES, ADM_USERS};
use crate::sync::materialize::{ApplyOutcome, WaitsFor, DEFER_MISSING_PARENT};
use crate::sync::row_layout::EntityLayout;

/// Conflict kind of a grant whose author does not hold the right to make it here.
pub const GRANT_BY_UNAUTHORISED_ADMIN: &str = "GRANT_BY_UNAUTHORISED_ADMIN";

/// Roles only a plant administrator grants (`commands::admin_users::ADMIN_ROLE_NAMES`).
const ADMIN_ROLE_NAMES: [&str; 2] = ["Superadmin", "Administrator"];

/// The rights that let someone give access ([`after_rights_change`]).
const GRANTING_RIGHTS: [&str; 2] = [ADM_USERS, ADM_ROLES];

/// Tables whose rows decide who holds [`GRANTING_RIGHTS`].
const RIGHTS_ENTITY_TYPES: [&str; 4] = ["user_accounts", "user_scope_assignments", "role_permissions", "roles"];

/// Grants listed in a review notice; the others are counted.
const NOTICE_MAX_LINES: usize = 20;

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("grant re-check of a received row: {e}"))
}

pub(crate) fn truthy(p: &Payload, key: &str) -> bool {
    match p.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        Some(Value::String(s)) => s.trim() == "1" || s.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn conflict(message: String) -> Option<ApplyOutcome> {
    Some(ApplyOutcome::Conflict {
        code: GRANT_BY_UNAUTHORISED_ADMIN.into(),
        message,
    })
}

fn wait_for(entity_type: &str, sync_id: &str, message: String) -> Option<ApplyOutcome> {
    Some(ApplyOutcome::Deferred {
        code: DEFER_MISSING_PARENT.into(),
        message,
        waits_for: Some(WaitsFor {
            entity_type: entity_type.to_string(),
            sync_id: sync_id.to_string(),
        }),
    })
}

pub(crate) fn wait(message: String) -> Option<ApplyOutcome> {
    Some(ApplyOutcome::Deferred {
        code: DEFER_MISSING_PARENT.into(),
        message,
        waits_for: None,
    })
}

/// Local id of a row named by its sync id; `None` when not here.
pub(crate) async fn local_id(conn: &impl ConnectionTrait, table: &str, sync_id: &str) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT id FROM \"{table}\" WHERE entity_sync_id = ?"),
        [sync_id.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "id").map_err(err))
    .transpose()
}

/// A plant administrator here, now (`auth::rbac::user_has_elevated_admin_access`).
pub(crate) async fn is_plant_admin(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<bool> {
    let Ok(id) = i32::try_from(user_id) else {
        return Ok(false);
    };
    crate::auth::rbac::user_has_elevated_admin_access(conn, id).await
}

/// `user_id` may use `permission` to give access here, now: a plant administrator, or a holder of
/// it plant-wide through a permanent role in force (`settings::site::holds_tenant_right`).
pub(crate) async fn holds(conn: &impl ConnectionTrait, user_id: i64, permission: &str) -> AppResult<bool> {
    Ok(is_plant_admin(conn, user_id).await?
        || crate::settings::site::holds_tenant_right(conn, user_id, permission).await?)
}

/// Rows that would make `user_sync_id` a plant administrator still wait in the inbox: an account row
/// with the bootstrap flag, or a plant-wide permanent assignment to an administrator role.
async fn admin_rights_may_still_arrive(conn: &impl ConnectionTrait, user_sync_id: &str) -> AppResult<bool> {
    const WAITING: &str = "apply_status IN ('pending', 'deferred') AND operation NOT IN ('delete', 'archive')";
    let placeholders = vec!["?"; ADMIN_ROLE_NAMES.len()].join(", ");
    let mut values: Vec<sea_orm::Value> = vec![user_sync_id.into(), user_sync_id.into()];
    values.extend(ADMIN_ROLE_NAMES.iter().map(|n| (*n).into()));
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS w FROM sync_inbox \
                  WHERE {WAITING} AND ( \
                    (entity_type = 'user_accounts' AND entity_sync_id = ? \
                       AND json_extract(payload_json, '$.is_admin') IN (1, '1', 'true')) \
                    OR (entity_type = 'user_scope_assignments' \
                       AND json_extract(payload_json, '$.user_sync_id') = ? \
                       AND json_extract(payload_json, '$.scope_type') = 'tenant' \
                       AND COALESCE(json_extract(payload_json, '$.is_emergency'), 0) IN (0, '0', 'false') \
                       AND json_extract(payload_json, '$.deleted_at') IS NULL \
                       AND json_extract(payload_json, '$.role_sync_id') IN ( \
                         SELECT entity_sync_id FROM roles WHERE name IN ({placeholders})))) \
                  LIMIT 1"
            ),
            values,
        ))
        .await?
        .is_some())
}

// ── Grants accepted for review ─────────────────────────────────────────────────────────────────

/// A grant applied although its author does not hold the right here now, for the review notice:
/// one readable line (what, to whom, by whom, the day it was made).
#[derive(Debug, Clone)]
pub(crate) struct ReviewItem {
    /// The row the notice is about (`notification_events` source).
    pub(crate) entity_type: &'static str,
    pub(crate) source: String,
    pub(crate) line: String,
}

/// The day of a timestamp (`YYYY-MM-DD`) for the lines of a notice; "?" when unreadable.
pub(crate) fn day(raw: Option<&str>) -> String {
    raw.and_then(crate::settings::site::instant)
        .map_or_else(|| "?".to_string(), |t| t.format("%Y-%m-%d").to_string())
}

/// Message key of the review notice of grants applied from authors without the right.
const REVIEW_MESSAGE_KEY: &str = "grants_from_former_authors";

tokio::task_local! {
    /// The review notice of the current materialization run ([`with_review_collection`]): its
    /// event, once one grant was listed, so the next grants of the run join the same notice.
    static RUN_NOTICE: std::sync::Mutex<Option<i64>>;
    /// Set while the rows still waiting are applied after a complete pull
    /// ([`release_after_complete_pull`]); `true` in its last pass, when nothing waits any longer.
    static CAUGHT_UP: bool;
}

/// After a complete pull: an author who never arrived will not arrive.
fn caught_up() -> bool {
    CAUGHT_UP.try_with(|_| ()).is_ok()
}

/// The last pass after a complete pull: rights still waiting in the inbox will not apply either.
fn final_pass() -> bool {
    CAUGHT_UP.try_with(|f| *f).unwrap_or(false)
}

fn review_texts(lines: &[String]) -> (String, String) {
    let shown: Vec<&str> = lines.iter().take(NOTICE_MAX_LINES).map(String::as_str).collect();
    let more = lines.len().saturating_sub(shown.len());
    let list = if more > 0 {
        format!("{}; … (+{more})", shown.join("; "))
    } else {
        shown.join("; ")
    };
    let body = format!(
        "Reçu par synchronisation et appliqué (ce poste ne sait pas quand l'auteur a perdu le droit) : {list}. / \
         Received by sync and applied (this PC does not know when the author lost the right): {list}."
    );
    (list, body)
}

/// Adds a line to the notice of this run (`event_id`); `false` when that notice is not here (its
/// row was rolled back with the grant that raised it).
async fn append_to_notice(conn: &impl ConnectionTrait, event_id: i64, line: &str) -> AppResult<bool> {
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT message_params_json FROM notification_events WHERE id = ? AND message_key = ?",
            [event_id.into(), REVIEW_MESSAGE_KEY.into()],
        ))
        .await?
    else {
        return Ok(false);
    };
    let params: Option<String> = row.try_get("", "message_params_json").map_err(err)?;
    let mut lines: Vec<String> = params
        .as_deref()
        .and_then(|p| serde_json::from_str::<Value>(p).ok())
        .and_then(|p| p.get("lines").cloned())
        .and_then(|l| serde_json::from_value(l).ok())
        .unwrap_or_default();
    lines.push(line.to_string());
    let (list, body) = review_texts(&lines);
    let params = json!({ "grants": list, "count": lines.len(), "lines": lines }).to_string();
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notification_events SET message_params_json = ? WHERE id = ?",
        [params.into(), event_id.into()],
    ))
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications SET body = ? WHERE notification_event_id = ?",
        [body.into(), event_id.into()],
    ))
    .await?;
    Ok(true)
}

/// Lists a grant in the review notice, in the transaction of its apply: the notice of this run when
/// there is one, a new one otherwise. A grant whose apply is undone (waiting, refused, failed) takes
/// its line with it, so nothing is listed twice or for a grant not applied.
async fn record_for_review(conn: &impl ConnectionTrait, item: &ReviewItem) -> AppResult<()> {
    let run_event = RUN_NOTICE.try_with(|n| n.lock().ok().and_then(|g| *g)).ok().flatten();
    if let Some(event_id) = run_event {
        if append_to_notice(conn, event_id, &item.line).await? {
            return Ok(());
        }
    }
    let lines = vec![item.line.clone()];
    let (list, body) = review_texts(&lines);
    crate::settings::received_notice::send_grant_review_notice(
        conn,
        item.entity_type,
        &item.source,
        "Accès reçus d'un auteur qui n'a plus le droit : à revoir / Grants received from an author who no longer \
         holds the right: to review"
            .to_string(),
        body,
        crate::notifications::emitter::NotificationMessage::new(
            REVIEW_MESSAGE_KEY,
            json!({ "grants": list, "count": 1, "lines": lines }),
        ),
        true,
    )
    .await?;
    let event_id = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT MAX(id) AS id FROM notification_events WHERE message_key = ?",
            [REVIEW_MESSAGE_KEY.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "id").ok().flatten());
    // Outside a run (a conflict taken from the Sync centre) there is nothing to remember.
    let _ = RUN_NOTICE.try_with(|n| {
        if let Ok(mut g) = n.lock() {
            *g = event_id;
        }
    });
    Ok(())
}

/// Accepts a grant whose author does not hold the right here now and whose loss of it is not known
/// here (see the module rule): never silently. It is listed in the administrators' review notice in
/// the transaction of its apply; when the notice cannot be written, the grant is not applied: it
/// waits and is tried again at the next synchronisation (fail closed).
pub(crate) async fn accept_for_review(
    conn: &impl ConnectionTrait,
    item: ReviewItem,
) -> AppResult<Option<ApplyOutcome>> {
    conn.execute_unprepared("SAVEPOINT grant_review").await?;
    match record_for_review(conn, &item).await {
        Ok(()) => {
            conn.execute_unprepared("RELEASE grant_review").await?;
            Ok(None)
        }
        Err(e) => {
            conn.execute_unprepared("ROLLBACK TO grant_review").await?;
            conn.execute_unprepared("RELEASE grant_review").await?;
            tracing::error!(error = %e, source = %item.source, "grant for review: notice not written, the grant waits");
            Ok(wait(format!(
                "{}: the administrators' review notice could not be written; tried again later",
                item.source
            )))
        }
    }
}

/// Runs a materialization (`sync::materialize::materialize_inbox`) so that the grants it applies for
/// review are listed in ONE notice for the run ([`record_for_review`]).
pub(crate) async fn with_review_collection<T>(run: impl std::future::Future<Output = AppResult<T>>) -> AppResult<T> {
    // Already within a run (the passes after a complete pull): the same notice.
    if RUN_NOTICE.try_with(|_| ()).is_ok() {
        return run.await;
    }
    RUN_NOTICE.scope(std::sync::Mutex::new(None), run).await
}

/// After a complete pull (`sync::transport::worker`: the exchanges reached the server head, every
/// received row materialized): the grants still waiting are applied again knowing that nothing more
/// will arrive, so none waits for ever:
/// - first until nothing more applies, an author never received being judged as one whose loss is
///   not known here (applied, listed for review), while rights still waiting in the inbox keep their
///   grants waiting: a grant by B then waits for B's own role if that role can still apply;
/// - then once more with those waits ended: each grant is decided ([`author_check`]).
pub(crate) async fn release_after_complete_pull(db: &sea_orm::DatabaseConnection) -> AppResult<()> {
    let waiting = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            // Accounts too: a password change or a re-enable waits for its author
            // (`sync::credential_check`).
            "SELECT 1 AS w FROM sync_inbox WHERE apply_status = 'deferred' \
               AND entity_type IN ('user_scope_assignments', 'role_permissions', 'user_accounts') LIMIT 1"
                .to_string(),
        ))
        .await?
        .is_some();
    if !waiting {
        return Ok(());
    }
    // `materialize_inbox` sweeps the waiting rows until a sweep applies nothing: a fixpoint. Both
    // passes share one review notice.
    with_review_collection(async {
        CAUGHT_UP
            .scope(false, crate::sync::materialize::materialize_inbox(db, 200, true))
            .await?;
        CAUGHT_UP
            .scope(true, crate::sync::materialize::materialize_inbox(db, 200, true))
            .await?;
        AppResult::Ok(())
    })
    .await
}

/// When an author's account stopped giving its rights ([`loss_time`]).
#[derive(Debug, Clone, Copy)]
enum AccountLoss {
    Active,
    At(chrono::DateTime<Utc>),
    Unknown,
}

/// What this PC knows of the loss of a right by an author who does not hold it now ([`loss_time`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Loss {
    /// Lost at this time.
    At(chrono::DateTime<Utc>),
    /// Held through a role that does not carry it any more (a role permission deleted, which
    /// leaves no trace), or through the bootstrap flag removed: when is not known here.
    Unknown,
    /// Never held here: no assignment (live or ended) could have given it, and no bootstrap flag.
    /// Assignments are ended, never deleted, by the commands (`commands::admin_users`).
    Never,
}

/// When `author` lost the right to make this grant, as this PC knows it: the end of the last
/// plant-wide permanent assignment that gave it (ended, or past its end), or the account disabled
/// or deleted (its `deleted_at`, else `updated_at`), whichever came first. `admin_only`: the right
/// is plant administration (an administrator role), else `permission` (an administrator role or a
/// role carrying it).
async fn loss_time(conn: &impl ConnectionTrait, author: i64, permission: &str, admin_only: bool) -> AppResult<Loss> {
    let instant = |raw: Option<String>| raw.as_deref().and_then(crate::settings::site::instant);
    let Some(account) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active, is_admin, deleted_at, updated_at FROM user_accounts WHERE id = ?",
            [author.into()],
        ))
        .await?
    else {
        return Ok(Loss::Never);
    };
    let deleted_at: Option<String> = account.try_get("", "deleted_at").map_err(err)?;
    let is_admin = account.try_get::<i64>("", "is_admin").map_err(err)? == 1;
    let active = account.try_get::<i64>("", "is_active").map_err(err)? == 1 && deleted_at.is_none();
    // When the account stopped giving its rights: deleted (`deleted_at`), or disabled at the time
    // its ledger records (`admin_change_events`, `user_deactivated`, synchronised with it). Never
    // `updated_at`: any later edit of the account moves it. Disabled with no such record here: not
    // known.
    let account_lost = if active {
        AccountLoss::Active
    } else if let Some(t) = instant(deleted_at) {
        AccountLoss::At(t)
    } else {
        let disabled: Option<String> = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT MAX(acted_at) AS t FROM admin_change_events \
                  WHERE action = 'user_deactivated' AND target_user_id = ? AND apply_result = 'applied'",
                [author.into()],
            ))
            .await?
            .and_then(|r| r.try_get::<Option<String>>("", "t").ok().flatten());
        instant(disabled).map_or(AccountLoss::Unknown, AccountLoss::At)
    };
    let placeholders = vec!["?"; ADMIN_ROLE_NAMES.len()].join(", ");
    let mut values: Vec<sea_orm::Value> = Vec::new();
    values.extend(ADMIN_ROLE_NAMES.iter().map(|n| (*n).into()));
    values.push(i64::from(admin_only).into());
    values.push(permission.into());
    values.push(author.into());
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT usa.deleted_at, usa.valid_from, usa.valid_to, \
                        CASE WHEN r.name IN ({placeholders}) OR (? = 0 AND EXISTS ( \
                          SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id \
                           WHERE rp.role_id = r.id AND p.name = ?)) THEN 1 ELSE 0 END AS gives \
                   FROM user_scope_assignments usa JOIN roles r ON r.id = usa.role_id \
                  WHERE usa.user_id = ? AND usa.scope_type = 'tenant' AND usa.is_emergency = 0"
            ),
            values,
        ))
        .await?;
    let now = Utc::now();
    let (mut live, mut any_role) = (false, false);
    let mut last_end: Option<chrono::DateTime<Utc>> = None;
    for r in rows {
        any_role = true;
        if r.try_get::<i64>("", "gives").map_err(err)? == 0 {
            continue;
        }
        let deleted = instant(r.try_get("", "deleted_at").map_err(err)?);
        let to = instant(r.try_get("", "valid_to").map_err(err)?);
        let from = instant(r.try_get("", "valid_from").map_err(err)?);
        match deleted.or(to.filter(|t| *t < now)) {
            Some(end) => last_end = Some(last_end.map_or(end, |l| l.max(end))),
            None if from.is_none_or(|f| f <= now) => live = true,
            None => {}
        }
    }
    if !is_admin && !live && last_end.is_none() && (admin_only || !any_role) {
        return Ok(Loss::Never);
    }
    Ok(match (account_lost, live || is_admin, last_end) {
        (AccountLoss::At(a), true, _) | (AccountLoss::At(a), false, None) => Loss::At(a),
        (AccountLoss::At(a), false, Some(e)) => Loss::At(a.min(e)),
        // Disabled at a time not known here: the loss is not known either (never later than an
        // assignment's end, but maybe earlier).
        (AccountLoss::Unknown, _, _) => Loss::Unknown,
        (AccountLoss::Active, _, Some(e)) => Loss::At(e),
        (AccountLoss::Active, _, None) => Loss::Unknown,
    })
}

/// The author of a received grant may make it here (see the module rule); `None` when the grant is
/// applied. `granted_at`: when the row says it was made (sender's clock); `review`: how it is
/// listed when it is applied for review. Without one (`sync::credential_check`: a password or a
/// re-enable is held back at no cost), an author never received after a complete pull, or whose
/// loss of the right is not known here, is refused instead (a conflict).
pub(crate) async fn author_check(
    conn: &impl ConnectionTrait,
    what: &str,
    author_sync: &str,
    permission: &str,
    admin_role: bool,
    granted_at: Option<&str>,
    review: Option<ReviewItem>,
) -> AppResult<Option<ApplyOutcome>> {
    let Some(author) = local_id(conn, "user_accounts", author_sync).await? else {
        if caught_up() {
            // A complete pull did not bring the author: it will never arrive.
            return match review {
                Some(review) => accept_for_review(conn, review).await,
                None => Ok(conflict(format!(
                    "{what}: its author {author_sync} never arrived here; not applied"
                ))),
            };
        }
        return Ok(wait_for(
            "user_accounts",
            author_sync,
            format!("{what}: its author {author_sync} is not applied yet"),
        ));
    };
    // Emergency grants never count (`holds_tenant_right`, `auth::rbac::elevated_admin_sql`): an
    // author holding the right only through an emergency elevation is judged as one without it, by
    // design (an emergency never lets its holder grant anything, `admin_users::caller_permissions`).
    let admin_only = if !holds(conn, author, permission).await? {
        false
    } else if admin_role && !is_plant_admin(conn, author).await? {
        true
    } else {
        return Ok(None);
    };
    if !final_pass()
        && (crate::permit::sync_check::rights_may_still_arrive(conn, author_sync, permission).await?
            || admin_rights_may_still_arrive(conn, author_sync).await?)
    {
        return Ok(wait(format!(
            "{what}: the rights of its author {author_sync} are still arriving"
        )));
    }
    let needed = if admin_only { "plant administration" } else { permission };
    let lost = match loss_time(conn, author, permission, admin_only).await? {
        Loss::Unknown => {
            return match review {
                Some(review) => accept_for_review(conn, review).await,
                None => Ok(conflict(format!(
                    "{what}: its author no longer holds {needed} here, since a time this PC does not know; not applied"
                ))),
            }
        }
        Loss::Never => {
            return Ok(conflict(format!(
                "{what}: made by someone who never held {needed} here; not applied, for an administrator to decide"
            )))
        }
        Loss::At(lost) => lost,
    };
    let cutoff = lost - chrono::Duration::minutes(crate::settings::site::MAX_CLOCK_SKEW_MINUTES);
    if granted_at
        .and_then(crate::settings::site::instant)
        .is_some_and(|at| at < cutoff)
    {
        // Made while its author still held the right, as far as this PC knows.
        return Ok(None);
    }
    Ok(conflict(format!(
        "{what}: made after its author lost {needed} (here: on {lost}), or at a time that cannot be read; not \
         applied, for an administrator to decide"
    )))
}

/// How a person is named in a notice: display name, else username, else the sync id (an account
/// not received).
pub(crate) async fn person_label(conn: &impl ConnectionTrait, user_sync: &str) -> AppResult<String> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(display_name, username) AS n FROM user_accounts WHERE entity_sync_id = ?",
            [user_sync.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<String>>("", "n").ok().flatten())
        .unwrap_or_else(|| user_sync.to_string()))
}

/// Name and system flag of a role named by its sync id; `None` when not here.
async fn role_named(conn: &impl ConnectionTrait, role_sync: &str) -> AppResult<Option<(String, bool)>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT name, COALESCE(is_system, 0) AS is_system FROM roles WHERE entity_sync_id = ?",
        [role_sync.into()],
    ))
    .await?
    .map(|r| {
        Ok((
            r.try_get::<String>("", "name").map_err(err)?,
            r.try_get::<i64>("", "is_system").map_err(err)? != 0,
        ))
    })
    .transpose()
}

/// Received grant re-checked against its author; `Some` stops the apply (see the module rule).
pub(crate) async fn inbound_grant_check(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    match layout.def.entity_type {
        "user_scope_assignments" => assignment_check(conn, entity_sync_id, received).await,
        "role_permissions" => role_permission_check(conn, received).await,
        "user_accounts" => account_check(conn, entity_sync_id, received).await,
        "delegated_admin_policies" => delegation_policy_check(conn, entity_sync_id, received).await,
        _ => Ok(None),
    }
}

/// Domains of a policy (`allowed_domains_json`); `None` when unreadable.
fn policy_domains(p: &Payload) -> Option<BTreeSet<String>> {
    match p.get("allowed_domains_json")? {
        Value::String(raw) => serde_json::from_str(raw).ok(),
        Value::Array(_) => serde_json::from_value(p.get("allowed_domains_json")?.clone()).ok(),
        _ => None,
    }
}

/// Whether a received policy gives more than the one held here: new here, another role or scope
/// (any field other than the domains, the step-up flag, the author and the bookkeeping), a domain
/// added or unreadable, or step-up no longer required. Fail closed on anything unreadable.
fn policy_widens(held: Option<&Payload>, received: &Payload) -> bool {
    let Some(held) = held else {
        return true;
    };
    const BOOKKEEPING: [&str; 5] = [
        "allowed_domains_json",
        "requires_step_up_for_publish",
        "changed_by_sync_id",
        "updated_at",
        "row_version",
    ];
    if received
        .iter()
        .any(|(k, v)| !BOOKKEEPING.contains(&k.as_str()) && held.get(k) != Some(v))
    {
        return true;
    }
    if received.contains_key("allowed_domains_json") {
        match (policy_domains(held), policy_domains(received)) {
            (Some(h), Some(r)) if r.is_subset(&h) => {}
            _ => return true,
        }
    }
    received.contains_key("requires_step_up_for_publish")
        && !truthy(received, "requires_step_up_for_publish")
        && truthy(held, "requires_step_up_for_publish")
}

/// A received delegation policy that gives more than the one held here: its author
/// (`changed_by_sync_id`) must have held `adm.roles` (the right of the screen) when it was made
/// (`updated_at`); one that names nobody is a conflict.
async fn delegation_policy_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    let held = crate::permit::sync_check::local_wire(conn, "delegated_admin_policies", entity_sync_id).await?;
    if !policy_widens(held.as_ref(), received) {
        return Ok(None);
    }
    let what = format!("delegation policy {entity_sync_id}");
    let role_name = match field(received, "admin_role_sync_id") {
        Some(role_sync) => match role_named(conn, role_sync).await? {
            Some((name, _)) => name,
            None => {
                return Ok(wait_for(
                    "roles",
                    role_sync,
                    format!("{what}: role {role_sync} is not applied yet"),
                ))
            }
        },
        None => "?".to_string(),
    };
    let changed_at = field(received, "updated_at");
    let domains =
        policy_domains(received).map_or_else(|| "?".to_string(), |d| d.into_iter().collect::<Vec<_>>().join(", "));
    let Some(author_sync) = field(received, "changed_by_sync_id") else {
        // A policy made before 0.9.2 names nobody. During this PC's first download it is one the
        // server already held: applied, and listed for the administrators. Afterwards it is a
        // conflict; re-saving the policy on a PC that holds it records its author.
        if !crate::sync::plant_join::complete_pull_done(conn).await? {
            return accept_for_review(
                conn,
                ReviewItem {
                    entity_type: "delegated_admin_policies",
                    source: entity_sync_id.to_string(),
                    line: format!(
                        "{role_name} : délégation d'administration / delegated administration ({domains}), \
                         auteur non enregistré / author not recorded, {}",
                        day(changed_at)
                    ),
                },
            )
            .await;
        }
        return Ok(conflict(format!(
            "{what}: a delegation of administration created or widened without saying who did it; not applied, \
             for an administrator to decide (saving the policy again on a PC that holds it records its author)"
        )));
    };
    let review = ReviewItem {
        entity_type: "delegated_admin_policies",
        source: entity_sync_id.to_string(),
        line: format!(
            "{role_name} : délégation d'administration / delegated administration ({domains}), par / by {}, {}",
            person_label(conn, author_sync).await?,
            day(changed_at)
        ),
    };
    author_check(conn, &what, author_sync, ADM_ROLES, false, changed_at, Some(review)).await
}

/// A validity bound moved so that the assignment is in force longer: `earlier` for a start, else an
/// end. Unreadable bounds that differ count as widened (fail closed).
fn widened(held: Option<&str>, received: Option<&str>, earlier: bool) -> bool {
    match (held, received) {
        (h, r) if h == r => false,
        // A bound removed: open on that side.
        (Some(_), None) => true,
        // A bound added: narrower.
        (None, Some(_)) => false,
        (Some(h), Some(r)) => match (crate::settings::site::instant(h), crate::settings::site::instant(r)) {
            (Some(h), Some(r)) => {
                if earlier {
                    r < h
                } else {
                    r > h
                }
            }
            _ => true,
        },
        (None, None) => false,
    }
}

/// Scope types whose reference is an org node (`sync::row_spec::SCOPE_TARGETS`).
const NODE_SCOPES: [&str; 3] = ["entity", "org_node", "site"];

/// The scope of an assignment held here changed: another type, or another node / team, except the
/// move an organisation publish makes from a node deleted here to the node replacing it
/// (`org::validation::remap_scope_reference`, `node_replaces_deleted_node`). `Err(outcome)`: the
/// received node is not applied yet (the row waits for it).
async fn scope_changed(
    conn: &impl ConnectionTrait,
    held: &Payload,
    received: &Payload,
) -> AppResult<Result<bool, Option<ApplyOutcome>>> {
    let ref_key = "scope_reference_sync_id";
    if field(held, "scope_type") != field(received, "scope_type") {
        return Ok(Ok(true));
    }
    let (held_ref, received_ref) = (field(held, ref_key), field(received, ref_key));
    if held_ref == received_ref {
        return Ok(Ok(false));
    }
    let scope_type = field(received, "scope_type").unwrap_or("tenant");
    let (Some(held_ref), Some(received_ref)) = (held_ref, received_ref) else {
        return Ok(Ok(true));
    };
    if !NODE_SCOPES.contains(&scope_type) {
        return Ok(Ok(true));
    }
    let Some(received_node) = local_id(conn, "org_nodes", received_ref).await? else {
        return Ok(Err(wait_for(
            "org_nodes",
            received_ref,
            format!("role assignment moved to node {received_ref}, not applied yet"),
        )));
    };
    let Some(held_node) = local_id(conn, "org_nodes", held_ref).await? else {
        return Ok(Ok(true));
    };
    if crate::org::validation::node_replaces_deleted_node(conn, held_node, received_node).await? {
        return Ok(Ok(false));
    }
    // The publish moving it has only partly arrived: wait for the activation of its structure.
    if crate::org::validation::node_copy_awaits_activation(conn, Some(held_node), received_node).await? {
        return Ok(Err(wait(format!(
            "role assignment moved to node {received_ref}, whose structure is not active here yet"
        ))));
    }
    Ok(Ok(true))
}

/// Whether the received assignment gives access the one held here does not (new, restored,
/// another role or scope, a validity widened, an emergency made permanent or extended).
async fn assignment_grants(
    conn: &impl ConnectionTrait,
    held: Option<&Payload>,
    received: &Payload,
) -> AppResult<Result<bool, Option<ApplyOutcome>>> {
    // A row received ended gives nothing.
    if field(received, "deleted_at").is_some() {
        return Ok(Ok(false));
    }
    let Some(held) = held else {
        return Ok(Ok(true));
    };
    if field(held, "deleted_at").is_some() || field(held, "role_sync_id") != field(received, "role_sync_id") {
        return Ok(Ok(true));
    }
    match scope_changed(conn, held, received).await? {
        Ok(true) => return Ok(Ok(true)),
        Ok(false) => {}
        Err(outcome) => return Ok(Err(outcome)),
    }
    let emergency_widened = (truthy(held, "is_emergency") && !truthy(received, "is_emergency"))
        || (truthy(received, "is_emergency")
            && widened(
                field(held, "emergency_expires_at"),
                field(received, "emergency_expires_at"),
                false,
            ));
    Ok(Ok(emergency_widened
        || widened(
            field(held, "valid_from"),
            field(received, "valid_from"),
            true,
        )
        || widened(
            field(held, "valid_to"),
            field(received, "valid_to"),
            false,
        )))
}

async fn assignment_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    let held = crate::permit::sync_check::local_wire(conn, "user_scope_assignments", entity_sync_id).await?;
    match assignment_grants(conn, held.as_ref(), received).await? {
        Ok(false) => return Ok(None),
        Ok(true) => {}
        Err(outcome) => return Ok(outcome),
    }
    let what = format!("role assignment {entity_sync_id}");
    let (Some(user_sync), Some(role_sync)) = (field(received, "user_sync_id"), field(received, "role_sync_id")) else {
        // Incomplete: the codec refuses or defers it.
        return Ok(None);
    };
    let Some((role_name, _)) = role_named(conn, role_sync).await? else {
        return Ok(wait_for(
            "roles",
            role_sync,
            format!("{what}: role {role_sync} is not applied yet"),
        ));
    };
    let admin_role = ADMIN_ROLE_NAMES.contains(&role_name.as_str());
    let Some(author_sync) = field(received, "assigned_by_sync_id") else {
        return unauthored_assignment(conn, &what, user_sync, admin_role, received).await;
    };
    if author_sync == user_sync {
        return Ok(conflict(format!(
            "{what}: a role given by an account to itself; not applied, for an administrator to decide"
        )));
    }
    let granted_at = if held.is_some() {
        field(received, "updated_at")
    } else {
        field(received, "created_at")
    };
    let review = ReviewItem {
        entity_type: "user_scope_assignments",
        source: entity_sync_id.to_string(),
        line: format!(
            "{} : rôle / role {role_name}, par / by {}, {}",
            person_label(conn, user_sync).await?,
            person_label(conn, author_sync).await?,
            day(granted_at)
        ),
    };
    author_check(
        conn,
        &what,
        author_sync,
        ADM_USERS,
        admin_role,
        granted_at,
        Some(review),
    )
    .await
}

/// An assignment that names no author (see the module rule for the ones accepted).
async fn unauthored_assignment(
    conn: &impl ConnectionTrait,
    what: &str,
    user_sync: &str,
    admin_role: bool,
    received: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    let Some(user) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, is_admin FROM user_accounts WHERE entity_sync_id = ?",
            [user_sync.into()],
        ))
        .await?
    else {
        return Ok(wait_for(
            "user_accounts",
            user_sync,
            format!("{what}: account {user_sync} is not applied yet"),
        ));
    };
    let user_id: i64 = user.try_get("", "id").map_err(err)?;
    // The bootstrap account is a plant administrator by its flag: the role adds nothing.
    if user.try_get::<i64>("", "is_admin").map_err(err)? == 1 {
        return Ok(None);
    }
    let plant_wide = field(received, "scope_type") == Some("tenant") && !truthy(received, "is_emergency");
    if admin_role && plant_wide {
        let recorded = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS r FROM audit_events \
                  WHERE action_code = ? AND target_type = 'user_account' AND target_id = ? LIMIT 1",
                [
                    crate::auth::recovery_code::AUDIT_USED.into(),
                    user_id.to_string().into(),
                ],
            ))
            .await?
            .is_some();
        if recorded {
            return Ok(None);
        }
        let record_waiting = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS r FROM sync_inbox \
                  WHERE entity_type = 'audit_events' AND apply_status IN ('pending', 'deferred') \
                    AND json_extract(payload_json, '$.action_code') = ? \
                    AND json_extract(payload_json, '$.target_sync_id') = ? LIMIT 1",
                [crate::auth::recovery_code::AUDIT_USED.into(), user_sync.into()],
            ))
            .await?
            .is_some();
        if record_waiting {
            return Ok(wait(format!(
                "{what}: the recovery-code record of account {user_sync} is still arriving"
            )));
        }
    }
    Ok(conflict(format!(
        "{what}: a role given without saying who gave it; not applied, for an administrator to decide"
    )))
}

async fn role_permission_check(conn: &impl ConnectionTrait, received: &Payload) -> AppResult<Option<ApplyOutcome>> {
    let (Some(role_sync), Some(permission)) = (field(received, "role_sync_id"), field(received, "permission_key"))
    else {
        return Ok(None);
    };
    let held = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS h FROM role_permissions rp JOIN roles r ON r.id = rp.role_id \
               JOIN permissions p ON p.id = rp.permission_id \
              WHERE r.entity_sync_id = ? AND p.name = ?",
            [role_sync.into(), permission.into()],
        ))
        .await?
        .is_some();
    if held {
        return Ok(None);
    }
    let what = format!("permission {permission} of role {role_sync}");
    let Some((role_name, is_system)) = role_named(conn, role_sync).await? else {
        return Ok(wait_for(
            "roles",
            role_sync,
            format!("{what}: the role is not applied yet"),
        ));
    };
    let Some(author_sync) = field(received, "granted_by_sync_id") else {
        // Migrations and the seeder grant the permissions of system roles on every PC.
        if is_system {
            return Ok(None);
        }
        return Ok(conflict(format!(
            "{what}: a permission given without saying who gave it; not applied, for an administrator to decide"
        )));
    };
    let granted_at = field(received, "granted_at");
    let review = ReviewItem {
        entity_type: "role_permissions",
        source: format!("{role_sync}:{permission}"),
        line: format!(
            "{role_name} : droit / right {permission}, par / by {}, {}",
            person_label(conn, author_sync).await?,
            day(granted_at)
        ),
    };
    author_check(
        conn,
        &what,
        author_sync,
        ADM_ROLES,
        ADMIN_ROLE_NAMES.contains(&role_name.as_str()),
        granted_at,
        Some(review),
    )
    .await
}

/// The account holds, or gets back once active, a right to give access here: the bootstrap flag, or
/// a plant-wide permanent assignment in force to an administrator role or to a role carrying
/// `adm.users` / `adm.roles` (whatever the account's own state).
async fn account_carries_granting_rights(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<bool> {
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(crate::rbac::resolver::permanent_binds(&now));
    values.push(ADM_USERS.into());
    values.push(ADM_ROLES.into());
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS c FROM user_accounts ua WHERE ua.id = ? AND (ua.is_admin = 1 OR EXISTS ( \
                   SELECT 1 FROM user_scope_assignments usa JOIN roles r ON r.id = usa.role_id \
                    WHERE usa.user_id = ua.id AND usa.scope_type = 'tenant' AND {in_force} \
                      AND r.deleted_at IS NULL AND (r.name IN {admin_roles} OR EXISTS ( \
                        SELECT 1 FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id \
                         WHERE rp.role_id = r.id AND p.name IN (?, ?)))))",
                in_force = crate::rbac::resolver::PERMANENT_ASSIGNMENT_IN_FORCE_SQL,
                admin_roles = crate::auth::rbac::ELEVATED_ADMIN_ROLES_SQL,
            ),
            values,
        ))
        .await?
        .is_some())
}

/// A received account (the row never says who changed it):
/// - new here with the bootstrap flag, while this PC records its first administrator
///   (`auth::builtin_admin::first_admin_marker_closes`) and has finished a complete pull
///   (`sync::plant_join::complete_pull_done`): a second first administrator; refused (the
///   first-start and join flows create one only on an empty plant, `sync::plant_join`). Before
///   that pull ends, the server's own accounts are still arriving: a plant set up by older versions
///   may hold several first administrators, and the marker may already be closed by one of them
///   (`backfill_first_admin_marker` at a restart during the pull), so they are applied;
/// - held here without the bootstrap flag and received with it: no command does that; refused;
/// - held here disabled or deleted and received active or restored (`reactivate_user`, a restore)
///   while it holds a right to give access here ([`account_carries_granting_rights`]): refused.
///   The dev-only built-in "admin" whose password was never changed is left to its own rule
///   (`auth::builtin_admin::after_inbound_user_account`).
async fn account_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    let Some(held) = crate::permit::sync_check::local_wire(conn, "user_accounts", entity_sync_id).await? else {
        if truthy(received, "is_admin")
            && crate::auth::builtin_admin::first_admin_marker_closes(conn).await?
            && crate::sync::plant_join::complete_pull_done(conn).await?
        {
            return Ok(conflict(format!(
                "account {entity_sync_id}: a new account with the bootstrap administrator flag, while this PC already \
                 has its first administrator; not applied, for an administrator to decide"
            )));
        }
        return Ok(None);
    };
    if truthy(received, "is_admin") && !truthy(&held, "is_admin") {
        return Ok(conflict(format!(
            "account {entity_sync_id}: the bootstrap administrator flag set on an existing account (no screen does \
             that, and the row does not say who did it); not applied, for an administrator to decide"
        )));
    }
    let reactivated =
        received.contains_key("is_active") && truthy(received, "is_active") && !truthy(&held, "is_active");
    let restored = received.contains_key("deleted_at")
        && field(received, "deleted_at").is_none()
        && field(&held, "deleted_at").is_some();
    if !reactivated && !restored {
        return Ok(None);
    }
    let Some(user_id) = local_id(conn, "user_accounts", entity_sync_id).await? else {
        return Ok(None);
    };
    let builtin = crate::auth::builtin_admin::builtin_admin_row(conn)
        .await?
        .is_some_and(|b| b.user_id == user_id && b.password_never_changed);
    if builtin || !account_carries_granting_rights(conn, user_id).await? {
        return Ok(None);
    }
    Ok(conflict(format!(
        "account {entity_sync_id}: an account holding administration rights here was re-enabled or restored on \
         another PC (the row does not say who did it); not applied, for an administrator to decide"
    )))
}

// ── Convergence ────────────────────────────────────────────────────────────────────────────────

/// Who held [`GRANTING_RIGHTS`] before a received change ([`rights_before`]); `None` for a table
/// that does not decide them.
pub(crate) struct RightsBefore(Option<BTreeSet<(i64, &'static str)>>);

/// Active accounts holding each of [`GRANTING_RIGHTS`] here, now (the rule of [`holds`]).
async fn granting_right_holders(conn: &impl ConnectionTrait) -> AppResult<BTreeSet<(i64, &'static str)>> {
    let now = crate::rbac::resolver::assignment_clock();
    let mut holders = BTreeSet::new();
    for permission in GRANTING_RIGHTS {
        let mut values: Vec<sea_orm::Value> = crate::rbac::resolver::permanent_binds(&now).to_vec();
        values.extend(crate::rbac::resolver::permanent_binds(&now));
        values.push(permission.into());
        let rows = conn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT ua.id FROM user_accounts ua \
                      WHERE ua.is_active = 1 AND ua.deleted_at IS NULL AND (ua.is_admin = 1 \
                        OR EXISTS (SELECT 1 FROM user_scope_assignments usa JOIN roles r ON r.id = usa.role_id \
                                    WHERE usa.user_id = ua.id AND usa.scope_type = 'tenant' AND {in_force} \
                                      AND r.deleted_at IS NULL AND r.name IN {admin_roles}) \
                        OR EXISTS (SELECT 1 FROM user_scope_assignments usa \
                                     JOIN role_permissions rp ON rp.role_id = usa.role_id \
                                     JOIN permissions p ON p.id = rp.permission_id \
                                    WHERE usa.user_id = ua.id AND usa.scope_type = 'tenant' AND {in_force} \
                                      AND p.name = ?))",
                    in_force = crate::rbac::resolver::PERMANENT_ASSIGNMENT_IN_FORCE_SQL,
                    admin_roles = crate::auth::rbac::ELEVATED_ADMIN_ROLES_SQL,
                ),
                values,
            ))
            .await?;
        for r in rows {
            holders.insert((r.try_get::<i64>("", "id").map_err(err)?, permission));
        }
    }
    Ok(holders)
}

/// Before a received row of a table deciding who may give access is applied.
pub(crate) async fn rights_before(conn: &impl ConnectionTrait, layout: &EntityLayout) -> AppResult<RightsBefore> {
    if !RIGHTS_ENTITY_TYPES.contains(&layout.def.entity_type) {
        return Ok(RightsBefore(None));
    }
    Ok(RightsBefore(Some(granting_right_holders(conn).await?)))
}

/// When the received row records the withdrawal: its `deleted_at`, an end of validity already
/// reached (assignments), else its `updated_at`. `None`: no time (a delete).
fn withdrawal_time(entity_type: &str, received: Option<&Payload>) -> Option<chrono::DateTime<Utc>> {
    let received = received?;
    let at = |key: &str| field(received, key).and_then(crate::settings::site::instant);
    at("deleted_at")
        .or_else(|| {
            if entity_type == "user_scope_assignments" {
                at("valid_to").filter(|t| *t <= Utc::now())
            } else {
                None
            }
        })
        .or_else(|| at("updated_at"))
}

/// A grant made by someone who lost the right to make it, for the review notice.
struct LateGrant {
    at: String,
    line: String,
}

async fn grants_made_by(
    conn: &impl ConnectionTrait,
    user_id: i64,
    since: Option<chrono::DateTime<Utc>>,
) -> AppResult<Vec<LateGrant>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT usa.created_at AS at, COALESCE(u.display_name, u.username) || ' : rôle / role ' || r.name \
                    AS line \
               FROM user_scope_assignments usa JOIN roles r ON r.id = usa.role_id \
               JOIN user_accounts u ON u.id = usa.user_id \
              WHERE usa.assigned_by_id = ? AND usa.deleted_at IS NULL \
             UNION ALL \
             SELECT rp.granted_at AS at, r.name || ' : droit / right ' || p.name AS line \
               FROM role_permissions rp JOIN roles r ON r.id = rp.role_id \
               JOIN permissions p ON p.id = rp.permission_id \
              WHERE rp.granted_by_id = ? \
             UNION ALL \
             SELECT CASE WHEN pw.password_set_by_id = ? THEN pw.password_set_at ELSE pw.password_changed_at END \
                      AS at, \
                    'Mot de passe de / password of ' || COALESCE(pw.display_name, pw.username) AS line \
               FROM user_accounts pw \
              WHERE (pw.password_set_by_id = ? OR pw.password_changed_by_id = ?) AND pw.id <> ? \
             UNION ALL \
             SELECT dap.updated_at AS at, \
                    r.name || ' : délégation d''administration / delegated administration ' \
                      || dap.allowed_domains_json AS line \
               FROM delegated_admin_policies dap JOIN roles r ON r.id = dap.admin_role_id \
              WHERE dap.changed_by_id = ? \
              ORDER BY at",
            // Passwords of other accounts this person set (`sync::credential_check`), the account
            // having changed it since included; their own password is not a grant. Delegation
            // policies this person created or last changed (migration 262).
            [
                user_id.into(),
                user_id.into(),
                user_id.into(),
                user_id.into(),
                user_id.into(),
                user_id.into(),
                user_id.into(),
            ],
        ))
        .await?;
    let cutoff = since.map(|t| t - chrono::Duration::minutes(crate::settings::site::MAX_CLOCK_SKEW_MINUTES));
    let mut grants = Vec::new();
    for r in rows {
        let at: Option<String> = r.try_get("", "at").map_err(err)?;
        let at = at.unwrap_or_default();
        // An unreadable time is listed (fail closed).
        let late = match (cutoff, crate::settings::site::instant(&at)) {
            (Some(cutoff), Some(t)) => t >= cutoff,
            _ => true,
        };
        if late {
            grants.push(LateGrant {
                at,
                line: r.try_get::<String>("", "line").map_err(err)?,
            });
        }
    }
    Ok(grants)
}

/// After a received row of a table deciding who may give access was applied (or deleted): when it
/// took [`GRANTING_RIGHTS`] away from someone here, the grants they made from the withdrawal on are
/// listed in one review notice (see the module documentation). `received`: `None` for a delete.
pub(crate) async fn after_rights_change(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    received: Option<&Payload>,
    before: RightsBefore,
) -> AppResult<()> {
    let Some(before) = before.0 else {
        return Ok(());
    };
    let after = granting_right_holders(conn).await?;
    let lost: Vec<(i64, &'static str)> = before.difference(&after).copied().collect();
    if lost.is_empty() {
        return Ok(());
    }
    let since = withdrawal_time(layout.def.entity_type, received);
    let mut people: Vec<String> = Vec::new();
    let mut grants: Vec<LateGrant> = Vec::new();
    let mut seen = BTreeSet::new();
    for (user_id, _) in &lost {
        if !seen.insert(*user_id) {
            continue;
        }
        let name = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(display_name, username) AS n FROM user_accounts WHERE id = ?",
                [(*user_id).into()],
            ))
            .await?
            .map(|r| r.try_get::<String>("", "n"))
            .transpose()
            .map_err(err)?
            .unwrap_or_else(|| user_id.to_string());
        let theirs = grants_made_by(conn, *user_id, since).await?;
        if theirs.is_empty() {
            continue;
        }
        for g in theirs {
            grants.push(LateGrant {
                line: format!("{}, par / by {name}, {}", g.line, day(Some(&g.at))),
                at: g.at,
            });
        }
        people.push(name);
    }
    if grants.is_empty() {
        return Ok(());
    }
    let rights: BTreeSet<&str> = lost.iter().map(|(_, p)| *p).collect();
    let rights = rights.into_iter().collect::<Vec<_>>().join(", ");
    let since_text = since.map_or_else(|| "?".to_string(), |t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string());
    let shown: Vec<String> = grants.iter().take(NOTICE_MAX_LINES).map(|g| g.line.clone()).collect();
    let more = grants.len().saturating_sub(shown.len());
    let list = if more > 0 {
        format!("{}; … (+{more})", shown.join("; "))
    } else {
        shown.join("; ")
    };
    let who = people.join(", ");
    crate::settings::received_notice::send_grant_review_notice(
        conn,
        layout.def.entity_type,
        entity_sync_id,
        "Droits d'administration retirés sur un autre poste : attributions à revoir / Administration rights \
         removed on another PC: grants to review"
            .to_string(),
        format!(
            "Reçu par synchronisation : {who} n'a plus {rights} (retiré le {since_text}). Attributions faites \
             depuis, à vérifier (rien n'a été retiré automatiquement) : {list}. / Received by sync: {who} no longer \
             holds {rights} (withdrawn on {since_text}). Grants made since, to check (nothing was withdrawn \
             automatically): {list}."
        ),
        crate::notifications::emitter::NotificationMessage::new(
            "admin_grants_to_review",
            json!({
                "people": who,
                "rights": rights,
                "since": since.map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                "grants": list,
                "count": grants.len(),
            }),
        ),
        false,
    )
    .await
}

#[cfg(test)]
mod policy_widens_tests {
    use super::{policy_widens, Payload};
    use serde_json::json;

    fn policy(fields: serde_json::Value) -> Payload {
        let mut p: Payload = json!({
            "entity_sync_id": "pol",
            "admin_role_sync_id": "role-a",
            "managed_scope_type": "tenant",
            "allowed_domains_json": "[\"inv\",\"ot\"]",
            "requires_step_up_for_publish": 1,
            "changed_by_sync_id": "chef-sync",
            "updated_at": "2026-10-25T08:00:00Z",
        })
        .as_object()
        .cloned()
        .unwrap();
        for (k, v) in fields.as_object().unwrap() {
            p.insert(k.clone(), v.clone());
        }
        p
    }

    #[test]
    fn what_gives_more_is_checked_and_what_narrows_is_not() {
        let held = policy(json!({}));
        assert!(policy_widens(None, &held), "new here");
        assert!(!policy_widens(
            Some(&held),
            &policy(json!({"updated_at": "2026-10-26T08:00:00Z"}))
        ));
        assert!(!policy_widens(
            Some(&held),
            &policy(json!({"allowed_domains_json": "[\"inv\"]"}))
        ));
        assert!(policy_widens(
            Some(&held),
            &policy(json!({"allowed_domains_json": "[\"inv\",\"ot\",\"di\"]"}))
        ));
        assert!(policy_widens(
            Some(&held),
            &policy(json!({"allowed_domains_json": "not json"}))
        ));
        assert!(policy_widens(
            Some(&held),
            &policy(json!({"admin_role_sync_id": "role-b"}))
        ));
        assert!(policy_widens(
            Some(&held),
            &policy(json!({"managed_scope_type": "org_node"}))
        ));
        assert!(policy_widens(
            Some(&held),
            &policy(json!({"managed_scope_reference_sync_id": "site-1"}))
        ));
        assert!(policy_widens(
            Some(&held),
            &policy(json!({"requires_step_up_for_publish": 0}))
        ));
        let relaxed = policy(json!({"requires_step_up_for_publish": 0}));
        assert!(!policy_widens(Some(&relaxed), &held), "step-up required again");
    }
}
