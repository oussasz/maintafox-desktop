# Cost Center Structure And Budget Versioning

**PRD:** §6.24

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Cost Center Hierarchy And Budget Baseline Schema
- Add budget backend module and migrations for `cost_centers`, `budget_versions`, and `budget_lines` with FK integrity, timestamps, and `row_version` on mutable records.
- Model `cost_centers` with hierarchy and scope fields from the PRD: `entity_id`, `parent_cost_center_id`, `budget_owner_id`, `erp_external_id`, and `is_active`.
- Model `budget_versions` with governed scenario and status fields: `scenario_type` (`original`, `approved`, `reforecast`, `what_if`) and `status` (`draft`, `submitted`, `approved`, `frozen`, `closed`, `superseded`).
- Model `budget_lines` with period-aware planning structure: `period_month`, `budget_bucket`, `planned_amount`, `source_basis`, `justification_note`, and optional reporting tags for asset family, work category, and shutdown package.
- Add labor-budget dimensions on budget lines for `team_id`, `skill_pool_id`, and contractor lane so labor baselines can later reconcile with planning and WO actuals.
- Enforce hierarchical integrity rules: no self-parenting, no cyclical parent chain, and no active child under an inactive parent cost center.

**Cursor Prompt (S1)**
```text
Build the budget baseline schema layer with cost center hierarchy, versioned budget records, and period-aware budget lines. Enforce hierarchy integrity, row_version concurrency, and labor-budget dimensions from the start so later forecast and actual rollups stay traceable.
```

### S2 - Budget Lifecycle Governance And Approval Discipline
- Implement budget lifecycle transitions with hard guards: `draft -> submitted -> approved -> frozen`, with `closed` and `superseded` end states and no direct mutation of frozen baselines.
- Enforce "one frozen control baseline per fiscal year and scenario" at the service layer; attempts to freeze a second active baseline for the same scope must fail with typed validation errors.
- Require reason capture and approver evidence for manual baseline adjustments after submission; adjustments on approved/frozen versions must create a governed successor version instead of in-place edits.
- Add workforce-capacity assumption capture on version header or supporting metadata: headcount basis, shift-hours basis, expected utilization, contractor share, and planning-note reference.
- Add permission boundaries for `fin.view`, `fin.budget`, and `fin.report`; read access must not imply create/edit/freeze authority.
- Keep all budget writes in the finance service layer with transactional guards; no direct SQL or ad-hoc lifecycle transitions from command handlers.

**Cursor Prompt (S2)**
```text
Implement budget lifecycle governance with strict draft/submitted/approved/frozen transitions, single frozen baseline enforcement per fiscal year and scenario, and governed successor-version creation instead of mutating frozen records. Capture labor-capacity planning assumptions as auditable baseline evidence.
```

### S3 - Budget Authoring UX, IPC Contracts, And Baseline Explainability
- Add finance IPC contracts to `shared/ipc-types.ts` for cost center tree, budget version list, version lifecycle actions, and budget line editing with runtime validation consistent with the rest of the app.
- Replace `BudgetPage` placeholder behavior with real cost center structure and baseline authoring flows: hierarchy browser, version selector, line-item editor, and status banner showing whether the version is editable or frozen.
- Show explicit baseline explainability fields in the UI: scenario type, planning basis, source basis mix, labor-capacity assumptions, and ERP external reference where present.
- Add edit guards in the frontend so frozen versions become read-only while draft/submitted versions remain editable only for users with `fin.budget`.
- Populate finance i18n namespaces for budget lifecycle states, scenario types, bucket names, and freeze-governance messages in both `en` and `fr`.
- Add tests for hierarchy validation, duplicate frozen-baseline rejection, stale `row_version` rejection, permission enforcement, and frozen-version read-only behavior.

**Cursor Prompt (S3)**
```text
Deliver budget baseline authoring end-to-end with typed IPC contracts, real BudgetPage editing flows, baseline explainability fields, lifecycle-aware edit guards, and regression tests for hierarchy rules, duplicate frozen baseline rejection, permissions, and stale row_version conflicts.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
