# Spare Parts Cost Valuation And WO Auto Pricing

**PRD:** §6.8 (extended from research for enterprise-grade CMMS behavior)

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (task list + short Cursor prompts + direct implementation in `maintafox-desktop/`).

## Why this is added

Research in `docs/research` shows that strong CMMS platforms keep **cost provenance** and separate:
- planned unit cost on work orders
- stock valuation cost used for inventory and finance posting
- provisional vs posted financial values

This closes the current gap where WO part unit cost is manually entered and not policy-driven.

## Delivery Slices

### S1 - Cost Policy Engine And Provenance Model
- Add valuation policy entities (`cost_policies`, optional `article_cost_profile`) with precedence by site/family/article.
- Support methods: moving weighted average, standard cost, last receipt, contract-linked estimate.
- Persist cost provenance on inventory and WO events (`cost_source_type`, `cost_source_ref`, `cost_effective_at`, `is_provisional`).
- Enforce governed currency/UoM compatibility against reference data to avoid mixed-unit or mixed-currency misvaluation.

**Cursor Prompt (S1)**
```text
Implement the valuation policy engine with deterministic precedence and cost provenance persistence. Ensure cost records are reference-governed (currency/UoM compatible) and auditable at event level.
```

### S2 - WO Auto-Pricing And Replenishment Intelligence
- Auto-fill WO part `unit_cost` from policy evaluation when selecting a spare part in planning/execution.
- Display cost-source badge (`CONTRACT`, `LAST_RECEIPT`, `MOVING_AVG`, `STANDARD`) with timestamped source context.
- Keep manual override allowed only with required reason and audit trace.
- Add replenishment-aware projected cost on reorder/requisition using preferred supplier and recent receipt/contract signals.

**Cursor Prompt (S2)**
```text
Deliver WO auto-pricing from valuation policy at part selection time, with source badges and controlled manual override. Extend reorder/requisition flows with projected replenishment cost and confidence metadata.
```

### S3 - Posted-Cost Separation, ERP Reconciliation, Reporting, And Tests
- Separate planned WO cost from posted actual cost driven by issued transactions.
- Add ERP reconciliation mapping states (`posted`, `pending_reconcile`, `reconciled`, `conflict`) and conflict diagnostics.
- Build valuation and variance reporting (planned vs actual vs posted, source distribution, override frequency).
- Add deterministic tests for policy precedence, moving-average recomputation, auto-fill/override behavior, and reconciliation conflict workflows.

**Cursor Prompt (S3)**
```text
Finalize cost valuation maturity by separating planned vs posted cost, adding ERP reconciliation state handling, and shipping valuation/variance reporting with deterministic test coverage for policy and reconciliation behavior.
```

## Acceptance direction

- New WO part rows auto-populate unit cost without manual typing in normal cases.
- Every cost shown in WO/inventory has a traceable source and timestamp.
- Planned cost, operational valuation, and posted financial cost remain distinguishable.
- No silent cost changes after posting; adjustments create explicit cost events.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*

## Source set used for strategy

- `docs/research/MODULE_6_8_SPARE_PARTS_AND_INVENTORY_MANAGEMENT.md`
- `docs/research/MODULE_6_24_BUDGET_AND_COST_CENTER_MANAGEMENT.md`
- `docs/research/MODULE_6_22_ERP_AND_EXTERNAL_SYSTEMS_CONNECTOR.md`
- `docs/research/MODULES_6_4_6_5_REQUEST_TO_WORK_ORDER_LIFECYCLE.md`
- `docs/research/MODULES_6_9_6_16_PREVENTIVE_MAINTENANCE_AND_PLANNING_SCHEDULING.md`
