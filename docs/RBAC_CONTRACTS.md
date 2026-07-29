# RBAC Contracts

Reference for the role-based access control model, permission naming, macros,
dangerous action guards, and frontend usage.

> **Canonical SSOT (2026-07):** Permission names, metadata, aliases, dependencies,
> and default role sets live in [`shared/rbac/permission-registry.json`](../shared/rbac/permission-registry.json).
> Generated constants: `crate::rbac::permissions::*` (Rust) and `P` / `PermissionName`
> from `@shared/rbac/permissions.generated` (TypeScript).
> **Never** use permission-name string literals in application code.
> Run `pnpm rbac:generate` after registry edits and `pnpm rbac:check` in CI.
> See [`.cursor/rules/rbac-permissions.mdc`](../.cursor/rules/rbac-permissions.mdc).

## Permission Naming Convention

All permissions use dot-notation: `domain.action` or `domain.action.scope`.

Rules:
- Domain prefix must match a registered catalog category (see the registry)
- Action suffix is lowercase alphanumeric (dots allowed only for scoped actions like `di.create.own`)
- Scope is optional and scopes to a specific resource sub-domain
- Examples: use `P.EQ_VIEW`, `P.OT_APPROVE`, `P.ADM_USERS` — never raw strings

## System Roles

| Role | Description | Can Be Deleted |
|------|-------------|---------------|
| Administrator | Full system access including admin operations | No |
| Supervisor | All operational access; no admin permissions | No |
| Operator | Create/edit day-to-day operational records; no delete or approval | No |
| Readonly | View-only access to all operational modules | No |

System roles are seeded on first launch. Tenants can clone them as role templates
and customize the clone; the originals remain unchanged.

## Permission Catalogue

The live catalogue is defined by the registry (100+ permissions including sync,
licensing, entitlements, integrity, and vendor-console catalog-only entries).
Do not maintain a parallel table here — open the registry / Permissions admin tab.

Historical note: older docs listed ~68 permissions and obsolete aliases such as
`inv.order` / `inv.adjust` / `di.edit`. Those aliases are migrated onto canonical
names by migration `m20260729_000137_rbac_permission_normalization`.

## Permission Catalogue (legacy snapshot — superseded)

The table below is retained only for archaeology. Prefer the registry.

| Domain | Prefix | Permissions |
|--------|--------|-------------|
| Equipment | `eq` | `eq.view`, `eq.manage`, `eq.import`, `eq.delete` |
| Intervention Requests | `di` | `di.view`, `di.create`, `di.edit`, `di.delete`, `di.close` |
| Work Orders | `ot` | `ot.view`, `ot.create`, `ot.edit`, `ot.delete`, `ot.close`, `ot.approve` |
| Organization | `org` | `org.view`, `org.manage` |
| Personnel | `per` | `per.view`, `per.manage`, `per.sensitiveview` |
| Reference Data | `ref` | `ref.view`, `ref.manage`, `ref.publish` |
| Inventory | `inv` | `inv.view`, `inv.manage`, `inv.adjust`, `inv.order` |
| Preventive Maintenance | `pm` | `pm.view`, `pm.manage`, `pm.approve` |
| RAMS / Reliability | `ram` | `ram.view`, `ram.manage` |
| Reports & Analytics | `rep` | `rep.view`, `rep.export`, `rep.manage` |
| Archive Explorer | `arc` | `arc.view`, `arc.export` |
| Documentation | `doc` | `doc.view`, `doc.manage` |
| Administration | `adm` | `adm.users`, `adm.roles`, `adm.permissions`, `adm.settings`, `adm.audit` |
| Planning | `plan` | `plan.view`, `plan.manage` |
| Audit Log | `log` | `log.view`, `log.export` |
| Training | `trn` | `trn.view`, `trn.manage`, `trn.certify` |
| IoT Integration | `iot` | `iot.view`, `iot.manage` |
| ERP Connector | `erp` | `erp.view`, `erp.manage`, `erp.sync` |
| Work Permits | `ptw` | `ptw.view`, `ptw.create`, `ptw.approve`, `ptw.cancel` |
| Budget / Finance | `fin` | `fin.view`, `fin.manage`, `fin.approve` |
| Inspection | `ins` | `ins.view`, `ins.manage`, `ins.complete` |
| Configuration Engine | `cfg` | `cfg.view`, `cfg.manage`, `cfg.publish` |

## Dangerous Actions

Permissions with `is_dangerous = 1` are highlighted in the UI and require a comment
when used by Supervisors or higher. Permissions with both `is_dangerous = 1` and
`requires_step_up = 1` require `verify_step_up` to be called before the action
proceeds. The step-up window is valid for **120 seconds**.

Dangerous permissions: `eq.delete`, `di.delete`, `ot.delete`, `ref.publish`,
`inv.adjust`, `adm.users`, `adm.roles`, `adm.permissions`, `log.export`,
`trn.certify`, `erp.manage`, `erp.sync`, `ptw.approve`, `ptw.cancel`,
`fin.approve`, `cfg.manage`, `cfg.publish`.

## Backend Macros

### require_session!

```rust
let user = require_session!(state);
// user: AuthenticatedUser { user_id, username, display_name, is_admin, ... }
```

Fails with `AppError::Auth` (code: `AUTH_ERROR`) if no active session.

### require_permission!

```rust
let user = require_session!(state);
require_permission!(state, &user, "eq.manage", PermissionScope::Global);
```

Takes 4 arguments: `(state, user, permission_name, scope)`.
Automatically checks the permission against the database AND verifies step-up
if the permission has `requires_step_up = 1`.
Fails with `AppError::PermissionDenied` (code: `PERMISSION_DENIED`) if not granted.
Fails with `AppError::StepUpRequired` (code: `STEP_UP_REQUIRED`) if step-up is needed.

### require_step_up!

```rust
let user = require_session!(state);
require_step_up!(state);
```

Standalone step-up check (when not using `require_permission!`).
Fails with `AppError::StepUpRequired` (code: `STEP_UP_REQUIRED`) if step-up
was not verified within 120 seconds.

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
  fallback={<p>Acces non autorise</p>}
>
  <UserManagementPanel />
</PermissionGate>
```

### Step-up verification (for dangerous actions)

```typescript
import { verifyStepUp } from "@/services/rbac-service";

try {
  const result = await verifyStepUp(password);
  // result.success === true, result.expires_at is the window end
  // Now call the dangerous IPC command within 120 seconds
} catch (err) {
  // err.code === "AUTH_ERROR" → wrong password
}
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

## Error Codes

| Code | Meaning | Frontend Action |
|------|---------|----------------|
| `AUTH_ERROR` | No active session or wrong password | Show login prompt |
| `PERMISSION_DENIED` | Session valid but permission not granted | Show "insufficient rights" message |
| `STEP_UP_REQUIRED` | Permission granted but step-up not verified | Show password re-entry modal |

## File Index

| File | Purpose |
|------|---------|
| `src-tauri/src/auth/rbac.rs` | Permission check engine, user permission loading |
| `src-tauri/src/auth/mod.rs` | `require_permission!` and `require_step_up!` macros |
| `src-tauri/src/auth/session_manager.rs` | Step-up state on `LocalSession` |
| `src-tauri/src/db/seeder.rs` | Permission and role seed data |
| `src-tauri/src/commands/rbac.rs` | `get_my_permissions`, `verify_step_up` IPC commands |
| `shared/ipc-types.ts` | `PermissionRecord`, `StepUpRequest`, `StepUpResponse` |
| `src/services/rbac-service.ts` | Frontend IPC wrappers with Zod validation |
| `src/hooks/use-permissions.ts` | `usePermissions()` hook with `can()` and `refresh()` |
| `src/components/PermissionGate.tsx` | Declarative permission guard component |
