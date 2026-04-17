# Break Ins Freeze Windows And Team Notifications

**PRD:** §6.16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Break-In Schema, Freeze Enforcement, And Governance Rules

- Add `schedule_break_ins` migration: schedule_commitment_id, break_in_reason (emergency/safety/production_loss/regulatory/other), approved_by_id, created_at; enforce FK to `schedule_commitments` and require non-null approver for all break-in records.
- Implement freeze-window enforcement on `schedule_commitments`: any attempt to insert, update, or delete a committed slot whose `frozen_at` is set must be rejected unless it is routed through the break-in or authorized reschedule workflow.
- Add production window manager backend: `plan.windows` permission required to create/update/delete `planning_windows`; `is_locked` flag must surface as a hard gate in the commitment service (not just a UI hint).
- Add coded break-in reason validation: reject break-in records without a recognized `break_in_reason` code; extend the reason taxonomy if plant-specific production-loss codes are needed via governed lookup domains.
- Enforce approver presence on break-ins: emergency and safety codes require `approved_by_id` to be a personnel record with `plan.confirm` or `plan.windows` scope; override-without-approval path must be tracked as a dangerous audit event.
- Keep break-in source discipline: break-ins must reference a specific `schedule_commitment_id`; break-ins that create net-new work without a prior commitment are sourced through the candidate/commit path first.

**Cursor Prompt (S1)**
```text
Implement break-in schema, freeze-window enforcement, and production window manager backend. Freeze violations must be hard-rejected by the service layer, break-ins must require coded reasons and approver evidence, and dangerous overrides must be captured in the audit trail.
```

### S2 - Break-In Assignee Validation, Notification Payload, And Change Log

- Add break-in assignee validation: emergency assignment still checks real-time personnel availability and qualification through the same evaluator used by the readiness engine; do not bypass qualification checks for break-in work.
- Require explicit override reason when a qualification or availability check is bypassed on a break-in; log override actor, reason code, and timestamp in `schedule_change_log`.
- Implement schedule change log writes for all break-in, reschedule, and freeze-breach events: field_changed, old_value, new_value, changed_by_id, changed_at, reason_code, reason_note must all be populated — no partial audit records.
- Build notification payload richness for break-in and freeze-breach events: include impacted assignee identity, old slot, new slot, skill gap flag (if qualification was insufficient), and estimated cost-impact delta in the notification body sent to SP07.
- Emit break-in and freeze-breach events into the activity feed (module 6.17) using the same correlation pattern as PM misses; break-in events must link to the source commitment record.
- Add duplicate-notification suppression for repeated freeze-breach warnings on the same commitment slot within the same planning period.

**Cursor Prompt (S2)**
```text
Wire break-in assignee validation, rich notification payloads, and schedule change log discipline. Every break-in must check availability and qualifications, produce an override reason when bypassed, and emit a structured notification including impacted assignee, slot delta, and cost-impact estimate.
```

### S3 - Production Window Manager UX, Team Notifications, And Tests

- Add planning IPC contracts for `planning_windows`, `schedule_break_ins`, and `schedule_change_log` to `shared/ipc-types.ts` with Zod runtime decoding in the planning service.
- Implement production window manager panel on `PlanningPage`: CRUD for planning windows, freeze toggle with `plan.windows` permission guard, and visual overlay of locked vs open windows on the Gantt timeline.
- Implement notify-teams action: after a schedule is confirmed (`plan.confirm`), trigger in-app and OS notifications to all assigned technicians through module 6.14; notification body must include committed work items, start times, and any break-in modifications for the upcoming period.
- Expose break-in work tracking view for supervisors: list break-ins by reason code, approver, impacted commitments, and schedule-discipline impact for the current period.
- Populate planning i18n keys for break-in reasons, freeze-window states, and notification bodies in both `en` and `fr` locale namespaces.
- Add regression tests: freeze-violation rejection, break-in approval enforcement, override audit trail completeness, notification emission with correct payload fields, and duplicate-notification suppression.

**Cursor Prompt (S3)**
```text
Finalize break-in and notification delivery end-to-end: production window manager panel, freeze overlay on Gantt, notify-teams action with rich payload, supervisor break-in tracking view, and regression tests for freeze enforcement, approval gating, notification correctness, and duplicate suppression.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
