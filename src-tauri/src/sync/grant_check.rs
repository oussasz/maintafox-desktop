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
//! - `user_accounts` held here without the bootstrap flag (`is_admin`) and received with it: no
//!   command sets that flag on an existing account (only the first-start account and the dev-only
//!   built-in "admin" are created with it), and the row does not say who changed it: refused.
//!
//! "Now" and not "at the time of the row": role permissions are deleted, so a past set of rights
//! cannot be rebuilt, and the row's times come from the sender's clock. Judging now fails closed.
//! A failure is a [`GRANT_BY_UNAUTHORISED_ADMIN`] conflict, decided by an administrator in the
//! Sync centre; "take the received version" goes through this check again (it never bypasses it):
//! the grant is applied once its author holds the right here again, or an administrator makes the
//! grant here themselves. An author not applied here yet makes the row wait for that account, as
//! for any missing parent; so do rights of the author still waiting in the inbox (the precedent of
//! `qualification::sync_check`, `permit::sync_check::rights_may_still_arrive`).
//!
//! Rows without an author that this check accepts: an assignment to an account holding the
//! bootstrap flag (the first administrator, `product_license`; the dev seeder's built-in account),
//! which the flag makes a plant administrator anyway; the Administrator role given by a vendor
//! recovery code (`auth::recovery_code`, its `admin_recovery.used` audit record here; waiting while
//! that record is in the inbox); a permission of a system role (migrations and the seeder grant
//! them on every PC). Any other new grant without an author is a conflict.
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
//! `delegated_admin_policies` records no author (no column names who created or widened a
//! policy): it cannot be judged here (reported; adding the column is a schema change).

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

fn truthy(p: &Payload, key: &str) -> bool {
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

fn wait(message: String) -> Option<ApplyOutcome> {
    Some(ApplyOutcome::Deferred {
        code: DEFER_MISSING_PARENT.into(),
        message,
        waits_for: None,
    })
}

/// Local id of a row named by its sync id; `None` when not here.
async fn local_id(conn: &impl ConnectionTrait, table: &str, sync_id: &str) -> AppResult<Option<i64>> {
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
async fn is_plant_admin(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<bool> {
    let Ok(id) = i32::try_from(user_id) else {
        return Ok(false);
    };
    crate::auth::rbac::user_has_elevated_admin_access(conn, id).await
}

/// `user_id` may use `permission` to give access here, now: a plant administrator, or a holder of
/// it plant-wide through a permanent role in force (`settings::site::holds_tenant_right`).
async fn holds(conn: &impl ConnectionTrait, user_id: i64, permission: &str) -> AppResult<bool> {
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

/// The author named by `key` may make this grant here (see the module rule); `None` when they may.
async fn author_check(
    conn: &impl ConnectionTrait,
    what: &str,
    author_sync: &str,
    permission: &str,
    admin_role: bool,
) -> AppResult<Option<ApplyOutcome>> {
    let Some(author) = local_id(conn, "user_accounts", author_sync).await? else {
        return Ok(wait_for(
            "user_accounts",
            author_sync,
            format!("{what}: its author {author_sync} is not applied yet"),
        ));
    };
    if !holds(conn, author, permission).await? {
        if crate::permit::sync_check::rights_may_still_arrive(conn, author_sync, permission).await?
            || admin_rights_may_still_arrive(conn, author_sync).await?
        {
            return Ok(wait(format!(
                "{what}: the rights of its author {author_sync} are still arriving"
            )));
        }
        return Ok(conflict(format!(
            "{what}: made by someone who does not hold {permission} here now (withdrawn on another PC, or never \
             held); not applied, for an administrator to decide"
        )));
    }
    if admin_role && !is_plant_admin(conn, author).await? {
        if admin_rights_may_still_arrive(conn, author_sync).await? {
            return Ok(wait(format!(
                "{what}: the administrator rights of its author {author_sync} are still arriving"
            )));
        }
        return Ok(conflict(format!(
            "{what}: an administrator role given by someone who is not a plant administrator here now; not applied, \
             for an administrator to decide"
        )));
    }
    Ok(None)
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
        "user_accounts" => account_flag_check(conn, entity_sync_id, received).await,
        _ => Ok(None),
    }
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
    Ok(Ok(!crate::org::validation::node_replaces_deleted_node(
        conn,
        held_node,
        received_node,
    )
    .await?))
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
    author_check(conn, &what, author_sync, ADM_USERS, admin_role).await
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
    author_check(
        conn,
        &what,
        author_sync,
        ADM_ROLES,
        ADMIN_ROLE_NAMES.contains(&role_name.as_str()),
    )
    .await
}

async fn account_flag_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<ApplyOutcome>> {
    if !truthy(received, "is_admin") {
        return Ok(None);
    }
    let Some(held) = crate::permit::sync_check::local_wire(conn, "user_accounts", entity_sync_id).await? else {
        return Ok(None);
    };
    if truthy(&held, "is_admin") {
        return Ok(None);
    }
    Ok(conflict(format!(
        "account {entity_sync_id}: the bootstrap administrator flag set on an existing account (no screen does \
         that, and the row does not say who did it); not applied, for an administrator to decide"
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
            "SELECT usa.created_at AS at, 'role ' || r.name || ' -> ' || COALESCE(u.display_name, u.username) \
                    || ' (' || usa.scope_type || ')' AS line \
               FROM user_scope_assignments usa JOIN roles r ON r.id = usa.role_id \
               JOIN user_accounts u ON u.id = usa.user_id \
              WHERE usa.assigned_by_id = ? AND usa.deleted_at IS NULL \
             UNION ALL \
             SELECT rp.granted_at AS at, 'permission ' || p.name || ' -> role ' || r.name AS line \
               FROM role_permissions rp JOIN roles r ON r.id = rp.role_id \
               JOIN permissions p ON p.id = rp.permission_id \
              WHERE rp.granted_by_id = ? \
              ORDER BY at",
            [user_id.into(), user_id.into()],
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
                at: g.at,
                line: format!("{} ({name})", g.line),
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
    let shown: Vec<String> = grants
        .iter()
        .take(NOTICE_MAX_LINES)
        .map(|g| format!("{} {}", g.at, g.line))
        .collect();
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
    )
    .await
}
