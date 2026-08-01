# Capacity Calendars Commitment And Gantt Logic

**PRD:** §6.16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Capacity Rules, Planning Windows, And Commitment Schema

- Add planning migrations for `capacity_rules` (entity_id, team_id, effective_date, available_hours_per_day, max_overtime_hours_per_day) and `planning_windows` (entity_id, window_type, start_datetime, end_datetime, is_locked) as defined in the PRD data model.
- Add `schedule_commitments` migration: schedule_period_start/end, source_type/id, committed_start/end, assigned_team_id, assigned_personnel_id, frozen_at, committed_by_id, row_version.
- Enforce `plan.windows` permission for capacity-rule and planning-window mutations; `plan.confirm` for commitment creation and freeze actions.
- Implement capacity availability service: compute available hours per team/day from active `capacity_rules` and subtract committed hours from existing `schedule_commitments` to produce real-time load indicators.
- Enforce planning-window lock semantics: a `is_locked = true` window must block commitment creation inside its time range; unlocked maintenance windows are scheduling-eligible.
- Add duplicate-period protection for capacity rules: no overlapping effective-date ranges for the same entity/team combination.

**Cursor Prompt (S1)**
```text
Build the capacity and commitment schema layer: capacity_rules, planning_windows, and schedule_commitments with locking semantics, correct FK integrity, row_version concurrency, and plan.* permission enforcement on every mutation path.
```

### S2 - Commitment Workflow, Double-Booking Prevention, And Gantt Engine

- Implement schedule commitment creation with backend validation: readiness check against `schedule_candidates`, availability capacity check against current team load, and lock-window constraint check — all three must pass before commitment persists.
- Enforce double-booking prevention: no-overlap checks across WO/PM/permit duties per personnel, with a structured override workflow that requires explicit reason capture and records the override actor in `schedule_change_log`.
- Add hard availability gate: reject assignment with `ASSIGNEE_UNAVAILABLE` structured error (personnel_id, conflict intervals, block_type, critical flag) when selected personnel overlaps blocked windows from personnel calendars (SP01).
- Implement drag-reschedule validation: when a committed item is moved to a new slot, all three checks re-run in a single atomic backend transaction; partial success is not allowed.
- Add commitment UX lock contract: expose a `has_blocking_conflict` flag and `nearest_feasible_window` field in the commitment response so the frontend can disable assign/commit CTA and show the inline resolution hint without polling.
- Add cost-aware commitment: compute estimated labor cost at commitment time using personnel rate card + planned hours from SP01; expose `estimated_labor_cost`, `budget_threshold`, and `cost_variance_warning` fields in the commitment payload.

**Cursor Prompt (S2)**
```text
Implement schedule commitment with three-gate backend validation (readiness, capacity, lock-window), double-booking prevention with override audit, hard ASSIGNEE_UNAVAILABLE rejection, and cost-aware commitment payload. Drag-reschedule must re-run all checks atomically.
```

### S3 - Gantt UI, Assignment Calendar UX, Freeze Action, And Tests

- Add planning IPC contracts to `shared/ipc-types.ts` for commitments, capacity load, and Gantt timeline data with Zod runtime decoding in the planning service.
- Implement Gantt timeline component on `PlanningPage`: horizontal drag-and-drop timeline, capacity indicator bar (green/amber/red thresholds), overtime display, and blocked-work override indicator.
- Add assignment calendar UX: in the WO assignment flow, display per-person calendar lane with availability/blocked segments from personnel calendars before the assignee is confirmed.
- Implement schedule freeze action (`plan.confirm` permission): snapshot the committed schedule for the selected period by setting `frozen_at`; post-freeze changes must go through the break-in or reschedule workflow rather than silent edits.
- Implement Gantt PDF export hook: A3/A4 print-ready export with company logo header showing the committed schedule for the selected period.
- Consume SP01 personnel availability data for team calendar lanes; do not replicate personnel availability tables — query through the personnel module contract.
- Add regression tests: commitment validation gating (all three checks), double-booking detection, override audit trail completeness, freeze-period protection, and stale row-version rejection on commitment edits.

**Cursor Prompt (S3)**
```text
Finalize Gantt and commitment delivery end-to-end: Gantt timeline with drag-and-drop, assignment calendar lane UX, schedule freeze action, Gantt PDF export, and full regression tests for commitment gating, double-booking, override audits, and freeze-period integrity. No placeholder data paths at completion.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
