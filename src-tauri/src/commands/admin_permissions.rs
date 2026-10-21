//! Permission Catalog administration IPC commands — Phase 2 SP06-F02.
//!
//! Permission gates:
//!   adm.permissions — list_permissions, get_permission_dependencies,
//!                     create_custom_permission
//!   adm.roles       — validate_role_permissions

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::rbac::resolver;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

// ═══════════════════════════════════════════════════════════════════════════════
//  DTOs — inputs and outputs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct PermissionListFilter {
    pub category: Option<String>,
    pub is_dangerous: Option<bool>,
    pub search: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PermissionWithSystem {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub is_dangerous: bool,
    pub requires_step_up: bool,
    pub is_system: bool,
}

#[derive(Debug, Serialize)]
pub struct PermissionDependencyRow {
    pub id: i64,
    pub permission_name: String,
    pub required_permission_name: String,
    pub dependency_type: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCustomPermissionInput {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ValidateRolePermissionsInput {
    pub permission_names: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MissingDependency {
    pub permission_name: String,
    pub required_permission_name: String,
    pub dependency_type: String,
}

#[derive(Debug, Serialize)]
pub struct RoleValidationResult {
    pub missing_hard_deps: Vec<MissingDependency>,
    pub warn_deps: Vec<MissingDependency>,
    pub unknown_permissions: Vec<String>,
    pub is_valid: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  Commands
// ═══════════════════════════════════════════════════════════════════════════════

/// List all permissions with optional filtering by category, danger flag, or
/// free-text search on name/description.  Sorted by category then name.
#[tauri::command]
pub async fn list_permissions(
    filter: PermissionListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PermissionWithSystem>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_PERMISSIONS,
        PermissionScope::Global
    );

    let mut conditions = Vec::<String>::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref cat) = filter.category {
        conditions.push("p.category = ?".to_owned());
        values.push(cat.clone().into());
    }
    if let Some(dangerous) = filter.is_dangerous {
        conditions.push("p.is_dangerous = ?".to_owned());
        values.push(i32::from(dangerous).into());
    }
    if let Some(ref search) = filter.search {
        conditions.push("(p.name LIKE ? OR p.description LIKE ?)".to_owned());
        let pattern = format!("%{search}%");
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT p.id, p.name, p.description, p.category, \
                p.is_dangerous, p.requires_step_up, p.is_system \
         FROM permissions p \
         {where_clause} \
         ORDER BY p.category, p.name"
    );

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    let mut permissions = Vec::with_capacity(rows.len());
    for r in &rows {
        permissions.push(PermissionWithSystem {
            id: r.try_get("", "id")?,
            name: r.try_get("", "name")?,
            description: r.try_get("", "description").ok(),
            category: r.try_get("", "category")?,
            is_dangerous: r.try_get::<i32>("", "is_dangerous")? != 0,
            requires_step_up: r.try_get::<i32>("", "requires_step_up")? != 0,
            is_system: r.try_get::<i32>("", "is_system")? != 0,
        });
    }

    Ok(permissions)
}

/// Return all dependency rows where the given permission appears as either
/// the dependent or the required side.
#[tauri::command]
pub async fn get_permission_dependencies(
    permission_name: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<PermissionDependencyRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_PERMISSIONS,
        PermissionScope::Global
    );

    let sql = "SELECT id, permission_name, required_permission_name, dependency_type \
               FROM permission_dependencies \
               WHERE permission_name = ? OR required_permission_name = ?";

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [permission_name.clone().into(), permission_name.into()],
        ))
        .await?;

    let mut deps = Vec::with_capacity(rows.len());
    for r in &rows {
        deps.push(PermissionDependencyRow {
            id: r.try_get("", "id")?,
            permission_name: r.try_get("", "permission_name")?,
            required_permission_name: r.try_get("", "required_permission_name")?,
            dependency_type: r.try_get("", "dependency_type")?,
        });
    }

    Ok(deps)
}

/// Reserved system-namespace prefixes.  Custom permissions must NOT start with
/// any of these; they must use the `cst.` prefix.
const SYSTEM_PREFIXES: &[&str] = &[
    "eq.", "di.", "ot.", "org.", "per.", "ref.", "inv.", "pm.", "ram.", "rep.", "arc.", "doc.", "plan.", "log.",
    "trn.", "iot.", "erp.", "ptw.", "fin.", "ins.", "cfg.", "adm.",
];

/// Create a tenant-defined custom permission.
///
/// Enforces:
/// - Name must start with `cst.`
/// - Name must not collide with a system namespace prefix
/// - `is_dangerous` = 0, `requires_step_up` = 0, `is_system` = 0
/// - Requires step-up authentication
#[tauri::command]
pub async fn create_custom_permission(
    input: CreateCustomPermissionInput,
    state: State<'_, AppState>,
) -> AppResult<PermissionWithSystem> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_PERMISSIONS,
        PermissionScope::Global
    );
    require_step_up!(state);

    let name = input.name.trim().to_lowercase();

    // ── Validate `cst.` prefix ───────────────────────────────────────────
    if !name.starts_with("cst.") {
        return Err(AppError::ValidationFailed(vec![
            "Custom permission name must start with 'cst.' prefix".to_owned(),
        ]));
    }

    // ── Ensure no system prefix collision ────────────────────────────────
    for prefix in SYSTEM_PREFIXES {
        if name.starts_with(prefix) {
            return Err(AppError::ValidationFailed(vec![format!(
                "Permission name must not use reserved system prefix '{prefix}'"
            )]));
        }
    }

    // ── Minimum length after prefix ──────────────────────────────────────
    if name.len() < 5 {
        return Err(AppError::ValidationFailed(vec![
            "Permission name must have at least one character after 'cst.' prefix".to_owned(),
        ]));
    }

    let category = input.category.unwrap_or_else(|| "custom".to_owned());
    let description = input.description.unwrap_or_default();

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO permissions (name, description, category, is_dangerous, requires_step_up, is_system, created_at) \
             VALUES (?, ?, ?, 0, 0, 0, datetime('now'))",
            vec![
                name.clone().into(),
                description.into(),
                category.into(),
            ],
        ))
        .await
        .map_err(|e: sea_orm::DbErr| {
            if e.to_string().contains("UNIQUE") {
                AppError::ValidationFailed(vec![format!(
                    "Permission '{name}' already exists"
                )])
            } else {
                AppError::Database(e)
            }
        })?;

    // Fetch the inserted row to return full data including generated id
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, description, category, is_dangerous, requires_step_up, is_system \
             FROM permissions WHERE name = ?",
            [name.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to read back created permission")))?;

    let created_name: String = row.try_get("", "name")?;
    crate::commands::admin_governance::write_admin_event(
        &state.db,
        i64::from(user.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "permission_created",
            summary: Some(format!("Custom permission '{created_name}' created")),
            step_up_used: true,
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;

    Ok(PermissionWithSystem {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        description: row.try_get("", "description").ok(),
        category: row.try_get("", "category")?,
        is_dangerous: false,
        requires_step_up: false,
        is_system: false,
    })
}

/// Validate a proposed set of permissions for a role, checking hard and warn
/// dependencies plus unknown names.
#[tauri::command]
pub async fn validate_role_permissions(
    input: ValidateRolePermissionsInput,
    state: State<'_, AppState>,
) -> AppResult<RoleValidationResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_ROLES,
        PermissionScope::Global
    );

    let names: HashSet<String> = input.permission_names.into_iter().collect();

    // ── 1. Find unknown permissions ──────────────────────────────────────
    let unknown_permissions = if names.is_empty() {
        Vec::new()
    } else {
        let placeholders: String = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT name FROM permissions WHERE name IN ({placeholders})");
        let values: Vec<sea_orm::Value> = names.iter().map(|n| n.clone().into()).collect();

        let rows = state
            .db
            .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
            .await?;

        let mut known = HashSet::new();
        for r in &rows {
            if let Ok(n) = r.try_get::<String>("", "name") {
                known.insert(n);
            }
        }

        names.difference(&known).cloned().collect::<Vec<_>>()
    };

    // ── 2. Fetch dependency rows for the proposed set ────────────────────
    let deps = resolver::dependency_warnings_for(&state.db, &names).await?;

    let mut missing_hard_deps = Vec::new();
    let mut warn_deps = Vec::new();

    for dep in &deps {
        if !names.contains(&dep.required_permission_name) {
            let entry = MissingDependency {
                permission_name: dep.permission_name.clone(),
                required_permission_name: dep.required_permission_name.clone(),
                dependency_type: dep.dependency_type.clone(),
            };
            if dep.dependency_type == "hard" {
                missing_hard_deps.push(entry);
            } else {
                warn_deps.push(entry);
            }
        }
    }

    let is_valid = missing_hard_deps.is_empty() && unknown_permissions.is_empty();

    Ok(RoleValidationResult {
        missing_hard_deps,
        warn_deps,
        unknown_permissions,
        is_valid,
    })
}

/// Users currently holding a role (valid, non-emergency or unexpired emergency assignments).
#[derive(Debug, Serialize)]
pub struct RoleHolderCount {
    pub role_id: i64,
    pub active_users: i64,
}

/// Everything the role editor needs, readable with `adm.roles` alone: the permission catalog
/// (with descriptions and danger flags), the declared dependencies between permissions, and
/// how many active users hold each role.
#[derive(Debug, Serialize)]
pub struct RoleEditorCatalog {
    pub permissions: Vec<PermissionWithSystem>,
    pub dependencies: Vec<PermissionDependencyRow>,
    pub role_holders: Vec<RoleHolderCount>,
}

#[tauri::command]
pub async fn get_role_editor_catalog(state: State<'_, AppState>) -> AppResult<RoleEditorCatalog> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_ROLES,
        PermissionScope::Global
    );
    role_editor_catalog(&state.db).await
}

pub(crate) async fn role_editor_catalog(db: &sea_orm::DatabaseConnection) -> AppResult<RoleEditorCatalog> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, name, description, category, is_dangerous, requires_step_up, is_system \
             FROM permissions ORDER BY category, name"
                .to_string(),
        ))
        .await?;
    let mut permissions = Vec::with_capacity(rows.len());
    for r in &rows {
        permissions.push(PermissionWithSystem {
            id: r.try_get("", "id")?,
            name: r.try_get("", "name")?,
            description: r.try_get::<Option<String>>("", "description")?,
            category: r.try_get("", "category")?,
            is_dangerous: r.try_get::<i32>("", "is_dangerous")? != 0,
            requires_step_up: r.try_get::<i32>("", "requires_step_up")? != 0,
            is_system: r.try_get::<i32>("", "is_system")? != 0,
        });
    }

    let dep_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, permission_name, required_permission_name, dependency_type \
             FROM permission_dependencies ORDER BY permission_name, required_permission_name"
                .to_string(),
        ))
        .await?;
    let mut dependencies = Vec::with_capacity(dep_rows.len());
    for r in &dep_rows {
        dependencies.push(PermissionDependencyRow {
            id: r.try_get("", "id")?,
            permission_name: r.try_get("", "permission_name")?,
            required_permission_name: r.try_get("", "required_permission_name")?,
            dependency_type: r.try_get("", "dependency_type")?,
        });
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let holder_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT usa.role_id, COUNT(DISTINCT usa.user_id) AS cnt \
             FROM user_scope_assignments usa \
             INNER JOIN user_accounts ua ON ua.id = usa.user_id AND ua.is_active = 1 AND ua.deleted_at IS NULL \
             WHERE usa.deleted_at IS NULL \
               AND (usa.valid_from IS NULL OR usa.valid_from <= ?) \
               AND (usa.valid_to IS NULL OR usa.valid_to >= ?) \
               AND (usa.is_emergency = 0 OR usa.emergency_expires_at > ?) \
             GROUP BY usa.role_id",
            [now.clone().into(), now.clone().into(), now.into()],
        ))
        .await?;
    let mut role_holders = Vec::with_capacity(holder_rows.len());
    for r in &holder_rows {
        role_holders.push(RoleHolderCount {
            role_id: r.try_get("", "role_id")?,
            active_users: r.try_get("", "cnt")?,
        });
    }

    Ok(RoleEditorCatalog {
        permissions,
        dependencies,
        role_holders,
    })
}
