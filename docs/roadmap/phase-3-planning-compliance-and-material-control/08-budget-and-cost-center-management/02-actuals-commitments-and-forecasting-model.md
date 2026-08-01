# Actuals Commitments And Forecasting Model

**PRD:** §6.24

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Actual Cost Event Ledger And Posting Provenance
- Add finance migrations for `budget_actuals`, `budget_commitments`, and `budget_forecasts` with FK integrity to cost centers, budget versions, work orders, and source records where available.
- Implement governed actual-cost ingestion from execution evidence: WO labor, WO parts, WO services, WO tools, purchase receipts, contract call-offs, and manual adjustments with mandatory reason code.
- Preserve cost provenance on every `budget_actuals` row: `source_type`, `source_id`, `work_order_id`, `equipment_id`, `posting_status`, `posted_at`, source currency, and base currency.
- Split labor actuals by personnel, team, and rate-card lane (`regular`, `overtime`, `contractor`) so labor cost rollups stay auditable instead of collapsing into one total.
- Implement provisional vs posted rules: WO-related values stay provisional until closure-quality and cost-evidence conditions are satisfied; only posted actuals participate in official budget reporting and ERP-export readiness.
- Add reversal discipline for posted actuals: reversals must retain original event linkage and produce a replacement or correction record rather than destructive overwrite.

**Cursor Prompt (S1)**
```text
Build the budget actuals ledger with provenance-preserving cost events from work orders, inventory, purchasing, and contract sources. Keep labor split by personnel/team/rate-card lane and enforce provisional-versus-posted behavior with reversal traceability instead of destructive edits.
```

### S2 - Commitments Layer And Forecast Generation Contracts
- Implement `budget_commitments` ingestion from approved PO lines, contract reservations, and shutdown packages while keeping commitments visibly separate from posted actuals.
- Add planning handshake for labor commitments: use committed assignment hours from the planning module as forecast seed inputs before execution starts, without misclassifying them as actuals.
- Implement forecast generation service for `budget_forecasts` with explicit `forecast_method` values (`burn_rate`, `pm_occurrence`, `shutdown_loaded`, `manual`) and stored `confidence_level`.
- Feed forecast generation from PM occurrences, ready backlog demand, committed labor plans, and known shutdown packages so forecast lines represent explainable operational demand.
- Include efficiency-linked forecasting fields: historical plan-vs-actual labor variance, repeat-delay exposure, and contractor dependence should influence forecast confidence and variance commentary.
- Add duplicate-generation protection and traceable rerun behavior: each forecast batch should record generation timestamp, version context, and filter scope so repeated runs remain explainable.

**Cursor Prompt (S2)**
```text
Implement commitments and forecast generation as separate, explainable layers: PO and contract obligations stay distinct from actuals, while PM/backlog/shutdown/planning demand generates budget_forecasts with explicit method and confidence metadata. Forecast reruns must be idempotent and auditable.
```

### S3 - Query Layer, UX Surfaces, Permissions, And Tests
- Add typed finance query contracts for actuals, commitments, and forecasts by period, cost center, bucket, asset family, work category, and labor lane.
- Replace placeholder budget detail views with real panels for posted-vs-provisional actuals, open commitments, forecast drivers, and source-record drilldowns.
- Add frontend explainability for labor forecasting: show planning-hour seed, productivity factor, and contractor-share assumptions beside forecast totals.
- Enforce permission boundaries so `fin.view` can inspect actual and forecast summaries while any manual adjustment or rerun action requires `fin.budget`; export/report surfaces remain separately guarded by `fin.report`.
- Populate finance i18n keys for posting status, forecast methods, confidence levels, and source provenance labels in both `en` and `fr`.
- Add automated tests for provisional/posting transitions, reversal integrity, duplicate forecast-run protection, commitment-vs-actual separation, permission gating, and typed query correctness.

**Cursor Prompt (S3)**
```text
Deliver the actuals/commitments/forecasting model end-to-end with typed query contracts, real drilldown surfaces, forecast explainability in the UI, strict fin.* permission splits, and tests for posting rules, reversals, duplicate forecast generation, and commitment-versus-actual separation.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
