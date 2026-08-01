# SP07-F02 Gap Review and Handoff

Date: 2026-04-13
Scope: Phase 2 / Sub-phase 07 / File 02 (Archive migration, writer/integrity layer, archive commands, archive UI)

## Review Findings (ordered by severity)

1. High - Archive route integration gap (frontend)
   - `src/components/archive/ArchiveExplorer.tsx` and `src/components/archive/RetentionPolicyPanel.tsx` exist, but `src/pages/ArchivePage.tsx` still renders `ModulePlaceholder`.
   - Impact: Archive UI deliverables are not reachable from the live route.
   - Carry-forward action: First task in SP07-F03/04 must replace placeholder with mounted archive components and permission-gate the retention panel.

2. Medium - Immutable logging expectation on blocked guard paths
   - Requirement: purge/restore preserve immutable `archive_actions` on success/failure path.
   - Status: Patched in `src-tauri/src/commands/archive.rs`.
   - Result: blocked restore/purge guards now attempt to append failed action rows; success paths still append success rows.

3. Medium - Retention policy "Change history" persistence
   - `RetentionPolicyPanel` currently shows session-local history only.
   - Impact: No durable evidence trail in UI until activity/audit tables are wired.
   - Carry-forward action: Back history sidebar from `activity_events` / `audit_events` once SP07-F03 commands are live.

4. Low - Folder tree interaction regression risk
   - Archive tree filtering behavior needs explicit interaction coverage to ensure module/class/year expansion still works after filter-state updates.
   - Carry-forward action: add frontend interaction test in SP07-F03/04 validation suite.

## Applied Fixes in Current Iteration

- Updated `src-tauri/src/commands/archive.rs`:
  - Restore guard failures now append `archive_actions` with `result_status='failed'` before returning blocked/validation errors.
  - Purge blocked items now append `archive_actions` with `result_status='failed'`.
  - Added shared `write_archive_action(...)` helper for consistent immutable append behavior.

## Roadmap Carry-Forward Updates Added

- Updated File 03 roadmap:
  - `docs/roadmap/phase-2-core-execution-backbone/07-notifications-archive-and-audit-visibility/03-activity-feed-and-immutable-audit-journal.md`
  - Added explicit frontend carry-forward tasks:
    - mount archive UI in `ArchivePage`
    - verify tree interactivity
    - persist retention history
    - emit activity events for blocked restore/purge paths
  - Corrected migration path references to `src-tauri/src/migrations/...`.
  - Updated FK guidance from `org_units` to verified `org_nodes` convention.

- Updated File 04 roadmap:
  - `docs/roadmap/phase-2-core-execution-backbone/07-notifications-archive-and-audit-visibility/04-observability-permissions-and-cross-module-validation.md`
  - Added Phase-closure checks for archive page integration, immutable blocked-action logging, tree interaction validation, and persistent retention history.
  - Corrected migration path references to `src-tauri/src/migrations/...`.

## Validation Snapshot

- `cargo check` passed after archive command logging changes.
- `pnpm typecheck` passed.

## Next-file Execution Priority

1. Wire `ArchivePage` to real archive components (not placeholder).
2. Persist retention-policy history through activity/audit commands.
3. Add interaction tests for archive tree expand/collapse + filtering.
4. Add observability tests verifying blocked restore/purge paths are logged and visible.
