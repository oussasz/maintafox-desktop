//! Delegation boundary validation — Phase 2 SP06-F03.
//!
//! Determines whether a delegated admin is authorised to assign a specific
//! permission at a given scope, based on `delegated_admin_policies` rows.
//!
//! Two entry points:
//! - [`can_delegate_permission`] — full async check against the database
//! - [`validate_delegation_boundary`] — pure sync check on a loaded policy

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppResult;
use super::model::DelegatedAdminPolicy;

/// Check whether `delegator_user_id` is permitted — via delegation policies —
/// to assign `permission_name` to `target_user_id` at the given scope.
///
/// Algorithm:
/// 1. Find all `delegated_admin_policies` where the `admin_role_id` is held
///    by the delegator (via `user_scope_assignments`).
/// 2. For each matching policy, check scope alignment: the policy's
///    `managed_scope_type` must match `target_scope_type`, and
///    `managed_scope_reference` must match (or be NULL = unrestricted).
/// 3. Check that the permission's domain (prefix up to first `.`) is in the
///    policy's `allowed_domains_json` array.
/// 4. Return `true` only if at least one policy satisfies all three checks.
pub async fn can_delegate_permission(
    db: &DatabaseConnection,
    delegator_user_id: i64,
    _target_user_id: i64,
    permission_name: &str,
    target_scope_type: &str,
    target_scope_reference: Option<&str>,
) -> AppResult<bool> {
    // Extract the domain prefix (e.g. "ot" from "ot.create")
    let domain = match permission_name.split('.').next() {
        Some(d) => d,
        None => return Ok(false),
    };

    // Find all delegation policies whose admin_role_id is held by the delegator.
    let policies = load_policies_for_user(db, delegator_user_id).await?;

    for policy in &policies {
        // Scope type must match
        if policy.managed_scope_type != target_scope_type {
            continue;
        }

        // Scope reference: if policy has a specific reference, it must match;
        // NULL in policy = unrestricted within the scope type.
        if let Some(ref policy_ref) = policy.managed_scope_reference {
            match target_scope_reference {
                Some(target_ref) if target_ref == policy_ref => {}
                _ => continue,
            }
        }

        // Domain must be in the allowed_domains_json array
        if validate_delegation_boundary(policy, domain) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Pure validation: does `permission_domain` appear in the policy's
/// `allowed_domains_json` array?
///
/// `permission_domain` should be the prefix up to the first `.`
/// (e.g. `"ot"` for `"ot.create"`), or a full permission name — in which case
/// the domain is extracted automatically.
pub fn validate_delegation_boundary(
    policy: &DelegatedAdminPolicy,
    permission_name: &str,
) -> bool {
    let domain = permission_name
        .split('.')
        .next()
        .unwrap_or(permission_name);

    // Parse allowed_domains_json as a JSON array of strings
    let domains: Vec<String> = serde_json::from_str(&policy.allowed_domains_json)
        .unwrap_or_default();

    domains.iter().any(|d| d == domain)
}

/// Load all `delegated_admin_policies` whose `admin_role_id` is held by the
/// given user through an active `user_scope_assignments` row.
async fn load_policies_for_user(
    db: &DatabaseConnection,
    user_id: i64,
) -> AppResult<Vec<DelegatedAdminPolicy>> {
    let sql = "\
        SELECT dap.id, dap.admin_role_id, dap.managed_scope_type, \
               dap.managed_scope_reference, dap.allowed_domains_json, \
               dap.requires_step_up_for_publish \
        FROM delegated_admin_policies dap \
        INNER JOIN user_scope_assignments usa \
          ON usa.role_id = dap.admin_role_id \
        WHERE usa.user_id = ? \
          AND usa.deleted_at IS NULL \
          AND (usa.is_emergency = 0 OR usa.emergency_expires_at > datetime('now'))";

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [user_id.into()],
        ))
        .await?;

    let policies = rows
        .iter()
        .filter_map(|r| {
            Some(DelegatedAdminPolicy {
                id: r.try_get("", "id").ok()?,
                admin_role_id: r.try_get("", "admin_role_id").ok()?,
                managed_scope_type: r.try_get("", "managed_scope_type").ok()?,
                managed_scope_reference: r.try_get("", "managed_scope_reference").ok(),
                allowed_domains_json: r
                    .try_get("", "allowed_domains_json")
                    .ok()
                    .unwrap_or_else(|| "[]".to_string()),
                requires_step_up_for_publish: r
                    .try_get::<i32>("", "requires_step_up_for_publish")
                    .ok()
                    .map(|v| v != 0)
                    .unwrap_or(true),
            })
        })
        .collect();

    Ok(policies)
}
