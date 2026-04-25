# Planning KPIs Validation And Operational Controls

**PRD:** §6.16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Core KPI Query Layer And Metric Contracts

- Implement KPI query service in `src-tauri/src/planning/` covering the seven core planning metrics: schedule adherence, ready backlog size, blocked backlog size (by blocker type), emergency break-in ratio, committed vs completed hours, duration-estimate accuracy, and PM-work completed on committed date.
- Standardize KPI payload fields for workforce efficiency metrics: `planned_labor_hours`, `actual_labor_hours`, `labor_variance_hours`, `wrench_time_ratio`, `labor_cost_per_effective_hour`, `assignment_replan_count` — all returned as typed Rust structs with nullable/zero-safe handling.
- Add cost-performance KPI queries: planned labor cost vs actual labor cost by WO assignee and team; expose variance drilldowns with WO-level granularity linked to `schedule_commitments` and personnel rate cards.
- Add workforce efficiency KPI queries: wrench-time ratio proxy, assignment utilization rate, schedule-commit hit rate by technician/team, and repeat-delay causes (grouped by blocker taxonomy from `scheduling_conflicts`).
- Preserve auditability of KPI derivations: period boundaries, included statuses, and exclusion rules must be documented in code/test assertions — no opaque aggregates.
- Enforce `plan.view` for KPI read access; KPI endpoints must not expose raw personnel cost data to roles without `per.report` in addition to `plan.view`.

**Cursor Prompt (S1)**
```text
Implement the planning KPI query layer with the seven core schedule-discipline metrics plus workforce efficiency and cost-performance KPIs. Standardize payload field contracts, enforce plan.view + per.report permission split for cost data, and document derivation rules in tests.
```

### S2 - Cross-Module Causality, Drilldowns, And Dashboard Integration

- Ensure KPI drilldowns preserve DI origin: every schedule adherence deviation must be traceable back to its source DI/WO identifier, assignee availability conflict record, and asset readiness risk reason codes from the readiness evaluator.
- Implement cross-module causality links in drilldown payloads: blocked-backlog items must link to their `scheduling_conflicts` records; break-in items must link to `schedule_break_ins`; labor variance items must link to WO actuals from module 6.5.
- Add team-workload balancing KPI outputs: load percentage by team per period, skill-bottleneck identification (skills in demand vs available coverage), and one-click rebalance suggestion inputs (candidate moves that satisfy skill/readiness constraints).
- Add planning accuracy trend queries: rolling duration-estimate accuracy by WO type, PM strategy, and assignee team; surface chronic underestimators or systematically overloaded teams.
- Integrate KPI outputs with dashboard/report service contracts: expose typed planning KPI summaries consumable by the main dashboard and future report module without ad-hoc SQL in the UI layer.
- Add ERP schedule export hook (Enterprise-gated): expose committed weekly maintenance schedule as a JSON snapshot on the ERP connector endpoint for production-planning alignment.

**Cursor Prompt (S2)**
```text
Wire planning KPI drilldowns with full cross-module causality (DI origin, conflict records, WO actuals), add team workload and planning accuracy trend queries, and expose dashboard-ready typed contracts. Implement ERP schedule export hook behind the Enterprise feature flag.
```

### S3 - Validation Suite, Operator Checklist, And End-To-End Completion

- Add scenario tests for reschedule and conflict paths: availability-calendar conflicts, stale assignment race conditions, override-audit completeness, and freeze-breach handling.
- Add edge tests for KPI derivation correctness: zero-commitment periods, all-blocked-backlog states, periods with only break-in work, and mixed PM/WO/inspection commitment sets.
- Add performance tests for KPI query response under realistic data volumes (hundreds of commitments, thousands of candidates, multi-period backlog history).
- Add integration tests for full planning chain: candidate creation -> readiness evaluation -> conflict detection -> commitment -> freeze -> break-in -> KPI derivation -> notification emission -> activity feed event.
- Add permission tests proving `plan.view` can read KPI and backlog endpoints while `plan.edit`, `plan.confirm`, and `plan.windows` remain separate enforcement layers.
- Gate completion on clean `cargo check` and `pnpm typecheck`, plus updated roadmap completion note in this file.

**Cursor Prompt (S3)**
```text
Finalize planning KPI validation with a full scenario test matrix including reschedule conflicts, stale assignments, all-blocked states, and multi-module chain integration. Enforce permission boundaries for all KPI and planning endpoints and confirm clean cargo check and pnpm typecheck before sign-off.
```

---

## Cross-Module Validation Matrix

| Area | Validation Scope | Expected Result |
|---|---|---|
| Readiness evaluation | Five-dimension blocker check (parts/skills/permits/windows/prerequisites) | All five dimensions evaluated; each unresolved dimension generates a typed `scheduling_conflicts` record with correct `conflict_type`. |
| Permission boundaries | `plan.view` vs `plan.edit` vs `plan.confirm` vs `plan.windows` | Read-only users access backlog and KPI views; mutating endpoints (commit, freeze, break-in, window management) require the specific permission scope. |
| Commitment gating | Three-gate validation (readiness + capacity + lock-window) before commit | Commitment fails with typed error if any of the three gates is unmet; partial success is not allowed. |
| Double-booking prevention | No-overlap check per personnel across WO/PM/permit duties | Overlapping assignments are rejected with structured `ASSIGNEE_UNAVAILABLE` payload; override requires explicit reason and creates `schedule_change_log` record. |
| Personnel availability binding | Assignee conflict checks against personnel calendar (shift, leave, restriction, training blocks) | Unavailable personnel blocks assignment commit; `nearest_feasible_window` is returned inline for planner resolution. |
| Freeze-window enforcement | Mutations to frozen commitment slots without break-in workflow | Direct edits to frozen commitments are hard-rejected by the service layer; break-in path enforces approver and coded reason. |
| Break-in governance | Emergency assignment qualification check and override audit | Emergency assignments still run qualification check; bypasses require override reason and generate a dangerous audit event in module 6.17. |
| Notification richness | Break-in/freeze-breach notification payloads | Notifications include impacted assignee, old/new slot, skill gap flag, and cost-impact delta; duplicate suppression prevents repeated alerts for the same slot. |
| KPI derivation integrity | Core seven metrics + workforce efficiency + cost-performance KPIs | KPI values are reproducible across equivalent period/filter inputs; derivation rules are covered by assertions in test layer. |
| Cross-module causality | KPI drilldowns preserving DI origin, conflict records, and WO actuals | Drilldown payloads carry source DI/WO identifiers, assignee conflict references, and asset risk reason codes without data loss. |
| ERP export hook | Committed weekly schedule JSON on connector endpoint | Enterprise-gated endpoint returns correctly structured snapshot; non-Enterprise license returns feature-not-available error (not 500). |
| Concurrency safety | Stale row_version on candidate, commitment, and conflict updates | Conflicts fail explicitly with typed error; no silent overwrite on any stale write path in the planning module. |

## Operator Checklist (QA/UAT)

- [ ] Create schedule candidates from WO, PM occurrence, and inspection follow-up sources; confirm `source_type` and `source_id` are correctly populated.
- [ ] Run readiness evaluator against a candidate missing parts, missing qualification, and missing permit simultaneously; confirm all three blockers appear as separate `scheduling_conflicts` records with correct `conflict_type`.
- [ ] Attempt to commit a candidate while a personnel availability conflict exists; confirm `ASSIGNEE_UNAVAILABLE` rejection with populated conflict intervals and `nearest_feasible_window`.
- [ ] Attempt double-booking of the same personnel for two overlapping commitments; confirm no-overlap rejection and verify override workflow creates a `schedule_change_log` record with actor and reason.
- [ ] Freeze a schedule period and attempt a direct slot edit; confirm service-layer rejection without going through break-in workflow.
- [ ] Create a break-in with emergency reason; confirm approver is required, qualification check runs, and a rich notification (assignee, slot delta, cost-impact) is emitted to affected technicians.
- [ ] Bypass qualification check on a break-in via override; confirm dangerous audit event is captured in module 6.17 with override actor, reason code, and timestamp.
- [ ] Query each of the seven core KPIs for a test period and verify values are non-zero and consistent with committed/completed records in the database.
- [ ] Query workforce efficiency KPIs (`wrench_time_ratio`, `assignment_replan_count`, `schedule_commit_hit_rate`) and confirm fields are populated with typed values, not nulls.
- [ ] Query cost-performance KPI drilldown for a specific WO assignee and verify planned vs actual labor cost variance links back to WO actuals.
- [ ] Confirm KPI drilldowns for a blocked-backlog item link to its `scheduling_conflicts` record and carry DI origin and asset risk reason codes.
- [ ] Force stale `row_version` on a commitment update and verify conflict failure without side effects.
- [ ] Confirm `cargo check`, planning test suite, and `pnpm typecheck` are clean before sign-off.

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
