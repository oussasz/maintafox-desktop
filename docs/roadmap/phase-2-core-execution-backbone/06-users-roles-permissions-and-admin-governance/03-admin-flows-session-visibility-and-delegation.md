# Phase 2 - Sub-phase 06 - File 03
# Admin Flows, Session Visibility, and Delegation

## Context and Purpose

Files 01 and 02 define the structural schema (roles, scope assignments, permission catalog)
and the enforcement runtime (scope chain, dependency validation, resolver). File 03
operationalizes the four harder admin workflows that separate a basic RBAC system from a
mature governance layer:

1. **Session and device visibility** — administrators can see who is logged in, on which
   device, and under which scope assignments. Security events (failed step-up, device
   revocation, emergency access use) are visible in real time.
2. **Delegated administration** — a site manager can be granted the ability to manage users
   within their scope without becoming a full system administrator. The delegation boundary
   (domains allowed, scope allowed, step-up required to publish) is explicitly governed.
3. **Emergency elevation** — time-boxed role grants with expiry, reason, approver capture,
   and automatic expiry enforcement. Used when a maintenance supervisor needs a temporary
   elevated permission during an incident without a permanent role change.
4. **Role and user import/export** — multi-site parity without bypassing validation. A role
   model exported from Site A can be reviewed and imported to Site B, but all dependency
   checks and scope rules run on import.

---

## PRD Alignment Checklist

- [x] Session visibility: administrator can see active sessions, device trust state, and
      last-seen timestamps (PRD §6.7 "Access Simulation & Auditability")
- [x] Delegated admin policies governance (PRD §6.7 "Delegated Administration & Emergency Access")
- [x] Time-boxed emergency elevation with expiry (PRD §6.7 explicit)
- [x] Reason, expiry, approver captured on emergency grants (PRD §6.7 explicit)
- [x] Role and permission changes previewed before activation (PRD §6.7 "Access Simulation")
- [x] Changes recorded as dangerous admin events in 6.17 audit history (PRD §6.7 explicit)
- [x] Export and import of role models (PRD §6.7 explicit)

---

## Architecture Rules Applied

- **`delegated_admin_policies` governs what a delegated admin CAN manage, not who.** A
  delegated admin is still a user holding an `adm.users` or `adm.roles` permission — but
  scoped. The `delegated_admin_policies` table further restricts what domains they may assign
  and whether publishing requires step-up.
- **Emergency elevation is an augmented `user_scope_assignments` row.** The `is_emergency=1`
  flag and `emergency_expires_at` column already exist from migration 025. What this file
  adds is the enforcement: expired emergency grants are excluded by the resolver
  (resolver already checks `emergency_expires_at > datetime('now')`). This file adds
  the grant workflow, revocation, and expiry notification.
- **Preview before activation is a frontend responsibility.** The `simulate_access` command
  (built in F01) is the mechanism. The admin UI calls simulate_access with the proposed
  role change applied hypothetically (not yet saved) before commit. The UI must show the
  diff: permissions that would be added, removed, or scope-changed.
- **Import validation runs the full dependency check.** `import_role_model` calls
  `validate_role_permissions` on every imported role before inserting into the DB.
  Any role with missing hard dependencies blocks the import for that role with a clear error.
- **Dangerous admin events feed SP07.** All `adm.*` commands that mutate users, roles, or
  emergency grants must write to `admin_change_events` (migration 027). SP07 picks these up
  for the Activity Feed and Immutable Audit Journal.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260701_000027_admin_change_events.rs` | Immutable admin event ledger + delegated admin policy enhancements |
| `src-tauri/src/commands/admin_governance.rs` | IPC commands: session visibility, delegation, emergency elevation, import/export |
| `src-tauri/src/rbac/delegation.rs` | `can_delegate_permission`, `validate_delegation_boundary` |
| `src/components/admin/SessionVisibilityPanel.tsx` | Live session viewer for administrators |
| `src/components/admin/DelegationManagerPanel.tsx` | Delegated admin policy definition UI |
| `src/components/admin/EmergencyElevationPanel.tsx` | Emergency grant creation and active-grant monitoring |
| `src/components/admin/RoleImportExportPanel.tsx` | Role model export/import with validation preview |

---

## Migration 027 — Admin Change Events

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the admin change event ledger migration.

STEP 1 - CREATE src-tauri/migrations/m20260701_000027_admin_change_events.rs

CREATE TABLE IF NOT EXISTS admin_change_events (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  action          TEXT NOT NULL,
    -- user_created / user_deactivated / role_assigned / role_revoked /
    --  role_created / role_updated / role_deleted / role_retired /
    --  permission_granted / permission_revoked /
    --  emergency_grant_created / emergency_grant_expired / emergency_grant_revoked /
    --  delegation_policy_created / delegation_policy_updated / delegation_policy_deleted /
    --  role_imported / role_exported / access_simulation_run
  actor_id        INTEGER NULL REFERENCES user_accounts(id),
  target_user_id  INTEGER NULL REFERENCES user_accounts(id),
  target_role_id  INTEGER NULL REFERENCES roles(id),
  acted_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  scope_type      TEXT NULL,
  scope_reference TEXT NULL,
  summary         TEXT NULL,
  diff_json       TEXT NULL,
    -- for role changes: { "added": [...], "removed": [...] }
  step_up_used    INTEGER NOT NULL DEFAULT 0,
  ip_address      TEXT NULL,
  apply_result    TEXT NOT NULL DEFAULT 'applied'
    -- applied / blocked
);

CREATE INDEX IF NOT EXISTS idx_ace_action  ON admin_change_events(action);
CREATE INDEX IF NOT EXISTS idx_ace_actor   ON admin_change_events(actor_id);
CREATE INDEX IF NOT EXISTS idx_ace_target  ON admin_change_events(target_user_id);
CREATE INDEX IF NOT EXISTS idx_ace_acted   ON admin_change_events(acted_at);

ACCEPTANCE CRITERIA
- Migration 027 applies; admin_change_events table present
- cargo check passes
```

---

## Delegation and Session Governance Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement admin governance commands.

STEP 1 - CREATE src-tauri/src/rbac/delegation.rs

Function:
`async fn can_delegate_permission(
    pool: &SqlitePool,
    delegator_user_id: i64,
    target_user_id: i64,
    permission_name: &str,
    target_scope_type: &str,
    target_scope_reference: Option<&str>,
) -> Result<bool>`

Algorithm:
1. Look up all delegated_admin_policies where admin_role_id is held by the delegator.
2. Check that target_scope_type and target_scope_reference match policy.managed_scope_type
   and policy.managed_scope_reference (or NULL = unrestricted within type).
3. Check that permission_name's domain (prefix up to first '.') is in the policy's
   allowed_domains_json array.
4. Return true only if all three match; otherwise false.

Function:
`fn validate_delegation_boundary(
    policy: &DelegatedAdminPolicy,
    permission_name: &str,
) -> bool`
  Returns true if permission domain is in allowed_domains_json.


STEP 2 - CREATE src-tauri/src/commands/admin_governance.rs

--- SESSION VISIBILITY ---

list_active_sessions
  Permission: adm.users (tenant scope)
  Output: Vec<SessionSummary> { user_id, username, device_id, device_name, device_trust_status,
    session_started_at, last_activity_at, is_current_session, current_role_names: Vec<String> }
  Query: JOIN user_sessions (from migration 006 circa Phase 1) + user_scope_assignments +
    roles. Filter: sessions where expires_at > now AND is_revoked = 0.
  Note: is_current_session = true when the logged-in user calling this command has the same
    session_id as the row.

revoke_session
  Permission: adm.users + requires_step_up
  Input: session_id: i64
  Guard: cannot revoke your own current session
  Sets is_revoked = 1 in user_sessions; writes admin_change_event action='session_revoked'

--- DELEGATION MANAGEMENT ---

list_delegation_policies
  Permission: adm.roles (tenant scope)
  Output: Vec<DelegatedAdminPolicy> with admin role name resolved

create_delegation_policy
  Permission: adm.roles + requires_step_up
  Input: CreateDelegationInput {
    admin_role_id, managed_scope_type, managed_scope_reference?,
    allowed_domains: Vec<String>,   -- validated against known categories
    requires_step_up_for_publish: bool
  }
  Validation:
    - admin_role_id must be a non-system role (is_system=0) or explicit admin role
    - allowed_domains must be subset of known permission category names
    - managed_scope_reference must exist in org_units if scope_type != 'tenant'
  Inserts into delegated_admin_policies
  Writes admin_change_event action='delegation_policy_created'

update_delegation_policy
  Permission: adm.roles + requires_step_up
  Input: UpdateDelegationInput { policy_id, allowed_domains?, requires_step_up_for_publish? }
  Writes admin_change_event action='delegation_policy_updated' with diff_json

delete_delegation_policy
  Permission: adm.roles + requires_step_up
  Input: policy_id: i64
  Writes admin_change_event action='delegation_policy_deleted'
  Deletes row.

--- EMERGENCY ELEVATION ---

grant_emergency_elevation
  Permission: adm.users + requires_step_up
  Input: EmergencyGrantInput {
    user_id, role_id, scope_type, scope_reference?,
    emergency_reason: String,   -- required; stored in user_scope_assignments
    expires_in_minutes: i64     -- 1 <= expires_in_minutes <= 480 (8h max)
  }
  Inserts user_scope_assignments row with:
    is_emergency = 1
    emergency_reason = input
    emergency_expires_at = datetime('now', '+N minutes')
    granted_by_id = actor_id
  Writes admin_change_event action='emergency_grant_created'

revoke_emergency_elevation
  Permission: adm.users + requires_step_up
  Input: assignment_id: i64
  Guard: target assignment must have is_emergency = 1
  Deletes or sets emergency_expires_at = datetime('now') (expire immediately)
  Writes admin_change_event action='emergency_grant_revoked'

list_emergency_grants
  Permission: adm.users
  Output: active emergency scope assignment rows with username, role name, scope, reason,
    expiry, granted_by username

--- ROLE IMPORT/EXPORT ---

export_role_model
  Permission: adm.roles (tenant scope)
  Input: role_ids: Vec<i64>
  Output: RoleExportPayload { roles: Vec<RoleExportEntry>, exported_at, exported_by }
  RoleExportEntry { id, name, description, permissions: Vec<String>, is_system }
  Write admin_change_event action='role_exported'

import_role_model
  Permission: adm.roles + requires_step_up
  Input: RoleImportPayload { roles: Vec<RoleImportEntry> }
  RoleImportEntry { name, description, permissions: Vec<String> }
  For each imported role:
    - Call validate_role_permissions on the permission list
    - If any has missing hard deps: add to errors, skip that role
    - Otherwise: insert into roles (role_type='custom', is_system=0) + role_permissions;
      write admin_change_event action='role_imported' with diff_json = permissions list
  Output: ImportResult { imported_count, skipped: Vec<(name, Vec<String>)> }

REGISTER all commands in Tauri invoke_handler.

ACCEPTANCE CRITERIA
- cargo check passes
- emergency grant expires after expiry time (resolver excludes it after emergency_expires_at)
- revoke_emergency_elevation immediately excludes the grant from resolver checks
- import_role_model with a role missing hard deps: that role is skipped, not entire import
- export → import round-trip preserves permission names accurately
```

---

## Admin Governance Frontend

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the admin governance panels.

CREATE src/components/admin/SessionVisibilityPanel.tsx

- Table: username, device name, device trust status (badge), session start, last activity.
  Current session row highlighted.
- "Revoke" button per row (disabled for current session); triggers revoke_session confirm modal.
- Auto-refresh every 30 seconds.
- Requires adm.users permission.

CREATE src/components/admin/DelegationManagerPanel.tsx

- List of delegation policies: admin role name, managed scope, allowed domains (chip list),
  step-up required badge.
- "Create Policy" button (adm.roles guard) → modal with:
  - admin role selector (non-system roles only)
  - scope type + scope picker (org_unit selector if not 'tenant')
  - domain multi-select (list of all known categories from permissions table)
  - toggle: requires step-up to publish
- Edit / Delete buttons per row with confirmation.

CREATE src/components/admin/EmergencyElevationPanel.tsx

- Active emergency grants table: user, role, scope, reason, expires_at, granted_by, countdown.
- "Grant Emergency Access" button (adm.users guard) → modal prompts:
  user picker, role picker, scope picker, reason (required, min 20 chars), expiry minutes (1-480)
- "Revoke" button per active grant row.
- Expired grants shown in greyed-out state with "Expired" badge.
- Countdown: live UI countdown (JavaScript interval) showing time remaining.

CREATE src/components/admin/RoleImportExportPanel.tsx

- Export section: multi-select from role list → "Export JSON" downloads role model as JSON.
- Import section: file picker for JSON → parses payload → Preview table showing each role and
  its permissions, with hard-dep warnings highlighted → "Confirm Import" button calls
  import_role_model → shows ImportResult (imported_count + skipped list).

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- SessionVisibilityPanel auto-refresh without blinking; current session row highlighted
- EmergencyElevationPanel countdown closes grant row when expiry passes without page reload
- RoleImportExportPanel shows hard-dep warnings before confirm import; user can see which
  roles will be skipped before committing
```

---

## SP06-F03 Completion Checklist

- [ ] Migration 027 applies; admin_change_events table with all columns present
- [ ] `list_active_sessions` returns sessions with device trust status
- [ ] `grant_emergency_elevation` inserts with expiry, writes admin_change_event
- [ ] `revoke_emergency_elevation` immediately drops grant from resolver
- [ ] Emergency grant expiry enforced in resolver (field checked at query time)
- [ ] `create_delegation_policy` validates allowed_domains against known category list
- [ ] `import_role_model` partial success: valid roles imported, invalid skipped
- [ ] `export_role_model` output can be reimported cleanly
- [ ] All adm.* mutation commands write to admin_change_events (feeds SP07 Activity Feed)
- [ ] SessionVisibilityPanel, DelegationManagerPanel, EmergencyElevationPanel, RoleImportExportPanel all type-check

---

## Sprint S4 — Account Lockout, StepUpDialog, and Online Presence

> **Gaps addressed:** (1) `user_accounts` has `failed_login_attempts` and `locked_until`
> columns since migration 002 but no enforcement logic exists — this is an OWASP requirement.
> (2) `verifyStepUp` IPC command and `rbac-service.ts` wrapper exist, but no reusable UI
> dialog triggers them — every dangerous action would need to independently build a
> re-authentication prompt. (3) The web tracks online users with a heartbeat; the desktop
> has `SessionVisibilityPanel` for admin view but no lightweight presence indicator for
> general UI (e.g., green dot on user avatars in assignment dropdowns). This sprint closes
> all three gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/auth/lockout.rs` | Account lockout enforcement logic |
| `src-tauri/src/commands/auth.rs` (patch) | Integrate lockout into `login` command |
| `src-tauri/src/rbac/cache.rs` (patch) | `rbac_settings` cache for lockout config |
| `src/components/auth/StepUpDialog.tsx` | Reusable step-up re-authentication dialog |
| `src/hooks/use-step-up.ts` | Hook wrapping StepUpDialog for action execution |
| `src/components/admin/OnlinePresenceIndicator.tsx` | Green/gray dot component |
| `src-tauri/src/commands/admin_users.rs` (patch) | `unlock_user_account` command |
| `src/i18n/locale-data/{fr,en}/auth.json` (patch) | Lockout and step-up dialog labels |

### GAP-03 — Account Lockout Enforcement

**Problem:** The `login` command in `auth.rs` validates password but never increments
`failed_login_attempts` or checks `locked_until`. An attacker can brute-force passwords
indefinitely.

**New file: `src-tauri/src/auth/lockout.rs`**

```rust
pub struct LockoutPolicy {
    pub max_attempts: i32,         // default: 5
    pub lockout_minutes: i64,      // default: 15
    pub progressive: bool,         // default: true — doubles lockout on repeated lockouts
}

impl LockoutPolicy {
    /// Load from rbac_settings table with fallback defaults
    pub async fn load(pool: &SqlitePool) -> Self { ... }
}

/// Check if the account is currently locked.
/// Returns Ok(()) if unlocked, Err(AppError::AccountLocked { until }) if locked.
pub async fn check_lockout(
    pool: &SqlitePool,
    user_id: i64,
) -> AppResult<()> {
    // SELECT locked_until FROM user_accounts WHERE id = ?
    // If locked_until IS NOT NULL AND locked_until > datetime('now') → Err
    // If locked_until IS NOT NULL AND locked_until <= datetime('now') → auto-unlock:
    //   UPDATE user_accounts SET locked_until = NULL, failed_login_attempts = 0
}

/// Record a failed login attempt. Locks account if threshold exceeded.
pub async fn record_failed_attempt(
    pool: &SqlitePool,
    user_id: i64,
    policy: &LockoutPolicy,
) -> AppResult<()> {
    // UPDATE user_accounts SET failed_login_attempts = failed_login_attempts + 1
    // If new count >= max_attempts:
    //   Calculate lockout_duration:
    //     base = lockout_minutes
    //     if progressive: multiply by 2^(consecutive_lockouts - 1), cap at 24h
    //   UPDATE SET locked_until = datetime('now', '+N minutes')
    //   Write audit event: action='account_locked', summary includes attempt count
}

/// Reset failed attempts on successful login.
pub async fn reset_attempts(pool: &SqlitePool, user_id: i64) -> AppResult<()> {
    // UPDATE user_accounts SET failed_login_attempts = 0, locked_until = NULL
}
```

**Patch to `src-tauri/src/commands/auth.rs` — `login` command:**

```rust
// BEFORE password verification:
let user = find_user_by_username(pool, &input.username).await?;
check_lockout(pool, user.id).await?;  // NEW — returns AccountLocked error if locked

// AFTER password verification failure:
record_failed_attempt(pool, user.id, &policy).await?;  // NEW

// AFTER password verification success:
reset_attempts(pool, user.id).await?;  // NEW
```

**Frontend handling:**
The `LoginPage.tsx` already handles `AppError` variants. Add handling for `AccountLocked`:
- Show message: "Compte verrouillé. Réessayez dans X minutes." / "Account locked. Try again in X minutes."
- Show `locked_until` timestamp.
- No countdown timer (to avoid giving attackers timing information — security best practice).

**`rbac_settings` entries (add to migration 028 seed or new migration):**

```sql
INSERT OR IGNORE INTO rbac_settings (key, value, description) VALUES
  ('lockout_max_attempts', '5', 'Failed login attempts before account lockout'),
  ('lockout_base_minutes', '15', 'Base lockout duration in minutes'),
  ('lockout_progressive', '1', '1 = double lockout on repeated lockouts, capped at 24h');
```

**Admin unlock command:**

```rust
// src-tauri/src/commands/admin_users.rs — patch

#[tauri::command]
pub async fn unlock_user_account(
    state: tauri::State<'_, AppState>,
    user_id: i64,
) -> AppResult<()> {
    let admin = require_session!(state);
    require_permission!(state, &admin, "adm.users", PermissionScope::Global);
    require_step_up!(state);
    
    // UPDATE user_accounts SET failed_login_attempts = 0, locked_until = NULL
    // Write admin_change_event: action='account_unlocked'
}
```

Exposed in `UserListPanel` as a "Déverrouiller / Unlock" button on locked user rows.

### GAP-04 — StepUpDialog.tsx — Reusable Re-Authentication

**Problem:** Dangerous actions (role mutation, emergency elevation, user deactivation) require
step-up but there's no shared UI component. Without it, each panel would need to independently
build a password prompt + retry loop.

**Component: `src/components/auth/StepUpDialog.tsx`**

```typescript
interface StepUpDialogProps {
  open: boolean;
  onVerified: () => void;      // called after successful step-up
  onCancel: () => void;
  title?: string;              // e.g., "Confirm role deletion"
  description?: string;        // e.g., "This action requires re-authentication"
}
```

**Layout (Radix Dialog):**
```
┌──────────────────────────────────────────┐
│  🔒 Re-authentication Required           │
│                                          │
│  {title}                                 │
│  {description}                           │
│                                          │
│  ┌──────────────────────────────────┐    │
│  │  Password: ••••••••              │    │
│  └──────────────────────────────────┘    │
│                                          │
│  ⚠️ Step-up window: 120 seconds          │
│                                          │
│  [Cancel]                  [Verify]      │
└──────────────────────────────────────────┘
```

**Behaviour:**
1. Renders only when `open = true`.
2. Password field auto-focused.
3. On submit → calls `verifyStepUp({ password })` via rbac-service.
4. On success → calls `onVerified()`, dialog closes.
5. On failure → shows inline error "Mot de passe incorrect / Incorrect password".
6. After 3 failed attempts in the dialog → shows warning + disables for 30 seconds.
7. Enter key submits; Escape cancels.

**Hook: `src/hooks/use-step-up.ts`**

```typescript
interface UseStepUpReturn {
  /** Wraps an async action that needs step-up. Shows dialog if step-up not fresh. */
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
  /** The dialog element to render (place once in page layout) */
  StepUpDialogElement: React.ReactElement;
}

function useStepUp(): UseStepUpReturn {
  // 1. Try the action directly.
  // 2. If backend returns StepUpRequired error → open StepUpDialog.
  // 3. On verification → retry the action.
  // 4. Return the action's result.
}
```

**Usage pattern (in any panel):**

```tsx
function EmergencyElevationPanel() {
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const handleGrant = async () => {
    await withStepUp(() => grantEmergencyElevation(input));
    // Only reaches here if step-up succeeded and action completed
  };

  return (
    <>
      <Button onClick={handleGrant}>Grant Emergency Access</Button>
      {StepUpDialogElement}
    </>
  );
}
```

### GAP-10 — Online Presence Indicator

**Problem:** The web shows a live "online" dot + "active since" label for each user.
The desktop `SessionVisibilityPanel` shows sessions for admins, but no lightweight presence
indicator exists for general UI contexts (e.g., showing who is currently logged in when
assigning an OT to a technician).

**Component: `src/components/admin/OnlinePresenceIndicator.tsx`**

```typescript
interface OnlinePresenceIndicatorProps {
  userId: number;
  size?: 'sm' | 'md';  // default 'sm'
}
```

**Rendering:**
- **Active** (session exists, `last_activity_at` within 5 minutes): Green dot (`bg-emerald-500`)
- **Idle** (session exists, `last_activity_at` > 5 min but session not expired): Amber dot
- **Offline** (no active session, or session expired): Gray dot (`bg-gray-300`)

**Data source:** `list_active_sessions` is admin-only. For non-admin contexts, add a
lightweight command:

```rust
// src-tauri/src/commands/admin_users.rs — patch

#[tauri::command]
pub async fn get_user_presence(
    state: tauri::State<'_, AppState>,
    user_ids: Vec<i64>,
) -> AppResult<Vec<UserPresence>> {
    let _user = require_session!(state);
    // No specific permission required — any authenticated user can see presence
    // (presence is not sensitive; it's equivalent to seeing someone in an office)
    
    // Query: SELECT user_id, last_activity_at FROM app_sessions
    //   WHERE user_id IN (?) AND is_revoked = 0 AND expires_at > datetime('now')
    // Return: Vec<UserPresence { user_id, status: "active"|"idle"|"offline", last_activity_at }>
}
```

**Caching:** Presence data cached in a Zustand atom for 30 seconds. Batch-fetch for all
visible user IDs in a single IPC call.

**Usage contexts:**
- `UserListPanel` — presence dot next to each username
- OT intervener assignment dropdown — show who is currently active
- DI assignment dropdown — show reviewer availability
- `SessionVisibilityPanel` — already has full session data (no change needed)

### Acceptance Criteria

```
- cargo check passes with zero errors
- pnpm typecheck passes with zero errors
- Login with wrong password 5 times → account locked, error message shows lockout
- Login with correct password after lockout expires → succeeds, counter reset
- Progressive lockout: 2nd lockout = 30min, 3rd = 60min (capped at 24h)
- Admin can unlock account via UserListPanel "Unlock" button
- StepUpDialog opens on dangerous action; password verification works
- StepUpDialog closes after successful verification; original action completes
- StepUpDialog shows inline error on wrong password
- useStepUp hook integrates transparently — action code doesn't know about step-up
- OnlinePresenceIndicator shows green/amber/gray dots correctly
- get_user_presence batch-fetches for multiple user IDs in one call
- Presence dot updates within 30 seconds of user activity change
```

### Supervisor Verification — Sprint S4

**V1 — Account lockout.**
Attempt 5 wrong passwords for user "technicien". Verify 6th attempt shows
"Compte verrouillé" with time remaining. Wait 15 minutes (or admin unlock) → login succeeds.

**V2 — Progressive lockout.**
Lock account again after first lockout clears. Verify second lockout duration = 30 minutes.

**V3 — Admin unlock.**
Login as admin. Open Users tab. Find locked user. Click "Déverrouiller". Verify
`failed_login_attempts = 0` and `locked_until = NULL`. User can login immediately.

**V4 — StepUpDialog.**
Login as admin. Go to Roles tab. Click "Delete" on a custom role. Verify StepUpDialog
appears. Enter wrong password → inline error. Enter correct password → role deleted.

**V5 — Step-up window.**
Perform a dangerous action (triggers step-up dialog, enter correct password).
Immediately perform another dangerous action within 120 seconds. Verify no second
step-up prompt (window is still fresh).

**V6 — Presence indicator.**
Login as admin and technician on same machine (sequential sessions). Open UserListPanel.
Verify admin shows green dot. Logout admin. Verify admin dot turns gray within 30 seconds.

---

*End of Phase 2 - Sub-phase 06 - File 03*
