//! User and Role administration IPC commands — Phase 2 SP06-F01.
//!
//! Permission gates:
//!   adm.users  — list_users, get_user, list_assignable_roles, create_user, update_user, deactivate_user,
//!                assign_role_scope, revoke_role_scope, simulate_access, unlock_user_account,
//!                admin_set_user_password
//!   adm.roles  — list_roles, get_role, create_role, update_role, delete_role,
//!                list_role_templates

use std::collections::{HashMap, HashSet};

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::auth::rbac::PermissionScope;
use crate::auth::session_manager::AuthenticatedUser;
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
    pub emergency_expires_at: Option<String>,
    /// Name of the org node the assignment is scoped to (`None` for plant-wide assignments).
    pub scope_label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserWithRoles {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub personnel_id: Option<i64>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub identity_mode: String,
    pub is_active: bool,
    pub force_password_change: bool,
    pub last_seen_at: Option<String>,
    pub last_login_at: Option<String>,
    pub locked_until: Option<String>,
    /// `locked_until` is in the future (failed sign-ins locked the account).
    pub is_locked: bool,
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
    /// `active` | `inactive` | `locked` (locked = sign-in lockout still running).
    pub status: Option<String>,
    pub identity_mode: Option<String>,
    pub search: Option<String>,
    /// Users holding this role (any scope, assignment currently valid).
    pub role_id: Option<i64>,
    /// Users with an assignment on this org node or on a node below it.
    pub org_node_id: Option<i64>,
}

/// Disable or re-enable an account; the reason is kept in the admin change ledger.
#[derive(Debug, Deserialize)]
pub struct SetUserActiveInput {
    pub user_id: i64,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserInput {
    pub username: String,
    pub identity_mode: String,
    /// Tenant-scoped role assigned at creation (required — no implicit default).
    pub role_id: i64,
    pub personnel_id: Option<i64>,
    pub initial_password: Option<String>,
    pub force_password_change: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserInput {
    pub user_id: i64,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
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

/// Lightweight role row for **adm.users** flows (e.g. create user) — no permission matrix.
#[derive(Debug, Serialize)]
pub struct AssignableRoleSummary {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub role_type: String,
    pub status: String,
    pub is_system: bool,
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
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CloneRoleInput {
    pub source_role_id: i64,
    pub name: String,
    pub description: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct MissingTenantScopeUser {
    pub user_id: i64,
    pub username: String,
    pub identity_mode: String,
    pub has_any_role_assignment: bool,
}

#[derive(Debug, Serialize)]
pub struct TenantScopeBackfillResult {
    pub tenant_id: Option<String>,
    pub updated_count: i64,
    pub updated_user_ids: Vec<i64>,
}

fn normalize_email(raw: &str) -> AppResult<Option<String>> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }
    let at_idx = normalized.find('@');
    let is_valid = at_idx.is_some()
        && !normalized.contains(' ')
        && !normalized.starts_with('@')
        && !normalized.ends_with('@')
        && normalized.rfind('.').is_some_and(|dot| dot > at_idx.unwrap_or(0) + 1);
    if !is_valid {
        return Err(AppError::ValidationFailed(vec!["Email format is invalid.".into()]));
    }
    Ok(Some(normalized))
}

fn normalize_phone_e164(raw: &str) -> AppResult<Option<String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    // Keep only digits and an optional leading '+'.
    let mut out = String::with_capacity(trimmed.len());
    for (idx, ch) in trimmed.chars().enumerate() {
        if ch == '+' {
            if idx == 0 {
                out.push(ch);
            } else {
                return Err(AppError::ValidationFailed(vec![
                    "Phone number format is invalid.".into()
                ]));
            }
        } else if ch.is_ascii_digit() {
            out.push(ch);
        }
    }

    if out.starts_with("00") {
        out = format!("+{}", &out[2..]);
    } else if !out.starts_with('+') {
        out = format!("+{out}");
    }

    let digits = out.chars().filter(|c| c.is_ascii_digit()).count();
    if !(8..=15).contains(&digits) {
        return Err(AppError::ValidationFailed(vec![
            "Phone number must be a valid E.164 number (8-15 digits).".into(),
        ]));
    }

    Ok(Some(out))
}

// ── Presence DTO ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct UserPresence {
    pub user_id: i64,
    pub status: String, // "active" | "idle" | "offline"
    pub last_activity_at: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  RBAC change notification helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Invalidate the permission cache for a specific user and emit `rbac-changed`.
async fn notify_rbac_user_change(app: &AppHandle, state: &AppState, user_id: i64, action: &str) {
    // Waits for the lock: an invalidation is never skipped under contention.
    state.permission_cache.write().await.invalidate_user(user_id);
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
async fn notify_rbac_global_change(app: &AppHandle, state: &AppState, action: &str) {
    state.permission_cache.write().await.invalidate_all();
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

/// Columns read for every user row (list and detail).
const USER_ROW_COLUMNS: &str = "ua.id, ua.username, ua.display_name, ua.personnel_id, ua.email, ua.phone, \
     ua.identity_mode, ua.is_active, ua.force_password_change, ua.last_seen_at, ua.last_login_at, \
     ua.locked_until";

fn now_utc_text() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `locked_until` is in the future. Stored values are RFC 3339 / `...Z` text, compared as instants.
fn is_lock_running(locked_until: Option<&str>) -> bool {
    let Some(raw) = locked_until else { return false };
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(&chrono::Utc) > chrono::Utc::now())
        .unwrap_or(false)
}

fn map_user_row(r: &sea_orm::QueryResult, roles: Vec<RoleAssignmentSummary>) -> AppResult<UserWithRoles> {
    let locked_until = r.try_get::<Option<String>>("", "locked_until").unwrap_or(None);
    Ok(UserWithRoles {
        id: r.try_get("", "id")?,
        username: r.try_get("", "username")?,
        display_name: r.try_get::<Option<String>>("", "display_name").ok().flatten(),
        personnel_id: r.try_get::<Option<i64>>("", "personnel_id").ok().flatten(),
        email: r.try_get::<Option<String>>("", "email").ok().flatten(),
        phone: r.try_get::<Option<String>>("", "phone").ok().flatten(),
        identity_mode: r.try_get("", "identity_mode")?,
        is_active: r.try_get::<i32>("", "is_active")? == 1,
        force_password_change: r.try_get::<i32>("", "force_password_change")? == 1,
        last_seen_at: r.try_get::<Option<String>>("", "last_seen_at").ok().flatten(),
        last_login_at: r.try_get::<Option<String>>("", "last_login_at").ok().flatten(),
        is_locked: is_lock_running(locked_until.as_deref()),
        locked_until,
        roles,
    })
}

/// List users with their current role assignments.
#[tauri::command]
pub async fn list_users(filter: UserListFilter, state: State<'_, AppState>) -> AppResult<Vec<UserWithRoles>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    list_users_impl(&state.db, &filter).await
}

pub(crate) async fn list_users_impl(
    db: &sea_orm::DatabaseConnection,
    filter: &UserListFilter,
) -> AppResult<Vec<UserWithRoles>> {
    let mut conditions = vec!["ua.deleted_at IS NULL".to_owned()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    let now = now_utc_text();

    if let Some(active) = filter.is_active {
        conditions.push("ua.is_active = ?".to_owned());
        values.push(i32::from(active).into());
    }
    match filter.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => {}
        Some("active") => conditions.push("ua.is_active = 1".to_owned()),
        Some("inactive") => conditions.push("ua.is_active = 0".to_owned()),
        // Lock state is decided per row below (timestamps are compared as instants).
        Some("locked") => conditions.push("ua.locked_until IS NOT NULL".to_owned()),
        Some(other) => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Unknown user status filter '{other}' (expected active, inactive or locked)."
            )]))
        }
    }
    if let Some(ref mode) = filter.identity_mode {
        conditions.push("ua.identity_mode = ?".to_owned());
        values.push(mode.clone().into());
    }
    if let Some(ref search) = filter.search {
        let trimmed = search.trim();
        if !trimmed.is_empty() {
            conditions.push("(ua.username LIKE ? OR ua.display_name LIKE ? OR ua.email LIKE ?)".to_owned());
            let pattern = format!("%{trimmed}%");
            values.push(pattern.clone().into());
            values.push(pattern.clone().into());
            values.push(pattern.into());
        }
    }
    if let Some(role_id) = filter.role_id {
        conditions.push(
            "EXISTS (SELECT 1 FROM user_scope_assignments f WHERE f.user_id = ua.id AND f.role_id = ? \
             AND f.deleted_at IS NULL \
             AND (f.valid_from IS NULL OR f.valid_from <= ?) \
             AND (f.valid_to IS NULL OR f.valid_to >= ?) \
             AND (f.is_emergency = 0 OR f.emergency_expires_at > ?))"
                .to_owned(),
        );
        values.push(role_id.into());
        values.push(now.clone().into());
        values.push(now.clone().into());
        values.push(now.clone().into());
    }
    if let Some(node_id) = filter.org_node_id {
        conditions.push(
            "EXISTS (WITH RECURSIVE sub(id) AS ( \
             SELECT id FROM org_nodes WHERE id = ? AND deleted_at IS NULL \
             UNION SELECT n.id FROM org_nodes n JOIN sub ON n.parent_id = sub.id WHERE n.deleted_at IS NULL) \
             SELECT 1 FROM user_scope_assignments f \
             WHERE f.user_id = ua.id AND f.deleted_at IS NULL AND f.scope_type <> 'tenant' \
             AND f.scope_reference IN (SELECT CAST(id AS TEXT) FROM sub) \
             AND (f.valid_from IS NULL OR f.valid_from <= ?) \
             AND (f.valid_to IS NULL OR f.valid_to >= ?) \
             AND (f.is_emergency = 0 OR f.emergency_expires_at > ?))"
                .to_owned(),
        );
        values.push(node_id.into());
        values.push(now.clone().into());
        values.push(now.clone().into());
        values.push(now.into());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!("SELECT {USER_ROW_COLUMNS} FROM user_accounts ua WHERE {where_clause} ORDER BY ua.username ASC");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    let only_locked = filter.status.as_deref().map(str::trim) == Some("locked");
    let mut users = Vec::with_capacity(rows.len());
    for r in &rows {
        let uid: i64 = r.try_get("", "id")?;
        let roles = load_role_assignments(db, uid).await?;
        let user = map_user_row(r, roles)?;
        if only_locked && !user.is_locked {
            continue;
        }
        users.push(user);
    }

    Ok(users)
}

/// Get detailed user info including all scope assignments and effective permissions.
#[tauri::command]
pub async fn get_user(user_id: i64, state: State<'_, AppState>) -> AppResult<UserDetail> {
    let caller = require_session!(state);
    require_permission!(
        state,
        &caller,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {USER_ROW_COLUMNS} FROM user_accounts ua WHERE ua.id = ? AND ua.deleted_at IS NULL"),
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
        user: map_user_row(&row, roles)?,
        scope_assignments,
        effective_permissions: perm_list,
    })
}

/// List roles that may be selected when creating a user (**adm.users**).
///
/// Callers without **adm.roles** can still populate the create-user role dropdown.
/// Excludes deleted and retired roles. Does not load per-role permission names.
#[tauri::command]
pub async fn list_assignable_roles(state: State<'_, AppState>) -> AppResult<Vec<AssignableRoleSummary>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, description, role_type, status, is_system \
             FROM roles WHERE deleted_at IS NULL AND status != 'retired' \
             ORDER BY is_system DESC, name ASC",
            [],
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(AssignableRoleSummary {
            id: r.try_get("", "id")?,
            name: r.try_get("", "name")?,
            description: r.try_get("", "description").ok(),
            role_type: r.try_get("", "role_type")?,
            status: r.try_get("", "status")?,
            is_system: r.try_get::<i32>("", "is_system")? == 1,
        });
    }
    Ok(out)
}

/// An org node a role can be limited to (the assignment covers the node and everything below).
#[derive(Debug, Serialize)]
pub struct AssignableScopeNode {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub code: String,
    pub name: String,
    pub type_label: Option<String>,
    pub depth: i64,
}

/// Org nodes of the active structure, for scoping role assignments (**adm.users**; the org
/// screens need `org.view`, which an administrator may not hold).
#[tauri::command]
pub async fn list_assignable_scope_nodes(state: State<'_, AppState>) -> AppResult<Vec<AssignableScopeNode>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    list_assignable_scope_nodes_impl(&state.db).await
}

pub(crate) async fn list_assignable_scope_nodes_impl(
    db: &sea_orm::DatabaseConnection,
) -> AppResult<Vec<AssignableScopeNode>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT n.id, n.parent_id, n.code, n.name, COALESCE(n.depth, 0) AS depth, nt.label AS type_label \
             FROM org_nodes n \
             INNER JOIN org_structure_models m ON m.id = n.structure_model_id AND m.status = 'active' \
             LEFT JOIN org_node_types nt ON nt.id = n.node_type_id \
             WHERE n.deleted_at IS NULL \
             ORDER BY COALESCE(n.ancestor_path, ''), n.name"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(AssignableScopeNode {
            id: r.try_get("", "id")?,
            parent_id: r.try_get::<Option<i64>>("", "parent_id")?,
            code: r.try_get("", "code")?,
            name: r.try_get("", "name")?,
            type_label: r.try_get::<Option<String>>("", "type_label")?,
            depth: r.try_get("", "depth")?,
        });
    }
    Ok(out)
}

/// Verify `role_id` refers to a non-deleted, non-retired role (for inserts under a transaction).
async fn ensure_assignable_role_id<C>(conn: &C, role_id: i64) -> AppResult<()>
where
    C: ConnectionTrait,
{
    if role_id <= 0 {
        return Err(AppError::ValidationFailed(vec![
            "role_id must be a positive integer.".into()
        ]));
    }
    let ok = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM roles \
             WHERE id = ? AND deleted_at IS NULL AND status != 'retired' \
             LIMIT 1",
            [role_id.into()],
        ))
        .await?
        .is_some();
    if !ok {
        return Err(AppError::ValidationFailed(vec![format!(
            "The selected role (id {role_id}) is not available. It may have been deleted or retired."
        )]));
    }
    Ok(())
}

/// A username: trimmed, 3 to 64 characters, letters (any alphabet), digits and `. _ - @` only
/// (no spaces or control characters: it is typed at sign-in and compared case-insensitively).
pub(crate) fn normalize_username(raw: &str) -> AppResult<String> {
    let username = raw.trim();
    let count = username.chars().count();
    if !(3..=64).contains(&count) {
        return Err(AppError::ValidationFailed(vec![
            "The username must be 3 to 64 characters long.".into(),
        ]));
    }
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'))
    {
        return Err(AppError::ValidationFailed(vec![
            "The username may contain letters, digits and . _ - @ only.".into(),
        ]));
    }
    Ok(username.to_string())
}

/// Create a new user account.
#[tauri::command]
pub async fn create_user(input: CreateUserInput, app: AppHandle, state: State<'_, AppState>) -> AppResult<IdPayload> {
    let caller = require_session!(state);
    let role_id = input.role_id;
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        create_user_impl(&state, &caller, input).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "user_created",
        None,
        Some(role_id),
        &result,
    )
    .await;
    if let Ok(created) = &result {
        notify_rbac_user_change(&app, &state, created.id, "user_created").await;
    }
    result
}

pub(crate) async fn create_user_impl(
    state: &AppState,
    caller: &AuthenticatedUser,
    input: CreateUserInput,
) -> AppResult<IdPayload> {
    let username = normalize_username(&input.username)?;
    ensure_may_grant_role(&state.db, caller, input.role_id, None).await?;

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
                AppError::ValidationFailed(vec!["initial_password is required for local identity mode".into()])
            })?;
            validate_admin_set_password(&state.db, pw).await?;
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
                validate_admin_set_password(&state.db, pw).await?;
                Some(crate::auth::password::hash_password(pw)?)
            }
            None => None,
        },
        _ => unreachable!(),
    };

    let force_pw = input.force_password_change.unwrap_or(password_hash.is_some());

    if let Some(pid) = input.personnel_id {
        let p_exists = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM personnel WHERE id = ?",
                [pid.into()],
            ))
            .await?;
        if p_exists.is_none() {
            return Err(AppError::ValidationFailed(vec!["Personnel record not found.".into()]));
        }
        let linked = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE personnel_id = ? AND deleted_at IS NULL",
                [pid.into()],
            ))
            .await?;
        if linked.is_some() {
            return Err(AppError::ValidationFailed(vec![
                "This personnel record is already linked to another user account.".into(),
            ]));
        }
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    let assignment_sync_id = Uuid::new_v4().to_string();
    let identity_mode = input.identity_mode.clone();
    let role_id = input.role_id;

    let activated_tenant_id = crate::commands::product_license::get_activation_claim_tenant_id(&state.db).await?;

    let tx = state.db.begin().await?;

    // Unique regardless of case and of surrounding spaces (stored trimmed), checked in the
    // transaction that inserts.
    let exists = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE LOWER(TRIM(username)) = LOWER(?) AND deleted_at IS NULL",
            [username.clone().into()],
        ))
        .await?;
    if exists.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Username '{username}' is already taken"
        )]));
    }

    ensure_assignable_role_id(&tx, role_id).await?;

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (entity_sync_id, username, identity_mode, password_hash, personnel_id, \
                 is_active, is_admin, force_password_change, \
                 failed_login_attempts, created_at, updated_at, row_version) \
             VALUES (?, ?, ?, ?, ?, 1, 0, ?, 0, ?, ?, 1)",
        vec![
            entity_sync_id.into(),
            username.clone().into(),
            identity_mode.clone().into(),
            password_hash.map_or(sea_orm::Value::String(None), |h| h.into()),
            input.personnel_id.into(),
            i32::from(force_pw).into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;

    // Get the inserted ID
    let id_row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to retrieve inserted user id")))?;

    let new_id: i64 = id_row.try_get("", "id")?;

    // Tenant-scoped assignment: explicit role chosen by the administrator (no implicit default).
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments \
            (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
             valid_from, valid_to, assigned_by_id, notes, created_at, updated_at, row_version) \
         VALUES (?, ?, ?, 'tenant', ?, NULL, NULL, ?, ?, ?, ?, 1)",
        vec![
            assignment_sync_id.into(),
            new_id.into(),
            role_id.into(),
            activated_tenant_id
                .clone()
                .map_or(sea_orm::Value::String(None), |tid| tid.into()),
            i64::from(caller.user_id).into(),
            "Initial tenant-scoped role assignment (selected at user creation).".into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;

    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(caller.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "user_created",
            target_user_id: Some(new_id),
            target_role_id: Some(role_id),
            scope_type: Some("tenant"),
            summary: Some(format!("User account '{username}' created")),
            diff: Some(serde_json::json!({
                "username": username,
                "identity_mode": identity_mode,
                "role_id": role_id,
                "force_password_change": force_pw,
            })),
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;

    tx.commit().await?;

    {
        let new_id_str = new_id.to_string();
        let detail = serde_json::json!({
            "username": username,
            "identity_mode": identity_mode,
            "tenant_id": activated_tenant_id.unwrap_or_default(),
            "role_id": role_id,
        })
        .to_string();
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::USER_CREATED,
                actor_id: Some(caller.user_id),
                entity_type: Some("user_account"),
                entity_id: Some(new_id_str.as_str()),
                summary: "User account created",
                detail_json: Some(detail),
                ..Default::default()
            },
        )
        .await;
    }

    Ok(IdPayload { id: new_id })
}

/// List active users that cannot log in under the activated tenant due to missing tenant scope.
#[tauri::command]
pub async fn list_users_missing_tenant_scope(state: State<'_, AppState>) -> AppResult<Vec<MissingTenantScopeUser>> {
    let caller = require_session!(state);
    require_permission!(
        state,
        &caller,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    list_missing_tenant_scope_users(&state.db).await
}

/// Backfill tenant scope for users missing it, using the currently activated tenant claim.
#[tauri::command]
pub async fn backfill_users_missing_tenant_scope(state: State<'_, AppState>) -> AppResult<TenantScopeBackfillResult> {
    let caller = require_session!(state);
    require_permission!(
        state,
        &caller,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    require_step_up!(state);

    let activated_tenant_id = crate::commands::product_license::get_activation_claim_tenant_id(&state.db).await?;
    let missing = list_missing_tenant_scope_users(&state.db).await?;
    if missing.is_empty() {
        return Ok(TenantScopeBackfillResult {
            tenant_id: activated_tenant_id,
            updated_count: 0,
            updated_user_ids: Vec::new(),
        });
    }

    let role_id = resolve_default_tenant_membership_role_id(&state.db).await?;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let tx = state.db.begin().await?;
    let mut updated_count = 0_i64;
    let mut updated_user_ids: Vec<i64> = Vec::with_capacity(missing.len());

    for user in &missing {
        let assignment_sync_id = Uuid::new_v4().to_string();
        let exec = tx
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO user_scope_assignments \
                    (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
                     valid_from, valid_to, assigned_by_id, notes, created_at, updated_at, row_version) \
                 VALUES (?, ?, ?, 'tenant', ?, NULL, NULL, ?, ?, ?, ?, 1)",
                vec![
                    assignment_sync_id.into(),
                    user.user_id.into(),
                    role_id.into(),
                    activated_tenant_id
                        .clone()
                        .map_or(sea_orm::Value::String(None), |tid| tid.into()),
                    i64::from(caller.user_id).into(),
                    "Tenant scope backfill for existing user".into(),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        if exec.rows_affected() > 0 {
            updated_count += 1;
            updated_user_ids.push(user.user_id);
        }
    }

    tx.commit().await?;

    Ok(TenantScopeBackfillResult {
        tenant_id: activated_tenant_id,
        updated_count,
        updated_user_ids,
    })
}

/// Update an existing user account.
#[tauri::command]
pub async fn update_user(input: UpdateUserInput, state: State<'_, AppState>) -> AppResult<()> {
    let caller = require_session!(state);
    let target = input.user_id;
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        update_user_impl(&state, &caller, input).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "user_updated",
        Some(target),
        None,
        &result,
    )
    .await;
    result
}

pub(crate) async fn update_user_impl(
    state: &AppState,
    caller: &AuthenticatedUser,
    input: UpdateUserInput,
) -> AppResult<()> {
    // Enabling / disabling goes through `deactivate_user` / `reactivate_user` (step-up, mandatory
    // reason, ledger); editing the account details cannot switch it on or off.
    if input.is_active.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Use Disable / Enable on the account to switch it off or on (a reason is required).".into(),
        ]));
    }
    ensure_may_manage_account(&state.db, caller, input.user_id).await?;

    let old_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT username, display_name, email, phone, personnel_id, force_password_change, is_active \
             FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [input.user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UserAccount".into(),
            id: input.user_id.to_string(),
        })?;
    let mut diff = serde_json::Map::new();
    let mut note_change = |field: &str, old: serde_json::Value, new: serde_json::Value| {
        if old != new {
            diff.insert(field.to_string(), serde_json::json!({ "old": old, "new": new }));
        }
    };

    let mut sets = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    let mut new_username: Option<String> = None;
    if let Some(ref username) = input.username {
        let normalized = normalize_username(username)?;
        note_change(
            "username",
            serde_json::json!(old_row.try_get::<String>("", "username")?),
            serde_json::json!(normalized),
        );
        sets.push("username = ?");
        values.push(normalized.clone().into());
        new_username = Some(normalized);
    }
    if let Some(ref display_name) = input.display_name {
        let normalized = display_name.trim();
        note_change(
            "display_name",
            serde_json::json!(old_row.try_get::<Option<String>>("", "display_name")?),
            serde_json::json!((!normalized.is_empty()).then_some(normalized)),
        );
        sets.push("display_name = ?");
        if normalized.is_empty() {
            values.push(sea_orm::Value::String(None));
        } else {
            values.push(normalized.to_string().into());
        }
    }
    if let Some(ref email) = input.email {
        let normalized = normalize_email(email)?;

        if let Some(ref candidate) = normalized {
            let dup = state
                .db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM user_accounts \
                     WHERE LOWER(email) = LOWER(?) AND id != ? AND deleted_at IS NULL",
                    [candidate.clone().into(), input.user_id.into()],
                ))
                .await?;
            if dup.is_some() {
                return Err(AppError::ValidationFailed(vec![
                    "Email is already used by another account.".into(),
                ]));
            }
        }
        note_change(
            "email",
            serde_json::json!(old_row.try_get::<Option<String>>("", "email")?),
            serde_json::json!(normalized),
        );
        sets.push("email = ?");
        values.push(normalized.map_or(sea_orm::Value::String(None), |s| s.into()));
    }
    if let Some(ref phone) = input.phone {
        let normalized = normalize_phone_e164(phone)?;
        note_change(
            "phone",
            serde_json::json!(old_row.try_get::<Option<String>>("", "phone")?),
            serde_json::json!(normalized),
        );
        sets.push("phone = ?");
        values.push(normalized.map_or(sea_orm::Value::String(None), |s| s.into()));
    }
    if let Some(pid) = input.personnel_id {
        note_change(
            "personnel_id",
            serde_json::json!(old_row.try_get::<Option<i64>>("", "personnel_id")?),
            serde_json::json!(pid),
        );
        sets.push("personnel_id = ?");
        values.push(pid.into());
    }
    if let Some(fpc) = input.force_password_change {
        note_change(
            "force_password_change",
            serde_json::json!(old_row.try_get::<i32>("", "force_password_change")? == 1),
            serde_json::json!(fpc),
        );
        sets.push("force_password_change = ?");
        values.push(i32::from(fpc).into());
    }
    if let Some(active) = input.is_active {
        note_change(
            "is_active",
            serde_json::json!(old_row.try_get::<i32>("", "is_active")? == 1),
            serde_json::json!(active),
        );
        sets.push("is_active = ?");
        values.push(i32::from(active).into());
    }

    if sets.is_empty() || diff.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    sets.push("updated_at = ?");
    values.push(now.into());
    values.push(input.user_id.into());

    let set_clause = sets.join(", ");
    let sql = format!("UPDATE user_accounts SET {set_clause} WHERE id = ? AND deleted_at IS NULL");

    let changed: Vec<String> = diff.keys().cloned().collect();
    let tx = state.db.begin().await?;
    if let Some(ref username) = new_username {
        // Unique regardless of case and surrounding spaces, checked where the rename is written.
        let dup = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts \
                 WHERE LOWER(TRIM(username)) = LOWER(?) AND id != ? AND deleted_at IS NULL",
                [username.clone().into(), input.user_id.into()],
            ))
            .await?;
        if dup.is_some() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Username '{username}' is already taken"
            )]));
        }
    }
    let admins_before = active_admin_holders(&tx).await?;
    tx.execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;
    ensure_an_admin_remains(&tx, admins_before).await?;
    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(caller.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "user_updated",
            target_user_id: Some(input.user_id),
            summary: Some(format!("Account details changed: {}", changed.join(", "))),
            diff: Some(serde_json::Value::Object(diff)),
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;

    tx.commit().await?;

    state.permission_cache.write().await.invalidate_user(input.user_id);

    Ok(())
}

/// A mandatory free-text reason (disable, enable, emergency grant): trimmed, not empty.
fn required_reason(raw: &str, what: &str) -> AppResult<String> {
    let reason = raw.trim();
    if reason.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "A reason is required to {what}."
        )]));
    }
    if reason.chars().count() > 1000 {
        return Err(AppError::ValidationFailed(vec![
            "The reason must not exceed 1000 characters.".into(),
        ]));
    }
    Ok(reason.to_string())
}

/// Disable an account (set is_active = 0). The account and its history stay; the reason is
/// kept in the admin change ledger.
#[tauri::command]
pub async fn deactivate_user(input: SetUserActiveInput, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let caller = require_session!(state);
    let target = input.user_id;
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        set_user_active_impl(&state, &caller, input, false).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "user_deactivated",
        Some(target),
        None,
        &result,
    )
    .await;
    if result.is_ok() {
        notify_rbac_user_change(&app, &state, target, "user_deactivated").await;
    }
    result
}

/// Re-enable a disabled account, with a reason kept in the admin change ledger.
#[tauri::command]
pub async fn reactivate_user(input: SetUserActiveInput, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let caller = require_session!(state);
    let target = input.user_id;
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        set_user_active_impl(&state, &caller, input, true).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "user_reactivated",
        Some(target),
        None,
        &result,
    )
    .await;
    if result.is_ok() {
        notify_rbac_user_change(&app, &state, target, "user_reactivated").await;
    }
    result
}

/// Disable / re-enable an account (permission and step-up are checked by the caller).
pub(crate) async fn set_user_active_impl(
    state: &AppState,
    caller: &AuthenticatedUser,
    input: SetUserActiveInput,
    active: bool,
) -> AppResult<()> {
    let what = if active {
        "enable an account"
    } else {
        "disable an account"
    };
    let reason = required_reason(&input.reason, what)?;
    let user_id = input.user_id;
    ensure_may_manage_account(&state.db, caller, user_id).await?;

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT username, is_active FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UserAccount".into(),
            id: user_id.to_string(),
        })?;
    let username: String = row.try_get("", "username")?;
    let currently_active = row.try_get::<i32>("", "is_active")? == 1;
    if currently_active == active {
        return Err(AppError::ValidationFailed(vec![if active {
            format!("The account '{username}' is already enabled.")
        } else {
            format!("The account '{username}' is already disabled.")
        }]));
    }

    if !active {
        // Guard: cannot deactivate self
        if i64::from(caller.user_id) == user_id {
            return Err(AppError::ValidationFailed(vec![
                "Cannot deactivate your own account".into()
            ]));
        }
    }

    let now = now_utc_text();
    let tx = state.db.begin().await?;
    let admins_before = active_admin_holders(&tx).await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET is_active = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
        [i32::from(active).into(), now.into(), user_id.into()],
    ))
    .await?;
    ensure_an_admin_remains(&tx, admins_before).await?;
    // A disabled account keeps no open session: closed in the store, and the in-memory session
    // of this device ends at its next command (`require_session!`).
    let closed_sessions = if active {
        0
    } else {
        crate::auth::session_manager::revoke_user_sessions(&tx, user_id).await?
    };
    let action = if active { "user_reactivated" } else { "user_deactivated" };
    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(caller.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action,
            target_user_id: Some(user_id),
            summary: Some(reason.clone()),
            diff: Some(serde_json::json!({
                "username": username,
                "is_active": { "old": currently_active, "new": active },
                "reason": reason,
                "sessions_closed": closed_sessions,
            })),
            step_up_used: true,
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;
    tx.commit().await?;

    state.permission_cache.write().await.invalidate_user(user_id);

    {
        let target_str = user_id.to_string();
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: if active {
                    crate::audit::event_type::USER_REACTIVATED
                } else {
                    crate::audit::event_type::USER_DEACTIVATED
                },
                actor_id: Some(caller.user_id),
                entity_type: Some("user_account"),
                entity_id: Some(target_str.as_str()),
                summary: if active {
                    "User account re-enabled"
                } else {
                    "User account deactivated"
                },
                ..Default::default()
            },
        )
        .await;
    }

    Ok(())
}

/// Scope types an assignment may carry. `entity`, `site` and `org_node` reference an org node
/// (its subtree is covered, see `notifications::inbox::visible_to`); `team` references a team.
pub(crate) const ASSIGNMENT_SCOPE_TYPES: &[&str] = &["tenant", "entity", "site", "team", "org_node"];

/// Check a scope type and its reference. Every non-tenant scope must name a live org node or
/// team: an assignment whose reference matches nothing would grant nothing, silently.
pub(crate) async fn validate_assignment_scope<C>(
    conn: &C,
    scope_type: &str,
    scope_reference: Option<&str>,
) -> AppResult<()>
where
    C: ConnectionTrait,
{
    if !ASSIGNMENT_SCOPE_TYPES.contains(&scope_type) {
        return Err(AppError::ValidationFailed(vec![format!(
            "scope_type must be one of: {} (got '{scope_type}')",
            ASSIGNMENT_SCOPE_TYPES.join(", ")
        )]));
    }
    if scope_type == "tenant" {
        return Ok(());
    }
    let Some(reference) = scope_reference.map(str::trim).filter(|r| !r.is_empty()) else {
        return Err(AppError::ValidationFailed(vec![format!(
            "A {scope_type} scope needs the org node or team it applies to."
        )]));
    };
    let Ok(id) = reference.parse::<i64>() else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Scope reference '{reference}' is not a valid identifier."
        )]));
    };
    let sql = if scope_type == "team" {
        "SELECT 1 AS ok FROM teams WHERE id = ? LIMIT 1"
    } else {
        "SELECT 1 AS ok FROM org_nodes WHERE id = ? AND deleted_at IS NULL LIMIT 1"
    };
    let exists = conn
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
        .await?
        .is_some();
    if !exists {
        return Err(AppError::ValidationFailed(vec![format!(
            "The {scope_type} '{reference}' does not exist (or was deleted)."
        )]));
    }
    Ok(())
}

/// Normalise an optional validity bound to `YYYY-MM-DDTHH:MM:SSZ` (the format the permission
/// checks compare as text). Accepts a date (`YYYY-MM-DD`, start or end of day) or RFC 3339.
fn normalize_validity_bound(raw: Option<&str>, field: &str, end_of_day: bool) -> AppResult<Option<String>> {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let time = if end_of_day {
            chrono::NaiveTime::from_hms_opt(23, 59, 59)
        } else {
            chrono::NaiveTime::from_hms_opt(0, 0, 0)
        };
        let Some(time) = time else {
            return Err(AppError::Internal(anyhow::anyhow!("invalid constant time")));
        };
        return Ok(Some(
            date.and_time(time).and_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ));
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| Some(t.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string()))
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be a date (YYYY-MM-DD), got '{value}'.")]))
}

/// Validity window of an assignment: both bounds optional, start before end.
pub(crate) fn normalize_validity(
    valid_from: Option<&str>,
    valid_to: Option<&str>,
) -> AppResult<(Option<String>, Option<String>)> {
    let from = normalize_validity_bound(valid_from, "valid_from", false)?;
    let to = normalize_validity_bound(valid_to, "valid_to", true)?;
    if let (Some(f), Some(t)) = (&from, &to) {
        if f > t {
            return Err(AppError::ValidationFailed(vec![
                "The validity start must be before its end.".into(),
            ]));
        }
    }
    Ok((from, to))
}

/// Shared implementation for [`assign_role_scope`]. When `emit_frontend` is true and `app` is
/// `Some`, emits `rbac-changed` to the webview; integration tests pass `false` / `None` because
/// `AppHandle<MockRuntime>` is not compatible with production `AppHandle`.
pub(crate) async fn assign_role_scope_impl(
    state: &AppState,
    caller: &AuthenticatedUser,
    input: AssignRoleScopeInput,
    emit_frontend: bool,
    app: Option<&AppHandle>,
) -> AppResult<IdPayload> {
    validate_assignment_scope(&state.db, &input.scope_type, input.scope_reference.as_deref()).await?;
    let (valid_from, valid_to) = normalize_validity(input.valid_from.as_deref(), input.valid_to.as_deref())?;
    ensure_may_grant_role(&state.db, caller, input.role_id, Some(input.user_id)).await?;
    ensure_may_manage_account(&state.db, caller, input.user_id).await?;
    let scope_reference_input = if input.scope_type == "tenant" {
        input.scope_reference.clone()
    } else {
        input.scope_reference.as_deref().map(|r| r.trim().to_string())
    };

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
            "SELECT id FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
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
    let entity_sync_id = Uuid::new_v4().to_string();
    // Capture before move into SQL params
    let role_id_for_audit = input.role_id;
    let user_id_for_audit = input.user_id;
    let scope_type_for_audit = input.scope_type.clone();
    let scope_reference_for_audit = scope_reference_input.clone();

    // Replace-at-scope: at most one assignment per (user_id, scope_type, scope_reference).
    // The superseded rows (emergency elevations included), the new row, the last-administrator
    // check and the ledger entries are one transaction.
    let scope_ref_key = scope_reference_input.as_deref().unwrap_or("");
    let tx = state.db.begin().await?;
    let admins_before = active_admin_holders(&tx).await?;
    let replaced = tx
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT usa.id, usa.role_id, r.name AS role_name FROM user_scope_assignments usa \
             INNER JOIN roles r ON r.id = usa.role_id \
             WHERE usa.user_id = ? AND usa.scope_type = ? \
               AND COALESCE(usa.scope_reference, '') = ? AND usa.deleted_at IS NULL",
            vec![
                input.user_id.into(),
                input.scope_type.clone().into(),
                scope_ref_key.into(),
            ],
        ))
        .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_scope_assignments \
         SET deleted_at = ?, updated_at = ? \
         WHERE user_id = ? \
           AND scope_type = ? \
           AND COALESCE(scope_reference, '') = ? \
           AND deleted_at IS NULL",
        vec![
            now.clone().into(),
            now.clone().into(),
            input.user_id.into(),
            input.scope_type.clone().into(),
            scope_ref_key.into(),
        ],
    ))
    .await?;

    // INSERT — partial unique index `uidx_usa_user_role_scope` (deleted_at IS NULL)
    // prevents duplicate *active* rows for the same role + scope.
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments \
            (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
             valid_from, valid_to, assigned_by_id, \
             created_at, updated_at, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        vec![
            entity_sync_id.into(),
            input.user_id.into(),
            input.role_id.into(),
            input.scope_type.into(),
            scope_reference_for_audit
                .clone()
                .map_or(sea_orm::Value::String(None), |s| s.into()),
            valid_from.clone().map_or(sea_orm::Value::String(None), |s| s.into()),
            valid_to.clone().map_or(sea_orm::Value::String(None), |s| s.into()),
            i64::from(caller.user_id).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE constraint failed") {
            AppError::ValidationFailed(vec!["This user already has this role assigned at the same scope".into()])
        } else {
            AppError::Database(e)
        }
    })?;

    let id_row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to retrieve assignment id")))?;

    let new_id: i64 = id_row.try_get("", "id")?;

    ensure_an_admin_remains(&tx, admins_before).await?;

    for old in &replaced {
        let old_id: i64 = old.try_get("", "id")?;
        let old_role_id: i64 = old.try_get("", "role_id")?;
        let old_role_name: String = old.try_get("", "role_name")?;
        crate::commands::admin_governance::write_admin_event(
            &tx,
            i64::from(caller.user_id),
            &crate::commands::admin_governance::AdminEventRecord {
                action: "role_revoked",
                target_user_id: Some(user_id_for_audit),
                target_role_id: Some(old_role_id),
                scope_type: Some(scope_type_for_audit.as_str()),
                scope_reference: scope_reference_for_audit.as_deref(),
                summary: Some(format!("Role '{old_role_name}' replaced at this scope")),
                diff: Some(serde_json::json!({
                    "assignment_id": old_id,
                    "replaced_by_assignment_id": new_id,
                })),
                step_up_used: true,
            },
            crate::commands::admin_governance::APPLY_RESULT_APPLIED,
        )
        .await?;
    }

    let detail_json = serde_json::json!({
        "role_id": role_id_for_audit,
        "scope_type": scope_type_for_audit,
        "assignment_id": new_id,
        "valid_from": valid_from,
        "valid_to": valid_to,
    });
    let detail_str = detail_json.to_string();

    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(caller.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "role_assigned",
            target_user_id: Some(user_id_for_audit),
            target_role_id: Some(role_id_for_audit),
            scope_type: Some(scope_type_for_audit.as_str()),
            scope_reference: scope_reference_for_audit.as_deref(),
            summary: Some("Role scope assignment created".into()),
            diff: Some(detail_json.clone()),
            step_up_used: true,
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;
    tx.commit().await?;

    if emit_frontend {
        if let Some(app) = app {
            notify_rbac_user_change(app, state, user_id_for_audit, "role_assigned").await;
        }
    } else {
        state.permission_cache.write().await.invalidate_user(user_id_for_audit);
    }

    {
        let target_str = user_id_for_audit.to_string();
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::ROLE_ASSIGNED,
                actor_id: Some(caller.user_id),
                entity_type: Some("user_account"),
                entity_id: Some(target_str.as_str()),
                summary: "Role scope assignment created",
                detail_json: Some(detail_str.clone()),
                ..Default::default()
            },
        )
        .await;
    }

    let _ = crate::activity::emitter::emit_rbac_event(
        &state.db,
        Some(i64::from(caller.user_id)),
        "rbac.role_assigned",
        Some(detail_json),
    )
    .await;

    Ok(IdPayload { id: new_id })
}

/// Assign a role to a user at a given scope.
#[tauri::command]
pub async fn assign_role_scope(
    input: AssignRoleScopeInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<IdPayload> {
    let caller = require_session!(state);
    let (target_user, target_role) = (input.user_id, input.role_id);
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        assign_role_scope_impl(state.inner(), &caller, input, true, Some(&app)).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "role_assigned",
        Some(target_user),
        Some(target_role),
        &result,
    )
    .await;
    result
}

/// Revoke a role-scope assignment (soft-delete). The optional reason is kept in the ledger.
#[tauri::command]
pub async fn revoke_role_scope(
    assignment_id: i64,
    reason: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = require_session!(state);
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        revoke_assignment_impl(&state, &caller, assignment_id, reason.as_deref(), false).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "role_revoked",
        None,
        None,
        &result,
    )
    .await;
    let affected_user_id = result?;
    notify_rbac_user_change(&app, &state, affected_user_id, "role_revoked").await;
    Ok(())
}

/// End an assignment (role or emergency grant) and ledger it. Returns the affected user id.
/// `emergency_only` refuses ordinary assignments (the emergency screen revokes grants only).
pub(crate) async fn revoke_assignment_impl(
    state: &AppState,
    caller: &AuthenticatedUser,
    assignment_id: i64,
    reason: Option<&str>,
    emergency_only: bool,
) -> AppResult<i64> {
    let reason = reason.map(str::trim).filter(|r| !r.is_empty()).map(str::to_string);
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT usa.user_id, usa.role_id, usa.scope_type, usa.scope_reference, usa.is_emergency, \
                    r.name AS role_name, ua.username \
             FROM user_scope_assignments usa \
             INNER JOIN roles r ON r.id = usa.role_id \
             INNER JOIN user_accounts ua ON ua.id = usa.user_id \
             WHERE usa.id = ? AND usa.deleted_at IS NULL",
            [assignment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UserScopeAssignment".into(),
            id: assignment_id.to_string(),
        })?;
    let user_id: i64 = row.try_get("", "user_id")?;
    let role_id: i64 = row.try_get("", "role_id")?;
    let scope_type: String = row.try_get("", "scope_type")?;
    let scope_reference: Option<String> = row.try_get("", "scope_reference")?;
    let is_emergency = row.try_get::<i32>("", "is_emergency")? == 1;
    let role_name: String = row.try_get("", "role_name")?;
    let username: String = row.try_get("", "username")?;

    if emergency_only && !is_emergency {
        return Err(AppError::ValidationFailed(vec![
            "Assignment is not an emergency elevation — use revoke_role_scope instead".into(),
        ]));
    }
    ensure_may_manage_account(&state.db, caller, user_id).await?;

    let now = now_utc_text();
    let tx = state.db.begin().await?;
    let admins_before = active_admin_holders(&tx).await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_scope_assignments SET deleted_at = ?, updated_at = ? \
         WHERE id = ? AND deleted_at IS NULL",
        [now.clone().into(), now.into(), assignment_id.into()],
    ))
    .await?;
    ensure_an_admin_remains(&tx, admins_before).await?;
    let action = if is_emergency {
        "emergency_grant_revoked"
    } else {
        "role_revoked"
    };
    let summary = match &reason {
        Some(r) => format!("Role '{role_name}' withdrawn from '{username}': {r}"),
        None => format!("Role '{role_name}' withdrawn from '{username}'"),
    };
    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(caller.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action,
            target_user_id: Some(user_id),
            target_role_id: Some(role_id),
            scope_type: Some(scope_type.as_str()),
            scope_reference: scope_reference.as_deref(),
            summary: Some(summary),
            diff: Some(serde_json::json!({
                "assignment_id": assignment_id,
                "reason": reason,
            })),
            step_up_used: !emergency_only,
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;
    tx.commit().await?;

    state.permission_cache.write().await.invalidate_user(user_id);

    {
        let target_str = assignment_id.to_string();
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::ROLE_REVOKED,
                actor_id: Some(caller.user_id),
                entity_type: Some("user_scope_assignment"),
                entity_id: Some(target_str.as_str()),
                summary: "Role scope assignment revoked",
                ..Default::default()
            },
        )
        .await;
    }

    Ok(user_id)
}

// ═══════════════════════════════════════════════════════════════════════════════
//  ROLE COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

/// List all roles with their permission names.
#[tauri::command]
pub async fn list_roles(state: State<'_, AppState>) -> AppResult<Vec<RoleWithPermissions>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_ROLES,
        PermissionScope::Global
    );

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, description, role_type, status, is_system \
             FROM roles WHERE deleted_at IS NULL AND status != 'retired' ORDER BY is_system DESC, name ASC",
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
pub async fn get_role(role_id: i64, state: State<'_, AppState>) -> AppResult<RoleDetail> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_ROLES,
        PermissionScope::Global
    );

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name, description, role_type, status, is_system \
             FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
            [role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: role_id.to_string(),
        })?;

    let perms = load_role_permission_names(&state.db, role_id).await?;
    let perm_set: HashSet<String> = perms.iter().cloned().collect();
    // A failed dependency check is reported (the role editor shows it), never an empty list.
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
pub async fn create_role(input: CreateRoleInput, app: AppHandle, state: State<'_, AppState>) -> AppResult<IdPayload> {
    let user = require_session!(state);
    let result = async {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_ROLES,
            PermissionScope::Global
        );
        require_step_up!(state);
        let perm_set: HashSet<String> = input.permission_names.iter().cloned().collect();
        insert_custom_role(
            &state,
            &user,
            &input.name,
            input.description.as_deref(),
            &perm_set,
            None,
        )
        .await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(user.user_id),
        "role_created",
        None,
        None,
        &result,
    )
    .await;
    if result.is_ok() {
        notify_rbac_global_change(&app, &state, "role_created").await;
    }
    result
}

/// Copy an existing role (system or custom) into a new custom role with the same permissions.
#[tauri::command]
pub async fn clone_role(input: CloneRoleInput, app: AppHandle, state: State<'_, AppState>) -> AppResult<IdPayload> {
    let user = require_session!(state);
    let source_role_id = input.source_role_id;
    let result = async {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_ROLES,
            PermissionScope::Global
        );
        require_step_up!(state);
        clone_role_impl(&state, &user, input).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(user.user_id),
        "role_created",
        None,
        Some(source_role_id),
        &result,
    )
    .await;
    if result.is_ok() {
        notify_rbac_global_change(&app, &state, "role_created").await;
    }
    result
}

pub(crate) async fn clone_role_impl(
    state: &AppState,
    caller: &AuthenticatedUser,
    input: CloneRoleInput,
) -> AppResult<IdPayload> {
    let source = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT name FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
            [input.source_role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: input.source_role_id.to_string(),
        })?;
    let source_name: String = source.try_get("", "name")?;
    let perms: HashSet<String> = load_role_permission_names(&state.db, input.source_role_id)
        .await?
        .into_iter()
        .collect();
    insert_custom_role(
        state,
        caller,
        &input.name,
        input.description.as_deref(),
        &perms,
        Some((input.source_role_id, source_name.as_str())),
    )
    .await
}

/// Insert a custom role and its permissions, validated (unique name, known permissions, hard
/// dependencies), and ledger it. `cloned_from` records the source role of a copy.
async fn insert_custom_role(
    state: &AppState,
    caller: &AuthenticatedUser,
    name: &str,
    description: Option<&str>,
    perm_set: &HashSet<String>,
    cloned_from: Option<(i64, &str)>,
) -> AppResult<IdPayload> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::ValidationFailed(vec!["Role name cannot be empty".into()]));
    }
    let description = description.map(str::trim).filter(|d| !d.is_empty());

    let dup = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE LOWER(name) = LOWER(?) AND deleted_at IS NULL AND status != 'retired'",
            [name.into()],
        ))
        .await?;
    if dup.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Role name '{name}' is already taken"
        )]));
    }

    validate_permission_names_exist(&state.db, perm_set).await?;
    resolver::validate_hard_dependencies(&state.db, perm_set).await?;
    ensure_caller_holds(&state.db, caller, perm_set).await?;

    let now = now_utc_text();
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();

    let tx = state.db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO roles (entity_sync_id, name, description, role_type, status, is_system, \
                                created_at, updated_at, row_version) \
             VALUES (?, ?, ?, 'custom', 'active', 0, ?, ?, 1)",
        vec![
            entity_sync_id.into(),
            name.into(),
            description.map(str::to_string).into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;

    let id_row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() as id",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to retrieve role id")))?;
    let new_role_id: i64 = id_row.try_get("", "id")?;

    let mut sorted: Vec<&String> = perm_set.iter().collect();
    sorted.sort();
    for perm_name in &sorted {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at, granted_by_id) \
                 SELECT ?, p.id, ?, ? FROM permissions p WHERE p.name = ?",
            vec![
                new_role_id.into(),
                now.clone().into(),
                i64::from(caller.user_id).into(),
                (*perm_name).clone().into(),
            ],
        ))
        .await?;
    }

    let summary = match cloned_from {
        Some((_, source)) => format!("Role '{name}' created as a copy of '{source}'"),
        None => format!("Role '{name}' created"),
    };
    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(caller.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "role_created",
            target_role_id: Some(new_role_id),
            summary: Some(summary),
            diff: Some(serde_json::json!({
                "name": name,
                "description": description,
                "permissions": sorted,
                "cloned_from_role_id": cloned_from.map(|(id, _)| id),
            })),
            step_up_used: true,
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;

    tx.commit().await?;

    Ok(IdPayload { id: new_role_id })
}

/// Update a custom role's description and/or permission set (system roles: description only).
#[tauri::command]
pub async fn update_role(input: UpdateRoleInput, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    let role_id = input.role_id;
    let result = async {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_ROLES,
            PermissionScope::Global
        );
        require_step_up!(state);
        update_role_impl(&state, &user, input).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(user.user_id),
        "role_updated",
        None,
        Some(role_id),
        &result,
    )
    .await;
    // Role permissions changed → invalidate all cached entries (any user may hold this role)
    if let Ok(true) = result {
        notify_rbac_global_change(&app, &state, "role_updated").await;
    }
    result.map(|_| ())
}

/// Apply a role update; returns whether the permission set changed.
pub(crate) async fn update_role_impl(
    state: &AppState,
    user: &AuthenticatedUser,
    input: UpdateRoleInput,
) -> AppResult<bool> {
    let role_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT name, description, is_system FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
            [input.role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: input.role_id.to_string(),
        })?;

    let role_name: String = role_row.try_get("", "name")?;
    let old_description: Option<String> = role_row.try_get("", "description")?;
    let is_system = role_row.try_get::<i32>("", "is_system")? == 1;
    let add: Vec<String> = input.add_permissions.clone().unwrap_or_default();
    let remove: Vec<String> = input.remove_permissions.clone().unwrap_or_default();
    let has_perm_changes = !add.is_empty() || !remove.is_empty();

    if is_system && has_perm_changes {
        return Err(AppError::PermissionDenied(
            "System role permissions cannot be modified".into(),
        ));
    }
    // A role editor cannot hand out (to this role, possibly their own) permissions they lack.
    ensure_caller_holds(&state.db, user, &add.iter().cloned().collect()).await?;

    let new_description = input
        .description
        .as_deref()
        .map(str::trim)
        .map(|d| (!d.is_empty()).then(|| d.to_string()));
    let description_changed = matches!(&new_description, Some(d) if *d != old_description);
    if !description_changed && !has_perm_changes {
        return Ok(false);
    }

    let before: HashSet<String> = load_role_permission_names(&state.db, input.role_id)
        .await?
        .into_iter()
        .collect();

    let tx = state.db.begin().await?;
    let now = now_utc_text();

    if description_changed {
        if let Some(ref desc) = new_description {
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE roles SET description = ?, updated_at = ? WHERE id = ?",
                [desc.clone().into(), now.clone().into(), input.role_id.into()],
            ))
            .await?;
        }
    }

    if has_perm_changes {
        for perm_name in &remove {
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM role_permissions WHERE role_id = ? \
                     AND permission_id = (SELECT id FROM permissions WHERE name = ?)",
                [input.role_id.into(), perm_name.clone().into()],
            ))
            .await?;
        }

        let add_set: HashSet<String> = add.iter().cloned().collect();
        validate_permission_names_exist(&tx, &add_set).await?;
        for perm_name in &add {
            tx.execute(Statement::from_sql_and_values(
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

        // Re-check dependencies on final permission set
        let final_perms = load_role_permission_names(&tx, input.role_id).await?;
        let final_set: HashSet<String> = final_perms.into_iter().collect();
        resolver::validate_hard_dependencies(&tx, &final_set).await?;
    }

    let after: HashSet<String> = load_role_permission_names(&tx, input.role_id)
        .await?
        .into_iter()
        .collect();
    let mut added: Vec<&String> = after.difference(&before).collect();
    let mut removed: Vec<&String> = before.difference(&after).collect();
    added.sort();
    removed.sort();
    let perms_changed = !added.is_empty() || !removed.is_empty();

    let mut diff = serde_json::Map::new();
    if description_changed {
        diff.insert(
            "description".into(),
            serde_json::json!({ "old": old_description, "new": new_description.clone().flatten() }),
        );
    }
    if perms_changed {
        diff.insert("permissions_added".into(), serde_json::json!(added));
        diff.insert("permissions_removed".into(), serde_json::json!(removed));
    }
    if !diff.is_empty() {
        crate::commands::admin_governance::write_admin_event(
            &tx,
            i64::from(user.user_id),
            &crate::commands::admin_governance::AdminEventRecord {
                action: "role_updated",
                target_role_id: Some(input.role_id),
                summary: Some(format!(
                    "Role '{role_name}' updated (+{} / -{} permissions)",
                    added.len(),
                    removed.len()
                )),
                diff: Some(serde_json::Value::Object(diff)),
                step_up_used: true,
                ..Default::default()
            },
            crate::commands::admin_governance::APPLY_RESULT_APPLIED,
        )
        .await?;
    }

    tx.commit().await?;
    Ok(perms_changed)
}

/// Soft-retire a role (set status = 'retired').
#[tauri::command]
pub async fn delete_role(role_id: i64, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    let result = async {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_ROLES,
            PermissionScope::Global
        );
        require_step_up!(state);
        retire_role_impl(&state, &user, role_id).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(user.user_id),
        "role_retired",
        None,
        Some(role_id),
        &result,
    )
    .await;
    if result.is_ok() {
        notify_rbac_global_change(&app, &state, "role_deleted").await;
    }
    result
}

pub(crate) async fn retire_role_impl(state: &AppState, user: &AuthenticatedUser, role_id: i64) -> AppResult<()> {
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT name, is_system FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
            [role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: role_id.to_string(),
        })?;

    if row.try_get::<i32>("", "is_system")? == 1 {
        return Err(AppError::PermissionDenied("System role cannot be modified".into()));
    }
    let role_name: String = row.try_get("", "name")?;

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

    let now = now_utc_text();
    let tx = state.db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE roles SET status = 'retired', updated_at = ? WHERE id = ?",
        [now.into(), role_id.into()],
    ))
    .await?;
    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(user.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "role_retired",
            target_role_id: Some(role_id),
            summary: Some(format!("Role '{role_name}' retired")),
            step_up_used: true,
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// List all role templates.
#[tauri::command]
pub async fn list_role_templates(state: State<'_, AppState>) -> AppResult<Vec<model::RoleTemplate>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_ROLES,
        PermissionScope::Global
    );

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
    require_permission!(
        state,
        &caller,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );

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
    let (target_user, target_role) = (input.user_id, input.role_id);
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        grant_emergency_elevation_impl(&state, &caller, input).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "emergency_grant_created",
        Some(target_user),
        Some(target_role),
        &result,
    )
    .await;
    if result.is_ok() {
        notify_rbac_user_change(&app, &state, target_user, "emergency_granted").await;
    }
    result
}

/// The longest emergency elevation the plant allows (`rbac_settings.emergency_max_minutes`,
/// seeded to 480). A missing or invalid value refuses the grant rather than guessing a limit.
async fn ensure_emergency_within_limit(
    db: &sea_orm::DatabaseConnection,
    expires: chrono::DateTime<chrono::Utc>,
) -> AppResult<()> {
    let raw: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value FROM rbac_settings WHERE key = 'emergency_max_minutes'",
            [],
        ))
        .await?
        .and_then(|r| r.try_get("", "value").ok());
    let Some(max_minutes) = raw.and_then(|v| v.trim().parse::<i64>().ok()).filter(|m| *m > 0) else {
        return Err(AppError::ValidationFailed(vec![
            "The maximum emergency access duration (emergency_max_minutes) is not configured.".into(),
        ]));
    };
    if expires > chrono::Utc::now() + chrono::Duration::minutes(max_minutes) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Emergency access may last at most {max_minutes} minutes."
        )]));
    }
    Ok(())
}

/// Parse the expiry of an emergency grant (RFC 3339) and normalise it to UTC
/// `YYYY-MM-DDTHH:MM:SSZ`, the text format the permission checks compare against.
fn normalize_emergency_expiry(raw: &str) -> AppResult<chrono::DateTime<chrono::Utc>> {
    let expires = chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!("The expiry '{raw}' is not a date and time (RFC 3339).")])
        })?
        .with_timezone(&chrono::Utc);
    if expires <= chrono::Utc::now() {
        return Err(AppError::ValidationFailed(vec![
            "The emergency access must expire in the future.".into(),
        ]));
    }
    Ok(expires)
}

pub(crate) async fn grant_emergency_elevation_impl(
    state: &AppState,
    caller: &AuthenticatedUser,
    input: GrantEmergencyElevationInput,
) -> AppResult<IdPayload> {
    let reason = required_reason(&input.reason, "grant emergency access")?;
    let expires = normalize_emergency_expiry(&input.expires_at)?;
    ensure_emergency_within_limit(&state.db, expires).await?;
    ensure_may_grant_role(&state.db, caller, input.role_id, Some(input.user_id)).await?;
    ensure_may_manage_account(&state.db, caller, input.user_id).await?;
    let expires_text = expires.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    validate_assignment_scope(&state.db, &input.scope_type, input.scope_reference.as_deref()).await?;
    let scope_reference = if input.scope_type == "tenant" {
        None
    } else {
        input.scope_reference.as_deref().map(|r| r.trim().to_string())
    };

    let target = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT username, is_active FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [input.user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "UserAccount".into(),
            id: input.user_id.to_string(),
        })?;
    if target.try_get::<i32>("", "is_active")? != 1 {
        return Err(AppError::ValidationFailed(vec![
            "Emergency access cannot be granted to a disabled account.".into(),
        ]));
    }
    let username: String = target.try_get("", "username")?;
    let role_name: String = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT name FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
            [input.role_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Role".into(),
            id: input.role_id.to_string(),
        })?
        .try_get("", "name")?;

    let now = now_utc_text();
    let entity_sync_id = Uuid::new_v4().to_string();

    let tx = state.db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments \
         (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
          is_emergency, emergency_reason, emergency_expires_at, valid_to, \
          assigned_by_id, notes, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?)",
        [
            entity_sync_id.into(),
            input.user_id.into(),
            input.role_id.into(),
            input.scope_type.clone().into(),
            scope_reference.clone().into(),
            reason.clone().into(),
            expires_text.clone().into(),
            // The grant also closes its validity window (both are checked everywhere).
            expires_text.clone().into(),
            caller.user_id.into(),
            format!("Emergency grant by user {}: {}", caller.user_id, reason).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    let id: i64 = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to get last insert id")))?
        .try_get("", "id")?;

    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(caller.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "emergency_grant_created",
            target_user_id: Some(input.user_id),
            target_role_id: Some(input.role_id),
            scope_type: Some(input.scope_type.as_str()),
            scope_reference: scope_reference.as_deref(),
            summary: Some(format!(
                "Emergency role '{role_name}' granted to '{username}' until {expires_text}: {reason}"
            )),
            diff: Some(serde_json::json!({
                "assignment_id": id,
                "reason": reason,
                "expires_at": expires_text,
            })),
            step_up_used: true,
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;
    tx.commit().await?;

    state.permission_cache.write().await.invalidate_user(input.user_id);

    {
        let target_str = input.user_id.to_string();
        let detail = serde_json::json!({
            "role_id": input.role_id,
            "reason": reason,
            "assignment_id": id,
        })
        .to_string();
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::RBAC_EMERGENCY_GRANT_CREATED,
                actor_id: Some(caller.user_id),
                entity_type: Some("user_account"),
                entity_id: Some(target_str.as_str()),
                summary: "Emergency elevation granted",
                detail_json: Some(detail),
                ..Default::default()
            },
        )
        .await;
    }

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
    let result = async {
        require_permission!(
            state,
            &caller,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        revoke_assignment_impl(&state, &caller, input.assignment_id, input.reason.as_deref(), true).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(caller.user_id),
        "emergency_grant_revoked",
        None,
        None,
        &result,
    )
    .await;
    let target_user_id = result?;
    notify_rbac_user_change(&app, &state, target_user_id, "emergency_revoked").await;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
//  ACCOUNT LOCKOUT MANAGEMENT
// ═══════════════════════════════════════════════════════════════════════════════

/// Unlock a locked user account. Resets failed attempts, lockout timer, and
/// consecutive lockout counter. Writes an admin change event for audit.
#[tauri::command]
pub async fn unlock_user_account(user_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let admin = require_session!(state);
    let result = async {
        require_permission!(
            state,
            &admin,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        unlock_user_account_impl(&state, &admin, user_id).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(admin.user_id),
        "account_unlocked",
        Some(user_id),
        None,
        &result,
    )
    .await;
    result
}

async fn unlock_user_account_impl(state: &AppState, admin: &AuthenticatedUser, user_id: i64) -> AppResult<()> {
    ensure_may_manage_account(&state.db, admin, user_id).await?;
    // Verify user exists
    let exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;

    if exists.is_none() {
        return Err(AppError::NotFound {
            entity: "user_accounts".into(),
            id: user_id.to_string(),
        });
    }

    let now = chrono::Utc::now().to_rfc3339();
    let tx = state.db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts \
         SET failed_login_attempts = 0, \
             locked_until = NULL, \
             consecutive_lockouts = 0, \
             updated_at = ? \
         WHERE id = ?",
        [now.into(), user_id.into()],
    ))
    .await?;

    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(admin.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "account_unlocked",
            target_user_id: Some(user_id),
            summary: Some("Admin unlocked user account".into()),
            step_up_used: true,
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;
    tx.commit().await?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
//  ADMIN PASSWORD RESET (existing users)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct AdminSetUserPasswordInput {
    pub user_id: i64,
    /// When omitted/null, a policy-compliant temporary password is generated.
    pub temporary_password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminSetUserPasswordResult {
    pub temporary_password: String,
}

/// A password an administrator sets (new account, reset) meets both the fixed minimum of
/// [`validate_password_strength`] and the plant password policy (Settings › Security), so the
/// policy can only make it stronger.
async fn validate_admin_set_password(db: &sea_orm::DatabaseConnection, password: &str) -> AppResult<()> {
    validate_password_strength(password)?;
    let policy = crate::auth::password_policy::PasswordPolicy::load(db).await;
    crate::auth::password_policy::validate_password_strength(password, &policy).map_err(AppError::ValidationFailed)
}

fn generate_temporary_password(length: usize) -> String {
    use rand::Rng;
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
    const DIGIT: &[u8] = b"23456789";
    const SPECIAL: &[u8] = b"!@#$%&*";
    let mut rng = rand::thread_rng();
    let mut chars: Vec<u8> = Vec::with_capacity(length.max(4));
    chars.push(UPPER[rng.gen_range(0..UPPER.len())]);
    chars.push(LOWER[rng.gen_range(0..LOWER.len())]);
    chars.push(DIGIT[rng.gen_range(0..DIGIT.len())]);
    chars.push(SPECIAL[rng.gen_range(0..SPECIAL.len())]);
    let alphabet: Vec<u8> = UPPER
        .iter()
        .chain(LOWER.iter())
        .chain(DIGIT.iter())
        .chain(SPECIAL.iter())
        .copied()
        .collect();
    for _ in 0..length.saturating_sub(4) {
        chars.push(alphabet[rng.gen_range(0..alphabet.len())]);
    }
    // Fisher–Yates shuffle
    for i in (1..chars.len()).rev() {
        let j = rng.gen_range(0..=i);
        chars.swap(i, j);
    }
    String::from_utf8(chars).expect("temporary password is ASCII")
}

/// Set a temporary password for an existing local/hybrid user. Returns the plaintext
/// password once for admin show-once UI. Requires `adm.users` + step-up.
#[tauri::command]
pub async fn admin_set_user_password(
    input: AdminSetUserPasswordInput,
    state: State<'_, AppState>,
) -> AppResult<AdminSetUserPasswordResult> {
    let admin = require_session!(state);
    let target = input.user_id;
    let result = async {
        require_permission!(
            state,
            &admin,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        admin_set_user_password_impl(&state, &admin, input).await
    }
    .await;
    crate::commands::admin_governance::record_blocked_admin_action(
        &state.db,
        i64::from(admin.user_id),
        "password_reset_by_admin",
        Some(target),
        None,
        &result,
    )
    .await;
    result
}

async fn admin_set_user_password_impl(
    state: &AppState,
    admin: &AuthenticatedUser,
    input: AdminSetUserPasswordInput,
) -> AppResult<AdminSetUserPasswordResult> {
    ensure_may_manage_account(&state.db, admin, input.user_id).await?;
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, username, identity_mode, is_active, deleted_at \
             FROM user_accounts WHERE id = ?",
            [input.user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "user_accounts".into(),
            id: input.user_id.to_string(),
        })?;

    let deleted_at: Option<String> = row.try_get("", "deleted_at").ok().flatten();
    if deleted_at.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Cannot reset password for a deleted user account.".into(),
        ]));
    }

    let is_active: i32 = row.try_get("", "is_active").unwrap_or(0);
    if is_active != 1 {
        return Err(AppError::ValidationFailed(vec![
            "Cannot reset password for an inactive user account.".into(),
        ]));
    }

    let identity_mode: String = row.try_get("", "identity_mode").unwrap_or_else(|_| "local".into());
    if identity_mode == "sso" {
        return Err(AppError::ValidationFailed(vec![
            "SSO-only accounts do not have a local password to reset.".into(),
        ]));
    }

    let plaintext = match input
        .temporary_password
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pw) => {
            validate_admin_set_password(&state.db, pw).await?;
            pw.to_string()
        }
        None => {
            let policy = crate::auth::password_policy::PasswordPolicy::load(&state.db).await;
            let generated = generate_temporary_password(policy.min_length.max(16));
            validate_admin_set_password(&state.db, &generated).await?;
            generated
        }
    };

    let password_hash = crate::auth::password::hash_password(&plaintext)?;
    let now = chrono::Utc::now().to_rfc3339();

    let tx = state.db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts \
         SET password_hash = ?, \
             force_password_change = 1, \
             password_changed_at = ?, \
             failed_login_attempts = 0, \
             locked_until = NULL, \
             consecutive_lockouts = 0, \
             updated_at = ? \
         WHERE id = ?",
        [
            password_hash.into(),
            now.clone().into(),
            now.into(),
            input.user_id.into(),
        ],
    ))
    .await?;
    // The old password's sessions end: the user signs in again and must change the password.
    crate::auth::session_manager::revoke_user_sessions(&tx, input.user_id).await?;

    let username: String = row.try_get("", "username").unwrap_or_default();
    crate::commands::admin_governance::write_admin_event(
        &tx,
        i64::from(admin.user_id),
        &crate::commands::admin_governance::AdminEventRecord {
            action: "password_reset_by_admin",
            target_user_id: Some(input.user_id),
            summary: Some(format!(
                "Admin reset local password for user '{username}' (change required at next sign-in)"
            )),
            step_up_used: true,
            ..Default::default()
        },
        crate::commands::admin_governance::APPLY_RESULT_APPLIED,
    )
    .await?;
    tx.commit().await?;

    tracing::info!(
        actor_id = admin.user_id,
        target_user_id = input.user_id,
        "admin_set_user_password completed"
    );

    Ok(AdminSetUserPasswordResult {
        temporary_password: plaintext,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
//  USER PRESENCE
// ═══════════════════════════════════════════════════════════════════════════════

/// Batch-fetch presence status for a list of user IDs.
/// Any authenticated user can call this — presence is not sensitive data.
#[tauri::command]
pub async fn get_user_presence(user_ids: Vec<i64>, state: State<'_, AppState>) -> AppResult<Vec<UserPresence>> {
    let _user = require_session!(state);

    if user_ids.is_empty() {
        return Ok(vec![]);
    }

    // Build parameterized IN clause
    let placeholders: Vec<String> = user_ids.iter().map(|_| "?".to_string()).collect();
    let now_str = chrono::Utc::now().to_rfc3339();
    let sql = format!(
        "SELECT CAST(s.user_id AS TEXT) AS user_id, s.last_activity_at \
         FROM app_sessions s \
         WHERE CAST(s.user_id AS TEXT) IN ({}) \
           AND s.is_revoked = 0 \
           AND s.expires_at > ? \
         ORDER BY s.last_activity_at DESC",
        placeholders.join(", ")
    );

    let mut values: Vec<sea_orm::Value> = user_ids.iter().map(|id| sea_orm::Value::from(id.to_string())).collect();
    values.push(now_str.into());

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;

    // Collect best (most recent) session per user
    let mut best: HashMap<String, Option<String>> = HashMap::new();
    for r in &rows {
        let uid: String = r.try_get("", "user_id").unwrap_or_default();
        let activity: Option<String> = r.try_get("", "last_activity_at").ok();
        best.entry(uid).or_insert(activity);
    }

    let now = chrono::Utc::now();
    let idle_threshold = chrono::Duration::minutes(5);

    let mut results: Vec<UserPresence> = Vec::with_capacity(user_ids.len());
    for uid in &user_ids {
        let uid_str = uid.to_string();
        let (status, last_activity_at) = match best.get(&uid_str) {
            Some(Some(ts)) => {
                let parsed = chrono::DateTime::parse_from_rfc3339(ts).or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%SZ")
                        .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S"))
                        .map(|ndt| ndt.and_utc().fixed_offset())
                });
                match parsed {
                    Ok(dt) => {
                        if now.signed_duration_since(dt) <= idle_threshold {
                            ("active".to_string(), Some(ts.clone()))
                        } else {
                            ("idle".to_string(), Some(ts.clone()))
                        }
                    }
                    Err(_) => ("idle".to_string(), Some(ts.clone())),
                }
            }
            Some(None) => ("idle".to_string(), None),
            None => ("offline".to_string(), None),
        };

        results.push(UserPresence {
            user_id: *uid,
            status,
            last_activity_at,
        });
    }

    Ok(results)
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
             usa.valid_from, usa.valid_to, usa.is_emergency, usa.emergency_expires_at, \
             CASE WHEN usa.scope_type = 'team' THEN tm.name \
             WHEN usa.scope_type <> 'tenant' THEN onode.name END AS scope_label \
             FROM user_scope_assignments usa \
             INNER JOIN roles r ON r.id = usa.role_id \
             LEFT JOIN org_nodes onode ON usa.scope_type <> 'team' AND usa.scope_type <> 'tenant' \
             AND CAST(onode.id AS TEXT) = usa.scope_reference \
             LEFT JOIN teams tm ON usa.scope_type = 'team' AND CAST(tm.id AS TEXT) = usa.scope_reference \
             WHERE usa.user_id = ? AND usa.deleted_at IS NULL \
             ORDER BY r.name ASC",
            [user_id.into()],
        ))
        .await?;

    let mut assignments = Vec::with_capacity(rows.len());
    for r in &rows {
        assignments.push(RoleAssignmentSummary {
            assignment_id: r.try_get("", "assignment_id")?,
            role_id: r.try_get("", "role_id")?,
            role_name: r.try_get("", "role_name")?,
            scope_type: r.try_get("", "scope_type")?,
            scope_reference: r.try_get::<Option<String>>("", "scope_reference")?,
            valid_from: r.try_get::<Option<String>>("", "valid_from")?,
            valid_to: r.try_get::<Option<String>>("", "valid_to")?,
            is_emergency: r.try_get::<i32>("", "is_emergency")? == 1,
            emergency_expires_at: r.try_get::<Option<String>>("", "emergency_expires_at")?,
            scope_label: r.try_get::<Option<String>>("", "scope_label")?,
        });
    }

    Ok(assignments)
}

async fn resolve_default_tenant_membership_role_id<C>(conn: &C) -> AppResult<i64>
where
    C: ConnectionTrait,
{
    let bootstrap_role_row = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            r#"SELECT id
               FROM roles
               WHERE deleted_at IS NULL
                 AND status != 'retired'
               ORDER BY CASE name
                 WHEN 'Readonly' THEN 0
                 WHEN 'Operator' THEN 1
                 WHEN 'Maintenance Technician' THEN 2
                 WHEN 'Supervisor' THEN 3
                 WHEN 'Administrator' THEN 4
                 ELSE 99
               END, id
               LIMIT 1"#
                .to_string(),
        ))
        .await?;
    let bootstrap_role_id: i64 = bootstrap_role_row
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "Cannot create user: no active role is available for tenant membership.".into(),
            ])
        })?
        .try_get("", "id")?;
    Ok(bootstrap_role_id)
}

async fn list_missing_tenant_scope_users(db: &sea_orm::DatabaseConnection) -> AppResult<Vec<MissingTenantScopeUser>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ua.id AS user_id, ua.username, ua.identity_mode, \
                    CASE \
                      WHEN EXISTS ( \
                        SELECT 1 FROM user_scope_assignments usa_any \
                        WHERE usa_any.user_id = ua.id \
                          AND usa_any.deleted_at IS NULL \
                      ) THEN 1 ELSE 0 \
                    END AS has_any_role_assignment \
             FROM user_accounts ua \
             WHERE ua.is_active = 1 \
               AND ua.deleted_at IS NULL \
               AND NOT EXISTS ( \
                 SELECT 1 FROM user_scope_assignments usa \
                 WHERE usa.user_id = ua.id \
                   AND usa.scope_type = 'tenant' \
                   AND {} \
               ) \
             ORDER BY ua.username ASC",
                resolver::ASSIGNMENT_IN_FORCE_SQL
            ),
            resolver::in_force_binds(&resolver::assignment_clock()).to_vec(),
        ))
        .await?;

    let users = rows
        .iter()
        .filter_map(|r| {
            Some(MissingTenantScopeUser {
                user_id: r.try_get("", "user_id").ok()?,
                username: r.try_get("", "username").ok()?,
                identity_mode: r.try_get("", "identity_mode").ok()?,
                has_any_role_assignment: r.try_get::<i32>("", "has_any_role_assignment").ok()? == 1,
            })
        })
        .collect();
    Ok(users)
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
async fn load_role_permission_names(db: &impl ConnectionTrait, role_id: i64) -> AppResult<Vec<String>> {
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
async fn validate_permission_names_exist(db: &impl ConnectionTrait, names: &HashSet<String>) -> AppResult<()> {
    if names.is_empty() {
        return Ok(());
    }

    let placeholders: String = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("SELECT name FROM permissions WHERE name IN ({placeholders})");
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

// ═══════════════════════════════════════════════════════════════════════════════
//  ANTI-ESCALATION (who may grant / manage what)
// ═══════════════════════════════════════════════════════════════════════════════

/// A plant administrator: the rule of elevated admin access (`auth::rbac`): the active bootstrap
/// account, or an active account holding an administrator role plant-wide (tenant scope)
/// through a permanent assignment in force. An emergency elevation is temporary and cannot make
/// anything permanent.
async fn is_plant_admin<C: ConnectionTrait>(conn: &C, user_id: i64) -> AppResult<bool> {
    let Ok(id) = i32::try_from(user_id) else {
        return Ok(false);
    };
    crate::auth::rbac::user_has_elevated_admin_access(conn, id).await
}

/// The account has (or may get back by re-enabling) administrator access: the bootstrap flag or
/// any live assignment of an administrator role, emergency included, whatever the account state.
async fn is_admin_account<C: ConnectionTrait>(conn: &C, user_id: i64) -> AppResult<bool> {
    let placeholders = vec!["?"; ADMIN_ROLE_NAMES.len()].join(",");
    let sql = format!(
        "SELECT CASE WHEN EXISTS (SELECT 1 FROM user_accounts WHERE id = ? AND is_admin = 1) \
                  OR EXISTS (SELECT 1 FROM user_scope_assignments usa \
                             INNER JOIN roles r ON r.id = usa.role_id \
                             WHERE usa.user_id = ? AND usa.deleted_at IS NULL \
                               AND r.name IN ({placeholders})) \
                  THEN 1 ELSE 0 END AS v"
    );
    let mut values: Vec<sea_orm::Value> = vec![user_id.into(), user_id.into()];
    values.extend(ADMIN_ROLE_NAMES.iter().map(|n| (*n).into()));
    Ok(conn
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .and_then(|r| r.try_get::<i32>("", "v").ok())
        .unwrap_or(0)
        == 1)
}

/// Permissions the caller holds plant-wide through permanent assignments in force: a running
/// emergency grant is temporary and never lets its holder create, grant or manage anything.
async fn caller_permissions(
    db: &sea_orm::DatabaseConnection,
    caller: &AuthenticatedUser,
) -> AppResult<HashSet<String>> {
    resolver::permanent_tenant_permissions(db, i64::from(caller.user_id)).await
}

/// The permissions of `wanted` the caller lacks, sorted (see [`caller_permissions`]).
async fn missing_from_caller(
    db: &sea_orm::DatabaseConnection,
    caller: &AuthenticatedUser,
    wanted: &HashSet<String>,
) -> AppResult<Vec<String>> {
    let held = caller_permissions(db, caller).await?;
    let mut missing: Vec<String> = wanted.iter().filter(|p| !held.contains(*p)).cloned().collect();
    missing.sort();
    Ok(missing)
}

/// "a, b, c, …" (at most ten names) for a refusal message.
fn shown_permissions(missing: &[String]) -> String {
    let shown: Vec<&str> = missing.iter().take(10).map(String::as_str).collect();
    format!(
        "{}{}",
        shown.join(", "),
        if missing.len() > shown.len() { ", …" } else { "" }
    )
}

/// The caller holds every permission of `wanted` (role editors cannot hand out more than they
/// have); a plant administrator holds everything.
pub(crate) async fn ensure_caller_holds(
    db: &sea_orm::DatabaseConnection,
    caller: &AuthenticatedUser,
    wanted: &HashSet<String>,
) -> AppResult<()> {
    if wanted.is_empty() || is_plant_admin(db, i64::from(caller.user_id)).await? {
        return Ok(());
    }
    let missing = missing_from_caller(db, caller, wanted).await?;
    if missing.is_empty() {
        return Ok(());
    }
    Err(AppError::PermissionDenied(format!(
        "You can only grant permissions you hold yourself; missing: {}",
        shown_permissions(&missing)
    )))
}

/// Granting `role_id` to `target` (None for a new account): never to oneself; administrator
/// roles only by a plant administrator; other roles only when the caller holds all their
/// permissions.
async fn ensure_may_grant_role(
    db: &sea_orm::DatabaseConnection,
    caller: &AuthenticatedUser,
    role_id: i64,
    target: Option<i64>,
) -> AppResult<()> {
    if target == Some(i64::from(caller.user_id)) {
        return Err(AppError::PermissionDenied(
            "You cannot give a role or emergency access to your own account.".into(),
        ));
    }
    let role_name: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT name FROM roles WHERE id = ? AND deleted_at IS NULL",
            [role_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "name").ok());
    let Some(role_name) = role_name else {
        return Err(AppError::NotFound {
            entity: "Role".into(),
            id: role_id.to_string(),
        });
    };
    if ADMIN_ROLE_NAMES.contains(&role_name.as_str()) && !is_plant_admin(db, i64::from(caller.user_id)).await? {
        return Err(AppError::PermissionDenied(format!(
            "Only a plant administrator can grant the role '{role_name}'."
        )));
    }
    let perms: HashSet<String> = load_role_permission_names(db, role_id).await?.into_iter().collect();
    ensure_caller_holds(db, caller, &perms).await
}

/// Managing an account (details, disable, re-enable, unlock, password reset, roles, sessions):
/// an administrator account only by a plant administrator; any other account only by someone
/// who holds every permission that account has right now (otherwise a user manager could take
/// over a stronger account by resetting its password).
pub(crate) async fn ensure_may_manage_account(
    db: &sea_orm::DatabaseConnection,
    caller: &AuthenticatedUser,
    target: i64,
) -> AppResult<()> {
    let caller_id = i64::from(caller.user_id);
    if is_plant_admin(db, caller_id).await? {
        return Ok(());
    }
    if is_admin_account(db, target).await? {
        return Err(AppError::PermissionDenied(
            "Only a plant administrator can change an administrator account.".into(),
        ));
    }
    if target == caller_id {
        // One's own account: its permissions are one's own (grants to oneself are refused).
        return Ok(());
    }
    // Future grants count too: no takeover prepared before the account becomes stronger.
    let target_permissions = resolver::reachable_permissions(db, target).await?;
    let missing = missing_from_caller(db, caller, &target_permissions).await?;
    if missing.is_empty() {
        return Ok(());
    }
    Err(AppError::PermissionDenied(format!(
        "You can only manage an account whose permissions you all hold yourself; missing: {}",
        shown_permissions(&missing)
    )))
}

/// Role names that make an account a plant administrator (Phase 1 name `Administrator`).
const ADMIN_ROLE_NAMES: &[&str] = &["Superadmin", "Administrator"];

/// Accounts that can administer the plant, counted exactly like [`is_plant_admin`] (the elevated
/// admin rule of `auth::rbac`: the active bootstrap account, or a permanent, in-force,
/// tenant-scope administrator assignment). An emergency grant expires by itself, so it does
/// not keep the plant administrable.
async fn active_admin_holders<C: ConnectionTrait>(conn: &C) -> AppResult<i64> {
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM user_accounts ua WHERE {}",
        crate::auth::rbac::elevated_admin_sql()
    );
    let values = resolver::permanent_binds(&resolver::assignment_clock()).to_vec();
    Ok(conn
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0))
}

/// Run inside the transaction of a change, after it: refuses (the caller's transaction is then
/// dropped, i.e. rolled back) when the change took away the last plant administrator.
async fn ensure_an_admin_remains<C: ConnectionTrait>(conn: &C, before: i64) -> AppResult<()> {
    if before > 0 && active_admin_holders(conn).await? == 0 {
        return Err(AppError::ValidationFailed(vec![
            "This change would leave the plant without an active administrator.".into(),
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
pub(crate) fn validate_password_strength(password: &str) -> AppResult<()> {
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

#[cfg(test)]
mod admin_set_password_tests {
    use super::{generate_temporary_password, validate_password_strength};

    #[test]
    fn generated_temporary_password_meets_strength_policy() {
        for _ in 0..20 {
            let pw = generate_temporary_password(16);
            assert_eq!(generate_temporary_password(24).len(), 24);
            assert!(pw.len() >= 8, "len={}", pw.len());
            assert!(
                validate_password_strength(&pw).is_ok(),
                "generated password failed policy: {pw}"
            );
            assert!(pw.chars().any(|c| c.is_uppercase()));
            assert!(pw.chars().any(|c| c.is_lowercase()));
            assert!(pw.chars().any(|c| c.is_ascii_digit()));
            assert!(pw.chars().any(|c| "!@#$%&*".contains(c)));
        }
    }

    #[test]
    fn validate_password_strength_rejects_weak_passwords() {
        assert!(validate_password_strength("short").is_err());
        assert!(validate_password_strength("alllowercase1").is_err());
        assert!(validate_password_strength("ALLUPPERCASE1").is_err());
        assert!(validate_password_strength("NoDigitsHere").is_err());
        assert!(validate_password_strength("ValidPass1").is_ok());
    }
}

#[cfg(test)]
#[path = "admin_users_tests.rs"]
mod admin_users_tests;
