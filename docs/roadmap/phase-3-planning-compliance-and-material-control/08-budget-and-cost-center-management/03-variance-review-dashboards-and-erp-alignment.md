# Variance Review Dashboards And ERP Alignment

**PRD:** §6.24

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Variance Review Workflow And Driver Taxonomy
- Add `budget_variance_reviews` workflow with required fields from the PRD: `budget_version_id`, `cost_center_id`, `period_month`, `variance_amount`, `variance_pct`, `driver_code`, `action_owner_id`, `review_status`, and `reviewed_at`.
- Implement governed variance-review lifecycle: `open -> in_review -> actioned -> accepted -> closed`, with explicit reopen handling when new postings materially change the same period.
- Require coded driver taxonomy aligned with research and operations: `emergency_break_in`, `vendor_delay`, `labor_overrun`, `estimate_error`, `scope_change`, `price_increase`, `permit_delay`, `availability_loss`, `repeat_failure`, `shutdown_scope_growth`.
- Link every variance-review record to baseline, commitment, actual, and forecast snapshots used to open the review so later re-renders stay reproducible.
- Preserve accountable-owner discipline: material variance cannot move to closed state without an assigned reviewer and commentary on disposition or next action.
- Add typed service errors for missing driver code, duplicate open review for same scope, and invalid lifecycle transitions.

**Cursor Prompt (S1)**
```text
Implement variance review as a governed workflow with coded drivers, accountable owner, reproducible snapshot context, and strict lifecycle validation. Reviews must explain why the variance exists, not just store the amount.
```

### S2 - Dashboard Drilldowns, Cost Mix Analysis, And Operational Causality
- Build typed dashboard query layer for planned vs committed vs posted actual vs forecast views by cost center, month, bucket, entity, asset family, work category, and labor lane.
- Add spend-mix analysis for corrective, preventive, inspection, compliance, shutdown, improvement, and capex contexts so leadership can distinguish reactive spend from strategic spend.
- Implement workforce drilldowns by assignee, team, skill mix, and contractor-vs-employee split, including labor variance by planned hours, actual hours, and effective rate.
- Ensure each variance row links back to underlying WO, PM occurrence, inspection follow-up, or shutdown package records with delay reason, reassignment count, availability conflict, and repeat-work indicators where available.
- Add productivity overlays beside cost variance: hours overrun rate, first-pass completion effect, repeat-work penalty, and schedule-discipline impact so overspend can be interpreted operationally.
- Expose typed dashboard/report contracts for analytics consumers; no ad-hoc SQL in the UI layer and no dashboard-only metric logic that bypasses the finance service layer.

**Cursor Prompt (S2)**
```text
Build budget dashboard and drilldown queries that connect financial variance to real operational causes. Planned, committed, actual, and forecast values must be sliced by spend mix, team, assignee, and labor lane, with links back to WO/PM/inspection/shutdown source records and productivity overlays.
```

### S3 - ERP Alignment, Import-Export Contracts, And Validation
- Implement optional ERP cost-center master import contract with external ID mapping, inactive-record handling, and duplicate-code validation without requiring ERP dependency for local finance workflows.
- Prepare export-ready payloads for posted actuals and approved reforecast snapshots so Phase 4 connector work can reuse stable local contracts instead of re-deriving finance logic.
- Add governed exchange-rate and currency-display handling in finance query outputs so source-currency and base-currency values remain visible and reconcilable.
- Add mismatch and reconciliation flags for ERP alignment issues: unknown external code, inactive imported cost center, base-currency drift, and posting payload rejection.
- Build supervisor/controller UX indicators for "local only", "ERP linked", and "export pending" state on cost centers and variance-review artifacts.
- Add tests for duplicate ERP-code rejection, export payload structure, reconciliation-flag generation, permission boundaries (`fin.view` vs `fin.report`), and dashboard drilldown traceability.

**Cursor Prompt (S3)**
```text
Finalize variance and ERP alignment by adding optional ERP master import, export-ready payloads for posted actuals and approved reforecasts, currency-aware query outputs, reconciliation flags, and tests for traceability, permissions, and payload correctness.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
