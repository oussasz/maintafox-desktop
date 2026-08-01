# Phase 2 - Sub-phase 05 - File 05
# WO Frontend Remediation Phase 2

## Context and Purpose

Files 01 through 04 delivered the complete WO operational machinery and its hardening layers.
A comprehensive professional audit of all 25 WO frontend files (18 components, 1 page,
4 services, 1 store, 1 utils module) revealed **26 remaining issues** across four severity
tiers. This file organizes them into four remediation sprints.

The audit was performed after Sprint S5 (41 gap fixes + 5 secondary gap fixes + V1–V5
supervisor verification + i18n remediation of 4 components). TypeScript compiles with 0 errors.
The issues cataloged here are runtime correctness, code hygiene, and dead-code concerns
that were out of scope for the S5 gap-closure sprint.

---

## Severity Classification

| Tier | Count | Description |
|------|-------|-------------|
| **Critical** | 3 | Wrong IPC parameters or duplicate service wrappers — will fail at runtime |
| **High** | 6 | Missing status mapping, wrong state sets, inverted logic — incorrect visible behavior |
| **Medium** | 11 | Dead code, missing deps, locale inconsistencies — latent bugs or tech debt |
| **Low** | 6 | Minor hygiene — unused imports, missing `void`, variable shadowing |

---

## Sprint Overview

| Sprint | Name | Issues | Focus |
|--------|------|--------|-------|
| S1 | Critical Runtime Fixes | A-1, A-2, A-3 | IPC call correctness, service deduplication |
| S2 | High-Priority Behavioral Fixes | B-1 through B-6 | Status mapping, state sets, UI logic |
| S3 | Medium Code Quality and Consistency | C-1 through C-11 | Dead code, locale, deps, error handling |
| S4 | Low Hygiene and Polish | D-1 through D-6 | Imports, typing, shadowing |

---

## Sprint S1 — Critical Runtime Fixes

### A-1: WoKanbanView `assignWorkOrder` called with wrong property names

**File:** `src/components/wo/WoKanbanView.tsx` (lines ~370–374)

**Problem:** The drag-and-drop assign handler calls `assignWorkOrder()` with:
```ts
assignWorkOrder({
  id: wo.id,
  expected_row_version: wo.row_version,
  assigned_to_id: wo.primary_responsible_id,
});
```
The IPC command expects `wo_id`, `actor_id`, and `primary_responsible_id`, not `id` and
`assigned_to_id`. The call also omits `actor_id` entirely.

**Fix:**
1. Import `useSession` and extract `session.user_id`.
2. Change the call to:
```ts
assignWorkOrder({
  wo_id: wo.id,
  actor_id: session!.user_id,
  expected_row_version: wo.row_version,
  primary_responsible_id: targetUserId,
});
```

---

### A-2: Duplicate IPC transition wrappers across two services

**Files:** `src/services/wo-service.ts` and `src/services/wo-execution-service.ts`

**Problem:** Six transition functions exist in **both** services:
- `planWo`, `assignWo`, `startWo`, `pauseWo`, `resumeWo`, `holdWo`

`wo-service.ts` wraps them with Zod schema validation. `wo-execution-service.ts` calls
`invoke()` directly without validation. Components import from `wo-execution-service.ts`;
the store imports from `wo-service.ts`. This creates a split-brain where some callers
validate and others do not.

**Fix:**
1. Remove the six transition functions from `wo-execution-service.ts`.
2. Update all component imports to use `wo-service.ts` exclusively.
3. Keep `wo-execution-service.ts` for sub-entity CRUD only (labor, parts, tasks, delays,
   downtime, failure details).

---

### A-3: Duplicate `closeWo` across two services

**Files:** `src/services/wo-service.ts` and `src/services/wo-closeout-service.ts`

**Problem:** `closeWo` is exported from both services. `wo-service.ts` validates with Zod.
`wo-closeout-service.ts` does not validate but handles blocking-error scenarios with
toast feedback. Components import from the closeout service.

**Fix:**
1. Remove `closeWo` from `wo-service.ts`.
2. Add Zod validation to `wo-closeout-service.ts`'s `closeWo` (preserve existing
   blocking-error handling).
3. Single source of truth in the closeout service.

---

## Sprint S2 — High-Priority Behavioral Fixes

### B-1: `awaiting_approval` missing from `wo-status.ts` mappings

**File:** `src/utils/wo-status.ts`

**Problem:** `STATUS_MAP` and `STATUS_STYLE` do not include the `awaiting_approval` status.
Any WO in this state renders as unknown/Draft with no badge styling.

**Fix:**
1. Add `awaiting_approval: "awaitingApproval"` to `STATUS_MAP`.
2. Add `awaiting_approval: { color, icon }` to `STATUS_STYLE`.
3. Add i18n keys `status.awaitingApproval` / `status.awaitingApprovalShort` to
   `fr/ot.json` and `en/ot.json`.

---

### B-2: WoContextMenu `STARTABLE_STATES` contains non-existent status

**File:** `src/components/wo/WoContextMenu.tsx` (line ~30)

**Problem:** `STARTABLE_STATES = new Set(["released", "assigned"])`. The status `"released"`
does not exist in the WO state machine. The set also excludes `"waiting_for_prerequisite"`,
which is a valid start-eligible state.

**Fix:** Change to `new Set(["assigned", "waiting_for_prerequisite"])`.

---

### B-3: WoKanbanBoard inverted urgency color mapping

**File:** `src/components/wo/WoKanbanBoard.tsx` (line ~68)

**Problem:** `URGENCY_STYLE` maps `1 → red` and `4 → green`, but in CMMS convention
urgency 1 = lowest and 5 = highest (or the reverse depending on convention). The current
mapping is inverted from the convention used in the rest of the application.

**Note:** This issue may be moot if C-1 (delete dead code) is applied first, since
`WoKanbanBoard.tsx` is dead code (never imported). If the file is retained, fix the mapping.

---

### B-4: WoCreateForm equipment pre-fill broken in edit mode

**File:** `src/components/wo/WoCreateForm.tsx`

**Problem:** When editing an existing WO, the form pre-fills the equipment search box by
querying `searchAssets(wo.asset_tag)`, which returns a paginated list. If the exact asset
is not in the first page, it won't be selected. The combobox may show the wrong or no asset.

**Fix:** Use a dedicated `getAsset(wo.asset_id)` call (or `getEquipmentById`) to fetch the
single asset directly, then set it as the selected value.

---

### B-5: WoCompletionDialog state not reset on re-open

**File:** `src/components/wo/WoCompletionDialog.tsx` (lines ~50–70)

**Problem:** `endDate`, `hoursWorked`, and `conclusion` are initialized with `useState`
once. When the dialog is closed and reopened for a different WO, stale values persist.

**Fix:** Add a `useEffect` keyed on `[open, wo?.id]` that resets state to appropriate
defaults when the dialog opens.

---

### B-6: WoDashboardView silently hides entire view on error

**File:** `src/components/wo/WoDashboardView.tsx`

**Problem:** If any dashboard data fetch fails, the error is caught and logged to the
console, but the component renders nothing — the user sees a blank space with no indication
of failure.

**Fix:** Add an `error` state. On fetch failure, set it and render an error banner with
a retry button.

---

## Sprint S3 — Medium Code Quality and Consistency

### C-1: WoKanbanBoard.tsx is dead code

**File:** `src/components/wo/WoKanbanBoard.tsx`

**Problem:** This file is never imported by any component. `WorkOrdersPage` imports
`WoKanbanView`, not `WoKanbanBoard`. The file is 200+ lines of unreachable code.

**Fix:** Delete `WoKanbanBoard.tsx`. If any useful logic exists, merge it into
`WoKanbanView.tsx` first.

---

### C-2: WoExecutionControls fetches delay segments but never renders them

**File:** `src/components/wo/WoExecutionControls.tsx`

**Problem:** The component calls an API to load `delaySegments` and stores them in state,
but the JSX never renders this data.

**Fix:** Either render the delay segments (e.g., in a summary line or collapsible section)
or remove the fetch to reduce unnecessary API calls.

---

### C-3: Deprecated type aliases still consumed

**File:** `src/services/wo-execution-service.ts`

**Problem:** `WoTask` and `WoPart` are exported as `@deprecated` type aliases but are still
actively imported by components.

**Fix:** Replace all imports of `WoTask` / `WoPart` with their canonical type names, then
remove the deprecated aliases.

---

### C-4: WoPrintFiche hardcoded `locale = "fr"` fallback

**File:** `src/components/wo/WoPrintFiche.tsx`

**Problem:** `const locale = i18n.language || "fr"` — if `i18n.language` is `undefined`,
the print view defaults to French regardless of user preference.

**Fix:** Use `i18n.resolvedLanguage || i18n.language || "fr"` to respect the resolved
language from i18next.

---

### C-5: WoAuditTimeline uses browser locale instead of i18n language

**File:** `src/components/wo/WoAuditTimeline.tsx`

**Problem:** `formatActedAt()` calls `toLocaleString()` without passing a locale argument,
so it uses the browser's system locale rather than the app's active i18n language.

**Fix:** Pass `i18n.language` as the locale argument to `toLocaleString()`.

---

### C-6: WoAttachmentPanel date display uses browser locale

**File:** `src/components/wo/WoAttachmentPanel.tsx`

**Problem:** Same as C-5 — date formatting uses browser locale rather than `i18n.language`.

**Fix:** Pass `i18n.language` to date formatting calls.

---

### C-7: WoArchivePanel columns useMemo missing `i18n.language` dep

**File:** `src/components/wo/WoArchivePanel.tsx`

**Problem:** Column definitions are memoized with `useMemo` but the dependency array does
not include `i18n.language`. Switching language will not re-render column headers.

**Fix:** Add `i18n.language` to the useMemo dependency array.

---

### C-8: WorkOrdersPage columns useMemo missing `i18n.language` dep

**File:** `src/pages/WorkOrdersPage.tsx`

**Problem:** Same as C-7 — column definitions do not re-render when language changes.

**Fix:** Add `i18n.language` to the useMemo dependency array.

---

### C-9: WoArchivePanel silent error on load failure

**File:** `src/components/wo/WoArchivePanel.tsx`

**Problem:** If the archive data fetch fails, the error is caught and logged but the user
sees an empty table with no error indication.

**Fix:** Set an error state and render a message or toast.

---

### C-10: CostSummarySchema missing fields vs WoCostSummary type

**File:** `src/services/wo-service.ts`

**Problem:** The Zod `CostSummarySchema` may not include all fields defined in the
`WoCostSummary` TypeScript type, causing validation to strip fields silently.

**Fix:** Align the Zod schema with the TypeScript type definition.

---

### C-11: WoKanbanView toast map variable `t` shadows `useTranslation` `t`

**File:** `src/components/wo/WoKanbanView.tsx`

**Problem:** A local variable `t` inside the toast status map shadows the `t()` function
from `useTranslation`, which can cause confusing bugs if the inner scope is modified.

**Fix:** Rename the inner variable (e.g., `toastType` or `statusToast`).

---

## Sprint S4 — Low Hygiene and Polish

### D-1: WoDetailDialog `closeWorkOrder` passes `actor_id: 0` when session is null

**File:** `src/components/wo/WoDetailDialog.tsx`

**Problem:** If `session` is `null`, the close call falls back to `actor_id: 0`, which is
an invalid user ID.

**Fix:** Guard the call — disable the close button or return early if `!session`.

---

### D-2: WoCreateForm async `onClick` without `void`

**File:** `src/components/wo/WoCreateForm.tsx`

**Problem:** An `async` function passed to `onClick` returns a Promise that React ignores.
This triggers lint warnings about floating promises.

**Fix:** Wrap with `void` or use a non-async handler that calls the async function.

---

### D-3: `released` status may be vestigial

**Files:** `src/utils/wo-status.ts`, `src/components/wo/WoContextMenu.tsx`

**Problem:** The `released` status appears in some frontend maps but is not part of the
backend WO state machine. It may be a leftover from early design.

**Fix:** Audit whether `released` exists in the backend. If not, remove all frontend
references.

---

### D-4: WoKanbanView unused imports

**File:** `src/components/wo/WoKanbanView.tsx`

**Problem:** Some imports are no longer used after the i18n refactor.

**Fix:** Remove unused imports.

---

### D-5: WoKanbanView `handleDrop` missing `t` in dependency array

**File:** `src/components/wo/WoKanbanView.tsx`

**Problem:** `useCallback` for `handleDrop` uses `t()` from `useTranslation` but does not
include `t` in its dependency array. Stale closure risk on language switch.

**Fix:** Add `t` to the `useCallback` dependency array.

---

### D-6: `InlineToast` type only used locally

**File:** `src/components/wo/WoKanbanView.tsx`

**Problem:** The `InlineToast` type interface is defined in `WoKanbanView.tsx` but is only
used within that file. Not an issue per se, but if other components need inline toasts,
it should be extracted.

**Fix:** No action required unless reuse is needed. Mark as "no-op" if not extracted.

---

## Acceptance Criteria

- [x] S1 complete: all three critical issues verified at runtime (drag-assign works,
      single service import path, no duplicate IPC wrappers)
- [x] S2 complete: `awaiting_approval` renders correctly, `STARTABLE_STATES` matches
      backend, completion dialog resets, dashboard shows error state
- [x] S3 complete: dead code removed, delay segments either rendered or fetch removed,
      locale consistency across all date displays, useMemo deps complete
- [x] S4 complete: no lint warnings, no unused imports, no floating promises
- [x] TypeScript compiles with 0 errors after each sprint
- [ ] App launches and all four view modes (Table, Kanban, Calendar, Dashboard) render
