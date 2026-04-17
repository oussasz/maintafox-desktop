# Ready Backlog And Blocked Work Model

**PRD:** §6.16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Planning Backend Bootstrap And Schema Foundations

- Create planning backend module: `src-tauri/src/planning/` module + `src-tauri/src/commands/planning.rs` + command registration in `src-tauri/src/commands/mod.rs` and `src-tauri/src/lib.rs` invoke handler.
- Add first planning migration backbone: `schedule_candidates`, `scheduling_conflicts`, `schedule_change_log` with FK integrity, `row_version` where mutable, timestamps, and non-destructive lifecycle semantics.
- Model `schedule_candidates` with all PRD-specified fields: `source_type` (work_order/pm_occurrence/inspection_follow_up/project), `source_id`, `readiness_status` (not_ready/ready/committed/dispatched/completed), `readiness_score`, `required_skill_set_json`, `required_parts_ready`, `permit_status`, `shutdown_requirement`, `estimated_duration_hours`.
- Enforce permission boundary using `plan.view` / `plan.edit` / `plan.confirm` / `plan.windows` on every planning command; no command may bypass the permission check layer.
- Add baseline typed errors: invalid readiness transition, stale row version, unknown source reference, missing priority, and blocker-type constraint violations.
- Keep all planning writes in `src-tauri/src/planning/` service/query layer with transactional guards; no direct SQL in the command layer.

**Cursor Prompt (S1)**
```text
Bootstrap the planning backend from zero by adding a dedicated Rust planning module, first planning migrations for schedule_candidates and scheduling_conflicts, and Tauri command registration. Enforce plan.* permissions and optimistic concurrency from the start.
```

### S2 - Readiness Evaluator, Blocker Taxonomy, And Candidate Lifecycle

- Implement Rust readiness evaluator that scores each candidate against five structured blocker dimensions: parts availability (`required_parts_ready`), skill coverage (match against `required_skill_set_json` and personnel qualifications), permit status (`permit_status` from module 6.23), shutdown/access window availability, and prerequisite completeness (prior inspection or diagnostic step).
- Enforce explicit blocker taxonomy aligned with PRD and research: `missing_critical_part`, `no_qualified_technician`, `permit_not_ready`, `locked_window`, `prerequisite_incomplete`, `skill_gap`, `double_booking`.
- Record every detected conflict in `scheduling_conflicts` with `conflict_type`, `detected_at`, `resolved_at`, and `resolution_notes`.
- Implement personnel availability binding: include assignee/team availability windows from personnel calendars (shift, leave, medical/restriction blocks, training blocks) as a first-class blocker dimension.
- Add WO assignment guardrails: block assignment/commit when the selected personnel record is unavailable; return structured `ASSIGNEE_UNAVAILABLE` payload with `personnel_id`, conflict intervals, block type, and `critical` flag.
- Add DI-to-WO readiness preflight fields on candidates: `suggested_assignees`, `availability_conflict_count`, `skill_match_score`, `estimated_labor_cost_range`, `blocking_flags` so planning context is preserved from conversion through commitment.
- Include asset-driven readiness hints on blocked-work scoring: `open_work_count`, `next_available_window`, `estimated_assignment_risk`, `risk_reason_codes`.

**Cursor Prompt (S2)**
```text
Implement the planning readiness evaluator with a structured five-dimension blocker taxonomy (parts, skills, permits, windows, prerequisites) and persist detected conflicts in scheduling_conflicts. Include personnel availability binding and DI-to-WO preflight fields so assignment guardrails are enforced at evaluation time, not only at UI level.
```

### S3 - Backlog UX, IPC Contracts, And Regression Safety

- Add planning IPC contracts to `shared/ipc-types.ts` and frontend planning service calls with runtime Zod decoding consistent with other modules.
- Replace `src/pages/PlanningPage.tsx` placeholder with real ready-backlog and blocked-backlog board columns bound to live readiness evaluation outputs (no mock rows, no static datasets).
- Implement planner explainability panel: show readiness score decomposition (parts/skills/permits/availability) before a candidate can be moved to committed schedule.
- Add `plan.view` read-only guards so backlog and conflict views are accessible without `plan.edit`; commit and override actions require elevated permissions.
- Populate planning i18n namespaces (`src/i18n/locale-data/en/planning.json`, `src/i18n/locale-data/fr/planning.json`) with all keys used by backlog and conflict UI.
- Add unit and integration tests: readiness evaluation accuracy, blocker taxonomy coverage, permission gating, stale row-version rejection, and duplicate candidate protection.
- Add smoke coverage that planning navigation, backlog load, and readiness evaluation work end-to-end against real IPC (not placeholder rendering).

**Cursor Prompt (S3)**
```text
Finalize ready-backlog delivery end-to-end: typed IPC contracts, real planning page backlog columns with readiness score decomposition, localized strings, and tests for evaluation accuracy, permission boundaries, and concurrency edge cases. No placeholder data paths at completion.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
