# Phase 2 - Sub-phase 06 - File 04
# Governance Testing and Security Audit Controls

## Context and Purpose

Files 01 through 03 delivered the complete RBAC structural and operational layer: scoped role
assignments, the permission catalog with 70+ rows, scope chain resolution, dependency
validation, emergency elevation, delegation governance, session management, and import/export.

File 04 closes sub-phase 06 with two hardening layers:

1. **Security audit controls** — migration 028 patches the `admin_change_events` table to
   ensure all critical admin mutations write permanent change records. A `list_admin_events`
   command provides supervisors and auditors a filterable view into every user/role/permission
   change, emergency grant, and session revocation, with diff JSON visible to adm.permissions
   holders.
2. **RBAC test suite** — 15 tests covering scope resolution, dependency enforcement, dangerous
   permission step-up, delegation boundary validation, emergency grant lifecycle, import/export
   round-trips, and the full user lifecycle from creation to deactivation.

---

## PRD Alignment Checklist

- [x] Role and permission changes previewed before activation and recorded as dangerous admin
      events in 6.17 audit history with actor, scope, diff summary, step-up reauthentication flag
      (PRD §6.7 "Access Simulation & Auditability")
- [x] `adm.users` / `adm.roles` / `adm.permissions` permission domains explicitly confirmed
      in the catalog (PRD §6.7 "Permissions" line item)
- [x] Time-boxed emergency elevation with reason, expiry, approver captured (PRD §6.7)
- [x] Entire module aligned with step-up reauthentication from PRD §6.1 and §6.18

---

## Architecture Rules Applied

- **Tests run against in-memory SQLite seeded by all migrations 001–027.** Rust tests use
  `sqlx::SqlitePool::connect(":memory:")` and apply all migrations in order. This makes the
  test suite a living integration check on migration chain correctness.
- **`admin_change_events` is the 6.17 data feed for admin operations.** SP07 (Notifications,
  Archive & Audit Visibility) will consume these rows as part of the immutable audit journal.
  This file must confirm the feed format is stable and tested.
- **RBAC module tests are gate-gated — they must all pass before SP06 is marked complete.**
  The test suite is deliberately broad enough to catch scope resolution bugs that would only
  appear in multi-entity deployments.
- **No mutation in list_admin_events.** This command is read-only. No purge, edit, or delete
  commands exist for admin_change_events. Audit records are immutable by design.

---

## Carry-over Gap Closure From File 03 Verification

The Supervisor V1-V6 runtime verification for File 03 exposed implementation gaps that must be
treated as part of File 04 hardening scope (especially frontend/runtime contracts):

1. **Tauri IPC argument casing contract (frontend):**
   Several `rbac-service.ts` wrappers used snake_case payload keys for direct command arguments.
   Tauri command argument binding expects camelCase keys at the JS boundary (for example `roleId`),
   causing runtime errors such as "missing required key" and silent panel failures.

2. **Role retirement visibility mismatch (backend + frontend behavior):**
   `delete_role` performs a soft-retire (`status='retired'`), but role listing/detail queries were
   still returning retired roles, making deletion appear unsuccessful in UI.

3. **Silent detail-load failures in Role editor (frontend):**
   Detail retrieval failures were swallowed in the panel flow, creating a dead UI state without
   actionable feedback.

### Mandatory Hardening Actions

- Ensure all direct invoke payload keys in `rbac-service.ts` use camelCase argument names:
  `userId`, `assignmentId`, `policyId`, `roleIds`, `sessionId`, `userIds`, etc.
- Treat reed roles as non-active in command queries used by list/detail/mutation paths:
  include `status != 'retired'` where active roles are expected.
- Keep role editor error-surfacing explicit: failed `get_role` calls must show a user-visible error.

### Additional Acceptance Criteria (Carry-over)

- Clicking any role in the role list always loads detail or displays an explicit error toast
  (never silent failure).
- Deleting a custom role removes it from visible active-role lists immediately after refresh.
- No frontend runtime error of form "command <x> missing required key <y>" for
  `get_role`, `delete_role`, `get_user`, `deactivate_user`, `revoke_role_scope`,
  `unlock_user_account`, `revoke_session`, `delete_delegation_policy`, `export_role_model`,
  `get_user_presence`.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260801_000028_rbac_hardening.rs` | Add `rbac_setting` table (configurable RBAC policy knobs); patch roles for graceful deny-all fallback |
| `src-tauri/src/commands/admin_audit.rs` | IPC commands: `list_admin_events`, `get_admin_event` |
| `src-tauri/src/rbac/tests.rs` | 15-test RBAC and governance test suite |
| `src/components/admin/AdminAuditTimeline.tsx` | Read-only admin event timeline for governance oversight |

---
tir
## Migration 028 — RBAC Hardening

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the final RBAC hardening migration.

STEP 1 - CREATE src-tauri/migrations/m20260801_000028_rbac_hardening.rs

CREATE TABLE IF NOT EXISTS rbac_settings (
  key             TEXT PRIMARY KEY,
  value           TEXT NOT NULL,
  description     TEXT NULL,
  is_sensitive    INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO rbac_settings (key, value, description) VALUES
  ('emergency_max_minutes',     '480',  'Maximum minutes for emergency elevation grants (default 8h)'),
  ('session_idle_timeout_min',  '60',   'Minutes of inactivity before session is considered idle'),
  ('require_step_up_on_role_change', '1', '1 = all role mutations require step-up reauthentication'),
  ('admin_event_retention_days', '730', 'Days to retain admin_change_events (min 365 for compliance)');

-- Ensure roles table has a 'deny_all_fallback' flag for graceful degradation:
-- When a user has NO active scope assignments, behavior is deny-all by default.
-- This is enforced in the resolver; this setting documents the policy.
INSERT OR IGNORE INTO rbac_settings (key, value, description) VALUES
  ('deny_all_fallback', '1', '1 = users with no active scope assignments get no permissions');

ACCEPTANCE CRITERIA
- Migration 028 applies; rbac_settings table present with 5 rows
- cargo check passes
```

---

## Admin Audit Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement admin audit read commands.

STEP 1 - CREATE src-tauri/src/commands/admin_audit.rs

Types:
AdminEventFilter { action?, actor_id?, target_user_id?, target_role_id?,
  date_from?, date_to?, apply_result?, limit, offset }

AdminChangeEventDetail (full row with actor username and target user/role names resolved)

Commands:

list_admin_events
  Permission: adm.users (to view user events) OR adm.roles (to view role events)
    If caller holds adm.permissions: all events visible
    If caller holds only adm.users: only events where action in
      (user_created, user_deactivated, role_assigned, role_revoked, session_revoked,
       emergency_grant_created, emergency_grant_revoked, emergency_grant_expired)
    If caller holds only adm.roles: only events where action in
      (role_created, role_updated, role_deleted, role_retired, role_imported, role_exported,
       permission_granted, permission_revoked, delegation_policy_created,
       delegation_policy_updated, delegation_policy_deleted)
  Input: AdminEventFilter
  Output: Vec<AdminChangeEventDetail>

get_admin_event
  Permission: adm.users OR adm.roles (same scope filter as list_admin_events)
  Input: event_id: i64
  Output: AdminChangeEventDetail with full diff_json

REGISTER in invoke_handler.

ACCEPTANCE CRITERIA
- cargo check passes
- list_admin_events with adm.users-only caller does not return role-related events
- get_admin_event returns full diff_json for role_updated events
```

---

## RBAC Full Test Suite

### AI Agent Prompt

```text
You are a senior Rust test engineer. Write the full RBAC governance test suite.

CREATE src-tauri/src/rbac/tests.rs

Use #[cfg(test)] with tokio::test. In-memory SQLite, all migrations applied.

--- SCOPE RESOLUTION ---

test_rbac_01_tenant_scope_grants_all_entities
  Create user with Supervisor role assigned at scope_type='tenant'.
  Call effective_permissions with org_node_id=Some(entity_leaf_id).
  Assert: ot.view, ot.edit, di.view, di.approve all in result set.

test_rbac_02_entity_scope_isolated
  Create user with Technician role at scope_type='org_node', scope_reference='entity_A_id'.
  Call effective_permissions with org_node_id=Some('entity_A_id').
  Assert: ot.view in result.
  Call effective_permissions with org_node_id=Some('entity_B_id').
  Assert: ot.view NOT in result.

test_rbac_03_no_assignments_deny_all
  Create user with zero scope assignments.
  Call effective_permissions with any org_node_id.
  Assert: result set is empty.

test_rbac_04_expired_scope_not_included
  Create scope assignment with valid_to = yesterday's date (yesterday in ISO-8601).
  Call effective_permissions.
  Assert: permissions from that assignment are NOT in result set.

test_rbac_05_future_scope_not_yet_active
  Create scope assignment with valid_from = tomorrow's date.
  Call effective_permissions.
  Assert: permissions from that assignment are NOT in result set.

--- DEPENDENCY ENFORCEMENT ---

test_rbac_06_hard_dep_blocks_role_creation
  Call create_role with permissions=['ot.close'] (missing ot.edit and ot.view as hard deps).
  Assert: returns error; error message lists 'ot.edit' and 'ot.view' as missing dependencies.

test_rbac_07_warn_dep_allows_role_creation_with_warning
  Call create_role with permissions=['ot.reopen', 'ot.close', 'ot.edit', 'ot.view'].
  (ot.reopen → ot.close is warn; all hard deps satisfied)
  Assert: succeeds;
  Then call validate_role_permissions(same_list).
  Assert warn_deps contains the ot.reopen→ot.close pair.

--- DANGEROUS PERMISSIONS ---

test_rbac_08_step_up_required_permission_check
  Fetch permission row for 'ot.close'.
  Assert: requires_step_up = 1.
  Fetch 'ot.view'.
  Assert: requires_step_up = 0.

test_rbac_09_custom_permission_cannot_set_dangerous
  Call create_custom_permission with name='cst.something', category='cst'.
  Fetch the created row.
  Assert: is_dangerous = 0, requires_step_up = 0 (enforced by command, not by caller input).

test_rbac_10_system_namespace_blocked
  Call create_custom_permission with name='ot.my_override'.
  Assert: returns error (system namespace 'ot.' disallowed for custom permissions).

--- EMERGENCY ELEVATION ---

test_rbac_11_emergency_grant_included_before_expiry
  Grant emergency elevation with expires_in_minutes=5.
  Immediately call effective_permissions.
  Assert: permissions from the emergency role ARE in result set.

test_rbac_12_emergency_grant_excluded_after_expiry
  Grant emergency elevation with expires_in_minutes=0 (already expired) or manipulate
  emergency_expires_at to a past timestamp.
  Call effective_permissions.
  Assert: permissions from emergency role are NOT in result set.

--- DELEGATION ---

test_rbac_13_delegation_boundary_enforced
  Create delegation policy: admin_role=SiteAdmin, allowed_domains=['ot','di'], scope=entity_A.
  User Bob is SiteAdmin at entity_A.
  Call can_delegate_permission(Bob, target, 'ot.edit', 'org_node', 'entity_A_id').
  Assert: true.
  Call can_delegate_permission(Bob, target, 'adm.users', 'org_node', 'entity_A_id').
  Assert: false ('adm' not in allowed_domains).

--- IMPORT/EXPORT ---

test_rbac_14_role_export_import_round_trip
  Create role 'TestRole' with permissions=['ot.view','ot.create','ot.edit'].
  Call export_role_model([test_role_id]).
  Deserialize export payload.
  Delete original TestRole (retire it).
  Call import_role_model with the exported payload (renamed to 'TestRoleImported').
  Assert: TestRoleImported exists with correct permissions.
  Assert: validate_role_permissions returns is_valid=true for the imported set.

--- FULL GOVERNANCE LIFECYCLE ---

test_rbac_15_full_admin_lifecycle

  Step 1 - Create user Alice:
    create_user(username='alice', identity_mode='local').
    Assert: user active, no scope assignments.

  Step 2 - Assign role:
    assign_role_scope(user=alice, role=Technician, scope_type='tenant').
    Call effective_permissions(alice, org_node_id=None).
    Assert: ot.view present; adm.users NOT present.

  Step 3 - Simulate access:
    simulate_access(user=alice, scope_type='tenant').
    Assert: permissions HashMap contains ot.view = true; adm.users = false.

  Step 4 - Update role permissions (add ot.create):
    Already present in Technician role; verify validate_role_permissions still returns
    is_valid=true after adding ot.create (has ot.view as hard dep → satisfied).

  Step 5 - Admin audit event:
    After assign_role_scope: list_admin_events(actor_id=admin).
    Assert: at least 1 event with action='role_assigned' and target_user_id=alice.id.

  Step 6 - Deactivate user:
    deactivate_user(user=alice).
    Fetch user; assert is_active = 0.
    Call effective_permissions(alice) → Assert: all scope assignments still exist
    (assignments are not deleted on deactivate; resolver returns empty set because
    is_active=0 check is at command layer).

ACCEPTANCE CRITERIA
- All 15 tests pass: cargo test rbac::tests
- Zero test compilation warnings
- test_rbac_15 Phase 5 confirms audit event exists after role assignment
```

### Supervisor Verification - Test Suite

**V1 - Test count.**
`cargo test rbac::tests -- --list` shows exactly 15 tests.

**V2 - All pass.**
`cargo test rbac::tests 2>&1` shows 0 failures, 0 errors.

**V3 - Entity isolation (test_rbac_02).**
Critical test: verifies that entity-scoped grants do NOT bleed into other entities.
This is the most important multi-tenant correctness requirement in the module.

**V4 - Expired grant exclusion (test_rbac_04, test_rbac_12).**
Two separate tests confirm time-based exclusion: valid_to on regular assignments
AND emergency_expires_at on emergency grants.

---

## Admin Audit Timeline

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the admin audit timeline component.

CREATE src/components/admin/AdminAuditTimeline.tsx

Props: filter?: Partial<AdminEventFilter>

Same pattern as WoAuditTimeline and DiAuditTimeline:

- Icon by action type:
  user_created/user_deactivated: user icon
  role_assigned/role_revoked: shield icon
  role_created/role_updated/role_deleted: pencil-shield icon
  emergency_grant_created: bolt icon
  emergency_grant_expired/emergency_grant_revoked: bolt-slash icon
  delegation_policy_created/updated/deleted: key icon
  role_imported/role_exported: arrows-right-left icon
  default: dot icon

- Each row: action badge, actor, target (user or role), scope, summary, acted_at,
  step_up_used badge, apply_result badge
- Diff JSON rendered as collapsible "View diff" section (JSON prettified)
- apply_result='blocked' rows shown with red background, not orange
- Read-only; no mutation

PATCH src/pages/AdminPage.tsx (or AdminLayout.tsx):
  Add "Governance Audit" tab rendering <AdminAuditTimeline />.
  Tab visible to users holding adm.users OR adm.roles OR adm.permissions.

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- Diff JSON is collapsible and prettified (not raw JSON string in the UI)
- Blocked events visible with red badge
- Step-up used events show green badge
```

---

## SP06 Module Completion Checklist

Before marking sub-phase 06 complete, verify all of the following:

**Schema (Migrations 025-028):**
- [ ] Migration 025: `user_scope_assignments`, `permission_dependencies`, `role_templates`, `delegated_admin_policies` present; 5 system roles, 4 templates, 19 dependency pairs seeded
- [ ] Migration 026: ≥ 70 permissions across ≥ 21 domains; INSERT OR IGNORE idempotent
- [ ] Migration 027: `admin_change_events` with diff_json, step_up_used, apply_result
- [ ] Migration 028: `rbac_settings` with 5 policy knobs

**Rust:**
- [ ] `cargo check` passes with zero errors
- [ ] `rbac::resolver::effective_permissions` uses scope chain (parent lookups)
- [ ] `require_permission!` macro accepts optional `org_node_id`
- [ ] `require_step_up!` checks session freshness against rbac_settings key
- [ ] All `adm.*` mutation commands write to `admin_change_events`
- [ ] Emergency grants excluded from permissions after `emergency_expires_at`
- [ ] `import_role_model` partial success: valid roles imported, invalid skipped

**TypeScript:**
- [ ] `pnpm typecheck` passes with zero errors
- [ ] All Rust commands have typed Zod-validated wrappers in `rbac-service.ts`

**Tests:**
- [ ] All 15 tests pass: `cargo test rbac::tests`
- [ ] test_rbac_02 entity isolation passes (critical multi-tenant requirement)
- [ ] test_rbac_15 full lifecycle passes with audit event confirmation

**Cross-module contracts established for future subphases:**

| Contract | Consuming Module |
|----------|-----------------|
| `require_permission!(pool, user_id, "X.Y", scope)` macro | All SP07+ modules — standard gate |
| `admin_change_events` append-only table | SP07 Activity Feed & Immutable Audit Journal |
| `effective_permissions` resolver | Planning (SP16), Budget (SP24), Permits (SP23) |
| `user_scope_assignments.is_emergency` | SP07 anomaly detection for unusual access patterns |
| `rbac_settings.emergency_max_minutes` | SP06 emergency elevation enforcement |
| `delegated_admin_policies.allowed_domains_json` | SP26 Config Engine (custom permissions governance) |
| `user_accounts.personnel_id` (nullable) | SP06 Personnel (workforce readiness binding — filled when Personnel module built) |

---

## Sprint S4 — Password Expiry Policy and PIN-Based Fast Unlock

> **Gaps addressed:** (1) `user_accounts` has a `password_changed_at` column since migration
> 002 but no expiry policy enforces periodic password rotation — a requirement for regulated
> industries (ISO 55001, IEC 62443). (2) `user_accounts` has a `pin_hash` column but no
> PIN creation, update, or fast-unlock flow exists. For field technicians using shared
> workstations, re-entering a full password to unlock an idle-locked session on every return
> is a significant workflow friction. This sprint closes both gaps and adds 4 new tests to
> the RBAC suite.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/auth/password_policy.rs` | Password expiry evaluation + enforcement |
| `src-tauri/src/commands/auth.rs` (patch) | Integrate expiry check into `login` and `get_session_info` |
| `src-tauri/src/auth/pin.rs` | PIN hash, verify, and unlock logic |
| `src-tauri/src/commands/auth.rs` (patch) | `set_pin`, `unlock_session_with_pin` commands |
| `src/pages/auth/LockScreen.tsx` (patch) | PIN entry option alongside password unlock |
| `src/components/admin/PasswordPolicyPanel.tsx` | Admin UI for configuring expiry settings |
| `src-tauri/src/rbac/tests.rs` (patch) | 4 new tests (RBAC-16 through RBAC-19) |
| `src/i18n/locale-data/{fr,en}/auth.json` (patch) | Password expiry and PIN labels |

### GAP-08 — Password Expiry Policy

**Problem:** `password_changed_at` is populated on password change but nothing ever checks
whether the password has expired. Users can keep the same password indefinitely.

**New file: `src-tauri/src/auth/password_policy.rs`**

```rust
pub struct PasswordPolicy {
    pub max_age_days: i64,              // default: 90; 0 = no expiry
    pub warn_days_before_expiry: i64,   // default: 14
    pub min_length: usize,              // default: 8
    pub require_uppercase: bool,        // default: true
    pub require_lowercase: bool,        // default: true
    pub require_digit: bool,            // default: true
    pub require_special: bool,          // default: false
}

impl PasswordPolicy {
    /// Load from rbac_settings table with fallback defaults
    pub async fn load(pool: &SqlitePool) -> Self { ... }
}

pub enum PasswordExpiryStatus {
    /// Password is valid; no action needed
    Valid,
    /// Password expires within warn_days_before_expiry
    ExpiringSoon { days_remaining: i64 },
    /// Password has expired; force_password_change must be set
    Expired,
    /// No password_changed_at recorded (legacy user); treat as expired
    NeverSet,
}

/// Check if a user's password has expired based on policy.
pub async fn check_password_expiry(
    pool: &SqlitePool,
    user_id: i64,
    policy: &PasswordPolicy,
) -> AppResult<PasswordExpiryStatus> {
    // SELECT password_changed_at FROM user_accounts WHERE id = ?
    // If max_age_days = 0 → return Valid (expiry disabled)
    // If password_changed_at IS NULL → return NeverSet
    // Compute age = now - password_changed_at in days
    // If age > max_age_days → return Expired
    // If age > (max_age_days - warn_days_before_expiry) → return ExpiringSoon
    // Else → return Valid
}

/// Validate password strength against policy rules.
pub fn validate_password_strength(
    password: &str,
    policy: &PasswordPolicy,
) -> Result<(), Vec<String>> {
    // Returns Ok(()) or Err(list of violated rules)
    // e.g., ["Minimum 8 characters", "Must contain uppercase letter"]
}
```

**Patch to `login` command:**

```rust
// After successful authentication + lockout check:
let policy = PasswordPolicy::load(&state.db).await?;
match check_password_expiry(&state.db, user.id, &policy).await? {
    PasswordExpiryStatus::Expired | PasswordExpiryStatus::NeverSet => {
        // Set force_password_change = 1 in DB
        // Session is created but ForcePasswordChangePage will intercept
    },
    PasswordExpiryStatus::ExpiringSoon { days_remaining } => {
        // Include warning in SessionInfo DTO (new field: password_expires_in_days)
        // Frontend shows a non-blocking toast: "Votre mot de passe expire dans X jours"
    },
    PasswordExpiryStatus::Valid => { /* no action */ }
}
```

**Patch to `SessionInfo` DTO:**

```rust
pub struct SessionInfo {
    // ... existing fields ...
    pub password_expires_in_days: Option<i64>,  // NEW — None if no expiry or not expiring soon
}
```

**Frontend toast (patch to `ShellLayout.tsx` or `AuthGuard`):**

When `sessionInfo.password_expires_in_days` is `Some(n)` and `n <= 14`:
- Show persistent toast (not auto-dismiss): "Votre mot de passe expire dans {n} jours.
  [Changer maintenant]"
- "Changer maintenant" link navigates to profile/password-change page.

**Patch to `force_change_password` command:**

Add password strength validation:
```rust
let policy = PasswordPolicy::load(&state.db).await?;
validate_password_strength(&input.new_password, &policy)
    .map_err(|violations| AppError::Validation(violations.join(", ")))?;
```

**`rbac_settings` entries:**

```sql
INSERT OR IGNORE INTO rbac_settings (key, value, description) VALUES
  ('password_max_age_days',      '90',    'Days before password expiry (0 = disabled)'),
  ('password_warn_days',         '14',    'Days before expiry to show warning'),
  ('password_min_length',        '8',     'Minimum password length'),
  ('password_require_uppercase', '1',     'Require at least one uppercase letter'),
  ('password_require_lowercase', '1',     'Require at least one lowercase letter'),
  ('password_require_digit',     '1',     'Require at least one digit'),
  ('password_require_special',   '0',     'Require at least one special character');
```

**Admin UI: `PasswordPolicyPanel.tsx`**

Small panel accessible from AdminPage → Settings sub-tab or inline in the Audit tab:
- Form fields for each `rbac_settings` password policy key
- Live preview: "Current policy: min 8 chars, uppercase + lowercase + digit, expires every 90 days"
- Save button → updates `rbac_settings` rows
- Permission: `adm.settings`

### GAP-09 — PIN-Based Fast Unlock

**Problem:** Field technicians on shared workstations must re-enter a full password every
time the 30-minute idle lock triggers. A 4–6 digit PIN provides faster unlock for idle-locked
sessions without compromising the full password.

**Scope:** PIN is ONLY for idle lock unlock. It cannot be used for:
- Initial login (always requires full password)
- Step-up re-authentication (always requires full password)
- Password changes (always requires current full password)

**New file: `src-tauri/src/auth/pin.rs`**

```rust
/// Hash a PIN using argon2id with reduced memory (16 MiB) since PINs are short.
/// Uses the same argon2id as password but with adjusted params for PIN-length inputs.
pub fn hash_pin(pin: &str) -> AppResult<String> {
    // argon2id, m=16384, t=3, p=1
    // Returns PHC-format hash string
}

/// Verify a PIN against a stored hash.
pub fn verify_pin(pin: &str, hash: &str) -> AppResult<bool> {
    // Constant-time comparison via argon2::verify
}

/// Validate PIN format: 4-6 digits only.
pub fn validate_pin_format(pin: &str) -> AppResult<()> {
    // Must be 4-6 characters, all digits
    // No sequential patterns (1234, 0000) — optional hardening
}
```

**New IPC commands (patch `src-tauri/src/commands/auth.rs`):**

```rust
#[tauri::command]
pub async fn set_pin(
    state: tauri::State<'_, AppState>,
    input: SetPinInput,  // { current_password: String, new_pin: String }
) -> AppResult<()> {
    let user = require_session!(state);
    // Verify current_password first (full password required to set/change PIN)
    verify_password(&input.current_password, &user_record.password_hash)?;
    validate_pin_format(&input.new_pin)?;
    let pin_hash = hash_pin(&input.new_pin)?;
    // UPDATE user_accounts SET pin_hash = ? WHERE id = ?
    // Write audit event: action='pin_set'
}

#[tauri::command]
pub async fn clear_pin(
    state: tauri::State<'_, AppState>,
    input: ClearPinInput,  // { current_password: String }
) -> AppResult<()> {
    let user = require_session!(state);
    verify_password(&input.current_password, &user_record.password_hash)?;
    // UPDATE user_accounts SET pin_hash = NULL WHERE id = ?
    // Write audit event: action='pin_cleared'
}

#[tauri::command]
pub async fn unlock_session_with_pin(
    state: tauri::State<'_, AppState>,
    input: PinUnlockInput,  // { pin: String }
) -> AppResult<SessionInfo> {
    // Session must exist AND be locked (is_locked = true)
    let session = get_locked_session!(state)?;
    
    // Load user's pin_hash
    let user = get_user_by_id(&state.db, session.user.user_id).await?;
    let pin_hash = user.pin_hash.ok_or(AppError::Auth("No PIN configured".into()))?;
    
    // Verify PIN
    if !verify_pin(&input.pin, &pin_hash)? {
        // Increment failed_pin_attempts (separate counter, or reuse failed_login_attempts)
        // After 3 failed PIN attempts → require full password (disable PIN unlock for this lock)
        return Err(AppError::Auth("Invalid PIN".into()));
    }
    
    // Unlock session
    session_manager.unlock()?;
    // Write audit event: action='session_unlocked_with_pin'
    
    Ok(session_info)
}
```

**Patch to `LockScreen.tsx`:**

```
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│       🔒  Session verrouillée / Session Locked               │
│                                                              │
│       Bonjour, {displayName}                                 │
│                                                              │
│  ┌─────────────────────────────────────────────────────┐     │
│  │  [PIN mode]  or  [Password mode]    ← toggle        │     │
│  └─────────────────────────────────────────────────────┘     │
│                                                              │
│  PIN mode (if pin_hash exists):                              │
│  ┌──┐ ┌──┐ ┌──┐ ┌──┐ ┌──┐ ┌──┐                            │
│  │  │ │  │ │  │ │  │ │  │ │  │  ← 4-6 digit boxes          │
│  └──┘ └──┘ └──┘ └──┘ └──┘ └──┘                            │
│  Auto-submit when PIN length reached                         │
│                                                              │
│  Password mode (always available):                           │
│  ┌──────────────────────────────────────────────┐            │
│  │  Password: ••••••••                          │            │
│  └──────────────────────────────────────────────┘            │
│  [Déverrouiller / Unlock]                                    │
│                                                              │
│  (switch) Utiliser le mot de passe / Use PIN                 │
│                                                              │
│  ⚠️ 3 failed PIN attempts → switch to password required      │
│                                                              │
│  [Se déconnecter / Sign out]                                 │
└──────────────────────────────────────────────────────────────┘
```

**Behaviour:**
- Default unlock mode: PIN if `pin_hash` configured, else password.
- Toggle link: "Utiliser le mot de passe" / "Utiliser le PIN" switches mode.
- PIN auto-submits when the configured length is reached (no button needed).
- After 3 failed PIN attempts → force switch to password mode, disable PIN toggle.
- PIN entry boxes use `inputMode="numeric"` for mobile keyboard compatibility.

**PIN Setup UI (Profile settings — not AdminPage):**

Add a "PIN de déverrouillage rapide / Quick Unlock PIN" section to the user's profile
settings page:
- Current state badge: "PIN configuré / PIN not set"
- "Configurer le PIN / Set PIN" button → modal:
  1. Enter current password (required)
  2. Enter new PIN (4-6 digits)
  3. Confirm PIN
- "Supprimer le PIN / Remove PIN" button → requires current password

### New Tests (RBAC-16 through RBAC-19)

Add to `src-tauri/src/rbac/tests.rs`:

```
test_rbac_16_password_expiry_enforced
  Create user with password_changed_at = 91 days ago.
  Load policy with max_age_days = 90.
  Call check_password_expiry.
  Assert: returns PasswordExpiryStatus::Expired.

test_rbac_17_password_expiry_warning
  Create user with password_changed_at = 80 days ago.
  Load policy with max_age_days = 90, warn_days = 14.
  Call check_password_expiry.
  Assert: returns PasswordExpiryStatus::ExpiringSoon { days_remaining: 10 }.

test_rbac_18_pin_unlock_success
  Create user with pin_hash set.
  Create a locked session.
  Call unlock_session_with_pin with correct PIN.
  Assert: session unlocked, audit event written.

test_rbac_19_pin_unlock_failure_locks_to_password
  Create user with pin_hash set.
  Create a locked session.
  Call unlock_session_with_pin with wrong PIN 3 times.
  Assert: 3rd call returns error indicating "PIN disabled, use password".
  Call unlock_session with correct password.
  Assert: session unlocked.
```

### Updated RBAC Test Count

Total tests after Sprint S4: **19** (original 15 + 4 new).

### Acceptance Criteria

```
- cargo check passes with zero errors
- pnpm typecheck passes with zero errors
- User with password_changed_at > 90 days ago → ForcePasswordChangePage on next login
- User with password_changed_at = 80 days ago → toast warning "expires in 10 days"
- Password expiry disabled when max_age_days = 0
- force_change_password validates password strength per policy
- PasswordPolicyPanel can update all policy settings (adm.settings required)
- set_pin requires current password verification
- PIN format validation: 4-6 digits only, rejects "123" and "1234567"
- unlock_session_with_pin succeeds with correct PIN
- 3 failed PIN attempts → PIN mode disabled, password required
- LockScreen shows PIN input boxes when pin_hash is configured
- LockScreen auto-submits PIN when digit count reached
- PIN toggle between PIN mode and password mode works
- All 19 tests pass: cargo test rbac::tests
```

### Supervisor Verification — Sprint S4

**V1 — Password expiry.**
Set password_max_age_days=1. Change a user's password_changed_at to 2 days ago.
Login as that user. Verify ForcePasswordChangePage appears.

**V2 — Expiry warning.**
Set password_max_age_days=30, warn_days=14. Set password_changed_at to 20 days ago.
Login. Verify toast: "Votre mot de passe expire dans 10 jours."

**V3 — Password strength.**
On ForcePasswordChangePage, enter "abc" → validation error list shown (too short, no uppercase,
no digit). Enter "Abcdef1!" → accepted.

**V4 — PIN setup.**
Go to profile settings. Click "Set PIN". Enter current password + "1234".
Verify pin_hash is now set in DB. Lock session (wait 30min or trigger manually).
Enter 1234 on lock screen → session unlocked.

**V5 — PIN lockout.**
Lock session. Enter wrong PIN 3 times. Verify PIN mode disabled, password field shown.
Enter correct password → session unlocked.

**V6 — Test suite.**
Run `cargo test rbac::tests -- --list` → shows 19 tests.
Run `cargo test rbac::tests` → 0 failures.

---

*End of Phase 2 - Sub-phase 06 - File 04*
