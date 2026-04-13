# SP07 File 05 — Frontend Hardening Carry-Forward

Date: 2026-04-13
Scope: Post-File-04 professional review (frontend-focused)

---

## Review Findings (Frontend)

### H-01: Single-item archive export path lacks error handling
**File:** `maintafox-desktop/src/components/archive/ArchiveExplorer.tsx`
**Severity:** HIGH  
**Issue:** `ArchiveItemDetailView` export action triggers `exportArchiveItems(...).then(...)` without `try/catch` or user feedback. IPC failures can become silent/unhandled promise errors.
**Fix:** Wrap single-item export flow in `try/catch`, set `actionError` with `toErrorMessage(err)`, and disable export button while request is in flight.

### H-02: Activity and audit filter controls still have weak accessibility semantics
**Files:** 
- `maintafox-desktop/src/components/activity/ActivityFeedPanel.tsx`
- `maintafox-desktop/src/components/activity/AuditLogViewer.tsx`
**Severity:** HIGH  
**Issue:** Several controls still rely only on placeholders (no explicit labels) and checkbox text is not consistently bound with `htmlFor` for assistive technology.
**Fix:** Add explicit `id` + `<label htmlFor=...>` or `aria-label` for all filter inputs and toggle controls. Keep placeholder text as helper only.

### M-01: Retention policy updates are fired on every numeric keystroke
**File:** `maintafox-desktop/src/components/archive/RetentionPolicyPanel.tsx`
**Severity:** MEDIUM  
**Issue:** `retention_years` updates are sent from `onChange`, producing multiple write requests while typing (e.g., entering "15" emits "1" then "15").
**Fix:** Switch to staged local edit state with commit on blur/Enter (or debounce), then persist once.

### M-02: Audit detail expansion hides detail-fetch errors
**File:** `maintafox-desktop/src/components/activity/AuditLogViewer.tsx`
**Severity:** MEDIUM  
**Issue:** On expansion, `getAuditEvent` errors are swallowed in a blank `catch`, leaving users with no explanation.
**Fix:** Surface a per-row error state (or shared non-blocking toast/error line) when detail loading fails.

### M-03: Saved-view selector UX in activity feed is non-persistent
**File:** `maintafox-desktop/src/components/activity/ActivityFeedPanel.tsx`
**Severity:** MEDIUM  
**Issue:** Saved view `<select>` uses fixed `value=""`, so chosen option is not reflected after selection.
**Fix:** Track selected saved-view id in state and bind to `<select value={selectedSavedViewId}>` for clearer UX and keyboard consistency.

---

## Execution Checklist for File 05

- [x] Add robust error handling for single-item archive export (`ArchiveExplorer` detail panel)
- [x] Complete accessibility labeling pass for activity/audit filter controls
- [x] Refactor `RetentionPolicyPanel` numeric edits to commit-on-blur/Enter
- [x] Add user-visible error feedback on audit detail expansion failure
- [x] Persist saved-view selection state in `ActivityFeedPanel`
- [x] Add/extend Vitest coverage for all above flows

**Completed:** 2026-04-13 — `pnpm typecheck` and targeted Vitest suites for ArchiveExplorer, ActivityFeedPanel, AuditLogViewer, and RetentionPolicyPanel all pass.

---

## Validation Plan

- `pnpm typecheck`
- `pnpm test -- src/components/archive/__tests__/ArchiveExplorer.test.tsx`
- Add targeted tests:
  - `src/components/activity/__tests__/ActivityFeedPanel.*.test.tsx`
  - `src/components/activity/__tests__/AuditLogViewer.*.test.tsx`
  - `src/components/archive/__tests__/RetentionPolicyPanel.*.test.tsx`

---

## Notes

- This carry-forward is frontend-only and does not change backend schemas or command contracts.
- Phase 3 tech debt list from File 04 remains valid; this file captures additional UX/accessibility/reliability hardening discovered after File 04 closure.
