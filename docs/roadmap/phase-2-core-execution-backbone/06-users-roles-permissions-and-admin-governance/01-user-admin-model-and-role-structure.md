# Phase 2 - Sub-phase 06 - File 01
# User Admin Model and Role Structure

## Context and Purpose

Sub-phase 06 delivers the RBAC foundation that governs every other module in Maintafox. Phase 1
already established the authentication, device trust, and step-up reauthentication layer (§6.1
migrations 001-008). What Phase 1 did NOT deliver was the full scoped role-assignment model:
multi-scope assignments, dangerous-permission handling, delegated administration, access
simulation, and full audit of all identity governance events.

This file establishes the core data model for users, roles, and permission structures. Files 02
through 04 build the permission domain enforcement, admin flows, and security audit layer.

The design principle for this module is: **authorization is a first-class operational constraint,
not a configuration page.** Scope restrictions on roles directly affect what maintenance
supervisors can plan, close, and report. Dangerous-action designation and step-up requirements
feed from the permission table into every other execution command in the application (SP04, SP05,
SP09, SP16, SP23, SP24, etc.).

---

## PRD Alignment Checklist

This file addresses PRD §6.7 requirements for:

- [x] `roles` with system/custom distinction, lifecycle states, non-deletable system roles
- [x] `permissions` (dot-notation, is_dangerous, requires_step_up)
- [x] `role_permissions` linker
- [x] `user_accounts` (identity_mode: local/sso/hybrid, personnel binding, force_password_change)
- [x] `user_scope_assignments` (scope_type: tenant/entity/site/team/org_node, effective dates)
- [x] `permission_dependencies` (hard / warn dependency types)
- [x] `role_templates` with module_set pre-packaging
- [x] `delegated_admin_policies` with allowed_domains_json and step-up requirements
- [x] System roles seeded, non-deletable, usable without customization
- [x] SSO and hybrid identity modes alongside local accounts

---

## Architecture Rules Applied

- **Phase 1 RBAC baseline retained.** Migrations 001-008 already created `user_accounts`,
  `roles`, `permissions`, `role_permissions`, and seeded a basic admin user. Migration 025
  (this file) augments rather than replaces those tables: it adds the columns and tables that
  implementing full scoped authorization requires while maintaining the already-functional
  baseline.
- **Scope-first authorization.** A user holding `ot.edit` scoped to entity A cannot see or
  edit WOs in entity B. The `require_permission!` macro must accept an `org_node_id` filter.
  Global scope (`scope_type = 'tenant'`) grants access everywhere.
- **Dangerous permissions require step-up at the record level.** The `is_dangerous` and
  `requires_step_up` flags on `permissions` rows are the programmatic trigger; every command
  that touches those permissions calls `require_step_up!`. These flags cannot be changed by
  tenant admins — they are write-protected system data.
- **`user_scope_assignments` is the authority source.** The old single-role-per-user pattern
  from Phase 1 seed data is retired. RBAC evaluation traverses `user_scope_assignments` rows
  filtered by valid_from ≤ today ≤ valid_to (or valid_to IS NULL) and scope_type / scope_reference.
- **Role templates are starting points, not enforcement mechanisms.** Applying a template
  copies the permission set into a new custom role — it does not create a permanent link.
  System role_templates cannot be modified; custom templates can.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260501_000025_rbac_scope_model.rs` | Augmented RBAC tables + 5 system roles seeded + `user_scope_assignments` |
| `src-tauri/src/rbac/model.rs` | Core domain types: Role, Permission, UserAccount, UserScopeAssignment, PermissionDependency, RoleTemplate, DelegatedAdminPolicy |
| `src-tauri/src/rbac/resolver.rs` | `effective_permissions(user_id, scope)` — resolves permissions through scope assignments, dependency validation, and date filtering |
| `src-tauri/src/rbac/macros.rs` | Updated `require_permission!` accepting optional entity scope; `require_step_up!` macro |
| `src-tauri/src/commands/admin_users.rs` | IPC commands: `list_users`, `get_user`, `create_user`, `update_user`, `deactivate_user`, `list_roles`, `get_role`, `create_role`, `update_role`, `delete_role` |
| `src/services/rbac-service.ts` | Frontend wrappers for all admin_users commands |
| `src/components/admin/UserListPanel.tsx` | User management UI |
| `src/components/admin/RoleEditorPanel.tsx` | Role creation and permission assignment UI |

---

## Prerequisites

- Phase 1 migrations 001-008 complete (user_accounts, roles, permissions, role_permissions seeded)
- `org_units` / `org_nodes` from SP01 (migration 009) for scope FK targets
- `personnel` from SP06 Personnel sub-system (nullable FK; Personnel is SP06-adjacent)

---

## Migration 025 — RBAC Scope Model Augmentation

### AI Agent Prompt

```text
You are a senior Rust engineer implementing SQLite migrations for Maintafox.

STEP 1 - CREATE src-tauri/migrations/m20260501_000025_rbac_scope_model.rs

AUGMENTATION COLUMNS (only add if they do not already exist):

ALTER TABLE roles ADD COLUMN IF NOT EXISTS role_type TEXT NOT NULL DEFAULT 'custom';
-- system/custom
ALTER TABLE roles ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active';
-- draft/active/retired
ALTER TABLE roles ADD COLUMN IF NOT EXISTS is_system INTEGER NOT NULL DEFAULT 0;
-- 1 = cannot be deleted or have permissions removed without superadmin

ALTER TABLE permissions ADD COLUMN IF NOT EXISTS is_dangerous INTEGER NOT NULL DEFAULT 0;
ALTER TABLE permissions ADD COLUMN IF NOT EXISTS requires_step_up INTEGER NOT NULL DEFAULT 0;
ALTER TABLE permissions ADD COLUMN IF NOT EXISTS category TEXT NULL;

--- Note: ALTER TABLE ... ADD COLUMN IF NOT EXISTS is NOT supported by all SQLite versions.
--- Instead, use a CREATE TABLE ... SELECT migration pattern to upgrade with safety:
--- Check if column exists before trying to add.

NEW TABLE: user_scope_assignments
CREATE TABLE IF NOT EXISTS user_scope_assignments (
  id                    INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id               INTEGER NOT NULL REFERENCES user_accounts(id) ON DELETE CASCADE,
  role_id               INTEGER NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  scope_type            TEXT NOT NULL DEFAULT 'tenant',
    -- tenant / entity / site / team / org_node
  scope_reference       TEXT NULL,
    -- org_node_id as text, or NULL for tenant-wide
  valid_from            TEXT NULL,
    -- ISO-8601 date or NULL (no start restriction)
  valid_to              TEXT NULL,
    -- ISO-8601 date or NULL (no expiry)
  granted_by_id         INTEGER NULL REFERENCES user_accounts(id),
  granted_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  is_emergency          INTEGER NOT NULL DEFAULT 0,
  emergency_reason      TEXT NULL,
  emergency_expires_at  TEXT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uidx_usa_user_role_scope
  ON user_scope_assignments(user_id, role_id, scope_type, COALESCE(scope_reference,''));
CREATE INDEX IF NOT EXISTS idx_usa_user  ON user_scope_assignments(user_id);
CREATE INDEX IF NOT EXISTS idx_usa_role  ON user_scope_assignments(role_id);
CREATE INDEX IF NOT EXISTS idx_usa_scope ON user_scope_assignments(scope_type, scope_reference);

NEW TABLE: permission_dependencies
CREATE TABLE IF NOT EXISTS permission_dependencies (
  id                        INTEGER PRIMARY KEY AUTOINCREMENT,
  permission_name           TEXT NOT NULL,
  required_permission_name  TEXT NOT NULL,
  dependency_type           TEXT NOT NULL DEFAULT 'warn'
    -- hard / warn
);

CREATE UNIQUE INDEX IF NOT EXISTS uidx_pd_pair
  ON permission_dependencies(permission_name, required_permission_name);

NEW TABLE: role_templates
CREATE TABLE IF NOT EXISTS role_templates (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  name            TEXT NOT NULL,
  description     TEXT NULL,
  module_set_json TEXT NOT NULL DEFAULT '[]',
  is_system       INTEGER NOT NULL DEFAULT 0
);

NEW TABLE: delegated_admin_policies
CREATE TABLE IF NOT EXISTS delegated_admin_policies (
  id                          INTEGER PRIMARY KEY AUTOINCREMENT,
  admin_role_id               INTEGER NOT NULL REFERENCES roles(id),
  managed_scope_type          TEXT NOT NULL,
  managed_scope_reference     TEXT NULL,
  allowed_domains_json        TEXT NOT NULL DEFAULT '[]',
  requires_step_up_for_publish INTEGER NOT NULL DEFAULT 1
);

--- SEED: 5 system roles (INSERT OR IGNORE)

INSERT OR IGNORE INTO roles (name, description, role_type, status, is_system) VALUES
  ('Superadmin',        'Full system access, all permissions, cannot be restricted',         'system', 'active', 1),
  ('Maintenance Supervisor', 'Manages WOs, DIs, personnel assignments, and planning',        'system', 'active', 1),
  ('Maintenance Technician', 'Executes WOs, submits DIs, records labor and parts',           'system', 'active', 1),
  ('Planner/Scheduler', 'Plans and schedules WOs and PM occurrences',                        'system', 'active', 1),
  ('Read Only Observer', 'Read-only access to all operational modules, no write permissions','system', 'active', 1);

--- SEED: role templates matching system roles (INSERT OR IGNORE)

INSERT OR IGNORE INTO role_templates (name, description, module_set_json, is_system) VALUES
  ('Supervisor Template',   'Pre-packaged permissions for a site maintenance supervisor',   '["ot","di","pm","per","eq","inv"]', 1),
  ('Technician Template',   'Pre-packaged permissions for a field technician',             '["ot","di","eq"]', 1),
  ('Planner Template',      'Pre-packaged permissions for a PM planner/scheduler',         '["ot","di","pm","plan","inv"]', 1),
  ('Observer Template',     'Read-only access to main operational modules',               '["ot.view","di.view","pm.view","eq.view"]', 1);

--- SEED: permission dependencies (INSERT OR IGNORE)
--- Pairs where granting domain.action without domain.view makes no sense

INSERT OR IGNORE INTO permission_dependencies (permission_name, required_permission_name, dependency_type)
VALUES
  ('ot.create',    'ot.view',     'hard'),
  ('ot.edit',      'ot.view',     'hard'),
  ('ot.approve',   'ot.view',     'hard'),
  ('ot.close',     'ot.edit',     'hard'),
  ('ot.reopen',    'ot.close',    'warn'),
  ('di.submit',    'di.view',     'hard'),
  ('di.review',    'di.view',     'hard'),
  ('di.approve',   'di.review',   'hard'),
  ('di.convert',   'di.approve',  'hard'),
  ('eq.edit',      'eq.view',     'hard'),
  ('eq.delete',    'eq.edit',     'warn'),
  ('pm.edit',      'pm.view',     'hard'),
  ('pm.delete',    'pm.edit',     'warn'),
  ('inv.manage',   'inv.view',    'hard'),
  ('inv.procure',  'inv.view',    'hard'),
  ('inv.count',    'inv.view',    'hard'),
  ('adm.roles',    'adm.users',   'warn'),
  ('adm.permissions', 'adm.roles','hard');

ACCEPTANCE CRITERIA
- Migration applies on fresh DB and on existing DB (augmented columns handled safely)
- 5 system role rows present after migration
- 4 role_template rows present after migration
- user_scope_assignments unique index prevents duplicate role+scope assignments
- permission_dependencies seed: 19 pairs present
```

### Supervisor Verification - Migration 025

**V1 - Column presence.**
`PRAGMA table_info(roles)` shows role_type, status, is_system columns.

**V2 - System roles.**
`SELECT COUNT(*) FROM roles WHERE is_system = 1` returns 5.

**V3 - Dependencies.**
`SELECT COUNT(*) FROM permission_dependencies` returns ≥ 19.

**V4 - Unique index enforcement.**
Attempt duplicate INSERT into user_scope_assignments; must raise UNIQUE constraint error.

---

## Core RBAC Domain Types

### AI Agent Prompt

```text
You are a senior Rust engineer. Write the rbac module types and live permission resolver.

STEP 1 - CREATE src-tauri/src/rbac/mod.rs
  pub mod model;
  pub mod resolver;
  pub mod macros;   // re-export from existing or new file

STEP 2 - CREATE src-tauri/src/rbac/model.rs

Structs (Serialize, Deserialize, sqlx::FromRow as appropriate):

RoleRow { id, name, description, role_type, status, is_system }
PermissionRow { id, name, description, category, is_dangerous, requires_step_up }
UserAccountRow { id, username, identity_mode, personnel_id, is_active, force_password_change, last_seen_at }
UserScopeAssignment { id, user_id, role_id, scope_type, scope_reference, valid_from, valid_to,
  granted_by_id, granted_at, is_emergency, emergency_reason, emergency_expires_at }
PermissionDependency { id, permission_name, required_permission_name, dependency_type }
RoleTemplate { id, name, description, module_set_json, is_system }
DelegatedAdminPolicy { id, admin_role_id, managed_scope_type, managed_scope_reference,
  allowed_domains_json, requires_step_up_for_publish }

STEP 3 - CREATE src-tauri/src/rbac/resolver.rs

Function:
`async fn effective_permissions(
    pool: &SqlitePool,
    user_id: i64,
    scope_type: &str,
    scope_reference: Option<&str>,
) -> Result<HashSet<String>>`

Algorithm:
1. SELECT role_id FROM user_scope_assignments WHERE user_id = ?
   AND (valid_from IS NULL OR valid_from <= date('now'))
   AND (valid_to IS NULL OR valid_to >= date('now'))
   AND (scope_type = 'tenant' OR (scope_type = ? AND scope_reference = ?))
   AND (is_emergency = 0 OR emergency_expires_at > datetime('now'))
2. For collected role_ids: SELECT DISTINCT p.name FROM permissions p
   JOIN role_permissions rp ON rp.permission_id = p.id
   WHERE rp.role_id IN (?)
3. Return HashSet<String> of permission names

Function:
`async fn user_has_permission(
    pool: &SqlitePool,
    user_id: i64,
    permission: &str,
    scope_reference: Option<&str>,
) -> Result<bool>`

Derives from effective_permissions, checks contains(permission).

STEP 4 - UPDATE src-tauri/src/rbac/macros.rs

Ensure `require_permission!` macro accepts:
  (pool, user_id, permission_str, scope_ref: Option<&str>)
  and calls user_has_permission, returning AppError::Forbidden if false.

Ensure `require_step_up!` macro accepts:
  (pool, session_id, required_freshness_seconds)
  and checks the session's step_up_verified_at against current time.

ACCEPTANCE CRITERIA
- cargo check passes with rbac::resolver in scope
- effective_permissions returns empty set for user with no assignments
- effective_permissions includes both tenant-wide and entity-scoped permissions for a user
  with assignments of each type
- require_permission! macro used in at least one existing command without compilation error
```

---

## User and Role Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the user and role admin IPC commands.

CREATE src-tauri/src/commands/admin_users.rs

--- USER COMMANDS ---

list_users
  Permission: adm.users (tenant scope)
  Input: filter { is_active?, identity_mode?, search? }
  Output: Vec<UserWithRoles> (user + list of current role assignments with scope)

get_user
  Permission: adm.users
  Input: user_id: i64
  Output: UserDetail (user + all scope assignments + effective permission domains for current date)

create_user
  Permission: adm.users + requires_step_up
  Input: CreateUserInput { username, identity_mode, personnel_id?, force_password_change? }
  Validation: username unique; identity_mode one of local/sso/hybrid
  Inserts into user_accounts; returns created user id.

update_user
  Permission: adm.users
  Input: UpdateUserInput { user_id, username?, personnel_id?, force_password_change?, is_active? }
  Note: cannot deactivate self; cannot deactivate last active superadmin

deactivate_user
  Permission: adm.users + requires_step_up
  Input: user_id: i64
  Guard: cannot deactivate self; cannot deactivate last active superadmin
  Sets is_active = 0

assign_role_scope
  Permission: adm.users + requires_step_up (for non-emergency)
  Input: AssignRoleScopeInput { user_id, role_id, scope_type, scope_reference?, valid_from?, valid_to? }
  Inserts into user_scope_assignments; checks for duplicate scope+role
  Returns assignment id

revoke_role_scope
  Permission: adm.users + requires_step_up
  Input: assignment_id: i64
  Deletes from user_scope_assignments

--- ROLE COMMANDS ---

list_roles
  Permission: adm.roles (any scope)
  Output: Vec<RoleWithPermissions>

get_role
  Permission: adm.roles
  Input: role_id: i64
  Output: RoleDetail (role + permissions + dependency warnings + templates derived from)

create_role
  Permission: adm.roles + requires_step_up
  Input: CreateRoleInput { name, description, permission_names: Vec<String> }
  Validation: name unique; all permission_names must exist in permissions table;
              dependency check: compute all hard dependencies; if any missing, return error listing them
  Inserts into roles (role_type='custom', is_system=0) + role_permissions
  Returns created role id

update_role
  Permission: adm.roles + requires_step_up
  Input: UpdateRoleInput { role_id, description?, add_permissions?: Vec<String>, remove_permissions?: Vec<String> }
  Guard: cannot add/remove from system roles (is_system=1)
  Dependency re-check on final permission set after add+remove

delete_role
  Permission: adm.roles + requires_step_up
  Guard: cannot delete if is_system=1
  Guard: cannot delete if any user_scope_assignments reference this role_id
  Soft-retire: set status = 'retired' only; do not hard-delete

list_role_templates
  Permission: adm.roles
  Output: Vec<RoleTemplate>

simulate_access
  Permission: adm.users
  Input: SimulateAccessInput { user_id, scope_type, scope_reference? }
  Returns: SimulateAccessResult {
    permissions: HashMap<String, bool>,
    assignments: Vec<UserScopeAssignment>,
    dependency_warnings: Vec<String>,
    blocked_by: Vec<String>  // list of missing hard dependencies
  }

REGISTER all commands in Tauri invoke_handler.

ACCEPTANCE CRITERIA
- cargo check passes
- create_role with missing hard dependency returns error listing the missing permissions
- delete system role returns AppError with 'system role cannot be modified' message
- simulate_access returns accurate permissions for scoped user
```

---

## Frontend Wrappers and Core UI

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the RBAC admin service and core UI panels.

CREATE src/services/rbac-service.ts

Zod schemas and TypeScript types for all model types.
Invoke wrappers: listUsers, getUser, createUser, updateUser, deactivateUser,
  assignRoleScope, revokeRoleScope,
  listRoles, getRole, createRole, updateRole, deleteRole,
  listRoleTemplates, simulateAccess.

CREATE src/components/admin/UserListPanel.tsx

- Paginated user table with columns: username, identity_mode, active status, role count, last_seen_at
- Filter: active/inactive, identity mode, name search
- Row actions: view detail, assign role, deactivate
- "Create User" button (adm.users permission guard)
- No inline editing; opens UserDetailModal or RoleAssignmentModal

CREATE src/components/admin/RoleEditorPanel.tsx

- Left: list of all roles with type badge (system/custom) and status badge
- Right: selected role's permission tree grouped by domain (ot.*, di.*, eq.*, etc.)
- Permission tree: each domain expandable; permissions toggleable for custom roles
  (system role permissions are read-only with lock icon)
- Dependency warnings shown inline in orange
- Hard dependency blocks shown in red before saving
- "Create Role from Template" button: loads role_templates, lets admin select one,
  pre-populates permission set for review before save
- Required: a "simulate_access" button: opens SimulateAccessModal (user picker + scope picker)

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- Zod validation on all response types
- System role permissions show lock icon; no toggle available for is_system=1 roles
- RoleEditorPanel renders dependency warnings before save
```

### Supervisor Verification - UI

**V1 - System role lock.**
Select "Superadmin" in RoleEditorPanel; all permission toggles are disabled.

**V2 - Dependency warning.**
Select a custom role; toggle on `ot.close` without `ot.edit` present; warning renders in red
before save button is active.

**V3 - Simulate access.**
Run simulateAccess for a user with entity-scoped supervisor assignment; result shows ot.*, di.*
permissions but NOT adm.* permissions.

---

## SP06-F01 Completion Checklist

- [ ] Migration 025 applies on fresh and existing DB
- [ ] 5 system roles seeded (is_system=1, cannot be deleted)
- [ ] 4 role templates seeded
- [ ] ≥ 19 permission_dependency rows seeded
- [ ] `user_scope_assignments` with unique index and emergency-elevation columns
- [ ] `effective_permissions` returns correct sets for tenant vs entity scope
- [ ] `require_permission!` macro updated to accept scope
- [ ] All user admin IPC commands registered
- [ ] All role admin IPC commands registered
- [ ] `simulate_access` returns HashMap of permission names with hard dependency warnings
- [ ] Frontend: UserListPanel and RoleEditorPanel type-check and render correctly

---

## Sprint S4 — AdminPage Layout, User Metrics, Role Chips, and User-Create Password

> **Gaps addressed:** The roadmap specifies 6+ admin panels (UserListPanel, RoleEditorPanel,
> PermissionCatalogPanel, SessionVisibilityPanel, DelegationManagerPanel,
> EmergencyElevationPanel, RoleImportExportPanel, AdminAuditTimeline) but no `AdminPage.tsx`
> that wires them together. The web app has 4 KPI metric cards above the user table; the
> desktop has no equivalent. The web's role list shows domain-coloured permission chips
> (`DI (5)`, `OT (7)`) — the desktop spec only describes the tree editor, not the list view.
> Finally, `create_user` omits an initial password field, making local-user creation
> impossible. This sprint closes all four gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/pages/AdminPage.tsx` | Tab-routed page replacing the `UsersPage` placeholder |
| `src/components/admin/AdminMetricCards.tsx` | KPI cards at the top of AdminPage |
| `src/components/admin/RoleEditorPanel.tsx` (patch) | Permission domain chips in role list |
| `src-tauri/src/commands/admin_users.rs` (patch) | `initial_password` field on `create_user` |
| `src-tauri/src/commands/admin_stats.rs` | `get_admin_stats` IPC command |
| `src/services/rbac-service.ts` (patch) | Typed wrappers for admin stats + updated `CreateUserInput` |
| `src/i18n/locale-data/{fr,en}/admin.json` (patch) | Admin page, tab, and metric labels |

### GAP-05 — AdminPage.tsx — Tab-Routed Admin Shell

Replaces the current `UsersPage.tsx` placeholder.

**Layout:**
```
┌──────────────────────────────────────────────────────────────────┐
│  Page Header: "Administration"                                   │
│  [AdminMetricCards — 4 KPI cards]                                │
├──────────────────────────────────────────────────────────────────┤
│  Tab Bar (permission-gated):                                     │
│  [Users] [Roles] [Permissions] [Sessions] [Delegation]           │
│  [Emergency] [Import/Export] [Audit]                             │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Active Tab → renders corresponding panel component              │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

**Tab configuration:**

| Tab Label (FR / EN) | Panel Component | Required Permission | Default |
|---------------------|-----------------|--------------------:|---------|
| Utilisateurs / Users | `UserListPanel` | `adm.users` | ✅ first visible |
| Rôles / Roles | `RoleEditorPanel` | `adm.roles` | |
| Permissions | `PermissionCatalogPanel` | `adm.permissions` | |
| Sessions | `SessionVisibilityPanel` | `adm.users` | |
| Délégation / Delegation | `DelegationManagerPanel` | `adm.roles` | |
| Urgence / Emergency | `EmergencyElevationPanel` | `adm.users` | |
| Import / Export | `RoleImportExportPanel` | `adm.roles` | |
| Audit | `AdminAuditTimeline` | `adm.users` OR `adm.roles` | |

**Behaviour:**
- Each tab is conditionally rendered using `<PermissionGate>`.
- Default active tab = first tab the user has permission for.
- Tab state stored in URL search param (`?tab=roles`) for deep-linking.
- If user has no `adm.*` permissions, `AdminPage` shows a 403 message.
- Page registered in router at `/admin` and sidebar nav item requires `adm.users` OR `adm.roles`.

**Implementation:**
- Use Radix `Tabs` primitive (`@radix-ui/react-tabs`) for accessible keyboard nav.
- Lazy-load each panel component via `React.lazy()`.

### GAP-06 — AdminMetricCards.tsx — KPI Cards

**Layout (horizontal row, 4 cards):**

| Card | Value Source | Icon | Colour |
|------|-------------|------|--------|
| Utilisateurs actifs / Active Users | `stats.active_users` | `Users` | blue |
| Rôles / Roles | `stats.total_roles` (badge: `system_roles` / `custom_roles`) | `Shield` | indigo |
| Sessions actives / Active Sessions | `stats.active_sessions` | `MonitorSmartphone` | emerald |
| Sans affectation / Unassigned | `stats.unassigned_users` (users with 0 scope assignments) | `AlertTriangle` | amber (red if > 0) |

**Data source:** Calls `get_admin_stats` IPC command on mount and every 60 seconds.

**`get_admin_stats` backend command:**

```rust
// src-tauri/src/commands/admin_stats.rs

#[derive(Serialize)]
pub struct AdminStatsPayload {
    pub active_users: i64,
    pub inactive_users: i64,
    pub total_roles: i64,
    pub system_roles: i64,
    pub custom_roles: i64,
    pub active_sessions: i64,
    pub unassigned_users: i64,       // users with 0 active scope assignments
    pub emergency_grants_active: i64, // active emergency elevations
}

// Permission: adm.users (tenant scope)
// Single query with CTEs for each metric
```

### GAP-11 — Role Permission Chips in Role List

**Patch to `RoleEditorPanel.tsx` — left panel (role list):**

Currently the spec describes a role list with type/status badges. Add domain-coloured
permission chips next to each role.

**Chip rendering logic:**
1. Group role permissions by domain prefix (e.g., `ot.*`, `di.*`, `eq.*`).
2. Count only action permissions (exclude the `*.view` base if domain has other actions).
3. Render as compact chips: `DI (5)` `OT (7)` `EQ (3)` with domain-specific colours.

**Domain colour mapping:**

| Domain | Chip Colour |
|--------|------------|
| `di` | `bg-blue-100 text-blue-800` |
| `ot` | `bg-emerald-100 text-emerald-800` |
| `eq` | `bg-orange-100 text-orange-800` |
| `pm` | `bg-violet-100 text-violet-800` |
| `inv` | `bg-amber-100 text-amber-800` |
| `per` | `bg-cyan-100 text-cyan-800` |
| `org` | `bg-pink-100 text-pink-800` |
| `ref` | `bg-slate-100 text-slate-800` |
| `adm` | `bg-red-100 text-red-800` |
| Other | `bg-gray-100 text-gray-800` |

**Chip overflow:** If role has > 6 domain chips, show first 5 + `+N more` chip that
expands on hover (tooltip listing all domains).

**Full permissions = special badge:** If a role has ALL permissions, show a single
`Accès complet / Full access` badge in gold instead of individual chips.

### GAP-12 — User-Create Password Flow

**Problem:** `CreateUserInput` has `username, identity_mode, personnel_id, force_password_change`
but no `initial_password`. A local-mode user cannot be created without a password.

**Patch to `src-tauri/src/commands/admin_users.rs` — `create_user`:**

```rust
pub struct CreateUserInput {
    pub username: String,
    pub identity_mode: String,     // "local" | "sso" | "hybrid"
    pub personnel_id: Option<i64>,
    pub initial_password: Option<String>, // NEW — required when identity_mode = "local"
    pub force_password_change: Option<bool>, // defaults to true
}
```

**Validation rules:**
- If `identity_mode = "local"`: `initial_password` is **required**, min 8 chars,
  must contain at least one uppercase, one lowercase, one digit.
- If `identity_mode = "sso"`: `initial_password` must be `None` (SSO users don't have passwords).
- If `identity_mode = "hybrid"`: `initial_password` is optional (can login via SSO or password).
- `force_password_change` defaults to `true` when `initial_password` is provided.
- Password is hashed with argon2id (reuse `hash_password()` from `auth/password.rs`).

**Patch to `UserListPanel.tsx` — Create User modal:**

Add to the create-user form:
- **Password** field (required for local, hidden for SSO): `type="password"`, min 8 chars.
- **Confirm Password** field: must match.
- **Strength indicator**: visual bar (weak/medium/strong) using zxcvbn or simple regex check.
- **Force change on first login** checkbox: defaults to checked.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- AdminPage renders with tab bar; tabs filtered by permission
- Default tab = first tab user has permission for
- URL deep-linking works (?tab=roles navigates to Roles tab)
- AdminMetricCards shows 4 KPI cards with correct values
- Unassigned users card turns red when count > 0
- Auto-refresh every 60s without UI flicker
- Role list shows domain-coloured permission chips
- Full-access roles show gold "Accès complet" badge
- Chip overflow shows "+N more" with hover tooltip
- create_user with identity_mode="local" requires initial_password
- create_user with identity_mode="sso" rejects initial_password
- Password hashed with argon2id, force_password_change defaults true
- Create User modal shows password strength indicator
- cargo check passes with zero errors
```

### Supervisor Verification — Sprint S4

**V1 — AdminPage tabs.**
Login as Administrateur; verify all 8 tabs visible. Login as Technicien; verify AdminPage
shows 403 (no `adm.*` permissions).

**V2 — Metrics.**
With 3 active users, 2 custom roles, 1 unassigned user: verify cards show correct numbers;
unassigned card is amber/red.

**V3 — Permission chips.**
View Supervisor role in list; verify chips show `DI (5)` `OT (6)` etc. View Superadmin;
verify single gold "Accès complet" badge.

**V4 — Create local user.**
Click "Create User"; set identity_mode=local; leave password empty → form validation blocks
save. Enter valid password → user created with `force_password_change=true`. Login as new
user → ForcePasswordChangePage appears.

**V5 — Create SSO user.**
Click "Create User"; set identity_mode=sso; password field hidden. User created successfully
without password.

### S4‑6 — Profile Page (`ProfilePage.tsx`) — GAP PRO‑01

```
LOCATION   src/pages/ProfilePage.tsx
ROUTE      /profile (replaces ModulePlaceholder)
STORE      No new store — reads from auth session + settings-store
SERVICE    user-service.ts (patch — add getMyProfile, updateMyProfile IPC wrappers)
COMMAND    get_my_profile, update_my_profile (Rust — reads/writes own user record)

DESCRIPTION
Current-user self-service page (no admin permission required — any authenticated user).

Layout — single-column, centered max-w-2xl:
  ┌───────────────────────────────────────────────────────────┐
  │  ┌────────┐                                               │
  │  │ Avatar │  {display_name}                               │
  │  │  (XL)  │  {role_name} · {email}                        │
  │  └────────┘  Member since {created_at}                    │
  │                                                           │
  │  ─────────────────────────────────────────────────────    │
  │                                                           │
  │  Personal Information                        [ Edit ]     │
  │  ┌──────────────┬──────────────────────────────────────┐  │
  │  │ Display name │ Jean Dupont                          │  │
  │  │ Email        │ jean@example.com                     │  │
  │  │ Phone        │ +33 6 12 34 56 78                    │  │
  │  │ Language     │ Français                             │  │
  │  └──────────────┴──────────────────────────────────────┘  │
  │                                                           │
  │  Security                                                 │
  │  ┌──────────────────────────────────────────────────────┐ │
  │  │ Password        Last changed: 2026-03-15  [ Change ] │ │
  │  │ PIN unlock      Enabled                   [ Manage ] │ │
  │  │ Trusted devices 2 devices      [ View / Revoke ]     │ │
  │  └──────────────────────────────────────────────────────┘ │
  │                                                           │
  │  Notification Preferences                                 │
  │  ┌──────────────────────────────────────────────────────┐ │
  │  │ Links to NotificationPreferencesPanel (SP07-F01)     │ │
  │  └──────────────────────────────────────────────────────┘ │
  │                                                           │
  │  Session History (last 10)                                │
  │  ┌──────────┬──────────┬──────────┬──────────┐           │
  │  │ Date     │ Device   │ Duration │ Status   │           │
  │  │ Apr 8    │ Desktop  │ 2h 15m   │ Active   │           │
  │  │ Apr 7    │ Desktop  │ 6h 30m   │ Closed   │           │
  │  └──────────┴──────────┴──────────┴──────────┘           │
  └───────────────────────────────────────────────────────────┘

- "Edit" personal info: opens inline edit with save/cancel
- "Change Password": opens change-password dialog (current + new + confirm)
- "Manage PIN": opens enable/disable/change PIN dialog
- "View / Revoke" trusted devices: lists device names with revoke button
- Session history: read-only, last 10 sessions from session_events table
- Notification preferences: reuses NotificationPreferencesPanel from SP07-F01
  (or shows "Available after notification module" placeholder if SP07 not yet built)

ACCEPTANCE CRITERIA
- any authenticated user can access /profile
- personal info edit saves via update_my_profile
- password change requires current password validation
- trusted device revocation works
- session history loads from session_events
```

### Supervisor Verification — Sprint S4 (continued)

**V6 — Profile page access.**
Login as any user. Navigate to /profile. Verify personal info, security section, and
session history render correctly.

**V7 — Profile edit.**
Edit display name. Save. Verify TopBar user menu shows updated name.

**V8 — Password change.**
Click "Change Password". Enter wrong current password → error. Enter correct current
password + valid new password → success toast.

---

*End of Phase 2 - Sub-phase 06 - File 01*
