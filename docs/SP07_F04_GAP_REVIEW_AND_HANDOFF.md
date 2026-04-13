# SP07-F04 Gap Review and Handoff

Date: 2026-04-13  
Scope: Post-File-04 professional review (frontend-first carry-forward)

---

## Findings (Ordered by Severity)

### F4-01 — Single-item archive export lacks safe error handling (HIGH)
**File:** `src/components/archive/ArchiveExplorer.tsx`  
**Issue:** The detail-panel single-item export path triggers `exportArchiveItems(...).then(...)` without a guarded `try/catch` flow. IPC failure can produce silent/unhandled promise behavior.  
**Recommended fix:** Introduce `actionBusy` + `actionError` handling for export path (same pattern used by restore/purge dialogs), and show user-visible failure text.

### F4-02 — Activity/Audit filter controls still lack explicit labels (HIGH)
**Files:**  
- `src/components/activity/ActivityFeedPanel.tsx`  
- `src/components/activity/AuditLogViewer.tsx`  
**Issue:** Multiple controls rely on placeholder-only semantics; screen readers do not get strong label associations.  
**Recommended fix:** Add explicit `id` + `<label htmlFor>` (or `aria-label`) for all filter inputs/toggles.

### F4-03 — Retention years update is write-on-keystroke (MEDIUM)
**File:** `src/components/archive/RetentionPolicyPanel.tsx`  
**Issue:** `retention_years` mutation is triggered on every `onChange` character, causing noisy backend writes during input composition.  
**Recommended fix:** Use staged local input state and commit on blur/Enter (or debounce).

### F4-04 — Audit detail fetch failure is silently swallowed (MEDIUM)
**File:** `src/components/activity/AuditLogViewer.tsx`  
**Issue:** Row expansion catches detail fetch errors but suppresses feedback. User sees expansion without diagnostics.  
**Recommended fix:** Track per-row detail error state (or global non-blocking alert) and render it in expanded panel.

### F4-05 — Saved view selector state is non-persistent (MEDIUM)
**File:** `src/components/activity/ActivityFeedPanel.tsx`  
**Issue:** Saved-view `<select>` is pinned to `value=""`, so selected choice is not reflected post-selection.  
**Recommended fix:** Track selected saved-view id in component state and bind to `value`.

---

## Carry-Forward Checklist

- [ ] Harden single-item archive export with guarded error/UI state
- [ ] Add explicit accessibility labels to activity/audit filters
- [ ] Refactor retention-years edit flow to commit-on-blur/Enter
- [ ] Surface audit detail fetch failures
- [ ] Persist selected saved-view state in activity feed
- [ ] Add targeted Vitest coverage for each item

---

## Validation Commands for Follow-up

- `pnpm typecheck`
- `pnpm test -- src/components/archive/__tests__/ArchiveExplorer.test.tsx`
- Add/execute targeted suites:
  - `src/components/activity/__tests__/ActivityFeedPanel.*.test.tsx`
  - `src/components/activity/__tests__/AuditLogViewer.*.test.tsx`
  - `src/components/archive/__tests__/RetentionPolicyPanel.*.test.tsx`
