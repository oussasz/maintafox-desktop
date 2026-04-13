# SP07-F03 Gap Review and Handoff

Date: 2026-04-13
Scope: Phase 2 / Sub-phase 07 / File 03 (Activity feed migration, emitter/writer, IPC commands, Activity/Audit UI)

## Review Summary

Professional end-of-file review covering all Rust backend, TypeScript frontend, shared contracts,
and integration wiring for SP07-F03. Findings organized by severity.

---

## CRITICAL / HIGH — Rust Backend

### B-01: `log.view` RBAC gate blocks entity-scoped callers (HIGH)
**File:** `src-tauri/src/commands/activity_feed.rs`
**Issue:** `require_permission!(..., "log.view", PermissionScope::Global)` only passes for users
with `scope_type='tenant'`. Users with entity/org_node-scoped `log.view` fail the gate.
The fallback `has_global_view` check (entity filtering) is thus **unreachable dead code**.
**Fix:** Change the top-level gate to support entity scope or use a two-tier check: attempt
Global first, fall back to Entity scope with `entity_scope_id IN (...)` filter. Apply to all
5 activity_feed commands.

### B-02: `get_event_chain` BFS produces duplicate nodes (HIGH)
**File:** `src-tauri/src/commands/activity_feed.rs`
**Issue:** When a neighbor is first encountered, `fetch_chain_node(...)` pushes it into `events`
AND enqueues it. When the neighbor is later popped, a second push occurs. Same event can appear
twice in the output.
**Fix:** Only call `fetch_chain_node` when popping from the queue (dequeue path), not when
discovering neighbors.

### B-03: Legacy `audit_events` schema collision (HIGH)
**File:** `src-tauri/src/migrations/m20261101_000036_activity_audit_log.rs`
**Issue:** Migration 001 defines `audit_events` with `id TEXT PK` and columns `event_type`,
`occurred_at`. Migration 036 uses `CREATE TABLE IF NOT EXISTS audit_events (id INTEGER PK ...)`,
so on existing DB the old table is NOT recreated. `add_column_if_missing` adds new columns to
the old table, creating a hybrid schema where `id` is TEXT in old rows.
**Fix:** Either rename the old table to `legacy_audit_events` and create the new shape, or verify
that `writer.rs` and `commands/audit_log.rs` handle the TEXT id column gracefully. Add acceptance test.

---

## MEDIUM — Rust Backend

### B-04: Audit action_code naming drift
**Files:** `src-tauri/src/commands/admin_users.rs`, `src-tauri/src/audit/mod.rs`
**Issue:** `admin_users.rs` uses `"auth.user_created"`, `"rbac.role_assigned"` while `event_type`
constants use `"user.created"`, `"role.assigned"`. Filtering will miss ad-hoc rows.
**Fix:** Add new constants to `audit::event_type` for the namespaced convention and use them.

### B-05: `emit_archive_activity_event` error branch is unreachable
**File:** `src-tauri/src/commands/archive.rs`
**Issue:** `emit_activity_event` always returns `Ok(())`. The `if let Err(err) = ...` branch in
`emit_archive_activity_event` is dead code.
**Fix:** Remove the dead `Err` branch or restructure for clarity.

### B-06: `list_audit_events` has no explicit permission gate
**File:** `src-tauri/src/commands/audit_log.rs`
**Issue:** Uses `require_session!` only. Any authenticated user can list their own audit rows.
**Fix:** Document as accepted design or add `log.view` as minimum gate.

---

## CRITICAL / HIGH — TypeScript Frontend

### F-01: `AuditLogViewer` has no initial data load (HIGH)
**File:** `src/components/activity/AuditLogViewer.tsx`
**Issue:** No `useEffect` calls `loadData` on mount. Table stays empty until "Apply filters" click.
**Fix:** Add `useEffect` that loads data on mount with default filter.

### F-02: `ActivityFeedPanel` filter triggers fetch on every keystroke (HIGH)
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `useEffect` runs `loadData` whenever `currentFilter` changes (every keystroke). "Apply"
button is redundant since the effect already fires.
**Fix:** Remove filter-dependency from `useEffect`; fetch only on "Apply" button and offset changes.

### F-03: `ActivityFeedPanel` no loading state shown (HIGH)
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `loading` state is tracked but never rendered. No spinner/skeleton during fetches.
**Fix:** Add loading spinner/skeleton when `loading` is true.

### F-04: `ArchiveExplorer` unused import `CheckCircle2` (HIGH — lint failure)
**File:** `src/components/archive/ArchiveExplorer.tsx`
**Issue:** `CheckCircle2` is imported but never used.
**Fix:** Remove the unused import.

### F-05: `ArchiveExplorer` bulk action handlers missing try/catch (HIGH)
**File:** `src/components/archive/ArchiveExplorer.tsx`
**Issue:** `runBulkExport`, `runBulkLegalHold`, `runBulkPurge` have no error handling.
**Fix:** Wrap each handler in try/catch with `setError(toErrorMessage(err))`.

---

## MEDIUM — TypeScript Frontend

### F-06: `ActivityFeedPanel` missing permission gate
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** No `usePermissions` / `PermissionGate`. UI renders for all users, backend-only enforcement.
**Fix:** Add `usePermissions` hook and conditionally render based on `log.view`.

### F-07: Correlation chain expansion — no error handling or loading state
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `handleExpand` calls `getEventChain` without try/catch; no loading indicator.
**Fix:** Add try/catch, per-row loading state, and error display.

### F-08: Save view — no error handling
**File:** `src/components/activity/ActivityFeedPanel.tsx`
**Issue:** `saveActivityFilter` called without try/catch.
**Fix:** Wrap in try/catch with user feedback.

### F-09: `RetentionPolicyPanel` re-fetches on row selection
**File:** `src/components/archive/RetentionPolicyPanel.tsx`
**Issue:** `load` depends on `selectedPolicyId`, causing full re-fetch on every row click.
**Fix:** Remove `selectedPolicyId` from `load`'s dependency array.

### F-10: `ActivityPage.tsx` accent error — "activite" → "activité"
**File:** `src/pages/ActivityPage.tsx`
**Fix:** Correct the accent.

### F-11: `ArchiveExplorer` misleading `pendingPurgeCount` label
**File:** `src/components/archive/ArchiveExplorer.tsx`
**Issue:** Counts items without legal hold, not truly pending-purge items.
**Fix:** Rename to "Purge-eligible (no hold)" or compute from retention eligibility.

---

## LOW — Accepted Tech Debt for Phase 3

- **L-01:** No Zod runtime validation on some IPC responses in UI components
- **L-02:** Accessibility gaps (aria-labels, keyboard navigation, AT announcements)
- **L-03:** `event_links` has no FK enforcement (application-layer only, SQLite limitation)
- **L-04:** Duplicate type definitions in `ipc-types.ts` vs `activity-service.ts`
- **L-05:** All frontend components use hardcoded English/French strings (i18n deferred to Phase 3)
- **L-06:** `normalize_event_table` maps unknown names to `"activity_events"` silently

---

## Applied Fixes in Current Iteration (SP07-F03 carry-forward commit)

1. **ArchivePage wired:** `src/pages/ArchivePage.tsx` now renders `ArchiveExplorer` + `RetentionPolicyPanel` in tabbed layout (replacing ModulePlaceholder).
2. **Archive activity events fixed:** Replaced `write_restore_activity_event_fire_and_log` (wrong schema) with `emit_archive_activity_event` using correct `activity_events` columns via `crate::activity::emitter`.
3. **Blocked-path emitters:** Restore-blocked, purge-blocked, and successful purge now emit `activity_events`.
4. **ActivityFilter extended:** Added `source_record_type` + `source_record_id` fields (Rust + TS + ipc-types).
5. **RetentionPolicyPanel history persisted:** Change history backed by `list_activity_events` query.
6. **Audit integration patches:** 5 high-risk SP06 adm.* mutations now call `crate::audit::emit`.
7. **Roadmap checklist updated:** All SP07-F03 items marked complete with carry-forward documented.

---

## Validation Snapshot

- `cargo check` passes (only pre-existing dead_code warnings)
- `pnpm typecheck` passes

---

## SP07-F04 Execution Priority

All items below are documented in the File 04 roadmap with resolution checklists.

**Must-fix before Phase 2 closure:**
1. B-01: Fix `log.view` RBAC gate for entity-scoped callers
2. B-02: Fix `get_event_chain` BFS duplicate nodes
3. B-03: Legacy `audit_events` schema coexistence test
4. F-01: Add initial load to `AuditLogViewer`
5. F-02: Fix `ActivityFeedPanel` filter fetch behavior
6. F-03: Add loading state to `ActivityFeedPanel`
7. F-04: Remove unused `CheckCircle2` import
8. F-05: Add error handling to bulk action handlers

**Should-fix during Phase 2 closure:**
9. B-04: Standardize audit action_code constants
10. B-05: Remove dead error branch in archive helper
11. F-06–F-11: Frontend polish (permission gates, error handling, labels)

**Remaining audit integration patches (SP07-F04 follow-up):**
- `update_user`, `create_role`, `update_role`, `delete_role` → audit emit
- Config mutation commands → `config.setting_changed` audit emit
- SP08+ module state transitions → activity + audit emit
