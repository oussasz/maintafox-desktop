# Phase 2 - Sub-phase 06 - File 02
# Permission Domains and Scope Enforcement

## Context and Purpose

File 01 built the structural schema: users, roles, scope assignments, templates, and the
resolver logic. File 02 operationalizes the complete `adm.*` permission domain, seeds all
cross-module permission domain rows that were seeded piecemeal in SP04–SP05, rationalizes the
full permission catalog into one authoritative migration (026), and implements scope enforcement
so that RBAC evaluation is consistent whether a user is approving a DI, closing a WO, adding a
PM plan, or publishing reference data.

The central design commitment is: **one permission domain can only be evaluated against one
scope chain.** A user with `ot.view` scoped to entity A cannot evaluate that permission for
entity B. The scope chain is: tenant → site → entity → team → org_node. When the requested
operation's entity context is wider than the user's most-general grant, access is denied.

---

## PRD Alignment Checklist

- [x] Full `adm.*` domain defined (adm.users / adm.roles / adm.permissions from PRD §6.7 explicit)
- [x] Consolidated cross-module permission domain table from PRD §6.7 (eq.*, di.*, ot.*, per.*, ref.*, inv.*, pm.*, ram.*, rep.*, arc.*, doc.*, plan.*, log.*, trn.*, iot.*, erp.*, ptw.*, fin.*, ins.*, cfg.*)
- [x] Scope matching logic: tenant-wide grants evaluated before entity-specific checks
- [x] Dangerous permission write-protection (is_dangerous=1 rows cannot be updated by tenant admins via the adm.permissions command)
- [x] `permission_dependencies` hard-block on role save validated and returned as structured error
- [x] Warning-dependency shown in UI without blocking save
- [x] `adm.permissions` domain: only the Superadmin or explicitly delegated admin can modify the dangerous/step-up flags on existing permissions

---

## Architecture Rules Applied

- **Permission catalog is additive, not replaceable.** Migration 026 uses `INSERT OR IGNORE`
  so any permissions already seeded by SP04-F04 (di.*) and SP05-F04 (ot.*) are not
  overwritten. This makes migrations idempotent and order-independent for the seed data.
- **Scope chain resolution is scope-by-scope, lowest-to-highest.** If a user has an entity-
  level grant for `ot.edit` on entity A and a tenant-level grant for `ot.view`, both grants
  are active simultaneously. For commands that require `ot.edit`, the resolver checks:
  does the user have ot.edit at tenant OR at the requesting entity's entity_id?
- **Global scope assignments (scope_type='tenant') are the override.** A tenant-wide
  Superadmin role assignment means all scoped evaluation succeeds regardless of entity context.
- **Permission domain rows are system data.** Domains derived from the PRD permission table
  are seeded during migration. Tenant admins can define additional custom permissions (prefix
  cst.*) but cannot rename, remove, or alter is_dangerous on system permissions.
- **`adm.*` domain is self-referential.** Performing `adm.*` actions requires that the admin
  user themselves was assigned a role carrying those permissions at tenant scope. Entity-scoped
  adm.* is conceptually invalid and blocked at the resolver level.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260601_000026_permission_catalog.rs` | Authoritative full permission seed across all 21 domains |
| `src-tauri/src/rbac/scope_chain.rs` | `ScopeChain` type and `resolve_scope_for_entity(entity_id, pool)` helper |
| `src-tauri/src/commands/admin_permissions.rs` | IPC commands for permission catalog management |
| `src/components/admin/PermissionCatalogPanel.tsx` | Read-only permission catalog panel with domain grouping and dangerous badges |

---

## Migration 026 — Full Permission Catalog

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the authoritative permission catalog migration.

STEP 1 - CREATE src-tauri/migrations/m20260601_000026_permission_catalog.rs

Use INSERT OR IGNORE to seed all permissions. Do not overwrite existing rows.

Seed the following permission rows with name, description, category, is_dangerous, requires_step_up:

-- Module: Equipment (eq.*)
('eq.view',         'View equipment records',                   'eq', 0, 0),
('eq.create',       'Create new equipment',                     'eq', 0, 0),
('eq.edit',         'Edit equipment fields and lifecycle',      'eq', 0, 0),
('eq.delete',       'Delete draft or decommissioned equipment', 'eq', 1, 0),
('eq.admin',        'Manage equipment import, merge, archive',  'eq', 1, 1),

-- Module: Intervention Requests (di.*)
('di.view',         'View intervention requests',               'di', 0, 0),
('di.submit',       'Submit new intervention requests',         'di', 0, 0),
('di.review',       'Review and enrich submitted DIs',          'di', 0, 0),
('di.approve',      'Approve or reject DIs in review',          'di', 0, 0),
('di.convert',      'Convert DI to work order',                 'di', 1, 0),
('di.delete',       'Delete draft DIs',                         'di', 1, 0),
('di.admin',        'Reopen, override, and manage DI settings', 'di', 1, 1),

-- Module: Work Orders (ot.*)
('ot.view',         'View work orders and details',             'ot', 0, 0),
('ot.create',       'Create new work orders',                   'ot', 0, 0),
('ot.edit',         'Edit, plan, assign, and execute WOs',      'ot', 0, 0),
('ot.approve',      'Approve work orders from draft',           'ot', 0, 0),
('ot.close',        'Close technically verified work orders',   'ot', 1, 1),
('ot.reopen',       'Reopen recently closed work orders',       'ot', 1, 1),
('ot.admin',        'Override, archive, manage WO settings',    'ot', 1, 0),
('ot.delete',       'Delete draft work orders',                 'ot', 1, 0),

-- Module: Organization (org.*)
('org.view',        'View org structure',                       'org', 0, 0),
('org.edit',        'Edit org units and hierarchies',           'org', 0, 0),
('org.admin',       'Publish, archive, and merge org units',    'org', 1, 1),

-- Module: Personnel (per.*)
('per.view',        'View personnel records',                   'per', 0, 0),
('per.create',      'Create new personnel records',             'per', 0, 0),
('per.edit',        'Edit personnel details and availability',  'per', 0, 0),
('per.delete',      'Deactivate personnel records',             'per', 1, 0),
('per.admin',       'Manage rate cards, onboarding, overrides', 'per', 1, 1),

-- Module: Reference Data (ref.*)
('ref.view',        'View reference domains and values',         'ref', 0, 0),
('ref.edit',        'Edit and draft reference domain values',    'ref', 0, 0),
('ref.publish',     'Publish reference domain versions',         'ref', 1, 1),
('ref.admin',       'Import, merge, retire reference domains',   'ref', 1, 1),

-- Module: Inventory (inv.*)
('inv.view',        'View stock, articles, and transactions',    'inv', 0, 0),
('inv.manage',      'Issue, return, adjust, transfer stock',     'inv', 0, 0),
('inv.procure',     'Create and approve purchase requisitions',  'inv', 1, 0),
('inv.count',       'Execute and post stock count sessions',     'inv', 1, 0),

-- Module: Preventive Maintenance (pm.*)
('pm.view',         'View PM plans and occurrences',             'pm', 0, 0),
('pm.create',       'Create new PM plans',                       'pm', 0, 0),
('pm.edit',         'Edit PM plans and versions',                'pm', 0, 0),
('pm.delete',       'Delete draft PM plans',                     'pm', 1, 0),

-- Module: RAMS / Reliability (ram.*)
('ram.view',        'View reliability events and KPIs',          'ram', 0, 0),
('ram.analyze',     'Perform FMECA and RCM analysis',            'ram', 0, 0),
('ram.export',      'Export reliability data and reports',       'ram', 0, 0),

-- Module: Reports/Analytics (rep.*)
('rep.view',        'View operational reports and dashboards',   'rep', 0, 0),
('rep.export',      'Export reports to PDF or Excel',            'rep', 0, 0),
('rep.admin',       'Manage report templates and sharing',       'rep', 1, 0),

-- Module: Archive Explorer (arc.*)
('arc.view',        'Browse archived records',                   'arc', 0, 0),
('arc.restore',     'Restore eligible archived records',         'arc', 1, 1),
('arc.purge',       'Purge records past retention policy',       'arc', 1, 1),

-- Module: Documentation (doc.*)
('doc.view',        'View documents and support articles',       'doc', 0, 0),
('doc.author',      'Author and publish documentation',          'doc', 0, 0),
('doc.admin',       'Manage doc lifecycle and acknowledgements', 'doc', 1, 0),

-- Module: Planning & Scheduling (plan.*)
('plan.view',       'View planning boards and schedule',         'plan', 0, 0),
('plan.edit',       'Schedule and commit work to calendar',      'plan', 0, 0),
('plan.admin',      'Manage capacity limits and constraints',    'plan', 1, 0),

-- Module: Activity Log (log.*)
('log.view',        'View activity feed events',                 'log', 0, 0),
('log.export',      'Export audit log',                          'log', 1, 0),
('log.admin',       'Manage activity feed settings',             'log', 1, 1),

-- Module: Training & Habilitation (trn.*)
('trn.view',        'View training and certification records',   'trn', 0, 0),
('trn.manage',      'Manage certifications and training records','trn', 0, 0),
('trn.override',    'Override qualification holds with reason',  'trn', 1, 1),

-- Module: IoT Gateway (iot.*)
('iot.view',        'View IoT device streams and alerts',        'iot', 0, 0),
('iot.configure',   'Configure IoT devices, rules, and mapping', 'iot', 1, 1),

-- Module: ERP Connector (erp.*)
('erp.view',        'View ERP connector status and logs',        'erp', 0, 0),
('erp.configure',   'Configure ERP mappings and sync contracts', 'erp', 1, 1),
('erp.reconcile',   'Run and approve ERP reconciliation runs',   'erp', 1, 1),

-- Module: Work Permits (ptw.*)
('ptw.view',        'View work permits',                          'ptw', 0, 0),
('ptw.issue',       'Issue and activate work permits',            'ptw', 1, 1),
('ptw.close',       'Close and cancel work permits',              'ptw', 1, 1),

-- Module: Budget/Finance (fin.*)
('fin.view',        'View budgets and cost center reports',       'fin', 0, 0),
('fin.manage',      'Manage budget baselines and forecasts',      'fin', 1, 0),
('fin.approve',     'Approve cost events and commitments',        'fin', 1, 1),
('fin.post',        'Post cost actuals to ERP',                   'fin', 1, 1),

-- Module: Inspection Rounds (ins.*)
('ins.view',        'View inspection rounds and checklists',      'ins', 0, 0),
('ins.execute',     'Execute inspection rounds and record results','ins', 0, 0),
('ins.admin',       'Manage inspection templates and schedules',  'ins', 1, 0),

-- Module: Configuration Engine (cfg.*)
('cfg.view',        'View configuration engine settings',         'cfg', 0, 0),
('cfg.edit',        'Edit and draft configuration changes',       'cfg', 1, 0),
('cfg.publish',     'Publish configuration changes globally',     'cfg', 1, 1),
('cfg.admin',       'Manage tenant customization rules',          'cfg', 1, 1),

-- Module: Administration (adm.*)
('adm.users',       'Manage user accounts and scope assignments', 'adm', 1, 1),
('adm.roles',       'Manage roles and role permissions',          'adm', 1, 1),
('adm.permissions', 'View and govern the permission catalog',     'adm', 1, 1);

ACCEPTANCE CRITERIA
- Migration 026 applies on fresh and existing DB (idempotent)
- SELECT COUNT(*) FROM permissions returns ≥ 70 after migration
- All rows with is_dangerous=1 have a clear description and at least one requires_step_up value
  (either 0 or 1 as appropriate)
- cargo check passes
```

### Supervisor Verification - Migration 026

**V1 - Permission count.**
`SELECT COUNT(*) FROM permissions` returns ≥ 70.

**V2 - Dangerous count.**
`SELECT COUNT(*) FROM permissions WHERE is_dangerous = 1` returns ≥ 25.

**V3 - Step-up subset.**
`SELECT COUNT(*) FROM permissions WHERE requires_step_up = 1` returns ≥ 20.

**V4 - Module coverage.**
`SELECT DISTINCT category FROM permissions ORDER BY 1` must show at least 20 distinct categories.

---

## Scope Chain Resolution

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the scope chain resolution helper.

CREATE src-tauri/src/rbac/scope_chain.rs

Purpose:
Given an org_node_id (an entity, site, team, or the tenant root), resolve the full
upward chain of scope references so that a user assignment at any parent scope is
automatically valid for child scopes.

Types:
ScopeNode { id: i64, parent_id: Option<i64>, scope_type: String }
ScopeChain { nodes: Vec<ScopeNode> }

Function:
`async fn resolve_scope_chain(
    pool: &SqlitePool,
    org_node_id: i64,
) -> Result<ScopeChain>`

Algorithm:
1. Walk up org_units hierarchy using recursive CTE:
   WITH RECURSIVE scope_cte(id, parent_id, scope_type) AS (
     SELECT id, parent_id, node_type FROM org_units WHERE id = ?
     UNION ALL
     SELECT o.id, o.parent_id, o.node_type
     FROM org_units o JOIN scope_cte s ON o.id = s.parent_id
   )
   SELECT * FROM scope_cte
2. Append a synthetic tenant node (id=0, parent_id=NULL, scope_type='tenant')
3. Return ScopeChain with all nodes ordered root→leaf

Update resolver.rs `effective_permissions` to:
  Accept org_node_id: Option<i64> instead of (scope_type, scope_reference).
  When org_node_id is Some:
    call resolve_scope_chain (or cache it per request context)
    match user_scope_assignments where scope_reference is one of the chain node IDs
    UNION with tenant-wide assignments (scope_type='tenant')
  When org_node_id is None (tenant-level operations like adm.*):
    match only tenant-scope assignments.

ACCEPTANCE CRITERIA
- cargo check passes
- resolve_scope_chain for a leaf entity returns all ancestor nodes plus tenant
- effective_permissions for user with entity-level assignment returns those permissions
  when called with any descendant node_id
- effective_permissions for user with tenant-level grant returns permissions for any node_id
```

---

## Permission Catalog Admin Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement permission catalog administration commands.

CREATE src-tauri/src/commands/admin_permissions.rs

list_permissions
  Permission: adm.permissions (tenant scope)
  Input: filter { category?, is_dangerous?, search? }
  Output: Vec<PermissionRow> sorted by category asc, name asc

get_permission_dependencies
  Permission: adm.permissions
  Input: permission_name: String
  Output: Vec<PermissionDependency> (all hard and warn pairs for this permission or requiring it)

create_custom_permission
  Permission: adm.permissions + requires_step_up
  Input: CreateCustomPermInput { name: String, description: String, category: String }
  Validation:
    - name must start with 'cst.' prefix (tenant-created permissions only)
    - name must be unique
    - is_dangerous = 0, requires_step_up = 0 always for tenant-created permissions
      (tenant cannot create dangerous permissions)
  Inserts into permissions

validate_role_permissions
  Permission: adm.roles
  Input: permission_names: Vec<String>
  Output: RoleValidationResult {
    missing_hard_deps: Vec<(String, String)>,   -- (missing_perm, required_by)
    warn_deps: Vec<(String, String)>,            -- (warn_perm, suggested_by)
    unknown_permissions: Vec<String>,
    is_valid: bool  -- true iff missing_hard_deps is empty AND unknown_permissions is empty
  }
  Algorithm:
    For each permission name in the input set:
      Lookup permission_dependencies WHERE permission_name IN (set)
      If dependency_type = 'hard' AND required_permission_name NOT IN set -> missing_hard_deps
      If dependency_type = 'warn' AND required_permission_name NOT IN set -> warn_deps
      If permission name not found in permissions table -> unknown_permissions

REGISTER all commands in Tauri invoke_handler.

ACCEPTANCE CRITERIA
- cargo check passes
- validate_role_permissions(['ot.close']) without ['ot.edit'] returns missing_hard_deps with
  the ot.close→ot.edit pair
- create_custom_permission with name='ot.edit' returns error (system namespace not allowed)
- create_custom_permission with name='cst.my_thing' succeeds
```

---

## Permission Catalog Panel

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the permission catalog admin panel.

CREATE src/components/admin/PermissionCatalogPanel.tsx

Layout:
- Left sidebar: list of permission domain groups (eq, di, ot, org, per, ref, inv, pm, ram,
  rep, arc, doc, plan, log, trn, iot, erp, ptw, fin, ins, cfg, adm)
- Right main area: permissions in the selected domain
  - Table: name | description | dangerous badge | step-up badge | type (system/custom)
  - System permissions: all fields read-only
  - Custom permissions (cst.*): description editable inline

Features:
- "Add Custom Permission" button (adm.permissions guard) - opens modal with name (cst.* prefix
  enforced), description, category fields; name validated to disallow system prefixes
- Dependency viewer: click a permission → sidebar panel showing all hard and warn dependencies
  for that permission (calls get_permission_dependencies)
- Export permission matrix to CSV/PDF (adm.permissions guard)

PATCH RoleEditorPanel.tsx (from F01):
- When a permission is toggled on in the role editor, immediately call
  validate_role_permissions with the new proposed set
- Hard dependency failures render as red error banners; save disabled
- Warning dependencies render as orange info banners; save still allowed

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- PermissionCatalogPanel is read-only for system permissions
- Dependency viewer shows hard deps in red, warn deps in orange
- RoleEditorPanel shows real-time hard-dep error when a permission with unsatisfied
  hard dependencies is toggled
```

---

## SP06-F02 Completion Checklist

- [ ] Migration 026 applies (idempotent INSERT OR IGNORE)
- [ ] ≥ 70 permission rows after migration
- [ ] All 21 domain categories covered in permissions table
- [ ] `resolve_scope_chain` correctly traverses org hierarchy upward
- [ ] `effective_permissions` uses scope chain — entity-level assignments grant access for descendants
- [ ] `validate_role_permissions` returns structured hard/warn arrays (not plain strings)
- [ ] `create_custom_permission` enforces `cst.*` prefix; blocks system namespace attempts
- [ ] `PermissionCatalogPanel` renders with domain grouping and dangerous/step-up badges
- [ ] `RoleEditorPanel` validates dependencies in real-time before save

---

## Sprint S4 — PermissionProvider Context, Permission Cache, and Per-Route Guards

> **Gaps addressed:** (1) Each `usePermissions()` hook instance fires an independent IPC
> call — the code itself notes "Phase 2 will introduce a PermissionProvider context" but
> no spec exists. (2) Every `check_permission()` call in Rust queries SQLite; with scoped
> resolution + dependency checks, this is expensive. No in-memory cache layer is specified.
> (3) The desktop has menu filtering via `requiredPermission` on `NavItem`, but no per-route
> guards — a user who knows the URL path can navigate to any route. This sprint closes all
> three performance and security gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/contexts/PermissionContext.tsx` | React context + provider for centralized permission state |
| `src/hooks/use-permissions.ts` (rewrite) | Thin wrapper over context instead of independent IPC |
| `src/components/auth/PermissionRoute.tsx` | Route-level permission guard component |
| `src/router.tsx` (patch) | Wrap protected routes with `<PermissionRoute>` |
| `src-tauri/src/rbac/cache.rs` | In-memory permission cache with event-driven invalidation |
| `src-tauri/src/rbac/resolver.rs` (patch) | Use cache layer instead of raw SQL on every check |
| `src-tauri/src/commands/admin_users.rs` (patch) | Emit `rbac-changed` event on role/assignment mutation |
| `src/i18n/locale-data/{fr,en}/auth.json` (patch) | Unauthorized page labels |

### GAP-01 — PermissionContext.tsx — Centralized Permission State

**Problem:** Every component calling `usePermissions()` triggers `invoke("get_my_permissions")`.
On a page with 10 `<PermissionGate>` components, that's 10 IPC round-trips.

**Solution:** Single `PermissionProvider` at the `AuthGuard` level loads permissions once,
stores in React context, and exposes the same `can()` API.

**Architecture:**

```
<AuthGuard>
  <PermissionProvider>       ← NEW: loads permissions once
    <ShellLayout>
      <Outlet />
    </ShellLayout>
  </PermissionProvider>
</AuthGuard>
```

**PermissionContext API:**

```typescript
interface PermissionContextValue {
  permissions: PermissionRecord[];
  isLoading: boolean;
  can: (permissionName: string) => boolean;
  canAny: (...permissionNames: string[]) => boolean;
  canAll: (...permissionNames: string[]) => boolean;
  refresh: () => Promise<void>;
}
```

**Behaviour:**
1. On mount: calls `get_my_permissions` once. Stores result in `useState`.
2. Listens for Tauri event `rbac-changed` (emitted by backend on any role/assignment mutation).
   On receive → calls `refresh()` to reload permissions.
3. Listens for `session-unlocked` event → calls `refresh()` (permissions may have changed
   while session was locked if admin modified roles in another session).
4. `can()`, `canAny()`, `canAll()` are memoized with `useMemo` over the permissions array.
5. While `isLoading`, all `can()` calls return `false` (deny-by-default).

**`usePermissions()` hook rewrite:**

```typescript
export function usePermissions() {
  const context = useContext(PermissionContext);
  if (!context) {
    throw new Error('usePermissions must be used within <PermissionProvider>');
  }
  return context;
}
```

No more independent `invoke()` calls. All consumers share one permission set.

**`<PermissionGate>` stays unchanged** — it already uses `usePermissions().can()`.

### GAP-02 — Permission Cache Layer (Rust)

**Problem:** `check_permission()` in `rbac.rs` runs a SQL query joining
`user_scope_assignments → role_permissions → permissions` on every call. Commands that
check multiple permissions (e.g., a page load checking 5 permissions) hit the DB 5 times.

**Solution:** In-memory cache inside `AppState` with event-driven invalidation.

**New file: `src-tauri/src/rbac/cache.rs`**

```rust
pub struct PermissionCache {
    /// Current user's effective permissions per scope key.
    /// Key: (user_id, scope_key) where scope_key = "tenant" | "entity:{id}" | "site:{id}"
    /// Value: (HashSet<String>, Instant)  — permissions + load timestamp
    entries: HashMap<(i64, String), (HashSet<String>, Instant)>,
    /// Maximum age before forced refresh (fallback safety net)
    max_age: Duration,  // default: 120 seconds
}

impl PermissionCache {
    pub fn new(max_age_secs: u64) -> Self { ... }

    /// Get cached permissions. Returns None if not cached or expired.
    pub fn get(&self, user_id: i64, scope_key: &str) -> Option<&HashSet<String>> { ... }

    /// Store permissions for a user+scope pair.
    pub fn put(&mut self, user_id: i64, scope_key: String, perms: HashSet<String>) { ... }

    /// Invalidate ALL entries for a given user (called on role/assignment change).
    pub fn invalidate_user(&mut self, user_id: i64) { ... }

    /// Invalidate ALL entries (called on role definition change affecting multiple users).
    pub fn invalidate_all(&mut self) { ... }
}
```

**Integration with `AppState`:**

```rust
pub struct AppState {
    pub db: SqlitePool,
    pub session: Arc<RwLock<SessionManager>>,
    pub permission_cache: Arc<RwLock<PermissionCache>>,  // NEW
    // ...
}
```

**Integration with `resolver.rs`:**

Patch `effective_permissions()` and `user_has_permission()`:
1. Check `permission_cache.get(user_id, scope_key)` first.
2. On cache hit → return cached set.
3. On cache miss → run SQL query → `permission_cache.put()` → return.

**Invalidation triggers (in `admin_users.rs` commands):**
- `assign_role_scope` → `cache.invalidate_user(target_user_id)` + emit `rbac-changed` event
- `revoke_role_scope` → `cache.invalidate_user(target_user_id)` + emit `rbac-changed` event
- `update_role` (permissions changed) → `cache.invalidate_all()` + emit `rbac-changed` event
- `delete_role` → `cache.invalidate_all()` + emit `rbac-changed` event
- `grant_emergency_elevation` → `cache.invalidate_user(target)` + emit `rbac-changed` event
- `revoke_emergency_elevation` → `cache.invalidate_user(target)` + emit `rbac-changed` event
- `deactivate_user` → `cache.invalidate_user(target)` + emit `rbac-changed` event

**Tauri event emission:**

```rust
// After any RBAC mutation:
app_handle.emit("rbac-changed", RbacChangedPayload {
    affected_user_id: Some(target_user_id), // or None for global changes
    action: "role_assigned",
})?;
```

This event is consumed by both:
- The Rust cache (invalidation)
- The frontend `PermissionProvider` (refresh)

### GAP-07 — PermissionRoute.tsx — Per-Route Permission Guards

**Problem:** Desktop has only sidebar filtering. A user who types `/admin` in the address bar
(or bookmarks it) bypasses menu-level filtering. Menu hiding is UX, not access control.

**Solution:** `<PermissionRoute>` component wrapping protected route segments.

**Component API:**

```typescript
interface PermissionRouteProps {
  permission?: string;       // single permission check
  anyOf?: string[];          // any of these permissions
  allOf?: string[];          // all of these permissions
  fallback?: React.ReactNode; // custom fallback (default: UnauthorizedPage)
}
```

**Behaviour:**
1. Uses `usePermissions()` from context.
2. While loading → renders `<LoadingSpinner />`.
3. If permission check fails → renders `<UnauthorizedPage />` (or custom fallback).
4. If permission check passes → renders `<Outlet />`.

**New component: `src/pages/UnauthorizedPage.tsx`**

Simple page with:
- Lock icon
- "Accès non autorisé / Unauthorized Access" heading
- "Vous n'avez pas les permissions nécessaires pour accéder à cette page."
- "Retour au tableau de bord / Return to Dashboard" link

**Router patch (`src/router.tsx`):**

```tsx
// Before (current):
<Route element={<AuthGuard />}>
  <Route element={<ShellLayout />}>
    <Route path="/admin" element={<AdminPage />} />
    <Route path="/requests" element={<RequestsPage />} />
    ...
  </Route>
</Route>

// After:
<Route element={<AuthGuard />}>
  <Route element={<PermissionProvider />}>   {/* NEW wrapper */}
    <Route element={<ShellLayout />}>
      <Route element={<PermissionRoute anyOf={["adm.users", "adm.roles"]} />}>
        <Route path="/admin" element={<AdminPage />} />
      </Route>
      <Route element={<PermissionRoute permission="di.view" />}>
        <Route path="/requests" element={<RequestsPage />} />
      </Route>
      <Route element={<PermissionRoute permission="ot.view" />}>
        <Route path="/work-orders" element={<WorkOrdersPage />} />
      </Route>
      <Route element={<PermissionRoute permission="eq.view" />}>
        <Route path="/equipment" element={<EquipmentPage />} />
      </Route>
      {/* Routes without permission guard: dashboard, settings, profile */}
      <Route path="/dashboard" element={<DashboardPage />} />
      ...
    </Route>
  </Route>
</Route>
```

**Route-to-permission mapping:**

| Route | Permission Guard |
|-------|-----------------|
| `/admin` | `anyOf: ["adm.users", "adm.roles"]` |
| `/requests` | `di.view` |
| `/work-orders` | `ot.view` |
| `/equipment` | `eq.view` |
| `/organization` | `org.view` |
| `/personnel` | `per.view` |
| `/reference` | `ref.view` |
| `/planning` | `plan.view` |
| `/reports` | `rep.view` |
| `/settings` | `adm.settings` |
| `/dashboard` | *(none — always accessible)* |
| `/unauthorized` | *(none — always accessible)* |

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- cargo check passes with zero errors
- PermissionProvider loads permissions once on mount (verified via IPC call count)
- 10 PermissionGate components on a page result in 1 IPC call, not 10
- rbac-changed event triggers immediate permission refresh in frontend
- Permission cache hit rate > 90% on repeated check_permission calls for same user+scope
- Cache invalidation on role_assigned / role_revoked clears correct user entries
- Cache invalidation on update_role clears ALL entries
- PermissionRoute blocks navigation to /admin for user without adm.* permissions
- PermissionRoute renders UnauthorizedPage (not blank screen)
- Direct URL navigation to /requests without di.view shows UnauthorizedPage
- Dashboard route accessible to all authenticated users (no guard)
```

### Supervisor Verification — Sprint S4

**V1 — PermissionProvider deduplication.**
Open DevTools → Network/IPC tab. Navigate to RequestsPage with 8 PermissionGate components.
Verify only 1 `get_my_permissions` call (not 8).

**V2 — Event-driven refresh.**
Login as Alice (Technicien). In a separate admin session, add `adm.users` to Alice's role.
Backend emits `rbac-changed`. Alice's sidebar should show the Admin menu item within seconds
without manual page refresh.

**V3 — Cache performance.**
Add console timing around `check_permission()` calls. First call: ~2ms (DB hit). Subsequent
calls for same user+scope: < 0.1ms (cache hit).

**V4 — Route guard.**
Login as Technicien (no `adm.*`). Type `/admin` in address bar. Verify UnauthorizedPage
renders with lock icon and French text. Verify browser back button works.

**V5 — Route guard pass-through.**
Login as Administrateur. Navigate to `/admin`. Verify AdminPage loads normally.

---

*End of Phase 2 - Sub-phase 06 - File 02*
