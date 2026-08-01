//! Live permission resolver — Phase 2 SP06-F01 / F02.
//!
//! Resolves the effective permission set for a user at a given scope by
//! traversing `user_scope_assignments` → `role_permissions` → `permissions`.
//!
//! Design notes:
//! - Uses `sea_orm::DatabaseConnection` and raw SQL (same pattern as `auth::rbac`).
//! - `deleted_at IS NULL` filter on `user_scope_assignments` honours soft-deletes.
//! - Emergency grants are included only while `emergency_expires_at > now`.
//! - Tenant-scoped assignments are always included regardless of requested scope.
//! - SP06-F02 adds `effective_permissions_for_node` which resolves the org
//!   hierarchy upward via `scope_chain::resolve_scope_chain` and matches
//!   assignments at any ancestor scope.

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

/// Compute the full set of permission names that `user_id` holds at the
/// requested scope.
///
/// The algorithm:
/// 1. Collect all `role_id`s from `user_scope_assignments` where:
///    - the assignment is not soft-deleted
///    - `valid_from` ≤ today (or NULL)
///    - `valid_to` ≥ today (or NULL)
///    - scope is `tenant` (always included) OR matches the requested scope
///    - non-emergency assignments always included; emergency ones only while
///      `emergency_expires_at > now`
/// 2. For those roles, select distinct permission names via `role_permissions`.
/// 3. Return the set.
///
/// Returns an **empty** `HashSet` when the user has no active assignments — not
/// an error.
pub async fn effective_permissions(
    db: &DatabaseConnection,
    user_id: i64,
    scope_type: &str,
    scope_reference: Option<&str>,
) -> AppResult<HashSet<String>> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // ── Step 1: collect active role_ids ───────────────────────────────────
    // Scope clause: always include tenant; if a specific scope is requested,
    // also include assignments that match exactly.
    let (scope_sql, values) = build_scope_query(user_id, scope_type, scope_reference, &now);

    let role_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &scope_sql,
            values,
        ))
        .await?;

    let role_ids: Vec<i64> = role_rows
        .iter()
        .filter_map(|r| r.try_get::<i64>("", "role_id").ok())
        .collect();

    if role_ids.is_empty() {
        return Ok(HashSet::new());
    }

    // ── Step 2: resolve permission names for collected roles ─────────────
    resolve_permissions_for_roles(db, &role_ids).await
}

/// Check whether `user_id` holds a specific `permission` at the given scope.
///
/// This is a thin wrapper around [`effective_permissions`] — it fetches the
/// full set and checks membership.  For hot-path single-permission checks the
/// existing `auth::rbac::check_permission` (COUNT-based) is more efficient;
/// this function is intended for cases where the full set is already needed or
/// the caller wants the simpler `bool` API.
pub async fn user_has_permission(
    db: &DatabaseConnection,
    user_id: i64,
    permission: &str,
    scope_type: &str,
    scope_reference: Option<&str>,
) -> AppResult<bool> {
    let perms = effective_permissions(db, user_id, scope_type, scope_reference).await?;
    Ok(perms.contains(permission))
}

// ── Scope-chain-aware resolution (SP06-F02) ──────────────────────────────────

/// Compute the effective permission set for `user_id` at the given org node,
/// taking into account all ancestor scopes via [`super::scope_chain::resolve_scope_chain`].
///
/// - `org_node_id = Some(id)` — resolve the hierarchy upward and include
///   assignments at any ancestor scope **plus** tenant-wide assignments.
/// - `org_node_id = None` — tenant-level operations (e.g. `adm.*`): only
///   tenant-scoped assignments are evaluated.
///
/// This is the preferred entry point for SP06-F02 onwards; the original
/// [`effective_permissions`] remains available for backward-compatible
/// call-sites that already supply `(scope_type, scope_reference)`.
pub async fn effective_permissions_for_node(
    db: &DatabaseConnection,
    user_id: i64,
    org_node_id: Option<i64>,
) -> AppResult<HashSet<String>> {
    match org_node_id {
        None => {
            // Tenant-level: only tenant-scoped assignments
            effective_permissions(db, user_id, "tenant", None).await
        }
        Some(node_id) => {
            let chain = super::scope_chain::resolve_scope_chain(db, node_id).await?;
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

            // Build scope matching clause:
            //   tenant-wide assignments  OR
            //   assignments whose scope_reference matches any node in the chain
            let scope_refs = chain.scope_references();

            if scope_refs.is_empty() {
                // Only the synthetic tenant node — fall back to tenant-only
                return effective_permissions(db, user_id, "tenant", None).await;
            }

            let ref_placeholders: String =
                scope_refs.iter().map(|_| "?").collect::<Vec<_>>().join(",");

            let sql = format!(
                "SELECT DISTINCT usa.role_id \
                 FROM user_scope_assignments usa \
                 WHERE usa.user_id = ? \
                   AND usa.deleted_at IS NULL \
                   AND (usa.valid_from IS NULL OR usa.valid_from <= ?) \
                   AND (usa.valid_to   IS NULL OR usa.valid_to   >= ?) \
                   AND ( \
                       usa.scope_type = 'tenant' \
                       OR usa.scope_reference IN ({ref_placeholders}) \
                   ) \
                   AND (usa.is_emergency = 0 OR usa.emergency_expires_at > ?)"
            );

            let mut values: Vec<sea_orm::Value> = Vec::with_capacity(4 + scope_refs.len());
            values.push(user_id.into());
            values.push(now.clone().into());
            values.push(now.clone().into());
            for r in &scope_refs {
                values.push(r.clone().into());
            }
            values.push(now.into());

            let role_rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    &sql,
                    values,
                ))
                .await?;

            let role_ids: Vec<i64> = role_rows
                .iter()
                .filter_map(|r| r.try_get::<i64>("", "role_id").ok())
                .collect();

            if role_ids.is_empty() {
                return Ok(HashSet::new());
            }

            resolve_permissions_for_roles(db, &role_ids).await
        }
    }
}

/// Check whether `user_id` holds a specific `permission` at the given org
/// node (scope-chain-aware).
pub async fn user_has_permission_at_node(
    db: &DatabaseConnection,
    user_id: i64,
    permission: &str,
    org_node_id: Option<i64>,
) -> AppResult<bool> {
    let perms = effective_permissions_for_node(db, user_id, org_node_id).await?;
    Ok(perms.contains(permission))
}

/// Load all `PermissionDependency` rows whose `permission_name` appears in a
/// given set.  Used by role-editing commands to check for missing hard deps.
pub async fn dependency_warnings_for(
    db: &DatabaseConnection,
    permission_names: &HashSet<String>,
) -> AppResult<Vec<super::model::PermissionDependency>> {
    if permission_names.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: String = permission_names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, permission_name, required_permission_name, dependency_type \
         FROM permission_dependencies \
         WHERE permission_name IN ({placeholders})"
    );

    let values: Vec<sea_orm::Value> = permission_names
        .iter()
        .map(|n| n.clone().into())
        .collect();

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    let deps = rows
        .iter()
        .filter_map(|r| {
            Some(super::model::PermissionDependency {
                id: r.try_get("", "id").ok()?,
                permission_name: r.try_get("", "permission_name").ok()?,
                required_permission_name: r.try_get("", "required_permission_name").ok()?,
                dependency_type: r.try_get("", "dependency_type").ok()?,
            })
        })
        .collect();

    Ok(deps)
}

/// Validate that all hard dependencies are satisfied within `names`.
/// Returns an `Err` listing the missing required permissions if any hard
/// dependency is unsatisfied.
pub async fn validate_hard_dependencies(
    db: &DatabaseConnection,
    names: &HashSet<String>,
) -> AppResult<()> {
    let deps = dependency_warnings_for(db, names).await?;

    let missing: Vec<String> = deps
        .iter()
        .filter(|d| d.dependency_type == "hard" && !names.contains(&d.required_permission_name))
        .map(|d| {
            format!(
                "'{}' requires '{}' (hard dependency)",
                d.permission_name, d.required_permission_name
            )
        })
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(missing))
    }
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Given a set of `role_id`s, resolve the distinct permission names they carry.
/// Shared by both the legacy `effective_permissions` path and the new
/// scope-chain-aware `effective_permissions_for_node`.
async fn resolve_permissions_for_roles(
    db: &DatabaseConnection,
    role_ids: &[i64],
) -> AppResult<HashSet<String>> {
    if role_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders: String = role_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

    let perm_sql = format!(
        "SELECT DISTINCT p.name \
         FROM permissions p \
         INNER JOIN role_permissions rp ON rp.permission_id = p.id \
         WHERE rp.role_id IN ({placeholders})"
    );

    let perm_values: Vec<sea_orm::Value> = role_ids.iter().copied().map(Into::into).collect();

    let perm_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &perm_sql,
            perm_values,
        ))
        .await?;

    let permissions: HashSet<String> = perm_rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .collect();

    Ok(permissions)
}

/// Build the SQL + values for Step 1 (role_id collection).
fn build_scope_query(
    user_id: i64,
    scope_type: &str,
    scope_reference: Option<&str>,
    now: &str,
) -> (String, Vec<sea_orm::Value>) {
    let scope_clause = if scope_type == "tenant" {
        "usa.scope_type = 'tenant'".to_owned()
    } else {
        "(usa.scope_type = 'tenant' OR (usa.scope_type = ? AND usa.scope_reference = ?))".to_owned()
    };

    let sql = format!(
        "SELECT DISTINCT usa.role_id \
         FROM user_scope_assignments usa \
         WHERE usa.user_id = ? \
           AND usa.deleted_at IS NULL \
           AND (usa.valid_from IS NULL OR usa.valid_from <= ?) \
           AND (usa.valid_to   IS NULL OR usa.valid_to   >= ?) \
           AND ({scope_clause}) \
           AND (usa.is_emergency = 0 OR usa.emergency_expires_at > ?)"
    );

    let mut values: Vec<sea_orm::Value> = vec![
        user_id.into(),
        now.to_owned().into(),
        now.to_owned().into(),
    ];

    if scope_type != "tenant" {
        values.push(scope_type.to_owned().into());
        values.push(scope_reference.unwrap_or("").to_owned().into());
    }

    values.push(now.to_owned().into());

    (sql, values)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_scope_query_tenant_only() {
        let (sql, vals) = build_scope_query(1, "tenant", None, "2026-04-12T00:00:00Z");
        assert!(sql.contains("usa.scope_type = 'tenant'"));
        // user_id + 2× now + emergency now = 4 values
        assert_eq!(vals.len(), 4);
    }

    #[test]
    fn build_scope_query_entity_scope() {
        let (sql, vals) = build_scope_query(1, "entity", Some("org-42"), "2026-04-12T00:00:00Z");
        assert!(sql.contains("usa.scope_type = ?"));
        // user_id + 2× now + scope_type + scope_ref + emergency now = 6 values
        assert_eq!(vals.len(), 6);
    }
}
