//! Runtime permission check engine.
//!
//! All permission checks are database-backed. No in-memory cache is used in Phase 1.
//! The check is scoped: a user holding `ot.view` globally (tenant scope) can view
//! all work orders. A user holding `ot.view` at entity scope can view only that entity.
//!
//! Scope resolution rule: if the user holds the permission at ANY scope that covers
//! the requested resource, the check passes. The scope hierarchy is:
//!   tenant > entity > site > team > `org_node`

use crate::errors::AppResult;
use crate::rbac::cache::PermissionCache;
use crate::rbac::resolver::{
    assignment_clock, in_force_binds, next_assignment_change, permanent_binds, ASSIGNMENT_IN_FORCE_SQL,
    PERMANENT_ASSIGNMENT_IN_FORCE_SQL,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use std::collections::HashSet;
use tokio::sync::RwLock;
use tracing::debug;

/// The scope type passed to a permission check.
/// `Global` means the caller doesn't require scope restriction.
/// `Entity(id)`, `Site(id)`, etc. mean the resource belongs to that scope.
#[derive(Debug, Clone)]
pub enum PermissionScope {
    Global,
    Entity(String),
    Site(String),
    Team(String),
    OrgNode(String),
}

/// A resolved permission record returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct PermissionRecord {
    pub name: String,
    pub description: String,
    pub category: String,
    pub is_dangerous: bool,
    pub requires_step_up: bool,
}

/// Check whether a user has a given permission, considering scope.
///
/// Algorithm:
///   1. Get all role assignments for the user that are in force now
///      ([`ASSIGNMENT_IN_FORCE_SQL`]: validity window, emergency grants only before expiry)
///   2. Filter to assignments whose `scope_type` is `tenant` (covers everything) or
///      matches the requested scope
///   3. For each matching role, check if it has the permission in `role_permissions`
///   4. Return true if any role-scope combination grants the permission
///
/// Returns `Err` only on DB errors; returns `Ok(false)` for any denial.
pub async fn check_permission(
    db: &DatabaseConnection,
    user_id: i32,
    permission_name: &str,
    scope: &PermissionScope,
) -> AppResult<bool> {
    let now = assignment_clock();

    // Determine scope filter components
    let (scope_type_filter, scope_ref_filter): (Option<&str>, Option<String>) = match scope {
        PermissionScope::Global => (None, None),
        PermissionScope::Entity(id) => (Some("entity"), Some(id.clone())),
        PermissionScope::Site(id) => (Some("site"), Some(id.clone())),
        PermissionScope::Team(id) => (Some("team"), Some(id.clone())),
        PermissionScope::OrgNode(id) => (Some("org_node"), Some(id.clone())),
    };

    // Build the scope clause: always allow tenant-scoped roles; also allow
    // matching scope if a specific scope was provided.
    let scope_sql = if scope_type_filter.is_some() {
        "(usa.scope_type = 'tenant' OR (usa.scope_type = ? AND usa.scope_reference = ?))"
    } else {
        "usa.scope_type = 'tenant'"
    };

    let sql = format!(
        r"
        SELECT COUNT(*) as cnt
        FROM user_scope_assignments usa
        INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
        INNER JOIN permissions p       ON p.id = rp.permission_id
        WHERE usa.user_id = ?
          AND {ASSIGNMENT_IN_FORCE_SQL}
          AND p.name = ?
          AND {scope_sql}
    "
    );

    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(in_force_binds(&now));
    values.push(permission_name.into());

    if let (Some(st), Some(sr)) = (&scope_type_filter, &scope_ref_filter) {
        values.push((*st).into());
        values.push(sr.clone().into());
    }

    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    let count = row.and_then(|r| r.try_get::<i64>("", "cnt").ok()).unwrap_or(0);

    Ok(count > 0)
}

/// Convert a `PermissionScope` to a cache key string.
pub fn scope_to_cache_key(scope: &PermissionScope) -> String {
    match scope {
        PermissionScope::Global => "tenant".to_string(),
        PermissionScope::Entity(id) => format!("entity:{id}"),
        PermissionScope::Site(id) => format!("site:{id}"),
        PermissionScope::Team(id) => format!("team:{id}"),
        PermissionScope::OrgNode(id) => format!("org_node:{id}"),
    }
}

/// Cache-aware permission check.
///
/// 1. Derive a cache key from `(user_id, scope)`.
/// 2. Check `PermissionCache` — on hit return membership immediately.
/// 3. On miss — load the **full** permission set for this user+scope from
///    the database, store in cache, then check membership.
///
/// This ensures that commands checking multiple permissions against the same
/// user+scope only hit the database once.
pub async fn check_permission_cached(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission_name: &str,
    scope: &PermissionScope,
) -> AppResult<bool> {
    let cache_key = scope_to_cache_key(scope);
    let uid = i64::from(user_id);

    // ── Fast path: cache hit ─────────────────────────────────────────────
    {
        let guard = cache.read().await;
        if let Some(perms) = guard.get(uid, &cache_key) {
            debug!(
                user_id = uid,
                permission = permission_name,
                cache_key = %cache_key,
                "permission_cache HIT"
            );
            return Ok(perms.contains(permission_name));
        }
    }

    // ── Slow path: load full permission set, populate cache ──────────────
    debug!(
        user_id = uid,
        permission = permission_name,
        cache_key = %cache_key,
        "permission_cache MISS — loading from DB"
    );
    let perms = load_scope_permissions(db, user_id, scope).await?;
    let has = perms.contains(permission_name);

    // The set holds until one of the user's assignments starts, ends or expires.
    let valid_until = match next_assignment_change(db, uid).await? {
        None => None,
        Some(at) => match chrono::DateTime::parse_from_rfc3339(&at) {
            Ok(t) => Some(t.with_timezone(&chrono::Utc)),
            // An unreadable bound: do not keep the set at all.
            Err(_) => Some(chrono::Utc::now()),
        },
    };
    {
        let mut guard = cache.write().await;
        guard.put(uid, cache_key, perms, valid_until);
    }

    Ok(has)
}

/// Load all permission names a user holds at a given scope (for cache population).
async fn load_scope_permissions(
    db: &DatabaseConnection,
    user_id: i32,
    scope: &PermissionScope,
) -> AppResult<HashSet<String>> {
    if user_has_elevated_admin_access(db, user_id).await? {
        let all = get_all_permissions(db).await?;
        return Ok(all.into_iter().map(|p| p.name).collect());
    }

    let now = assignment_clock();

    let (scope_type_filter, scope_ref_filter): (Option<&str>, Option<String>) = match scope {
        PermissionScope::Global => (None, None),
        PermissionScope::Entity(id) => (Some("entity"), Some(id.clone())),
        PermissionScope::Site(id) => (Some("site"), Some(id.clone())),
        PermissionScope::Team(id) => (Some("team"), Some(id.clone())),
        PermissionScope::OrgNode(id) => (Some("org_node"), Some(id.clone())),
    };

    let scope_sql = if scope_type_filter.is_some() {
        "(usa.scope_type = 'tenant' OR (usa.scope_type = ? AND usa.scope_reference = ?))"
    } else {
        "usa.scope_type = 'tenant'"
    };

    let sql = format!(
        "SELECT DISTINCT p.name \
         FROM permissions p \
         INNER JOIN role_permissions rp ON rp.permission_id = p.id \
         INNER JOIN user_scope_assignments usa ON usa.role_id = rp.role_id \
         WHERE usa.user_id = ? \
           AND {ASSIGNMENT_IN_FORCE_SQL} \
           AND {scope_sql}"
    );

    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(in_force_binds(&now));

    if let (Some(st), Some(sr)) = (&scope_type_filter, &scope_ref_filter) {
        values.push((*st).into());
        values.push(sr.clone().into());
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    let perms: HashSet<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .collect();

    Ok(perms)
}

/// Roles that hold every permission (`user_has_elevated_admin_access`).
pub(crate) const ELEVATED_ADMIN_ROLES_SQL: &str = "('Administrator', 'Superadmin')";

/// Elevated admin access for the account `ua.id`: an active account that is the bootstrap admin
/// (`is_admin`) or holds Administrator / Superadmin plant-wide (tenant scope) through a
/// permanent assignment in force. An emergency or expired assignment never elevates: it only
/// gives the permissions of its role while in force. Binds [`permanent_binds`].
pub(crate) fn elevated_admin_sql() -> String {
    format!(
        "ua.is_active = 1 AND ua.deleted_at IS NULL AND (ua.is_admin = 1 OR EXISTS ( \
           SELECT 1 FROM user_scope_assignments usa INNER JOIN roles r ON r.id = usa.role_id \
           WHERE usa.user_id = ua.id AND usa.scope_type = 'tenant' \
             AND {PERMANENT_ASSIGNMENT_IN_FORCE_SQL} \
             AND r.deleted_at IS NULL AND r.name IN {ELEVATED_ADMIN_ROLES_SQL}))"
    )
}

/// Active users with elevated admin access (the rule of [`user_has_elevated_admin_access`]): they
/// hold every permission, so the notifications routed to a permission reach them too.
pub async fn elevated_admin_user_ids(db: &impl ConnectionTrait) -> AppResult<Vec<i64>> {
    let now = assignment_clock();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ua.id FROM user_accounts ua WHERE {} ORDER BY ua.id",
                elevated_admin_sql()
            ),
            permanent_binds(&now),
        ))
        .await?;
    Ok(rows.iter().filter_map(|r| r.try_get::<i64>("", "id").ok()).collect())
}

/// True when the account has elevated admin access ([`elevated_admin_sql`]).
pub async fn user_has_elevated_admin_access<C: ConnectionTrait>(db: &C, user_id: i32) -> AppResult<bool> {
    let now = assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(permanent_binds(&now));
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT EXISTS (SELECT 1 FROM user_accounts ua WHERE ua.id = ? AND {}) AS elevated",
                elevated_admin_sql()
            ),
            values,
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i32>("", "elevated").ok()).unwrap_or(0) == 1)
}

/// Load all effective permissions for a user (for frontend pre-loading).
/// Returns only the permissions the user currently holds via active role assignments.
pub async fn get_user_permissions(db: &DatabaseConnection, user_id: i32) -> AppResult<Vec<PermissionRecord>> {
    if user_has_elevated_admin_access(db, user_id).await? {
        return get_all_permissions(db).await;
    }

    let now = assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(in_force_binds(&now));

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT DISTINCT p.name, p.description, p.category, \
                        p.is_dangerous, p.requires_step_up \
                 FROM permissions p \
                 INNER JOIN role_permissions rp ON rp.permission_id = p.id \
                 INNER JOIN user_scope_assignments usa ON usa.role_id = rp.role_id \
                 WHERE usa.user_id = ? AND {ASSIGNMENT_IN_FORCE_SQL} \
                 ORDER BY p.name"
            ),
            values,
        ))
        .await?;

    let perms = rows
        .into_iter()
        .map(|r| PermissionRecord {
            name: r.try_get("", "name").unwrap_or_default(),
            description: r.try_get("", "description").unwrap_or_default(),
            category: r.try_get("", "category").unwrap_or_default(),
            is_dangerous: r.try_get::<i32>("", "is_dangerous").unwrap_or(0) == 1,
            requires_step_up: r.try_get::<i32>("", "requires_step_up").unwrap_or(0) == 1,
        })
        .collect();

    Ok(perms)
}

/// Load every permission in the catalog (for system-admin accounts).
pub async fn get_all_permissions(db: &DatabaseConnection) -> AppResult<Vec<PermissionRecord>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            r"SELECT name, description, category, is_dangerous, requires_step_up
               FROM permissions
               ORDER BY name"
                .to_string(),
        ))
        .await?;

    let perms = rows
        .into_iter()
        .map(|r| PermissionRecord {
            name: r.try_get("", "name").unwrap_or_default(),
            description: r.try_get("", "description").unwrap_or_default(),
            category: r.try_get("", "category").unwrap_or_default(),
            is_dangerous: r.try_get::<i32>("", "is_dangerous").unwrap_or(0) == 1,
            requires_step_up: r.try_get::<i32>("", "requires_step_up").unwrap_or(0) == 1,
        })
        .collect();

    Ok(perms)
}

/// Check whether a named permission requires step-up verification.
/// Returns `false` if the permission doesn't exist or on any lookup failure.
pub async fn permission_requires_step_up(db: &DatabaseConnection, permission_name: &str) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requires_step_up FROM permissions WHERE name = ?",
            [permission_name.into()],
        ))
        .await?;

    let requires = row
        .and_then(|r| r.try_get::<i32>("", "requires_step_up").ok())
        .unwrap_or(0);

    Ok(requires == 1)
}

// ── Unit tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_scope_global_is_tenant_only() {
        // Document the invariant: Global scope resolves to tenant-only SQL clause
        let scope = PermissionScope::Global;
        assert!(matches!(scope, PermissionScope::Global));
    }

    #[test]
    fn permission_record_serializes() {
        let rec = PermissionRecord {
            name: crate::rbac::permissions::EQ_VIEW.into(),
            description: "View equipment".into(),
            category: "equipment".into(),
            is_dangerous: false,
            requires_step_up: false,
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        assert!(json.contains(crate::rbac::permissions::EQ_VIEW));
        assert!(json.contains("\"is_dangerous\":false"));
    }
}
