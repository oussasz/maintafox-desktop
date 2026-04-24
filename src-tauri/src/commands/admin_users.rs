//! User and Role administration IPC commands — Phase 2 SP06-F01.
//!
//! Permission gates:
//!   adm.users  — list_users, get_user, list_assignable_roles, create_user, update_user, deactivate_user,
//!                assign_role_scope, revoke_role_scope, simulate_access
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
    pub locked_until: Option<String>,
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
        return Err(AppError::ValidationFailed(vec![
            "Email format is invalid.".into(),
        ]));
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
                    "Phone number format is invalid.".into(),
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
        "SELECT ua.id, ua.username, ua.display_name, ua.personnel_id, ua.email, ua.phone, ua.identity_mode, \
                ua.is_active, ua.force_password_change, ua.last_seen_at, ua.locked_until \
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
            personnel_id: r.try_get::<Option<i64>>("", "personnel_id").ok().flatten(),
            email: r.try_get::<Option<String>>("", "email").ok().flatten(),
            phone: r.try_get::<Option<String>>("", "phone").ok().flatten(),
            identity_mode: r.try_get("", "identity_mode")?,
            is_active: r.try_get::<i32>("", "is_active")? == 1,
            force_password_change: r.try_get::<i32>("", "force_password_change")? == 1,
            last_seen_at: r.try_get("", "last_seen_at").ok(),
            locked_until: r.try_get::<Option<String>>("", "locked_until").unwrap_or(None),
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
            "SELECT id, username, display_name, personnel_id, email, phone, identity_mode, is_active, \
                    force_password_change, last_seen_at, locked_until \
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
            personnel_id: row.try_get::<Option<i64>>("", "personnel_id").ok().flatten(),
            email: row.try_get::<Option<String>>("", "email").ok().flatten(),
            phone: row.try_get::<Option<String>>("", "phone").ok().flatten(),
            identity_mode: row.try_get("", "identity_mode")?,
            is_active: row.try_get::<i32>("", "is_active")? == 1,
            force_password_change: row.try_get::<i32>("", "force_password_change")? == 1,
            last_seen_at: row.try_get("", "last_seen_at").ok(),
            locked_until: row.try_get::<Option<String>>("", "locked_until").unwrap_or(None),
            roles,
        },
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
    require_permission!(state, &user, "adm.users", PermissionScope::Global);

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

/// Verify `role_id` refers to a non-deleted, non-retired role (for inserts under a transaction).
async fn ensure_assignable_role_id<C>(conn: &C, role_id: i64) -> AppResult<()>
where
    C: ConnectionTrait,
{
    if role_id <= 0 {
        return Err(AppError::ValidationFailed(vec![
            "role_id must be a positive integer.".into(),
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

/// Create a new user account.
#[tauri::command]
pub async fn create_user(
    input: CreateUserInput,
    app: AppHandle,
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
            return Err(AppError::ValidationFailed(vec![
                "Personnel record not found.".into(),
            ]));
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
    let sync_id = Uuid::new_v4().to_string();
    let assignment_sync_id = Uuid::new_v4().to_string();
    // Capture before move into SQL params
    let username_for_audit = input.username.clone();
    let identity_mode_for_audit = input.identity_mode.clone();
    let role_id = input.role_id;

    let activated_tenant_id =
        crate::commands::product_license::get_activation_claim_tenant_id(&state.db).await?;

    let tx = state.db.begin().await?;

    ensure_assignable_role_id(&tx, role_id).await?;

    tx.execute(Statement::from_sql_and_values(
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
            (sync_id, user_id, role_id, scope_type, scope_reference, \
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

    tx.commit().await?;

    notify_rbac_user_change(&app, &state, new_id, "user_created");

    {
        let new_id_str = new_id.to_string();
        let detail = format!(
            r#"{{"username":"{}","identity_mode":"{}","tenant_id":"{}","role_id":{}}}"#,
            username_for_audit,
            identity_mode_for_audit,
            activated_tenant_id.unwrap_or_default(),
            role_id
        );
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::USER_CREATED,
                actor_id: Some(caller.user_id as i32),
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
pub async fn list_users_missing_tenant_scope(
    state: State<'_, AppState>,
) -> AppResult<Vec<MissingTenantScopeUser>> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    list_missing_tenant_scope_users(&state.db).await
}

/// Backfill tenant scope for users missing it, using the currently activated tenant claim.
#[tauri::command]
pub async fn backfill_users_missing_tenant_scope(
    state: State<'_, AppState>,
) -> AppResult<TenantScopeBackfillResult> {
    let caller = require_session!(state);
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    require_step_up!(state);

    let activated_tenant_id =
        crate::commands::product_license::get_activation_claim_tenant_id(&state.db).await?;
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
                    (sync_id, user_id, role_id, scope_type, scope_reference, \
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
    if let Some(ref display_name) = input.display_name {
        let normalized = display_name.trim();
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
        sets.push("email = ?");
        values.push(normalized.map_or(sea_orm::Value::String(None), |s| s.into()));
    }
    if let Some(ref phone) = input.phone {
        let normalized = normalize_phone_e164(phone)?;
        sets.push("phone = ?");
        values.push(normalized.map_or(sea_orm::Value::String(None), |s| s.into()));
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

    {
        let target_str = user_id.to_string();
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::USER_DEACTIVATED,
                actor_id: Some(caller.user_id as i32),
                entity_type: Some("user_account"),
                entity_id: Some(target_str.as_str()),
                summary: "User account deactivated",
                ..Default::default()
            },
        )
        .await;
    }

    Ok(())
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
    let sync_id = Uuid::new_v4().to_string();
    // Capture before move into SQL params
    let role_id_for_audit = input.role_id;
    let user_id_for_audit = input.user_id;
    let scope_type_for_audit = input.scope_type.clone();
    let scope_reference_for_audit = input.scope_reference.clone();

    // Replace-at-scope: at most one assignment per (user_id, scope_type,
    // scope_reference) before insert. Supersedes prior rows including
    // emergency elevations so admins can assign without duplicate-key errors.
    let scope_ref_key = input
        .scope_reference
        .as_deref()
        .unwrap_or("");
    state
        .db
        .execute(Statement::from_sql_and_values(
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
                scope_reference_for_audit
                    .clone()
                    .map_or(sea_orm::Value::String(None), |s| s.into()),
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

    if emit_frontend {
        if let Some(app) = app {
            notify_rbac_user_change(app, state, user_id_for_audit, "role_assigned");
        }
    } else if let Ok(mut cache) = state.permission_cache.try_write() {
        cache.invalidate_user(user_id_for_audit);
    }

    let detail_json = serde_json::json!({
        "role_id": role_id_for_audit,
        "scope_type": scope_type_for_audit,
        "assignment_id": new_id,
    });
    let detail_str = detail_json.to_string();

    crate::commands::admin_governance::record_admin_event(
        &state.db,
        "role_assigned",
        i64::from(caller.user_id),
        Some(user_id_for_audit),
        Some(role_id_for_audit),
        Some(scope_type_for_audit.as_str()),
        scope_reference_for_audit.as_deref(),
        Some("Role scope assignment created"),
        Some(detail_str.as_str()),
        true,
    )
    .await?;

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
    require_permission!(state, &caller, "adm.users", PermissionScope::Global);
    require_step_up!(state);

    assign_role_scope_impl(state.inner(), &caller, input, true, Some(&app)).await
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

    {
        let target_str = assignment_id.to_string();
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::ROLE_REVOKED,
                actor_id: Some(caller.user_id as i32),
                entity_type: Some("user_scope_assignment"),
                entity_id: Some(target_str.as_str()),
                summary: "Role scope assignment revoked",
                ..Default::default()
            },
        )
        .await;
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
    let warnings = match compute_dependency_warnings(&state.db, &perm_set).await {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(
                role_id = role_id,
                error = ?err,
                "get_role: dependency warning computation failed; returning empty warning list"
            );
            Vec::new()
        }
    };

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
            "SELECT id FROM roles WHERE LOWER(name) = LOWER(?) AND deleted_at IS NULL AND status != 'retired'",
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
            "SELECT is_system FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
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
            "SELECT is_system FROM roles WHERE id = ? AND deleted_at IS NULL AND status != 'retired'",
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

    {
        let target_str = input.user_id.to_string();
        let detail = format!(
            r#"{{"role_id":{},"reason":"{}","assignment_id":{}}}"#,
            input.role_id, input.reason.replace('"', "\\\""), id
        );
        crate::audit::emit(
            &state.db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::RBAC_EMERGENCY_GRANT_CREATED,
                actor_id: Some(caller.user_id as i32),
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
//  ACCOUNT LOCKOUT MANAGEMENT
// ═══════════════════════════════════════════════════════════════════════════════

/// Unlock a locked user account. Resets failed attempts, lockout timer, and
/// consecutive lockout counter. Writes an admin change event for audit.
#[tauri::command]
pub async fn unlock_user_account(
    user_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let admin = require_session!(state);
    require_permission!(state, &admin, "adm.users", PermissionScope::Global);
    require_step_up!(state);

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
    state
        .db
        .execute(Statement::from_sql_and_values(
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

    // Write admin change event
    let _ = state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO admin_change_events \
             (action, actor_id, target_user_id, summary, step_up_used) \
             VALUES ('account_unlocked', ?, ?, 'Admin unlocked user account', 1)",
            [
                (admin.user_id as i64).into(),
                user_id.into(),
            ],
        ))
        .await;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
//  USER PRESENCE
// ═══════════════════════════════════════════════════════════════════════════════

/// Batch-fetch presence status for a list of user IDs.
/// Any authenticated user can call this — presence is not sensitive data.
#[tauri::command]
pub async fn get_user_presence(
    user_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<UserPresence>> {
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

    let mut values: Vec<sea_orm::Value> = user_ids
        .iter()
        .map(|id| sea_orm::Value::from(id.to_string()))
        .collect();
    values.push(now_str.into());

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            values,
        ))
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
                let parsed = chrono::DateTime::parse_from_rfc3339(ts)
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%SZ")
                        .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S"))
                        .map(|ndt| ndt.and_utc().fixed_offset()));
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

async fn list_missing_tenant_scope_users(
    db: &sea_orm::DatabaseConnection,
) -> AppResult<Vec<MissingTenantScopeUser>> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
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
                   AND usa.deleted_at IS NULL \
                   AND (usa.valid_from IS NULL OR usa.valid_from <= ?) \
                   AND (usa.valid_to   IS NULL OR usa.valid_to   >= ?) \
               ) \
             ORDER BY ua.username ASC",
            vec![now.clone().into(), now.into()],
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
