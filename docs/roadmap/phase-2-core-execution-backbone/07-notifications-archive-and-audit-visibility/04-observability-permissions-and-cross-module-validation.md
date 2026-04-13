# Phase 2 - Sub-phase 07 - File 04
# Observability Permissions and Cross-Module Validation

## Context and Purpose

Files 01 through 03 delivered the three observability systems: the Notification delivery engine,
the Archive governance layer, and the Activity Feed / Immutable Audit Journal. File 04 closes
sub-phase 07 and Phase 2 as a whole with two layers:

1. **Observability permission hardening** — migration 032 seeds the `log.*`, `arc.*` permission
   domain rows, adds `adm.audit` as a dedicated audit-access permission, and ensures the
   notification system's admin permissions are anchored to `adm.settings`.
2. **Cross-module integration test suite** — 12 tests covering the complete observatory chain:
   emit → deliver → escalate → archive → audit, WO close → activity event, RBAC event →
   audit event, archive purge eligibility, and notification deduplication correctness.
3. **Phase 2 completion checkpoint** — a comprehensive system-wide checklist confirming that
   all Phase 2 sub-phases (SP01 through SP07) have delivered their structural commitments and
   that cross-module contracts are operative.

---

## PRD Alignment Checklist

- [x] `log.view` / `log.export` / `adm.audit` permission domain confirmed (PRD §6.17 explicit)
- [x] `arc.view` / `arc.restore` / `arc.export` / `arc.purge` aligned (PRD §6.12 explicit)
- [x] No special permission for own notifications — `list_notifications` uses current user context (PRD §6.14 explicit)
- [x] Step-up reauthentication visible in audit history for all requires_step_up commands (PRD §6.17)
- [x] Correlation chain validated end-to-end (PRD §6.17 "Event Correlation & Drill-Through")

---

## Architecture Rules Applied

- **Phase 2 forms a closed operational loop.** SP01 (Org Model) → SP02 (Equipment) → SP03
  (Reference Data) → SP04 (DI) → SP05 (WO) → SP06 (RBAC) → SP07 (Observability) together
  deliver: the operational data layer, the execution workflows, governed authorization, and the
  full visibility/audit/notification chain. No Phase 2 module is standalone or partial.
- **All SP07 emitter contracts are tested through integration tests.** The test suite in this
  file creates real operational events (WO close, DI approve, role assign) and asserts that
  activity_events, audit_events, and notifications are correctly populated.
- **The Phase 2 completion checklist is a structural gate for Phase 3 work.** Future
  sub-phases (PM Planning, Inventory, Reliability, Planning & Scheduling) build on Phase 2
  contracts. Proceeding to Phase 3 while Phase 2 has unresolved gaps produces compounding
  technical debt.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/migrations/m20261201_000032_observability_permissions.rs` | Seeds log.*, arc.*, and adm.audit permission rows; adds adm.settings to notifications admin |
| `src-tauri/src/observability/tests.rs` | 12-test cross-module integration suite |
| Phase 2 Completion Checklist (this document) | Structural gate before Phase 3 |

---

## Migration 032 — Observability Permissions

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the observability permissions seed migration.

CREATE src-tauri/src/migrations/m20261201_000032_observability_permissions.rs

INSERT OR IGNORE INTO permissions (name, description, category, is_dangerous, requires_step_up)
VALUES
  -- Already seeded in migration 026 for log.* and arc.* —
  -- These inserts are idempotent (INSERT OR IGNORE) so they are safe to repeat.
  -- They are listed here as a record of SP07's permission contract.

  -- Activity Log
  ('log.view',   'View activity feed events',       'log', 0, 0),
  ('log.export', 'Export audit log',                'log', 1, 0),
  ('log.admin',  'Manage activity feed settings',   'log', 1, 1),

  -- Audit
  ('adm.audit',  'Access full immutable audit journal, review security events', 'adm', 1, 0),

  -- Archive
  ('arc.view',   'Browse archived records',          'arc', 0, 0),
  ('arc.restore','Restore eligible archived records','arc', 1, 1),
  ('arc.export', 'Export archived record payloads',  'arc', 0, 0),
  ('arc.purge',  'Purge records past retention policy','arc', 1, 1),

  -- Notification admin (uses adm.settings category from 6.18)
  ('adm.settings', 'Manage system settings, connection profiles, and admin policies', 'adm', 1, 1);

-- Seed permission_dependencies for observability domains:
INSERT OR IGNORE INTO permission_dependencies (permission_name, required_permission_name, dependency_type)
VALUES
  ('log.export', 'log.view',    'hard'),
  ('log.admin',  'log.view',    'hard'),
  ('arc.restore','arc.view',    'hard'),
  ('arc.export', 'arc.view',    'hard'),
  ('arc.purge',  'arc.restore', 'hard'),
  ('adm.audit',  'log.view',    'warn');

ACCEPTANCE CRITERIA
- Migration 032 applies (idempotent)
- All permission rows present in permissions table
- arc.purge → arc.restore hard dependency seeded
- cargo check passes
```

---

## Cross-Module Integration Tests

### AI Agent Prompt

```text
You are a senior Rust test engineer. Write the SP07 cross-module integration test suite.

CREATE src-tauri/src/observability/tests.rs

Use #[cfg(test)] with tokio::test. In-memory SQLite, all migrations 001-032 applied.

--- NOTIFICATION INTEGRATION ---

test_obs_01_emit_creates_notification
  Call emit_event with category_code='wo_assigned', routing_mode='assignee'.
  Seed a WO with primary_responsible_id = test_user_id.
  Assert: notifications table has 1 row for test_user_id.
  Assert: delivery_state = 'delivered' (in_app delivery is immediate).

test_obs_02_dedupe_prevents_flood
  Call emit_event with same dedupe_key twice without acknowledging the first.
  Assert: notifications table still has EXACTLY 1 row for that dedupe_key condition.
  Assert: title/body updated on second emit (not a new row).

test_obs_03_snooze_wakes
  Create notification; call snooze_notification(snooze_minutes=1).
  Assert: delivery_state = 'snoozed'.
  Manually set snoozed_until to 5 minutes ago in the DB.
  Run the snooze sweep from the scheduler.
  Assert: delivery_state = 'delivered'.

test_obs_04_acknowledge_closes_escalation_path
  Create notification with requires_ack=1.
  Call acknowledge_notification.
  Assert: delivery_state = 'acknowledged'.
  Verify: escalation_level remains 0 (acknowledgement prevents escalation).

--- ARCHIVE INTEGRATION ---

test_obs_05_archive_wo_and_verify
  Close a WO (full lifecycle from SP05 test pattern).
  Call archive_record with source_module='wo', source_record_id=wo.id.as_str(),
    archive_class='operational_history', archive_reason_code='completed'.
  Assert: archive_items row exists.
  Assert: archive_payloads row exists with payload_size_bytes > 0.
  Call verify_checksum(archive_item_id).
  Assert: returns true (checksum valid).
  Assert: archive_actions has 2 rows: 'archive' and 'checksum_verified'.

test_obs_06_purge_blocked_by_retention
  Archive a WO with archived_at set to yesterday.
  Call purge_archive_items([archive_item_id], reason='test').
  Assert: returns error or PurgeResult with item in blocked list
    (retention_years=7 means purge cannot happen until year 7).

test_obs_07_legal_hold_blocks_purge
  Archive a WO.
  set_legal_hold(enable=true).
  Call purge_archive_items.
  Assert: blocked with legal_hold reason in response.

test_obs_08_restore_blocked_for_operational_history
  Archive a WO with archive_class='operational_history', restore_policy='not_allowed'.
  Call restore_archive_item.
  Assert: returns AppError::Forbidden (not_allowed restore_policy).

--- ACTIVITY FEED INTEGRATION ---

test_obs_09_wo_close_emits_activity_event
  Run full WO lifecycle to closure (calling close_wo).
  Assert: activity_events has at least 1 row with:
    event_code = 'wo.closed'
    source_module = 'wo'
    source_record_id = wo.id.to_string()

test_obs_10_rbac_mutation_emits_both_activity_and_audit
  Call assign_role_scope for a user.
  Assert: admin_change_events has 1 row with action='role_assigned'.
  Assert: audit_events has 1 row with action_code='rbac.role_assigned'.
  Assert: activity_events has 1 row with event_code='rbac.role_assigned'.
  (Both emitters fire from the same adm.* mutation command.)

--- AUDIT JOURNAL INTEGRATION ---

test_obs_11_audit_append_only
  Call write_audit_event. Get the inserted row id.
  Attempt: directly UPDATE audit_events SET result='altered' WHERE id = that_id.
  This is a raw SQL edit — allowed in tests to simulate tampering.
  Call verify_checksum logic equivalent on the audit row:
    Re-fetch; assert that the result column now differs from the original (tampered).
  Then: verify that no IPC command can perform this mutation
    (assert no 'update_audit_event' or 'delete_audit_event' command is registered
     in the Tauri command handler list).

test_obs_12_full_observability_chain
  Simulate: IoT anomaly → DI → WO → close → archive → activity feed chain:

  Step 1: Emit activity event with event_code='iot.threshold_exceeded', correlation_id=uuid_A.
  Step 2: Create DI with correlation_id=uuid_A; emit di.submitted with correlation_id=uuid_A.
  Step 3: Convert DI → WO; emit wo.created with correlation_id=uuid_A.
  Step 4: Run WO to closure; emit wo.closed with correlation_id=uuid_A.
  Step 5: Archive the closed WO; emit arc.archived with correlation_id=uuid_A.

  Assertions:
  - get_event_chain(root=iot_event.id) returns all 5 events (connected via event_links).
  - Chain is ordered by happened_at ASC.
  - audit_events has at least 1 row (from WO close requires_step_up audit).
  - archive_items has 1 row for the WO.
  - notifications has at least 1 row (WO assignment notification).

ACCEPTANCE CRITERIA
- All 12 tests pass: cargo test observability::tests
- test_obs_12 full chain passes end-to-end (5-step correlation visible in event chain)
- test_obs_11 confirms no IPC command modifies audit_events
- No new warnings introduced in the observability test module; workspace-wide `cargo test` may still emit unrelated warnings until cleaned up
```

---

## Phase 2 Completion Checklist

This checklist constitutes the structural gate for ALL Phase 3 work. Every item must be
verified before starting Phase 3.

**Migration index:** Numbers below match `maintafox-desktop/src-tauri/src/migrations/mod.rs`
(six-digit suffixes **001–038**). There is **no** `000016` migration; **017** follows **015**.
Domain permission rows (`org.*`, `eq.*`, `ref.*`, `di.*`, `ot.*`, observability, etc.) are seeded in the
**permission catalog** migration **029** unless a bullet states otherwise.

### SP01 — Organization and Site Model
- [ ] org_units hierarchy with recursive CTE queries working
- [ ] org_change_events append-only (migration **009** `org_audit_trail`)
- [ ] org.* permissions seeded (migration **029**)

### SP02 — Equipment and Asset Registry
- [ ] equipment table with full lifecycle state machine
- [ ] equipment_lifecycle_events, counters, meters, documents (migrations **010–012**)
- [ ] eq.* permissions seeded (migration **029**)

### SP03 — Reference Data Governance
- [ ] reference_domains, reference_sets, reference_values with publish lifecycle
- [ ] Protected analytical domains (failure hierarchies, failure modes, etc.) present
- [ ] ref.* permissions seeded (migrations **013–015** for schema; **029** for permissions)

### SP04 — Intervention Requests (DI)
- [ ] intervention_requests with 11-state machine
- [ ] SLA engine, attachments, WO conversion stubs (migrations **017–019**)
- [ ] di.* permissions seeded (migration **029**)
- [ ] di_change_events append-only audit (migration **020**)

### SP05 — Work Orders (OT)
- [ ] work_orders with 12-state machine; quality gates on close
- [ ] Planning sub-entities: interveners, parts, tasks, delay segments, downtime segments (migrations **022–024**)
- [ ] Closeout: failure details, verifications, attachments (migration **025**); WO change ledger **026**; conclusion column **027**
- [ ] ot.* permissions seeded (migration **029**)
- [ ] WoAnalyticsSnapshot struct ready for SP10 RAMS consumption
- [ ] CostPostingHook payload defined for SP24 Budget consumption

### SP06 — Users, Roles, Permissions, and Admin Governance
- [ ] user_scope_assignments with scope chain resolution (migration **028**)
- [ ] 70+ permissions across 21 domains (migration **029**)
- [ ] 5 system roles seeded (non-deletable)
- [ ] Dangerous-permission handling; step-up enforcement (migrations **029**, **032** hardening, **033** password policy)
- [ ] Delegation policies, emergency elevation, import/export
- [ ] admin_change_events append-only (migration **030**)
- [ ] RBAC test suite: `cargo test rbac::tests` — **21** tests (`auth::rbac::tests::*` + `rbac::tests::tests::test_rbac_*`). *Last run: 20 passed; `test_rbac_18_pin_unlock_success` failed.*

### SP07 — Notifications, Archive, and Audit Visibility
- [ ] Notification engine: emit, dedupe, escalation, snooze, ack (migration **034**)
- [ ] Archive: snapshot, retention, purge workflow, legal hold (migration **035**)
- [ ] Activity feed: append-only, visibility-scoped, correlation_id (migration **036**)
- [ ] Audit journal: append-only, before/after hashes, auth_context (migration **036**)
- [ ] Observability permissions seeded (migration **037**)
- [ ] `audit_events` INTEGER primary key / writer compatibility (migration **038**)
- [x] Integration tests: all 12 pass — `cargo test observability::tests` (verified)

### Cross-Module Schema Integrity
- [ ] Migration chain **001–038** applies cleanly on a fresh in-memory SQLite (**016** absent)
- [ ] All migrations are idempotent (no duplicate-seed errors on second apply)
- [ ] No foreign key violations in default seed data

### Rust Compilation
- [x] `cargo check` passes with zero errors (verified; may still show unrelated `dead_code` warnings e.g. `reference/imports.rs`)
- [ ] Full library test suite green — run `cargo test` and fix failures (do not rely on `grep`; on Windows use `Select-String` or inspect summary). *Last full run: 5 failing tests — `auth_integration_tests` v5/v6/v7, `test_rbac_18_pin_unlock_success`, `reference::publish_tests` v2.*

### TypeScript Compilation
- [x] `pnpm typecheck` passes with zero errors (verified)
- [ ] All Rust commands have matching Zod-validated TypeScript wrappers

### Known Nullable Placeholders (expected for Phase 2; resolved in Phase 3)
- `work_order_interveners.skill_id` — NULL until SP06 Personnel
- `work_order_parts.article_id` — NULL until SP08 Inventory
- `pm_occurrence_id` in WoAnalyticsSnapshot — NULL until SP09 PM Planning
- `permit_ids` in analytics snapshot — empty list until SP23 Work Permits
- `personnel_id` on user_accounts — may be NULL until sp06-personnel sub-module is built

---

## AI Agent Prompt Hardening (Mandatory for Cursor-Only Execution)

Use this prefix in every implementation prompt for this file:

```text
Execution constraints:
- Treat fixed roadmap migration numbers as logical targets; use next contiguous migration ID in repository.
- Keep permission seeding idempotent (INSERT OR IGNORE) and compatible with existing unique/index constraints.
- Keep Rust command registration and frontend command wrappers in strict sync.
- Use portable acceptance commands (cargo test, cargo check, pnpm typecheck) without shell-specific grep assumptions.
- Run validation after every substantive edit and report exact failing test names if any.
```

File 04 specific clarifications:

- Replace shell-specific acceptance such as `cargo test ... | grep ...` with plain `cargo test` and explicit pass/fail interpretation.
- For "no mutation command exists" assertions, validate by code-level command registration and permission contract tests rather than runtime reflection APIs.
- Phase completion checkboxes must map to concrete evidence (test name, command output, or query assertion) to remain AI-agent verifiable.

Carry-forward from SP07-F02 review:

- [x] `ArchivePage` now mounts `ArchiveExplorer` + `RetentionPolicyPanel` in a tabbed layout (done in SP07-F03 carry-forward commit).
- [x] Blocked restore/purge paths emit `activity_events` via `emit_archive_activity_event` helper (done in SP07-F03 carry-forward commit).
- [x] Folder-tree navigation after filter updates — Vitest: `src/components/archive/__tests__/ArchiveExplorer.test.tsx` (`keeps module / class / year tree clickable after search, legal-hold, and class-chip filters`). Run: `pnpm test -- src/components/archive/__tests__/ArchiveExplorer.test.tsx`.
- [x] Retention policy change history is backed by persisted `list_activity_events` query (done in SP07-F03 carry-forward commit).

---

## Carry-Forward from SP07-F03 Professional Gap Review

The following issues were identified during the SP07-F03 end-of-file professional review. All must be resolved during File 04 implementation or explicitly documented as accepted technical debt for Phase 3.

### CRITICAL / HIGH — Rust Backend

#### B-01: `log.view` RBAC gate blocks entity-scoped callers (HIGH)
**File:** `src-tauri/src/commands/activity_feed.rs`
**Issue:** `require_permission!(..., "log.view", PermissionScope::Global)` only passes for users with `scope_type='tenant'`. Users with entity/org_node-scoped `log.view` fail the gate entirely. The fallback `has_global_view` check (which adds entity filtering) is thus **unreachable dead code**.
**Fix:** Change the top-level gate to `require_permission!(..., "log.view", PermissionScope::Entity)` or use a two-tier check: attempt `Global` first, then fall back to `Entity` scope to build the `entity_scope_id IN (...)` filter. Apply consistently to `list_activity_events`, `get_activity_event`, `get_event_chain`, `save_activity_filter`, and `list_saved_activity_filters`.

#### B-02: `get_event_chain` BFS produces duplicate nodes (HIGH)
**File:** `src-tauri/src/commands/activity_feed.rs`
**Issue:** When a neighbor is first encountered, `fetch_chain_node(...)` pushes it into `events` AND enqueues it. When the neighbor is later popped from the queue, a second `fetch_chain_node(...)` push occurs. Same event can appear twice in the final sorted output.
**Fix:** Only call `fetch_chain_node` when popping from the queue (dequeue path), not when discovering neighbors. Alternatively, guard with `if !visited.contains(...)` before the push.

#### B-03: Legacy `audit_events` schema collision (HIGH)
**File:** `src-tauri/src/migrations/m20261101_000036_activity_audit_log.rs`
**Issue:** Migration 001 defines `audit_events` with `id TEXT PK` and columns `event_type`, `occurred_at`, etc. Migration 036 uses `CREATE TABLE IF NOT EXISTS audit_events (id INTEGER PK AUTOINCREMENT ...)`, so on an existing DB the old table is NOT recreated — `add_column_if_missing` adds new columns to the old table. Writers assume the new shape including `id INTEGER`. This creates a **hybrid schema** risk where `id` is TEXT in old rows and `action_code` column may be missing if column-add fails.
**Fix:** In migration 036, either: (1) rename the old table to `legacy_audit_events` and create the new one, then migrate data; or (2) verify that both `writer.rs` and `commands/audit_log.rs` gracefully handle the old TEXT id column and query only new-shape rows. Add a dedicated data-migration step or acceptance test.

#### B-04: Audit action_code naming drift (MEDIUM)
**Files:** `src-tauri/src/commands/admin_users.rs`, `src-tauri/src/audit/mod.rs`
**Issue:** `admin_users.rs` uses ad-hoc strings like `"auth.user_created"`, `"rbac.role_assigned"` while `audit::event_type` defines constants as `"user.created"`, `"role.assigned"`. Filtering and reporting by constant will miss the ad-hoc rows.
**Fix:** Either: (1) add new constants to `audit::event_type` matching the namespaced convention (`auth.user_created`, `rbac.role_assigned`, etc.) and use them from `admin_users.rs`; or (2) align all sites to the existing constants. Document the canonical naming convention.

#### B-05: `emit_archive_activity_event` error branch is unreachable (MEDIUM)
**File:** `src-tauri/src/commands/archive.rs`
**Issue:** `emit_activity_event` always returns `Ok(())` (errors are swallowed inside `emitter.rs`). The `if let Err(err) = ...` branch in `emit_archive_activity_event` never executes; the `tracing::warn!` is dead code.
**Fix:** Remove the dead `Err` branch or log unconditionally if fire-and-log tracing is desired. Alternatively, return `Result` from the inner function and handle differently.

#### B-06: `list_audit_events` has no explicit permission gate (MEDIUM)
**File:** `src-tauri/src/commands/audit_log.rs`
**Issue:** Uses `require_session!` only, then conditionally checks `adm.audit`. Any authenticated user can list their own audit rows (actor_id forced to self). Confirm this is intentional product behavior.
**Fix:** If intentional, document as accepted design; if not, add `require_permission!(..., "log.view", ...)` as a minimum gate.

#### B-07: `get_audit_event` mixed-language error message (LOW)
**File:** `src-tauri/src/commands/audit_log.rs`
**Issue:** Error message `"Permission requise : adm.audit or own actor_id event"` mixes French and English.
**Fix:** Standardize to French (matching auth error pattern) or English (matching internal error convention).

### CRITICAL / HIGH — TypeScript Frontend

#### F-01: `AuditLogViewer` has no initial data load (HIGH)
**File:** `src/components/activity/AuditLogViewer.tsx`
**Issue:** There is no `useEffect` that calls `loadData` on mount or when the filter changes. The table stays empty until the user manually clicks "Apply filters".
**Fix:** Add a `useEffect` that loads data on mount with default filter, and optionally reload when filter state changes (with debounce to avoid noisy calls).

#### F-02: `ActivityFeedPanel` filter changes trigger immediate fetch without debounce (HIGH)
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `useEffect` runs `loadData` whenever `currentFilter` changes, which happens on every keystroke in text fields. The "Apply filters" button is redundant since the effect already fires.
**Fix:** Either: (1) remove the filter-dependency from the `useEffect` and only fetch on "Apply" button click and offset changes; or (2) add debounce (300ms) to prevent noisy backend calls. Approach (1) is recommended for consistency with AuditLogViewer.

#### F-03: `ActivityFeedPanel` no loading state shown (HIGH)
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `setLoading(true/false)` runs in `loadData`, but there is no spinner, skeleton, or "Loading…" text rendered — only empty-state text gated by `!loading`. Users get no visual feedback during fetches.
**Fix:** Add a loading spinner or skeleton component when `loading` is true.

#### F-04: `ArchiveExplorer` unused import `CheckCircle2` (HIGH)
**File:** `src/components/archive/ArchiveExplorer.tsx`
**Issue:** `CheckCircle2` is imported from lucide-react but never used. Will fail strict lint rules.
**Fix:** Remove the unused import.

#### F-05: `ArchiveExplorer` bulk action handlers missing try/catch (HIGH)
**File:** `src/components/archive/ArchiveExplorer.tsx`
**Issue:** `runBulkExport`, `runBulkLegalHold`, and `runBulkPurge` have no error handling. IPC failures will surface as unhandled promise rejections.
**Fix:** Wrap each handler in try/catch with `setError(toErrorMessage(err))` pattern.

#### F-06: `ActivityFeedPanel` missing permission gate (MEDIUM)
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** Unlike audit/archive surfaces, there is no `usePermissions` / `PermissionGate` — the UI renders for all users and relies solely on backend enforcement.
**Fix:** Add `usePermissions` hook and conditionally render or show a "no access" state based on `log.view` capability. Same pattern as `AuditLogViewer`.

#### F-07: `ActivityFeedPanel` correlation chain expansion — no error handling or loading state (MEDIUM)
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `handleExpand` calls `getEventChain` without try/catch. Failures leave no user-visible error, and there is no in-flight/loading indicator for the chain request.
**Fix:** Add try/catch, a per-row loading state, and error display.

#### F-08: `ActivityFeedPanel` save view — no error handling (MEDIUM)
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `saveActivityFilter` is called without try/catch. Save failures are silent.
**Fix:** Wrap in try/catch with toast or error state feedback.

#### F-09: `RetentionPolicyPanel` `load` dependency causes re-fetch on row selection (MEDIUM)
**File:** `src/components/archive/RetentionPolicyPanel.tsx`
**Issue:** `load` depends on `selectedPolicyId`, so changing the selected row recreates `load`, retriggering the mount `useEffect` and re-fetching the full policy list each time.
**Fix:** Remove `selectedPolicyId` from `load`'s dependency array. Handle initial selection in a separate `useEffect` that runs only once after the first successful load.

#### F-10: All frontend components — hardcoded i18n strings (MEDIUM)
**Files:** All activity/audit/archive components
**Issue:** All user-facing copy is hardcoded English or mixed French/English. `ActivityPage.tsx` title has a missing accent ("activite" → "activité").
**Fix:** Defer to Phase 3 i18n pass, but fix the accent immediately and document as accepted tech debt.

#### F-11: `ArchiveExplorer` `pendingPurgeCount` label is misleading (MEDIUM)
**File:** `src/components/archive/ArchiveExplorer.tsx`
**Issue:** Counts items without legal hold, not items actually pending purge in a workflow sense.
**Fix:** Rename to "Purge-eligible (no hold)" or calculate based on retention policy eligibility.

### LOW — Accepted for Phase 3

#### L-01: No Zod runtime validation on IPC responses in UI components
**Scope:** All activity/audit/archive React components call service functions but some responses bypass Zod schemas.
**Status:** activity-service.ts does use Zod; archive-service.ts should be reviewed for consistency.

#### L-02: Accessibility gaps (aria-labels, keyboard navigation, AT announcements)
**Scope:** Filter inputs lack `aria-label`, auto-refresh checkbox not wired with `htmlFor`, correlation expansion not announced to assistive technology, `<details>/<summary>` keyboard interaction fragile.
**Status:** Document as Phase 3 accessibility pass.

#### L-03: `event_links` has no FK enforcement
**Scope:** Migration 036 creates `event_links` without FKs to `activity_events`/`audit_events`/`notification_events`.
**Status:** Accepted — cross-table polymorphic FK is a known SQLite limitation; enforced at application layer.

#### L-04: Duplicate type definitions in `ipc-types.ts` vs `activity-service.ts`
**Scope:** Same interfaces defined in two places; drift risk on future changes.
**Status:** Consolidate in Phase 3 by importing from `ipc-types.ts` in service layer.

---

## SP07-F03 Review Resolution Checklist

The following items MUST be resolved as part of File 04 implementation:

- [x] **B-01:** Fix `log.view` RBAC gate to support entity-scoped callers (not just tenant)
- [x] **B-02:** Fix `get_event_chain` BFS duplicate node bug
- [x] **B-03:** Add acceptance test or migration step for legacy `audit_events` schema coexistence
- [x] **B-04:** Standardize audit action_code naming convention with `audit::event_type` constants
- [x] **B-05:** Clean up dead error branch in `emit_archive_activity_event`
- [x] **F-01:** Add initial data load to `AuditLogViewer` on mount
- [x] **F-02:** Fix `ActivityFeedPanel` filter — fetch only on Apply button, not on every keystroke
- [x] **F-03:** Add loading state (spinner/skeleton) to `ActivityFeedPanel`
- [x] **F-04:** Remove unused `CheckCircle2` import from `ArchiveExplorer`
- [x] **F-05:** Add try/catch to `ArchiveExplorer` bulk action handlers
- [x] **F-06:** Add `usePermissions` gate to `ActivityFeedPanel` for `log.view`
- [x] **F-07:** Add error handling + loading state to correlation chain expansion
- [x] **F-08:** Add error handling to save view flow
- [x] **F-09:** Fix `RetentionPolicyPanel` load dependency causing re-fetch on row select
- [x] **F-10:** Fix "activite" → "activité" accent in `ActivityPage.tsx`
- [x] **F-11:** Rename misleading `pendingPurgeCount` label in `ArchiveExplorer`

### Accepted tech debt for Phase 3:
- L-01: Zod validation consistency audit across all service files
- L-02: Accessibility pass (aria-labels, keyboard nav, AT announcements)
- L-03: `event_links` FK enforcement at application layer (no DB FK)
- L-04: Consolidate duplicate type definitions between `ipc-types.ts` and service files

---

## SP07 and Phase 2 Closure Notes

With sub-phases 01–07 complete, Maintafox Phase 2 delivers:

- A governed operational data layer (org model, equipment, reference data)
- A complete corrective maintenance execution backbone (DI → WO with 23 state transitions,
  evidence-grade closeout, cost roll-up, and analytics snapshot)
- A scoped authorization system with 70+ permission rows, dependency validation, emergency
  elevation, and a delegation governance model
- Three observability systems that require no changes from source modules to work together:
  notifications fire from emit_event, activity_events from emit_activity_event, and audit_events
  from write_audit_event
- All cross-module analytical contracts (RAMS, Cost, Planning, Permits) are explicitly
  documented as nullable placeholders, ensuring Phase 3 modules slot in without schema rewrites

The Phase 2 system is designed to be **immediately useful to operate** (DI, WO, RBAC, Notifications)
and **ready to scale analytically** (archive, audit, analytics snapshots, cost hooks).

Post-closure frontend carry-forward review is tracked in:
`docs/roadmap/phase-2-core-execution-backbone/07-notifications-archive-and-audit-visibility/05-frontend-hardening-carry-forward.md`

---

*End of Phase 2 - Sub-phase 07 - File 04*
*End of Phase 2 - Core Execution Backbone*
