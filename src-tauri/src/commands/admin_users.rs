//! User and Role administration IPC commands — Phase 2 SP06-F01.
//!
//! Permission gates:
//!   adm.users  — list_users, get_user, create_user, update_user, deactivate_user,
//!                assign_role_scope, revoke_role_scope, simulate_access
//!   adm.roles  — list_roles, get_role, create_role, update_role, delete_role,
//!                list_role_templates

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::rbac::cache::RbacChangedPayload;
use crate::rbac::{model, resolver};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

// ═══════════════════════════════════════════════════════════════════════════════
//  DTOs — inputs and outputs
// ═══════════════════════════════════════════════════════════════════════════════

// ── User DTOs ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RoleAssignmentSummary {
    pub assignment_id: i64,
    pub role_id: i64,
    pub role_name: String,
    pub scope_type: String,
    pub scope_reference: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub is_emergency: bool,
}

#[derive(Debug, Serialize)]
pub struct UserWithRoles {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub identity_mode: String,
    pub is_active: bool,
    pub force_password_change: bool,
    pub last_seen_at: Option<String>,
    pub roles: Vec<RoleAssignmentSummary>,
}

#[derive(Debug, Serialize)]
pub struct UserDetail {
    pub user: UserWithRoles,
    pub scope_assignments: Vec<model::UserScopeAssignment>,
    pub effective_permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserListFilter {
    pub is_active: Option<bool>,
    pub identity_mode: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserInput {
    pub username: String,
    pub identity_mode: String,
    pub personnel_id: Option<i64>,
    pub initial_password: Option<String>,
    pub force_password_change: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserInput {
    pub user_id: i64,
    pub username: Option<String>,
    pub personnel_id: Option<i64>,
    pub force_password_change: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AssignRoleScopeInput {
    pub user_id: i64,
    pub role_id: i64,
    pub scope_type: String,
    pub scope_reference: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

// ── Role DTOs ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RoleWithPermissions {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub role_type: String,
    pub status: String,
    pub is_system: bool,
    pub permissions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RoleDetail {
    pub role: RoleWithPermissions,
    pub dependency_warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleInput {
    pub name: String,
    pub description: Option<String>,
    pub permission_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleInput {
    pub role_id: i64,
    pub description: Option<String>,
    pub add_permissions: Option<Vec<String>>,
    pub remove_permissions: Option<Vec<String>>,
}

// ── Simulate DTOs ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SimulateAccessInput {
    pub user_id: i64,
    pub scope_type: String,
    pub scope_reference: Option<String>,
}

// ── Emergency Elevation DTOs ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GrantEmergencyElevationInput {
    pub user_id: i64,
    pub role_id: i64,
    pub scope_type: String,
    pub scope_reference: Option<String>,
    pub reason: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
pub struct RevokeEmergencyElevationInput {
    pub assignment_id: i64,
}

#[derive(Debug, Serialize)]
pub struct SimulateAccessResult {
    pub permissions: HashMap<String, bool>,
    pub assignments: Vec<model::UserScopeAssignment>,
    pub dependency_warnings: Vec<String>,
    pub blocked_by: Vec<String>,
}

// ── ID wrapper ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct IdPayload {
    pub id: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  RBAC change notification helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Invalidate the permission cache for a specific user and emit `rbac-changed`.
fn notify_rbac_user_change(app: &AppHandle, state: &AppState, user_id: i64, action: &str) {
    // Cache invalidation (sync since we have &AppState not State<>)
    if let Ok(mut cache) = state.permission_cache.try_write() {
        cache.invalidate_user(user_id);
    }
    // Emit event to frontend PermissionProvider
    let _ = app.emit(
        "rbac-changed",
        RbacChangedPayload {
            affected_user_id: Some(user_id),
            action: action.to_string(),
        },
    );
}

/// Invalidate the entire permission cache and emit `rbac-changed`.
fn notify_rbac_global_change(app: &AppHandle, state: &AppState, action: &str) {
    if let Ok(mut cache) = state.permission_cache.try_write() {
        cache.invalidate_all();
    }
    let _ = app.emit(
        "rbac-changed",
        RbacChangedPayload {
            affected_user_id: None,
            action: action.to_string(),
        },
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
//  USER COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

/// List users with their current role assignments.
#[tauri::command]
pub async fn list_users(
    filter: UserListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<UserWithRoles>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.users", PermissionScope::Global);

    // Build WHERE clauses dynamically
    let mut conditions = vec!["ua.deleted_at IS NULL".to_owned()];
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(active) = filter.is_active {
        conditions.push("ua.is_active = ?".to_owned());
        values.push(i32::from(active).into());
    }
    if let Some(ref mode) = filter.identity_mode {
        conditions.push("ua.identity_mode = ?".to_owned());
        values.push(mode.clone().into());
    }
    if let Some(ref search) = filter.search {
        conditions.push("(ua.username LIKE ? OR ua.display_name LIKE ?)".to_owned());
        let pattern = format!("%{search}%");
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT ua.id, ua.username, ua.display_name, ua.identity_mode, \
                ua.is_active, ua.force_password_change, ua.last_seen_at \
         FROM user_accounts ua \
         WHERE {where_clause} \
         ORDER BY ua.username ASC"
    );

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    let mut users = Vec::with_capacity(rows.len());
    for r in &rows {
        let uid: i64 = r.try_get("", "id")?;
        let roles = load_role_assignments(&state.db, uid).await?;
        users.push(UserWithRoles {
            id: uid,
            username: r.try_get("", "username")?,
            display_name: r.try_get("", "display_name").ok(),
            identity_mode: r.try_get("", "identity_mode")?,
            is_active: r.try_get::<i32>("", "is_active")? == 1,
            force_password_change: r.try_get::<i32>("", "force_password_change")? == 1,
            last_seen_at: r.try_get("", "last_seen_at").ok(),
            roles,
        });
    }

    Ok(users)
}

/// Get detailed user info including all scope assignments and effective permissions.
#[tauri::command]
pub async fn get_user(
    user_id: i64,
    state: State<'_, AppState>,
) -> AppResult<UserDetail> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, username, display_name, identity_mode, is_active, \
                    force_password_change, last_seen_at \
             FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UserAccount".into(),
            id: user_id.to_string(),
        })?;

    let roles = load_role_assignments(&state.db, user_id).await?;
    let scope_assignments = load_scope_assignments(&state.db, user_id).await?;
    let perms = resolver::effective_permissions(&state.db, user_id, "tenant", None).await?;
    let mut perm_list: Vec<String> = perms.into_iter().collect();
    perm_list.sort();

    Ok(UserDetail {
        user: UserWithRoles {
            id: user_id,
            username: row.try_get("", "username")?,
            display_name: row.try_get("", "display_name").ok(),
            identity_mode: row.try_get("", "identity_mode")?,
            is_active: row.try_get::<i32>("", "is_active")? == 1,
            force_password_change: row.try_get::<i32>("", "force_password_change")? == 1,
            last_seen_at: row.try_get("", "last_seen_at").ok(),
            roles,
        },
        scope_assignments,
        effective_permissions: perm_list,
    })
}

/// Create a new user account.
#[tauri::command]
pub async fn create_user(
    input: CreateUserInput,
    state: State<'_, AppState>,
) -> AppResult<IdPayload> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    // TODO: re-enable require_step_up!(state) once StepUpDialog UI is built (SP06-F03)

    // Validate identity_mode
    if !["local", "sso", "hybrid"].contains(&input.identity_mode.as_str()) {
        return Err(AppError::ValidationFailed(vec![
            "identity_mode must be one of: local, sso, hybrid".into(),
        ]));
    }

    // Password validation
    let password_hash = match input.identity_mode.as_str() {
        "local" => {
            let pw = input.initial_password.as_deref().ok_or_else(|| {
                AppError::ValidationFailed(vec![
                    "initial_password is required for local identity mode".into(),
                ])
            })?;
            validate_password_strength(pw)?;
            Some(crate::auth::password::hash_password(pw)?)
        }
        "sso" => {
            if input.initial_password.is_some() {
                return Err(AppError::ValidationFailed(vec![
                    "initial_password must not be provided for SSO identity mode".into(),
                ]));
            }
            None
        }
        "hybrid" => match input.initial_password.as_deref() {
            Some(pw) => {
                validate_password_strength(pw)?;
                Some(crate::auth::password::hash_password(pw)?)
            }
            None => None,
        },
        _ => unreachable!(),
    };

    let force_pw = input
        .force_password_change
        .unwrap_or(password_hash.is_some());

    // Check username uniqueness
    let exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE LOWER(username) = LOWER(?) AND deleted_at IS NULL",
            [input.username.clone().into()],
        ))
        .await?;

    if exists.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Username '{}' is already taken",
            input.username
        )]));
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sync_id = Uuid::new_v4().to_string();

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts \
                (sync_id, username, identity_mode, password_hash, personnel_id, \
                 is_active, is_admin, force_password_change, \
                 failed_login_attempts, created_at, updated_at, row_version) \
             VALUES (?, ?, ?, ?, ?, 1, 0, ?, 0, ?, ?, 1)",
            vec![
                sync_id.into(),
                input.username.into(),
                input.identity_mode.into(),
                password_hash.map_or(sea_orm::Value::String(None), |h| h.into()),
                input.personnel_id.map_or(sea_orm::Value::Int(None), |pid| (pid as i32).into()),
                i32::from(force_pw).into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;

    // Get the inserted ID
    let id_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to retrieve inserted user id")))?;

    let new_id: i64 = id_row.try_get("", "id")?;
    Ok(IdPayload { id: new_id })
}

/// Update an existing user account.
#[tauri::command]
pub async fn update_user(
    input: UpdateUserInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);

    // Guard: cannot deactivate self
    if input.is_active == Some(false) && i64::from(caller.user_id) == input.user_id {
        return Err(AppError::ValidationFailed(vec![
            "Cannot deactivate your own account".into(),
        ]));
    }

    // Guard: cannot deactivate last active superadmin
    if input.is_active == Some(false) {
        guard_last_superadmin(&state.db, input.user_id).await?;
    }

    let mut sets = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref username) = input.username {
        // Check uniqueness
        let dup = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE LOWER(username) = LOWER(?) AND id != ? AND deleted_at IS NULL",
                [username.clone().into(), input.user_id.into()],
            ))
            .await?;
        if dup.is_some() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Username '{username}' is already taken"
            )]));
        }
        sets.push("username = ?");
        values.push(username.clone().into());
    }
    if let Some(pid) = input.personnel_id {
        sets.push("personnel_id = ?");
        values.push((pid as i32).into());
    }
    if let Some(fpc) = input.force_password_change {
        sets.push("force_password_change = ?");
        values.push(i32::from(fpc).into());
    }
    if let Some(active) = input.is_active {
        sets.push("is_active = ?");
        values.push(i32::from(active).into());
    }

    if sets.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    sets.push("updated_at = ?");
    values.push(now.into());
    values.push(input.user_id.into());

    let set_clause = sets.join(", ");
    let sql = format!("UPDATE user_accounts SET {set_clause} WHERE id = ? AND deleted_at IS NULL");

    state
        .db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    Ok(())
}

/// Deactivate a user account (set is_active = 0).
#[tauri::command]
pub async fn deactivate_user(
    user_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    require_step_up!(state);

    // Guard: cannot deactivate self
    if i64::from(caller.user_id) == user_id {
        return Err(AppError::ValidationFailed(vec![
            "Cannot deactivate your own account".into(),
        ]));
    }

    guard_last_superadmin(&state.db, user_id).await?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET is_active = 0, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
            [now.into(), user_id.into()],
        ))
        .await?;

    notify_rbac_user_change(&app, &state, user_id, "user_deactivated");
    Ok(())
}

/// Assign a role to a user at a given scope.
#[tauri::command]
pub async fn assign_role_scope(
    input: AssignRoleScopeInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<IdPayload> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    require_step_up!(state);

    // Validate scope_type
    if !["tenant", "entity", "site", "team", "org_node"].contains(&input.scope_type.as_str()) {
        return Err(AppError::ValidationFailed(vec![
            "scope_type must be one of: tenant, entity, site, team, org_node".into(),
        ]));
    }

    // Check user exists
    let user_exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [input.user_id.into()],
        ))
        .await?;
    if user_exists.is_none() {
        return Err(AppError::NotFound {
            entity: "UserAccount".into(),
            id: input.user_id.to_string(),
        });
    }

    // Check role exists
    let role_exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE id = ? AND deleted_at IS NULL",
            [input.role_id.into()],
        ))
        .await?;
    if role_exists.is_none() {
        return Err(AppError::NotFound {
            entity: "Role".into(),
            id: input.role_id.to_string(),
        });
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sync_id = Uuid::new_v4().to_string();

    // INSERT — the UNIQUE index (uidx_usa_user_role_scope) prevents duplicates
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
                (sync_id, user_id, role_id, scope_type, scope_reference, \
                 valid_from, valid_to, assigned_by_id, \
                 created_at, updated_at, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            vec![
                sync_id.into(),
                input.user_id.into(),
                input.role_id.into(),
                input.scope_type.into(),
                input.scope_reference.map_or(sea_orm::Value::String(None), |s| s.into()),
                input.valid_from.map_or(sea_orm::Value::String(None), |s| s.into()),
                input.valid_to.map_or(sea_orm::Value::String(None), |s| s.into()),
                i64::from(caller.user_id).into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                AppError::ValidationFailed(vec![
                    "This user already has this role assigned at the same scope".into(),
                ])
            } else {
                AppError::Database(e)
            }
        })?;

    let id_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to retrieve assignment id")))?;

    let new_id: i64 = id_row.try_get("", "id")?;
    notify_rbac_user_change(&app, &state, input.user_id, "role_assigned");
    Ok(IdPayload { id: new_id })
}

/// Revoke a role-scope assignment (soft-delete).
#[tauri::command]
pub async fn revoke_role_scope(
    assignment_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    require_step_up!(state);

    // Look up the affected user_id before soft-deleting
    let affected_user_id = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT user_id FROM user_scope_assignments WHERE id = ? AND deleted_at IS NULL",
            [assignment_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "user_id").ok());

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let result = state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_scope_assignments SET deleted_at = ?, updated_at = ? \
             WHERE id = ? AND deleted_at IS NULL",
            [now.clone().into(), now.into(), assignment_id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "UserScopeAssignment".into(),
            id: assignment_id.to_string(),
        });
    }

    if let Some(uid) = affected_user_id {
        notify_rbac_user_change(&app, &state, uid, "role_revoked");
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
//  ROLE COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

/// List all roles with their permission names.
#[tauri::command]
pub async fn list_roles(state: State<'_, AppState>) -> AppResult<Vec<RoleWithPermissions>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.roles", PermissionScope::Global);

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, description, role_type, status, is_system \
             FROM roles WHERE deleted_at IS NULL ORDER BY is_system DESC, name ASC",
            [],
        ))
        .await?;

    let mut roles = Vec::with_capacity(rows.len());
    for r in &rows {
        let role_id: i64 = r.try_get("", "id")?;
        let perms = load_role_permission_names(&state.db, role_id).await?;
        roles.push(RoleWithPermissions {
            id: role_id,
            name: r.try_get("", "name")?,
            description: r.try_get("", "description").ok(),
            role_type: r.try_get("", "role_type")?,
            status: r.try_get("", "status")?,
            is_system: r.try_get::<i32>("", "is_system")? == 1,
            permissions: perms,
        });
    }

    Ok(roles)
}

/// Get detailed info about a single role.
#[tauri::command]
pub async fn get_role(
    role_id: i64,
    state: State<'_, AppState>,
) -> AppResult<RoleDetail> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.roles", PermissionScope::Global);

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, description, role_type, status, is_system \
             FROM roles WHERE id = ? AND deleted_at IS NULL",
            [role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: role_id.to_string(),
        })?;

    let perms = load_role_permission_names(&state.db, role_id).await?;
    let perm_set: HashSet<String> = perms.iter().cloned().collect();
    let warnings = compute_dependency_warnings(&state.db, &perm_set).await?;

    Ok(RoleDetail {
        role: RoleWithPermissions {
            id: role_id,
            name: row.try_get("", "name")?,
            description: row.try_get("", "description").ok(),
            role_type: row.try_get("", "role_type")?,
            status: row.try_get("", "status")?,
            is_system: row.try_get::<i32>("", "is_system")? == 1,
            permissions: perms,
        },
        dependency_warnings: warnings,
    })
}

/// Create a new custom role with a set of permissions.
#[tauri::command]
pub async fn create_role(
    input: CreateRoleInput,
    state: State<'_, AppState>,
) -> AppResult<IdPayload> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.roles", PermissionScope::Global);
    require_step_up!(state);

    // Validate name is not empty
    if input.name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Role name cannot be empty".into(),
        ]));
    }

    // Check name uniqueness
    let dup = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE LOWER(name) = LOWER(?) AND deleted_at IS NULL",
            [input.name.clone().into()],
        ))
        .await?;
    if dup.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Role name '{}' is already taken",
            input.name
        )]));
    }

    // Validate all permission names exist
    let perm_set: HashSet<String> = input.permission_names.iter().cloned().collect();
    validate_permission_names_exist(&state.db, &perm_set).await?;

    // Hard dependency check
    resolver::validate_hard_dependencies(&state.db, &perm_set).await?;

    // Insert role
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sync_id = Uuid::new_v4().to_string();

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO roles (sync_id, name, description, role_type, status, is_system, \
                                created_at, updated_at, row_version) \
             VALUES (?, ?, ?, 'custom', 'active', 0, ?, ?, 1)",
            vec![
                sync_id.into(),
                input.name.into(),
                input.description.map_or(sea_orm::Value::String(None), |d| d.into()),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;

    let id_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to retrieve role id")))?;

    let new_role_id: i64 = id_row.try_get("", "id")?;

    // Link permissions
    for perm_name in &perm_set {
        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO role_permissions (role_id, permission_id, granted_at, granted_by_id) \
                 SELECT ?, p.id, ?, ? FROM permissions p WHERE p.name = ?",
                vec![
                    new_role_id.into(),
                    now.clone().into(),
                    i64::from(user.user_id).into(),
                    perm_name.clone().into(),
                ],
            ))
            .await?;
    }

    Ok(IdPayload { id: new_role_id })
}

/// Update a custom role's description and/or permission set.
#[tauri::command]
pub async fn update_role(
    input: UpdateRoleInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.roles", PermissionScope::Global);
    require_step_up!(state);

    // Load current role
    let role_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_system FROM roles WHERE id = ? AND deleted_at IS NULL",
            [input.role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: input.role_id.to_string(),
        })?;

    let is_system = role_row.try_get::<i32>("", "is_system")? == 1;
    let has_perm_changes = input.add_permissions.is_some() || input.remove_permissions.is_some();

    if is_system && has_perm_changes {
        return Err(AppError::PermissionDenied(
            "System role permissions cannot be modified".into(),
        ));
    }

    // Update description if provided
    if let Some(ref desc) = input.description {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE roles SET description = ?, updated_at = ? WHERE id = ?",
                [desc.clone().into(), now.into(), input.role_id.into()],
            ))
            .await?;
    }

    // Apply permission changes
    if has_perm_changes {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        // Remove permissions
        if let Some(ref remove) = input.remove_permissions {
            for perm_name in remove {
                state
                    .db
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "DELETE FROM role_permissions WHERE role_id = ? \
                         AND permission_id = (SELECT id FROM permissions WHERE name = ?)",
                        [input.role_id.into(), perm_name.clone().into()],
                    ))
                    .await?;
            }
        }

        // Add permissions
        if let Some(ref add) = input.add_permissions {
            let add_set: HashSet<String> = add.iter().cloned().collect();
            validate_permission_names_exist(&state.db, &add_set).await?;

            for perm_name in add {
                state
                    .db
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at, granted_by_id) \
                         SELECT ?, p.id, ?, ? FROM permissions p WHERE p.name = ?",
                        vec![
                            input.role_id.into(),
                            now.clone().into(),
                            i64::from(user.user_id).into(),
                            perm_name.clone().into(),
                        ],
                    ))
                    .await?;
            }
        }

        // Re-check dependencies on final permission set
        let final_perms = load_role_permission_names(&state.db, input.role_id).await?;
        let final_set: HashSet<String> = final_perms.into_iter().collect();
        resolver::validate_hard_dependencies(&state.db, &final_set).await?;
    }

    // Role permissions changed → invalidate all cached entries (any user may hold this role)
    if has_perm_changes {
        notify_rbac_global_change(&app, &state, "role_updated");
    }

    Ok(())
}

/// Soft-retire a role (set status = 'retired').
#[tauri::command]
pub async fn delete_role(
    role_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.roles", PermissionScope::Global);
    require_step_up!(state);

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_system FROM roles WHERE id = ? AND deleted_at IS NULL",
            [role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: role_id.to_string(),
        })?;

    if row.try_get::<i32>("", "is_system")? == 1 {
        return Err(AppError::PermissionDenied(
            "System role cannot be modified".into(),
        ));
    }

    // Check for active assignments
    let assignment_count = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) as cnt FROM user_scope_assignments \
             WHERE role_id = ? AND deleted_at IS NULL",
            [role_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0);

    if assignment_count > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Cannot retire role: {assignment_count} active user assignment(s) reference it"
        )]));
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE roles SET status = 'retired', updated_at = ? WHERE id = ?",
            [now.into(), role_id.into()],
        ))
        .await?;

    notify_rbac_global_change(&app, &state, "role_deleted");
    Ok(())
}

/// List all role templates.
#[tauri::command]
pub async fn list_role_templates(
    state: State<'_, AppState>,
) -> AppResult<Vec<model::RoleTemplate>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.roles", PermissionScope::Global);

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, description, module_set_json, is_system \
             FROM role_templates ORDER BY is_system DESC, name ASC",
            [],
        ))
        .await?;

    let templates = rows
        .iter()
        .filter_map(|r| {
            Some(model::RoleTemplate {
                id: r.try_get("", "id").ok()?,
                name: r.try_get("", "name").ok()?,
                description: r.try_get("", "description").ok(),
                module_set_json: r.try_get("", "module_set_json").ok()?,
                is_system: r.try_get::<i32>("", "is_system").ok()? == 1,
            })
        })
        .collect();

    Ok(templates)
}

/// Simulate the effective access for a user at a given scope.
#[tauri::command]
pub async fn simulate_access(
    input: SimulateAccessInput,
    state: State<'_, AppState>,
) -> AppResult<SimulateAccessResult> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);

    // Get effective permissions
    let perms = resolver::effective_permissions(
        &state.db,
        input.user_id,
        &input.scope_type,
        input.scope_reference.as_deref(),
    )
    .await?;

    // Build permission map (all known permissions with true/false)
    let all_perm_rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT name FROM permissions ORDER BY name",
            [],
        ))
        .await?;

    let mut permissions = HashMap::new();
    for r in &all_perm_rows {
        if let Ok(name) = r.try_get::<String>("", "name") {
            let has = perms.contains(&name);
            permissions.insert(name, has);
        }
    }

    // Load assignments
    let assignments = load_scope_assignments(&state.db, input.user_id).await?;

    // Compute dependency warnings and blocked_by
    let (dependency_warnings, blocked_by) = compute_simulation_warnings(&state.db, &perms).await?;

    Ok(SimulateAccessResult {
        permissions,
        assignments,
        dependency_warnings,
        blocked_by,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
//  EMERGENCY ELEVATION COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

/// Grant a time-boxed emergency role elevation to a user.
/// Creates a new user_scope_assignment with is_emergency=1 and
/// emergency_reason / emergency_expires_at columns.
#[tauri::command]
pub async fn grant_emergency_elevation(
    app: AppHandle,
    input: GrantEmergencyElevationInput,
    state: State<'_, AppState>,
) -> AppResult<IdPayload> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    require_step_up!(state);

    // Validate reason is non-empty
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Emergency elevation reason is required".into(),
        ]));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, scope_reference, \
              is_emergency, emergency_reason, emergency_expires_at, \
              assigned_by_id, notes, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?)",
            [
                sync_id.into(),
                input.user_id.into(),
                input.role_id.into(),
                input.scope_type.into(),
                input
                    .scope_reference
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                input.reason.clone().into(),
                input.expires_at.into(),
                caller.user_id.into(),
                format!("Emergency grant by user {}: {}", caller.user_id, input.reason).into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;

    let id_row = state
        .db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to get last insert id")))?;
    let id: i64 = id_row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("id decode: {e}")))?;

    notify_rbac_user_change(&app, &state, input.user_id, "emergency_granted");

    Ok(IdPayload { id })
}

/// Revoke an existing emergency elevation assignment.
/// Soft-deletes the assignment by setting deleted_at.
#[tauri::command]
pub async fn revoke_emergency_elevation(
    app: AppHandle,
    input: RevokeEmergencyElevationInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);

    // Lookup the assignment to get user_id and verify it is an emergency assignment
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT user_id, is_emergency FROM user_scope_assignments \
             WHERE id = ? AND deleted_at IS NULL",
            [input.assignment_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::NotFound {
                entity: "assignment".into(),
                id: input.assignment_id.to_string(),
            }
        })?;

    let target_user_id: i64 = row
        .try_get("", "user_id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("user_id decode: {e}")))?;
    let is_emergency: i32 = row
        .try_get("", "is_emergency")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("is_emergency decode: {e}")))?;

    if is_emergency != 1 {
        return Err(AppError::ValidationFailed(vec![
            "Assignment is not an emergency elevation — use revoke_role_scope instead".into(),
        ]));
    }

    let now = chrono::Utc::now().to_rfc3339();
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_scope_assignments SET deleted_at = ?, updated_at = ? WHERE id = ?",
            [now.clone().into(), now.into(), input.assignment_id.into()],
        ))
        .await?;

    notify_rbac_user_change(&app, &state, target_user_id, "emergency_revoked");

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
//  INTERNAL HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Load active role assignments for a user (summary view for user listing).
async fn load_role_assignments(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
) -> AppResult<Vec<RoleAssignmentSummary>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT usa.id as assignment_id, usa.role_id, r.name as role_name, \
                    usa.scope_type, usa.scope_reference, \
                    usa.valid_from, usa.valid_to, usa.is_emergency \
             FROM user_scope_assignments usa \
             INNER JOIN roles r ON r.id = usa.role_id \
             WHERE usa.user_id = ? AND usa.deleted_at IS NULL \
             ORDER BY r.name ASC",
            [user_id.into()],
        ))
        .await?;

    let assignments = rows
        .iter()
        .filter_map(|r| {
            Some(RoleAssignmentSummary {
                assignment_id: r.try_get("", "assignment_id").ok()?,
                role_id: r.try_get("", "role_id").ok()?,
                role_name: r.try_get("", "role_name").ok()?,
                scope_type: r.try_get("", "scope_type").ok()?,
                scope_reference: r.try_get("", "scope_reference").ok(),
                valid_from: r.try_get("", "valid_from").ok(),
                valid_to: r.try_get("", "valid_to").ok(),
                is_emergency: r.try_get::<i32>("", "is_emergency").ok()? == 1,
            })
        })
        .collect();

    Ok(assignments)
}

/// Load all scope assignments for a user (full detail view).
async fn load_scope_assignments(
    db: &sea_orm::DatabaseConnection,
    user_id: i64,
) -> AppResult<Vec<model::UserScopeAssignment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, user_id, role_id, scope_type, scope_reference, \
                    valid_from, valid_to, assigned_by_id, notes, \
                    is_emergency, emergency_reason, emergency_expires_at, \
                    created_at, deleted_at \
             FROM user_scope_assignments \
             WHERE user_id = ? AND deleted_at IS NULL \
             ORDER BY created_at DESC",
            [user_id.into()],
        ))
        .await?;

    let assignments = rows
        .iter()
        .filter_map(|r| {
            Some(model::UserScopeAssignment {
                id: r.try_get("", "id").ok()?,
                user_id: r.try_get("", "user_id").ok()?,
                role_id: r.try_get("", "role_id").ok()?,
                scope_type: r.try_get("", "scope_type").ok()?,
                scope_reference: r.try_get("", "scope_reference").ok(),
                valid_from: r.try_get("", "valid_from").ok(),
                valid_to: r.try_get("", "valid_to").ok(),
                assigned_by_id: r.try_get("", "assigned_by_id").ok(),
                notes: r.try_get("", "notes").ok(),
                is_emergency: r.try_get::<i32>("", "is_emergency").ok()? == 1,
                emergency_reason: r.try_get("", "emergency_reason").ok(),
                emergency_expires_at: r.try_get("", "emergency_expires_at").ok(),
                created_at: r.try_get("", "created_at").ok()?,
                deleted_at: r.try_get("", "deleted_at").ok(),
            })
        })
        .collect();

    Ok(assignments)
}

/// Load permission names for a role.
async fn load_role_permission_names(
    db: &sea_orm::DatabaseConnection,
    role_id: i64,
) -> AppResult<Vec<String>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.name FROM permissions p \
             INNER JOIN role_permissions rp ON rp.permission_id = p.id \
             WHERE rp.role_id = ? \
             ORDER BY p.name ASC",
            [role_id.into()],
        ))
        .await?;

    let names = rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .collect();

    Ok(names)
}

/// Validate that all permission names exist in the permissions table.
async fn validate_permission_names_exist(
    db: &sea_orm::DatabaseConnection,
    names: &HashSet<String>,
) -> AppResult<()> {
    if names.is_empty() {
        return Ok(());
    }

    let placeholders: String = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT name FROM permissions WHERE name IN ({placeholders})"
    );
    let values: Vec<sea_orm::Value> = names.iter().map(|n| n.clone().into()).collect();

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    let found: HashSet<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .collect();

    let missing: Vec<&String> = names.iter().filter(|n| !found.contains(*n)).collect();
    if !missing.is_empty() {
        return Err(AppError::ValidationFailed(
            missing
                .iter()
                .map(|n| format!("Permission '{n}' does not exist"))
                .collect(),
        ));
    }

    Ok(())
}

/// Guard: prevent deactivating the last active user holding the Superadmin role.
async fn guard_last_superadmin(
    db: &sea_orm::DatabaseConnection,
    target_user_id: i64,
) -> AppResult<()> {
    // Count active users who hold the Superadmin role (or Administrator - Phase 1 name)
    let count = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(DISTINCT ua.id) as cnt \
             FROM user_accounts ua \
             INNER JOIN user_scope_assignments usa ON usa.user_id = ua.id \
             INNER JOIN roles r ON r.id = usa.role_id \
             WHERE ua.is_active = 1 \
               AND ua.deleted_at IS NULL \
               AND usa.deleted_at IS NULL \
               AND (r.name = 'Superadmin' OR r.name = 'Administrator') \
               AND ua.id != ?",
            [target_user_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0);

    if count == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Cannot deactivate the last active superadmin user".into(),
        ]));
    }

    Ok(())
}

/// Compute dependency warning messages for a set of permission names.
async fn compute_dependency_warnings(
    db: &sea_orm::DatabaseConnection,
    perms: &HashSet<String>,
) -> AppResult<Vec<String>> {
    let deps = resolver::dependency_warnings_for(db, perms).await?;

    let warnings: Vec<String> = deps
        .iter()
        .filter(|d| !perms.contains(&d.required_permission_name))
        .map(|d| {
            let severity = if d.dependency_type == "hard" {
                "BLOCKED"
            } else {
                "Warning"
            };
            format!(
                "{severity}: '{0}' requires '{1}' ({2})",
                d.permission_name, d.required_permission_name, d.dependency_type
            )
        })
        .collect();

    Ok(warnings)
}

/// Compute simulation warnings and blocked_by lists.
async fn compute_simulation_warnings(
    db: &sea_orm::DatabaseConnection,
    effective_perms: &HashSet<String>,
) -> AppResult<(Vec<String>, Vec<String>)> {
    let deps = resolver::dependency_warnings_for(db, effective_perms).await?;

    let mut warnings = Vec::new();
    let mut blocked = Vec::new();

    for d in &deps {
        if !effective_perms.contains(&d.required_permission_name) {
            let msg = format!(
                "'{}' requires '{}' ({})",
                d.permission_name, d.required_permission_name, d.dependency_type
            );
            if d.dependency_type == "hard" {
                blocked.push(msg);
            } else {
                warnings.push(msg);
            }
        }
    }

    Ok((warnings, blocked))
}

/// Validate password strength: min 8 chars, at least one uppercase, one lowercase, one digit.
fn validate_password_strength(password: &str) -> AppResult<()> {
    let mut errors = Vec::new();

    if password.len() < 8 {
        errors.push("Password must be at least 8 characters".into());
    }
    if !password.chars().any(|c| c.is_uppercase()) {
        errors.push("Password must contain at least one uppercase letter".into());
    }
    if !password.chars().any(|c| c.is_lowercase()) {
        errors.push("Password must contain at least one lowercase letter".into());
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        errors.push("Password must contain at least one digit".into());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(errors))
    }
}
