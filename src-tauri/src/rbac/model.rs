//! Core RBAC domain types — Phase 2 SP06-F01.
//!
//! Each struct mirrors its corresponding SQLite table and is designed for
//! use with `sea_orm` raw-query result mapping via `QueryResult::try_get`.
//! Types are also `Serialize` / `Deserialize` for IPC transport to the frontend.

use serde::{Deserialize, Serialize};

// ── roles ────────────────────────────────────────────────────────────────────

/// A row from the `roles` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub role_type: String,
    pub status: String,
    pub is_system: bool,
}

// ── permissions ──────────────────────────────────────────────────────────────

/// A row from the `permissions` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub is_dangerous: bool,
    pub requires_step_up: bool,
}

// ── user_accounts ────────────────────────────────────────────────────────────

/// A row from the `user_accounts` table (subset of columns relevant to RBAC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccountRow {
    pub id: i64,
    pub username: String,
    pub identity_mode: String,
    pub personnel_id: Option<i64>,
    pub is_active: bool,
    pub force_password_change: bool,
    pub last_seen_at: Option<String>,
}

// ── user_scope_assignments ───────────────────────────────────────────────────

/// A row from the `user_scope_assignments` table.
///
/// Column names match the actual schema created by migration 002 + 028:
/// - `assigned_by_id` (not `granted_by_id`)
/// - `is_emergency`, `emergency_reason`, `emergency_expires_at` added by mig 028
/// - `deleted_at` soft-delete, `sync_id`, `row_version` from mig 002
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserScopeAssignment {
    pub id: i64,
    pub user_id: i64,
    pub role_id: i64,
    pub scope_type: String,
    pub scope_reference: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub assigned_by_id: Option<i64>,
    pub notes: Option<String>,
    pub is_emergency: bool,
    pub emergency_reason: Option<String>,
    pub emergency_expires_at: Option<String>,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

// ── permission_dependencies ──────────────────────────────────────────────────

/// A row from the `permission_dependencies` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDependency {
    pub id: i64,
    pub permission_name: String,
    pub required_permission_name: String,
    pub dependency_type: String,
}

// ── role_templates ───────────────────────────────────────────────────────────

/// A row from the `role_templates` table (migration 028).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleTemplate {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub module_set_json: String,
    pub is_system: bool,
}

// ── delegated_admin_policies ─────────────────────────────────────────────────

/// A row from the `delegated_admin_policies` table (migration 028).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedAdminPolicy {
    pub id: i64,
    pub admin_role_id: i64,
    pub managed_scope_type: String,
    pub managed_scope_reference: Option<String>,
    pub allowed_domains_json: String,
    pub requires_step_up_for_publish: bool,
}
