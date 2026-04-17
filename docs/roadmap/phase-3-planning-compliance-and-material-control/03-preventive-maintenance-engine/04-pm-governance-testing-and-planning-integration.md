# PM Governance Testing And Planning Integration

**PRD:** §6.9 / §6.16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - PM To Planning Contract (Without Fake Scheduler Features)
- Implement PM readiness projection contract that exposes `ready_for_scheduling` and blocked occurrences as consumable planning candidates.
- Return explicit blocker taxonomy aligned with research: missing parts, missing qualification, permit not ready, locked window, prerequisite incomplete.
- Keep implementation honest to current state: provide contract/API and persistence hooks even if full Planning page remains placeholder initially.
- Add assignment-policy prechecks using existing personnel availability/qualification and permit prerequisites where modules already expose data.
- Ensure PM candidate exposure does not mutate schedule commitment data directly until planning commit workflow is implemented.

**Cursor Prompt (S1)**
```text
Deliver PM-planning integration as a real readiness contract (candidate + blocker outputs) without pretending full scheduling already exists. Keep PM occurrences as source records and expose planner-safe projections only.
```

### S2 - Compliance, Efficiency, And Governance KPIs
- Add PM KPI query layer for compliance rate, overdue exposure, missed/deferred ratio, first-pass completion, and follow-up corrective ratio by period/entity/criticality.
- Add plan-vs-actual effort variance KPIs using existing WO actuals where available and PM estimates from plan versions.
- Add governance views for repeated deferrals, chronically late PMs, and strategy versions with deteriorating performance.
- Expose KPI outputs to dashboard/report services through typed contracts (no ad-hoc SQL in UI).
- Preserve auditability of KPI derivations (period boundaries, included statuses, and exclusion rules documented in code/tests).

**Cursor Prompt (S2)**
```text
Implement PM governance KPI queries and typed reporting contracts for compliance, overdue risk, first-pass completion, follow-up ratio, and plan-vs-actual effort variance with transparent derivation rules.
```

### S3 - Cross-Module Validation Matrix And Operator Checklist
- Add end-to-end tests for PM lifecycle + permissions + integration bridges (WO creation, inventory source tagging, notification emission, readiness projection).
- Validate that PM operations fail safely when dependent modules are unavailable or incomplete (graceful typed errors, no silent data loss).
- Add regression tests for concurrency (row-version conflicts on plan/occurrence updates and duplicate generation attempts).
- Maintain an operator validation checklist in this file footer for QA/UAT execution across PM strategy, generation, execution, and planning handoff.
- Gate completion on clean `cargo check` and `pnpm typecheck`, plus updated roadmap completion note.

**Cursor Prompt (S3)**
```text
Finalize PM governance with a full cross-module validation matrix and operator checklist: PM lifecycle, permissions, WO/inventory/notification integration, readiness contract behavior, and concurrency safety.
```

---

## Cross-Module Validation Matrix

| Area | Validation Scope | Expected Result |
|---|---|---|
| PM lifecycle | plan lifecycle + version governance + occurrence transitions + execution outcomes | Invalid transitions are rejected with typed validation errors; valid transitions persist auditable records. |
| Permission boundaries | `pm.view` vs `pm.edit` vs `pm.create` command access | Read-only users can query projections/metrics; mutating endpoints require edit/create scopes. |
| WO integration | optional PM -> WO generation and WO-backed execution actuals | Linked WO references remain traceable to occurrence IDs and execution actuals are reused when WO-backed. |
| Inventory integration | PM-origin stock movement / parts-dependency readiness blockers | PM-origin links stay traceable and readiness projection reports `missing_parts` when required parts are not planning-committed yet. |
| Notification/activity integration | due/missed/deferred/follow-up PM events | Notification emission stays dedupe-safe and activity events preserve occurrence/finding provenance. |
| Planning readiness contract | candidate + blocker outputs without schedule mutation | Projection reports planner-safe candidates/blockers while PM occurrences remain the source record. |
| Concurrency safety | stale row-version on plan/version/occurrence transitions | Conflicts fail explicitly; no silent overwrite on stale write paths. |

## Operator Checklist (QA/UAT)

- [ ] Generate PM occurrences twice with same inputs and confirm idempotent output (no duplicates).
- [ ] Run lifecycle transitions through valid path and verify invalid transitions are blocked with typed errors.
- [ ] Validate role boundary: `pm.view` can read occurrence/readiness/KPI endpoints but cannot mutate PM state.
- [ ] Execute one occurrence WO-backed and one non-WO-backed; confirm execution actuals and provenance links.
- [ ] Record execution findings that create follow-up DI/WO and verify notification + activity records are emitted once.
- [ ] Query readiness projection and confirm blocker taxonomy (`missing_parts`, `missing_qualification`, `permit_not_ready`, `locked_window`, `prerequisite_incomplete`) is explicit.
- [ ] Query PM governance KPI report and verify compliance, overdue risk, first-pass, follow-up ratio, and effort variance derivation notes are present.
- [ ] Force stale `row_version` in plan/version/occurrence updates and verify conflict failure without side effects.
- [ ] Confirm `cargo check`, PM test suite, and `pnpm typecheck` are clean before sign-off.

*Completion: 2026-04-15, verifier: pending, `cargo check` / `pnpm typecheck` notes: pending final rerun after S1-S3 implementation updates.*
