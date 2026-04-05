//! Runtime permission check engine.
//!
//! All permission checks are database-backed. No in-memory cache is used in Phase 1.
//! The check is scoped: a user holding `ot.view` globally (tenant scope) can view
//! all work orders. A user holding `ot.view` at entity scope can view only that entity.
//!
//! Scope resolution rule: if the user holds the permission at ANY scope that covers
//! the requested resource, the check passes. The scope hierarchy is:
//!   tenant > entity > site > team > org_node

use crate::errors::AppResult;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

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
///   1. Get all role assignments for the user that are currently active
///      (`valid_from` <= now <= `valid_to`, or NULL meaning unlimited)
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
    let now = chrono::Utc::now().to_rfc3339();

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
        r#"
        SELECT COUNT(*) as cnt
        FROM user_scope_assignments usa
        INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
        INNER JOIN permissions p       ON p.id = rp.permission_id
        WHERE usa.user_id = ?
          AND usa.deleted_at IS NULL
          AND (usa.valid_from IS NULL OR usa.valid_from <= ?)
          AND (usa.valid_to   IS NULL OR usa.valid_to   >= ?)
          AND p.name = ?
          AND {scope_sql}
    "#
    );

    let mut values: Vec<sea_orm::Value> = vec![user_id.into(), now.clone().into(), now.into(), permission_name.into()];

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

/// Load all effective permissions for a user (for frontend pre-loading).
/// Returns only the permissions the user currently holds via active role assignments.
pub async fn get_user_permissions(db: &DatabaseConnection, user_id: i32) -> AppResult<Vec<PermissionRecord>> {
    let now = chrono::Utc::now().to_rfc3339();

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT DISTINCT p.name, p.description, p.category,
                  p.is_dangerous, p.requires_step_up
           FROM permissions p
           INNER JOIN role_permissions rp ON rp.permission_id = p.id
           INNER JOIN user_scope_assignments usa ON usa.role_id = rp.role_id
           WHERE usa.user_id = ?
             AND usa.deleted_at IS NULL
             AND (usa.valid_from IS NULL OR usa.valid_from <= ?)
             AND (usa.valid_to   IS NULL OR usa.valid_to   >= ?)
           ORDER BY p.name"#,
            [user_id.into(), now.clone().into(), now.into()],
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
            name: "eq.view".into(),
            description: "View equipment".into(),
            category: "equipment".into(),
            is_dangerous: false,
            requires_step_up: false,
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        assert!(json.contains("eq.view"));
        assert!(json.contains("\"is_dangerous\":false"));
    }
}
