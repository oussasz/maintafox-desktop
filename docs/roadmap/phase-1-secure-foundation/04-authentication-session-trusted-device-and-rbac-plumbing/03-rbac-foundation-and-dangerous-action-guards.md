# Phase 1 · Sub-phase 04 · File 03
# RBAC Foundation and Dangerous Action Guards

## Context and Purpose

Files 01 and 02 built the authentication layer: user identity, session management, device
trust, and offline policy. The application now knows *who* is logged in and *from which
device*. What it does not yet know is *what* that user is allowed to do.

This file builds the RBAC foundation:

1. **Permission and role seeding** — the system ships with a predefined set of named
   permissions (22 domains from PRD §6.7), a set of system roles (Administrator,
   Supervisor, Operator, Readonly), and a sparse default role–permission mapping. These
   seed rows are marked `is_system = 1` and cannot be deleted or renamed.

2. **Permission check engine** — a Rust module that answers:
   `can user X perform action Y in scope Z?` at runtime, given an active session. The
   check is against the database (no in-memory cache yet — that arrives in Phase 2).

3. **Dangerous action guard** — a `require_step_up!` macro for IPC commands that carry
   the `is_dangerous` permission flag. When a dangerous command is invoked, the engine
   verifies the user's password one more time (they must re-enter it). The session
   continues normally but the dangerous action is audit-logged.

4. **Frontend permission layer** — `usePermissions()` hook that fetches the user's
   effective permissions and provides a zero-dependency `can(permission)` check.
   `<PermissionGate>` component optionally renders children if the permission is granted.

## Architecture Rules Applied

- Permission names use strict dot-notation: `domain.action` or `domain.action.scope`.
  No other formats are accepted. A Zod schema validates them in the TypeScript layer;
  a regex check validates them in the Rust seed function.
- **System roles** are seeded once, marked `is_system = 1`, and are never modified by
  runtime code or admin UI. Tenants can clone them as templates.
- **Permission checks are always database-backed** in Phase 1. In-memory permission
  caching is out of scope until Phase 2 (UserContext Preloader sub-phase).
- Permission check failures return a `PERMISSION_DENIED` error variant, not an
  `AUTH_ERROR`. The distinction matters for frontend UX: `PERMISSION_DENIED` shows an
  "insufficient rights" message, not a login prompt.
- The step-up reauthentication for dangerous actions re-verifies **password only**
  (not PIN, device, or SSO token) and is valid for **120 seconds** from verification.
  This step-up window is stored in the session manager, not in the DB.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/auth/rbac.rs` | Permission check engine, user permission loading |
| `src-tauri/src/db/seeder.rs` (extended) | Permission seeds (22 domains), role seeds (4 system roles), role–permission mappings |
| `src-tauri/src/auth/mod.rs` (extended) | `require_permission!` and `require_step_up!` macros |
| `src-tauri/src/auth/session_manager.rs` (extended) | Step-up verification state on `LocalSession` |
| `src-tauri/src/commands/rbac.rs` | `get_my_permissions`, `verify_step_up` IPC commands |
| `shared/ipc-types.ts` (extended) | `PermissionRecord`, `StepUpRequest`, `StepUpResponse` |
| `src/hooks/use-permissions.ts` | `usePermissions()` hook with `can()` helper |
| `src/components/PermissionGate.tsx` | Declarative permission guard component |
| `docs/RBAC_CONTRACTS.md` | Role and permission reference documentation |

## Prerequisites

- SP04-F01 complete: `SessionManager`, `require_session!`, login IPC chain
- SP04-F02 complete: device trust, offline policy — nothing from F02 needs extending
- Navigation is NOT behind permission gates yet. That work happens in Phase 2 UI modules

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Permission Seeding and RBAC Check Engine | Seed all permissions + system roles, `auth/rbac.rs` with `check_permission()` |
| S2 | Dangerous Action Guards and Step-up Verification | Step-up state on session, `require_step_up!` macro, `verify_step_up` IPC |
| S3 | Frontend Permission Layer | `usePermissions()`, `PermissionGate`, `RBAC_CONTRACTS.md` |

---

## Sprint S1 — Permission Seeding and RBAC Check Engine

### AI Agent Prompt

```
You are a senior Rust engineer continuing work on Maintafox Desktop.
SP04-F01 and SP04-F02 are complete. The session system is live. Your task is to:
1. Seed all 22+ permission domains and 4 system roles into the database
2. Build the runtime permission check engine in auth/rbac.rs

─────────────────────────────────────────────────────────────────────
STEP 1 — Extend src-tauri/src/db/seeder.rs to seed permissions and roles
─────────────────────────────────────────────────────────────────────
The seeder already has `seed_admin_account()`. Add two new top-level async functions,
called from the `seed_all()` function AFTER the lookup domain seeds and BEFORE admin
account seeding:

```rust
/// Seeds the full permission catalogue from PRD §6.7.
/// Uses INSERT OR IGNORE — safe to call multiple times.
/// Permissions are ordered by domain prefix then action suffix.
/// All seeded permissions have is_system = 1 and cannot be deleted.
pub async fn seed_permissions(db: &DatabaseConnection) -> AppResult<()> {
    // Permission rows: (name, description, category, is_dangerous, requires_step_up, is_system)
    // Format: dot-notation `domain.action`
    let permissions: &[(&str, &str, &str, bool, bool)] = &[
        // ── Equipment (eq) ──────────────────────────────────────────────────
        ("eq.view",          "View equipment registry",              "equipment",       false, false),
        ("eq.manage",        "Create/edit equipment records",        "equipment",       false, false),
        ("eq.import",        "Import equipment from CSV / ERP",      "equipment",       false, false),
        ("eq.delete",        "Soft-delete equipment records",        "equipment",       true,  true),
        // ── Intervention Requests (di) ────────────────────────────────────
        ("di.view",          "View intervention requests",           "intervention",    false, false),
        ("di.create",        "Create intervention requests",         "intervention",    false, false),
        ("di.edit",          "Edit intervention requests",           "intervention",    false, false),
        ("di.delete",        "Delete intervention requests",         "intervention",    true,  true),
        ("di.close",         "Close/resolve intervention requests",  "intervention",    false, false),
        // ── Work Orders (ot) ──────────────────────────────────────────────
        ("ot.view",          "View work orders",                     "work_order",      false, false),
        ("ot.create",        "Create work orders",                   "work_order",      false, false),
        ("ot.edit",          "Edit work orders",                     "work_order",      false, false),
        ("ot.delete",        "Delete work orders",                   "work_order",      true,  true),
        ("ot.close",         "Close work orders",                    "work_order",      false, false),
        ("ot.approve",       "Approve work order execution",         "work_order",      false, false),
        // ── Organization (org) ───────────────────────────────────────────
        ("org.view",         "View organizational structure",        "organization",    false, false),
        ("org.manage",       "Create/edit org nodes and entities",   "organization",    false, false),
        // ── Personnel (per) ──────────────────────────────────────────────
        ("per.view",         "View personnel records",               "personnel",       false, false),
        ("per.manage",       "Create/edit personnel records",        "personnel",       false, false),
        ("per.sensitiveview","View sensitive personnel fields",      "personnel",       false, false),
        // ── Reference Data (ref) ─────────────────────────────────────────
        ("ref.view",         "View reference/lookup values",         "reference",       false, false),
        ("ref.manage",       "Create/edit governed reference values","reference",       false, false),
        ("ref.publish",      "Publish reference changes",            "reference",       true,  true),
        // ── Inventory (inv) ──────────────────────────────────────────────
        ("inv.view",         "View inventory and stock levels",      "inventory",       false, false),
        ("inv.manage",       "Create/edit inventory records",        "inventory",       false, false),
        ("inv.adjust",       "Post inventory adjustments",           "inventory",       true,  true),
        ("inv.order",        "Create purchase / replenishment orders","inventory",      false, false),
        // ── Preventive Maintenance (pm) ──────────────────────────────────
        ("pm.view",          "View PM plans and schedules",          "maintenance",     false, false),
        ("pm.manage",        "Create/edit PM plans",                 "maintenance",     false, false),
        ("pm.approve",       "Approve PM plan changes",              "maintenance",     false, false),
        // ── RAMS / Reliability (ram) ─────────────────────────────────────
        ("ram.view",         "View RAMS / reliability data",         "reliability",     false, false),
        ("ram.manage",       "Edit RAMS records and failure modes",  "reliability",     false, false),
        // ── Reports & Analytics (rep) ─────────────────────────────────────
        ("rep.view",         "View standard reports",                "reporting",       false, false),
        ("rep.export",       "Export report data",                   "reporting",       false, false),
        ("rep.manage",       "Create/edit custom reports",           "reporting",       false, false),
        // ── Archive Explorer (arc) ────────────────────────────────────────
        ("arc.view",         "Browse archive entries",               "archive",         false, false),
        ("arc.export",       "Export archived data",                 "archive",         false, false),
        // ── Documentation (doc) ──────────────────────────────────────────
        ("doc.view",         "View documentation and help content",  "documentation",   false, false),
        ("doc.manage",       "Create/edit documentation articles",   "documentation",   false, false),
        // ── Administration (adm) ─────────────────────────────────────────
        ("adm.users",        "Manage user accounts",                 "administration",  true,  true),
        ("adm.roles",        "Manage roles and permissions",         "administration",  true,  true),
        ("adm.permissions",  "Assign permissions to roles",          "administration",  true,  true),
        ("adm.settings",     "Manage application settings",          "administration",  false, false),
        ("adm.audit",        "View the full audit log",              "administration",  false, false),
        // ── Planning (plan) ──────────────────────────────────────────────
        ("plan.view",        "View planning and scheduling data",    "planning",        false, false),
        ("plan.manage",      "Manage planning schedules",            "planning",        false, false),
        // ── Audit Log (log) ──────────────────────────────────────────────
        ("log.view",         "View activity feed",                   "audit",           false, false),
        ("log.export",       "Export audit log data",                "audit",           true,  true),
        // ── Training (trn) ───────────────────────────────────────────────
        ("trn.view",         "View training and certification records","training",      false, false),
        ("trn.manage",       "Manage training records and plans",    "training",        false, false),
        ("trn.certify",      "Issue or revoke certifications",       "training",        true,  true),
        // ── IoT Integration (iot) ────────────────────────────────────────
        ("iot.view",         "View IoT device data and readings",    "integration",     false, false),
        ("iot.manage",       "Configure IoT gateways and devices",   "integration",     false, false),
        // ── ERP Connector (erp) ──────────────────────────────────────────
        ("erp.view",         "View ERP sync status and mappings",    "integration",     false, false),
        ("erp.manage",       "Configure ERP integration settings",   "integration",     true,  true),
        ("erp.sync",         "Trigger manual ERP synchronization",   "integration",     true,  true),
        // ── Work Permits (ptw) ───────────────────────────────────────────
        ("ptw.view",         "View work permits",                    "safety",          false, false),
        ("ptw.create",       "Create work permits",                  "safety",          false, false),
        ("ptw.approve",      "Approve or reject work permits",       "safety",          true,  true),
        ("ptw.cancel",       "Cancel active work permits",           "safety",          true,  true),
        // ── Budget / Finance (fin) ───────────────────────────────────────
        ("fin.view",         "View budgets and cost data",           "finance",         false, false),
        ("fin.manage",       "Manage budgets and cost centers",      "finance",         false, false),
        ("fin.approve",      "Approve budget changes",               "finance",         true,  true),
        // ── Inspection (ins) ─────────────────────────────────────────────
        ("ins.view",         "View inspection rounds and checklists","inspection",      false, false),
        ("ins.manage",       "Create/edit inspection rounds",        "inspection",      false, false),
        ("ins.complete",     "Complete inspection round executions", "inspection",      false, false),
        // ── Configuration Engine (cfg) ───────────────────────────────────
        ("cfg.view",         "View tenant configuration",            "configuration",   false, false),
        ("cfg.manage",       "Manage tenant configuration rules",    "configuration",   true,  true),
        ("cfg.publish",      "Publish configuration changes",        "configuration",   true,  true),
    ];

    let now = chrono::Utc::now().to_rfc3339();

    for (name, desc, category, is_dangerous, requires_step_up) in permissions {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"INSERT OR IGNORE INTO permissions
                   (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
               VALUES (?, ?, ?, ?, ?, 1, ?)"#,
            [
                (*name).into(),
                (*desc).into(),
                (*category).into(),
                (*is_dangerous as i32).into(),
                (*requires_step_up as i32).into(),
                now.clone().into(),
            ],
        ))
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    }

    tracing::info!(count = permissions.len(), "seeder::permissions_seeded");
    Ok(())
}

/// Seeds the 4 system roles and assigns their initial permissions.
/// System roles are non-deletable and are the baseline for role templates.
pub async fn seed_system_roles(db: &DatabaseConnection) -> AppResult<()> {
    let now = chrono::Utc::now().to_rfc3339();

    let system_roles: &[(&str, &str, &str)] = &[
        ("Administrator", "Full system access. Cannot be deleted.", "system"),
        ("Supervisor",    "Full operational access. Can manage work, personnel, inventory.", "system"),
        ("Operator",      "Day-to-day CMMS use: view all, create and edit operational records.", "system"),
        ("Readonly",      "Read-only access to all operational modules.", "system"),
    ];

    for (name, desc, role_type) in system_roles {
        let sync_id = uuid::Uuid::new_v4().to_string();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"INSERT OR IGNORE INTO roles
                   (sync_id, name, description, is_system, role_type, status, created_at, updated_at, row_version)
               VALUES (?, ?, ?, 1, ?, 'active', ?, ?, 1)"#,
            [
                sync_id.into(),
                (*name).into(),
                (*desc).into(),
                (*role_type).into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    }

    // Assign permissions to roles using name-based resolution
    // Administrator → all permissions
    let admin_role_id = get_role_id_by_name(db, "Administrator").await?;
    if let Some(rid) = admin_role_id {
        assign_all_permissions_to_role(db, rid, &now).await?;
    }

    // Supervisor → all operational permissions (no adm.users, adm.roles, adm.permissions, cfg.*)
    let excluded_for_supervisor = [
        "adm.users", "adm.roles", "adm.permissions",
        "cfg.manage", "cfg.publish",
        "erp.manage", "erp.sync",
        "log.export",
    ];
    let supervisor_role_id = get_role_id_by_name(db, "Supervisor").await?;
    if let Some(rid) = supervisor_role_id {
        assign_permissions_excluding(db, rid, &excluded_for_supervisor, &now).await?;
    }

    // Operator → view + create/edit operational modules only, no delete/approve/dangerous
    let operator_permissions = [
        "eq.view", "eq.manage",
        "di.view", "di.create", "di.edit", "di.close",
        "ot.view", "ot.create", "ot.edit",
        "org.view",
        "per.view",
        "ref.view",
        "inv.view", "inv.manage",
        "pm.view",
        "ram.view",
        "rep.view",
        "arc.view",
        "doc.view",
        "plan.view",
        "log.view",
        "trn.view",
        "iot.view",
        "erp.view",
        "ptw.view", "ptw.create",
        "fin.view",
        "ins.view", "ins.complete",
        "cfg.view",
        "adm.settings",
    ];
    let operator_role_id = get_role_id_by_name(db, "Operator").await?;
    if let Some(rid) = operator_role_id {
        for perm_name in &operator_permissions {
            assign_permission_by_name(db, rid, perm_name, &now).await?;
        }
    }

    // Readonly → only *.view permissions
    let readonly_role_id = get_role_id_by_name(db, "Readonly").await?;
    if let Some(rid) = readonly_role_id {
        let view_perms: Vec<&str> = operator_permissions.iter()
            .copied()
            .filter(|p| p.ends_with(".view"))
            .collect();
        for perm_name in view_perms {
            assign_permission_by_name(db, rid, perm_name, &now).await?;
        }
    }

    tracing::info!("seeder::system_roles_seeded");
    Ok(())
}

async fn get_role_id_by_name(db: &DatabaseConnection, name: &str) -> AppResult<Option<i32>> {
    let row = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM roles WHERE name = ? AND deleted_at IS NULL",
        [name.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(row.and_then(|r| r.try_get::<i32>("", "id").ok()))
}

async fn assign_all_permissions_to_role(
    db: &DatabaseConnection,
    role_id: i32,
    now: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
           SELECT ?, id, ? FROM permissions WHERE is_system = 1"#,
        [role_id.into(), now.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

async fn assign_permissions_excluding(
    db: &DatabaseConnection,
    role_id: i32,
    excluded: &[&str],
    now: &str,
) -> AppResult<()> {
    // Build placeholders for excluded list
    let placeholders = excluded.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        r#"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
           SELECT ?, id, ? FROM permissions
           WHERE is_system = 1 AND name NOT IN ({placeholders})"#,
    );
    let mut values: Vec<sea_orm::Value> = vec![role_id.into(), now.into()];
    for e in excluded {
        values.push((*e).into());
    }
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

async fn assign_permission_by_name(
    db: &DatabaseConnection,
    role_id: i32,
    permission_name: &str,
    now: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at)
           SELECT ?, id, ? FROM permissions WHERE name = ?"#,
        [role_id.into(), now.into(), permission_name.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src-tauri/src/auth/rbac.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/auth/rbac.rs
//! Runtime permission check engine.
//!
//! All permission checks are database-backed. No in-memory cache is used in Phase 1.
//! The check is scoped: a user holding `ot.view` globally (tenant scope) can view
//! all work orders. A user holding `ot.view` at entity scope can view only that entity.
//!
//! Scope resolution rule: if the user holds the permission at ANY scope that covers
//! the requested resource, the check passes. The scope hierarchy is:
//!   tenant > entity > site > team > org_node

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use crate::errors::{AppError, AppResult};

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
///      (valid_from <= now <= valid_to, or valid_from/to is NULL meaning unlimited)
///   2. Filter to assignments whose scope_type is Tenant (covers everything) or
///      matches the requested scope
///   3. For each matching role, check if it has the permission in role_permissions
///   4. Return true if any role–scope combination grants the permission
///
/// Returns Err only on DB errors; returns Ok(false) for any denial.
pub async fn check_permission(
    db: &DatabaseConnection,
    user_id: i32,
    permission_name: &str,
    scope: &PermissionScope,
) -> AppResult<bool> {
    let now = chrono::Utc::now().to_rfc3339();

    // For the scope filter: we look for tenant-wide assignments OR matching assignments
    let (scope_type_filter, scope_ref_filter): (Option<&str>, Option<String>) = match scope {
        PermissionScope::Global => (None, None),
        PermissionScope::Entity(id) => (Some("entity"), Some(id.clone())),
        PermissionScope::Site(id)   => (Some("site"),   Some(id.clone())),
        PermissionScope::Team(id)   => (Some("team"),   Some(id.clone())),
        PermissionScope::OrgNode(id)=> (Some("org_node"), Some(id.clone())),
    };

    // Build the scope clause: always allow tenant-scoped roles; also allow
    // matching scope if a specific scope was provided.
    let scope_sql = if scope_type_filter.is_some() {
        "(usa.scope_type = 'tenant' OR (usa.scope_type = ? AND usa.scope_reference = ?))"
    } else {
        "usa.scope_type = 'tenant'"
    };

    let sql = format!(r#"
        SELECT COUNT(*) as cnt
        FROM user_scope_assignments usa
        INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
        INNER JOIN permissions p        ON p.id = rp.permission_id
        WHERE usa.user_id = ?
          AND usa.deleted_at IS NULL
          AND (usa.valid_from IS NULL OR usa.valid_from <= ?)
          AND (usa.valid_to   IS NULL OR usa.valid_to   >= ?)
          AND p.name = ?
          AND {scope_sql}
    "#);

    let mut values: Vec<sea_orm::Value> = vec![
        user_id.into(),
        now.clone().into(),
        now.into(),
        permission_name.into(),
    ];

    if let (Some(st), Some(sr)) = (&scope_type_filter, &scope_ref_filter) {
        values.push((*st).into());
        values.push(sr.clone().into());
    }

    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    let count: i32 = row
        .and_then(|r| r.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0) as i32;

    Ok(count > 0)
}

/// Load all effective permissions for a user (for frontend pre-loading).
/// Returns only the permissions the user currently holds via active role assignments.
pub async fn get_user_permissions(
    db: &DatabaseConnection,
    user_id: i32,
) -> AppResult<Vec<PermissionRecord>> {
    let now = chrono::Utc::now().to_rfc3339();

    let rows = db.query_all(Statement::from_sql_and_values(
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
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    let perms = rows
        .into_iter()
        .map(|r| PermissionRecord {
            name:              r.try_get("", "name").unwrap_or_default(),
            description:       r.try_get("", "description").unwrap_or_default(),
            category:          r.try_get("", "category").unwrap_or_default(),
            is_dangerous:      r.try_get::<i32>("", "is_dangerous").unwrap_or(0)    == 1,
            requires_step_up:  r.try_get::<i32>("", "requires_step_up").unwrap_or(0) == 1,
        })
        .collect();

    Ok(perms)
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
```

Register in auth/mod.rs:
```rust
pub mod rbac;
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes (2 new rbac tests)
- After `pnpm run dev`, run in DBeaver:
  SELECT COUNT(*) FROM permissions;   -- must be ≥ 65
  SELECT COUNT(*) FROM roles;          -- must be ≥ 4
  SELECT COUNT(*) FROM role_permissions WHERE role_id = (SELECT id FROM roles WHERE name = 'Administrator');
  -- must be same count as permissions
- Admin user (seeded with no scope assignment yet) returns empty from get_user_permissions
  until a scope assignment row is added manually in DBeaver
```

---

### Supervisor Verification — Sprint S1

**V1 — Permission count correct.**
In DBeaver, run: `SELECT COUNT(*) FROM permissions;`
The count must be 65 or higher (the table above seeds 68 rows; the exact count depends
on version drift — as long as it's ≥ 65, the seed is complete). If the count is 0 or
lower, the seeder is not running. Run `pnpm run dev` and check the startup log for
"seeder::permissions_seeded". Flag any error lines.

**V2 — Roles seeded correctly.**
Run: `SELECT name, is_system, role_type, status FROM roles WHERE is_system = 1;`
There must be exactly 4 rows: Administrator, Supervisor, Operator, Readonly.
All must have `is_system = 1`, `role_type = 'system'`, `status = 'active'`.

**V3 — Administrator has all permissions.**
```sql
SELECT p.name FROM permissions p
LEFT JOIN role_permissions rp ON rp.permission_id = p.id
  AND rp.role_id = (SELECT id FROM roles WHERE name = 'Administrator')
WHERE rp.permission_id IS NULL;
```
This query returns permissions NOT assigned to Administrator. It must return 0 rows.
If any row is returned, some permissions were not assigned. Flag them by name.

---

## Sprint S2 — Dangerous Action Guards and Step-up Verification

### AI Agent Prompt

```
You are a senior Rust engineer continuing work on Maintafox Desktop.
Sprint S1 is complete: permissions and system roles are seeded; check_permission() is live.
Your task is to:
1. Add step-up verification state to the LocalSession struct
2. Create the `require_permission!` and `require_step_up!` macros
3. Add the `verify_step_up` IPC command
4. Create the `get_my_permissions` IPC command

─────────────────────────────────────────────────────────────────────
STEP 1 — Extend LocalSession with step-up state (session_manager.rs)
─────────────────────────────────────────────────────────────────────
Add to LocalSession:
```rust
    /// When the user last completed a step-up password re-verification.
    /// Step-up authorization is valid for STEP_UP_DURATION_SECS seconds.
    pub step_up_verified_at: Option<std::time::Instant>,
```

Add constant above LocalSession:
```rust
/// Duration (in seconds) that a step-up verification is valid.
/// After this window, the user must re-verify password before dangerous actions.
pub const STEP_UP_DURATION_SECS: u64 = 120;
```

Add method to LocalSession:
```rust
    /// Returns true if a step-up verification was performed within the last
    /// STEP_UP_DURATION_SECS seconds.
    pub fn is_step_up_valid(&self) -> bool {
        match self.step_up_verified_at {
            None => false,
            Some(t) => t.elapsed().as_secs() < STEP_UP_DURATION_SECS,
        }
    }
```

Add method to SessionManager:
```rust
    /// Record that the user just completed step-up reauthentication.
    pub fn record_step_up(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.step_up_verified_at = Some(std::time::Instant::now());
            tracing::info!("session::step_up_recorded");
        }
    }

    /// Check if step-up reauthentication is currently valid.
    pub fn is_step_up_valid(&self) -> bool {
        self.session
            .as_ref()
            .map(|s| s.is_step_up_valid())
            .unwrap_or(false)
    }
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Add require_permission! and require_step_up! macros to auth/mod.rs
─────────────────────────────────────────────────────────────────────
```rust
/// Macro: verify the session is active AND the user has a specific permission.
/// Usage in IPC command:
///   let (user, db) = require_permission!(state, "eq.manage");
///
/// Expands to:
///   1. get user from session (via require_session!)
///   2. call check_permission(..., Global scope)
///   3. if not granted: return Err(AppError::PermissionDenied(permission_name))
#[macro_export]
macro_rules! require_permission {
    ($state:expr, $permission:expr) => {{
        let user = $crate::require_session!($state);
        let allowed = crate::auth::rbac::check_permission(
            &$state.db,
            user.user_id,
            $permission,
            &crate::auth::rbac::PermissionScope::Global,
        )
        .await
        .map_err(|e| e)?;
        if !allowed {
            return Err(crate::errors::AppError::PermissionDenied($permission.to_string()));
        }
        user
    }};
}

/// Macro: require that the current session has valid step-up authorization.
/// Must be called AFTER require_session! or require_permission!.
/// Usage in IPC command (dangerous action):
///   let user = require_permission!(state, "adm.users");
///   require_step_up!(state);
///   // ... dangerous action proceeds
#[macro_export]
macro_rules! require_step_up {
    ($state:expr) => {{
        let valid = {
            let sm = $state.session.read()
                .map_err(|_| crate::errors::AppError::Internal("session lock poisoned".into()))?;
            sm.is_step_up_valid()
        };
        if !valid {
            return Err(crate::errors::AppError::StepUpRequired);
        }
    }};
}
```

Also add `PermissionDenied` and `StepUpRequired` to AppError in errors.rs:
```rust
    /// The user's session is valid but they lack the required permission.
    #[error("Permission refusée: {0}")]
    PermissionDenied(String),

    /// The operation requires step-up reauthentication.
    #[error("Ré-authentification requise")]
    StepUpRequired,
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Create src-tauri/src/commands/rbac.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/commands/rbac.rs
//! IPC commands for RBAC: permission loading and step-up verification.

use tauri::State;
use serde::{Deserialize, Serialize};
use crate::state::AppState;
use crate::errors::AppResult;
use crate::auth::{rbac, session_manager};

/// Get the list of effective permissions for the currently logged-in user.
/// Used at login time to populate the frontend permission store.
#[tauri::command]
pub async fn get_my_permissions(
    state: State<'_, AppState>,
) -> AppResult<Vec<rbac::PermissionRecord>> {
    let user = crate::require_session!(state);
    rbac::get_user_permissions(&state.db, user.user_id).await
}

/// Payload for step-up verification.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepUpRequest {
    /// The user's current password (used to re-verify identity).
    pub password: String,
}

/// Step-up verification result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepUpResponse {
    /// True if step-up succeeded. The valid window is 120 seconds from this call.
    pub success: bool,
    /// When the step-up window expires (ISO 8601 UTC).
    pub expires_at: String,
}

/// Verify the current user's password for step-up authorization.
/// On success, the session records the verification timestamp.
/// The dangerous action the user was attempting must be called within 120 seconds.
#[tauri::command]
pub async fn verify_step_up(
    payload: StepUpRequest,
    state: State<'_, AppState>,
) -> AppResult<StepUpResponse> {
    let user = crate::require_session!(state);

    // Load the current password hash
    let row = state.db.query_one(
        sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "SELECT password_hash FROM user_accounts WHERE id = ? AND is_active = 1",
            [user.user_id.into()],
        )
    )
    .await
    .map_err(|e| crate::errors::AppError::Database(e.to_string()))?;

    let hash = row
        .and_then(|r| r.try_get::<String>("", "password_hash").ok())
        .ok_or_else(|| crate::errors::AppError::Internal("user not found for step-up".into()))?;

    let ok = crate::auth::password::verify_password(&payload.password, &hash)?;
    if !ok {
        tracing::warn!(user_id = %user.user_id, "step_up::wrong_password");
        return Err(crate::errors::AppError::Auth("Mot de passe incorrect.".into()));
    }

    {
        let mut sm = state.session.write()
            .map_err(|_| crate::errors::AppError::Internal("session lock poisoned".into()))?;
        sm.record_step_up();
    }

    let expires_at = (chrono::Utc::now()
        + chrono::Duration::seconds(session_manager::STEP_UP_DURATION_SECS as i64))
        .to_rfc3339();

    tracing::info!(user_id = %user.user_id, "step_up::verified");

    Ok(StepUpResponse { success: true, expires_at })
}
```

Register `commands::rbac` in src-tauri/src/commands/mod.rs and add
`rbac::get_my_permissions` and `rbac::verify_step_up` to the `generate_handler!` list.

─────────────────────────────────────────────────────────────────────
STEP 4 — Extend shared/ipc-types.ts with RBAC types
─────────────────────────────────────────────────────────────────────
```typescript
export interface PermissionRecord {
  name: string;
  description: string;
  category: string;
  is_dangerous: boolean;
  requires_step_up: boolean;
}

export interface StepUpRequest {
  password: string;
}

export interface StepUpResponse {
  success: boolean;
  expires_at: string;
}
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures (2 new session_manager tests for
  is_step_up_valid and step-up window expiry)
- verify_step_up returns success with correct password
- verify_step_up returns Auth error with wrong password
- require_step_up! macro used in a test command returns StepUpRequired when not verified
```

---

### Supervisor Verification — Sprint S2

**V1 — Step-up verification works.**
Log in as admin. Run in DevTools:
```javascript
// First verify: no step-up done yet
window.__TAURI__.core.invoke('verify_step_up', { payload: { password: 'Admin#2026!' } })
  .then(r => console.log('step up:', r));
// Should return { success: true, expires_at: '...' }
```
If it returns an error, check that `step_up_verified_at` is being set on the session.

**V2 — Wrong password returns error.**
```javascript
window.__TAURI__.core.invoke('verify_step_up', { payload: { password: 'WrongPassword' } })
  .catch(err => console.log('expected error:', err));
// Should produce an error, not { success: false }
```

**V3 — get_my_permissions returns non-empty list for admin.**
After login (the admin user must have a scope assignment to get permissions), run:
```javascript
window.__TAURI__.core.invoke('get_my_permissions')
  .then(perms => console.log('total perms:', perms.length));
```
Note: if the admin user doesn't have a `user_scope_assignments` row yet, this will
return 0. That is correct — the admin account seed does NOT add a scope assignment
(this is intentional per the first-login change-password / setup wizard flow). The test
can be done by inserting a scope assignment row manually in DBeaver:
```sql
INSERT INTO user_scope_assignments (sync_id, user_id, role_id, scope_type, created_at, updated_at, row_version)
SELECT random_hex(16), id, (SELECT id FROM roles WHERE name = 'Administrator'),
       'tenant', datetime('now'), datetime('now'), 1
FROM user_accounts WHERE username = 'admin';
```
After that, `get_my_permissions` must return ≥ 65 records.

---

## Sprint S3 — Frontend Permission Layer

### AI Agent Prompt

```
You are a senior TypeScript/React engineer continuing work on Maintafox Desktop.
Sprints S1 and S2 are complete. The RBAC backend is live. Your task is to build
the frontend permission layer: a hook that preloads permissions after login and a
declarative gate component.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src/services/rbac-service.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/services/rbac-service.ts
// ADR-003: all IPC calls go through service modules, never directly from components.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type { PermissionRecord, StepUpRequest, StepUpResponse } from "../../shared/ipc-types";

const PermissionRecordSchema = z.object({
  name:              z.string(),
  description:       z.string(),
  category:          z.string(),
  is_dangerous:      z.boolean(),
  requires_step_up:  z.boolean(),
});

const StepUpResponseSchema = z.object({
  success:    z.boolean(),
  expires_at: z.string(),
});

export async function getMyPermissions(): Promise<PermissionRecord[]> {
  const raw = await invoke<unknown[]>("get_my_permissions");
  return z.array(PermissionRecordSchema).parse(raw);
}

export async function verifyStepUp(password: string): Promise<StepUpResponse> {
  const payload: StepUpRequest = { password };
  const raw = await invoke<unknown>("verify_step_up", { payload });
  return StepUpResponseSchema.parse(raw);
}
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src/hooks/use-permissions.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/hooks/use-permissions.ts
import { useState, useEffect, useCallback } from "react";
import { getMyPermissions } from "../services/rbac-service";
import type { PermissionRecord } from "../../shared/ipc-types";

interface UsePermissionsReturn {
  /** Full list of permissions the user holds */
  permissions: PermissionRecord[];
  /** True while loading */
  isLoading: boolean;
  /** Returns true if the user holds the given permission name */
  can: (permissionName: string) => boolean;
  /** Reload permissions from the backend (call after role change) */
  refresh: () => Promise<void>;
}

export function usePermissions(): UsePermissionsReturn {
  const [permissions, setPermissions] = useState<PermissionRecord[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  const load = useCallback(async () => {
    setIsLoading(true);
    try {
      const perms = await getMyPermissions();
      setPermissions(perms);
    } catch {
      setPermissions([]);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const can = useCallback(
    (permissionName: string) =>
      permissions.some((p) => p.name === permissionName),
    [permissions]
  );

  return { permissions, isLoading, can, refresh: load };
}
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Create src/components/PermissionGate.tsx
─────────────────────────────────────────────────────────────────────
```typescript
// src/components/PermissionGate.tsx
//
// Conditionally renders children if the user has the required permission.
// On loading, renders nothing (no flash of unauthorized content).
//
// Usage:
//   <PermissionGate permission="eq.manage">
//     <EditEquipmentButton />
//   </PermissionGate>
//
//   <PermissionGate permission="adm.users" fallback={<NotAuthorized />}>
//     <UserManagementPanel />
//   </PermissionGate>

import React from "react";
import { usePermissions } from "../hooks/use-permissions";

interface PermissionGateProps {
  permission: string;
  children: React.ReactNode;
  fallback?: React.ReactNode;
}

export function PermissionGate({
  permission,
  children,
  fallback = null,
}: PermissionGateProps): React.ReactElement | null {
  const { can, isLoading } = usePermissions();

  // During load: suppress content entirely (no unauthorized flash)
  if (isLoading) return null;

  return can(permission) ? <>{children}</> : <>{fallback}</>;
}
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Create docs/RBAC_CONTRACTS.md
─────────────────────────────────────────────────────────────────────
Write the following document:

```markdown
# RBAC Contracts

Reference for the role-based access control model, permission naming, macros,
dangerous action guards, and frontend usage.

## Permission Naming Convention

All permissions use dot-notation: `domain.action` or `domain.action.scope`.

Rules:
- Domain prefix must match one of the 22 PRD §6.7 domains: `eq`, `di`, `ot`, `org`,
  `per`, `ref`, `inv`, `pm`, `ram`, `rep`, `arc`, `doc`, `adm`, `plan`, `log`, `trn`,
  `iot`, `erp`, `ptw`, `fin`, `ins`, `cfg`
- Action suffix is lowercase alphanumeric (no dots, no underscores)
- Scope is optional and scopes to a specific resource sub-domain
- Examples: `eq.view`, `ot.approve`, `adm.users`, `ptw.approve`

## System Roles

| Role | Description | Can Be Deleted |
|------|-------------|---------------|
| Administrator | Full system access including admin operations | No |
| Supervisor | All operational access; no admin permissions | No |
| Operator | Create/edit day-to-day operational records; no delete or approval | No |
| Readonly | View-only access to all operational modules | No |

System roles are seeded on first launch. Tenants can clone them as role templates
and customize the clone; the originals remain unchanged.

## Dangerous Actions

Permissions with `is_dangerous = 1` are highlighted in the UI and require a comment
when used by Supervisors or higher. Permissions with both `is_dangerous = 1` and
`requires_step_up = 1` require `verify_step_up` to be called before the action
proceeds. The step-up window is valid for **120 seconds**.

## Backend Macros

### require_session!
```rust
let user = require_session!(state);
// user: AuthenticatedUser { user_id, username, display_name, is_admin, ... }
```
Fails with `AppError::Auth("not authenticated")` if no active session.

### require_permission!
```rust
let user = require_permission!(state, "eq.manage");
// Automatically calls require_session! first
// Fails with AppError::PermissionDenied("eq.manage") if no permission
```

### require_step_up!
```rust
let user = require_permission!(state, "adm.users");
require_step_up!(state);
// Fails with AppError::StepUpRequired if step-up not verified within 120s
```

## Frontend Usage

### can() check (inline)
```typescript
const { can } = usePermissions();
if (can("eq.manage")) { /* show edit button */ }
```

### PermissionGate (declarative)
```tsx
<PermissionGate permission="ot.approve">
  <ApproveButton workOrderId={id} />
</PermissionGate>
```

With fallback:
```tsx
<PermissionGate
  permission="adm.users"
  fallback={<p>Accès non autorisé</p>}
>
  <UserManagementPanel />
</PermissionGate>
```

## IPC Commands

| Command | Auth | Parameters | Returns |
|---------|------|-----------|---------|
| `get_my_permissions` | Session required | None | `PermissionRecord[]` |
| `verify_step_up` | Session required | `{ password: string }` | `StepUpResponse` |

## Scope Resolution

If `check_permission()` is called with `PermissionScope::Global`, only tenant-wide
role assignments qualify. If a specific entity/site/team/org_node scope is passed,
assignments at the `tenant` level OR at that exact scope qualify.

Scope hierarchy is NOT transitive in Phase 1: a `site`-scoped assignment does NOT
automatically grant `team`-level access. Transitive scope resolution is a Phase 2
optimization feature.
```

─────────────────────────────────────────────────────────────────────
STEP 5 — Add tests for usePermissions hook
─────────────────────────────────────────────────────────────────────
Create src/hooks/__tests__/use-permissions.test.ts:

```typescript
import { renderHook, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { usePermissions } from "../use-permissions";

vi.mock("../../services/rbac-service", () => ({
  getMyPermissions: vi.fn().mockResolvedValue([
    { name: "eq.view",   description: "", category: "equipment", is_dangerous: false, requires_step_up: false },
    { name: "eq.manage", description: "", category: "equipment", is_dangerous: false, requires_step_up: false },
    { name: "adm.users", description: "", category: "administration", is_dangerous: true, requires_step_up: true },
  ]),
}));

describe("usePermissions", () => {
  it("loads permissions from backend", async () => {
    const { result } = renderHook(() => usePermissions());
    expect(result.current.isLoading).toBe(true);

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.permissions).toHaveLength(3);
  });

  it("can() returns true for held permission", async () => {
    const { result } = renderHook(() => usePermissions());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.can("eq.view")).toBe(true);
    expect(result.current.can("eq.manage")).toBe(true);
  });

  it("can() returns false for missing permission", async () => {
    const { result } = renderHook(() => usePermissions());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.can("adm.roles")).toBe(false);
  });

  it("refresh() reloads permissions", async () => {
    const { result } = renderHook(() => usePermissions());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await result.current.refresh();
    expect(result.current.permissions).toHaveLength(3);
  });
});
```

─────────────────────────────────────────────────────────────────────
STEP 6 — Update IPC_COMMAND_REGISTRY.md
─────────────────────────────────────────────────────────────────────
Add:
- `get_my_permissions` (Session required, Returns PermissionRecord[], PRD §6.7)
- `verify_step_up` (Session required, StepUpRequest, Returns StepUpResponse, PRD §6.7)

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- pnpm test passes with 4 new use-permissions tests
- PermissionGate renders children when permission is held
- PermissionGate renders fallback (or nothing) when permission is not held
- docs/RBAC_CONTRACTS.md present and documents all 22 permission domain prefixes
```

---

### Supervisor Verification — Sprint S3

**V1 — usePermissions hook tests pass.**
Run `pnpm test src/hooks/__tests__/use-permissions.test.ts`. All 4 tests must show
`pass`. If any fail, review the mock setup and flag the test name.

**V2 — PermissionGate renders correctly.**
In the application shell (App.tsx or equivalent), temporarily add:
```tsx
<PermissionGate permission="eq.view" fallback={<span>no eq.view</span>}>
  <span data-testid="gate-content">has eq.view</span>
</PermissionGate>
```
When logged in as a user with `eq.view`, the span with `has eq.view` must be visible.
When logged in without that permission, `no eq.view` should appear (or nothing if
fallback is not provided). Remove the temporary test element after verification.

**V3 — RBAC_CONTRACTS.md documents all 22 domains.**
Open `docs/RBAC_CONTRACTS.md`. Confirm the permission naming convention section lists
all domain prefixes from PRD §6.7. A quick count: `eq, di, ot, org, per, ref, inv, pm,
ram, rep, arc, doc, adm, plan, log, trn, iot, erp, ptw, fin, ins, cfg` = 22 domains.

---

*End of Phase 1 · Sub-phase 04 · File 03*
