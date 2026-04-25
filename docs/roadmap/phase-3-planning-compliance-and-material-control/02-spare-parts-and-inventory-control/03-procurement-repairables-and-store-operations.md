# Procurement Repairables And Store Operations

**PRD:** §6.8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (task list + short Cursor prompts + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Procurement Schema And Governance Backbone
- Add requisition/PO/GR schema with version-safe status transitions and links to reorder demand sources.
- Include supplier preference, lead-time, and contract price references at article + PO-line level.
- Enforce governed reference values for units, tax/VAT, and supplier classes (no ungoverned free-text fields).
- Add posting-status fields (`not_posted`, `posted`, `failed`, `reconciled`) for ERP-ready workflows.
- Define supplier-master strategy explicitly: current interim source is `external_companies` (`personnel` module tables), with migration plan to dedicated procurement supplier master (`inventory_suppliers` + contacts + payment/tax metadata) before PO go-live.
- Align supplier/tax governance with research expectations from `MODULE_6_8_SPARE_PARTS_AND_INVENTORY_MANAGEMENT.md` (sections on supplier semantics, procurement evidence, and ERP handoff).

**Cursor Prompt (S1)**
```text
Build procurement backbone for inventory with requisition/PO/GR schema, governed reference-domain usage, and ERP-ready posting states. Keep source-demand traceability from reorder/reservation through PO lines.
```

### S2 - Store Operations And Repairable Lifecycle
- Implement receiving, quarantine, inspection, release-to-stock, put-away, and bin-move commands with explicit inventory movement events.
- Add repairable cycle entities and commands (`removed`, `awaiting_repair`, `sent`, `returned`, `reinstalled`, `scrapped`) linked to WO/vendor/shipment references.
- Block invalid transitions (release while hold active, close repairable without terminal outcome).
- Preserve actor, reason, and timestamp for every lifecycle move.
- Add WO/job-kitting reservation checks that surface missing-part blockers directly to planning/assignment flows before personnel dispatch.
- Keep traceability from issued/released parts back to assigned personnel/team and scheduled window for later labor-plus-material efficiency analysis.

**Cursor Prompt (S2)**
```text
Implement store and repairable execution flows with strict lifecycle transitions and explicit stock events. Prevent invalid release/closure paths and keep full actor/reason/time provenance on every state change.
```

### S3 - UI Surfaces, Reservation-Aware Procurement, And E2E Validation
- Build requisition/PO/receipt UI surfaces and repairable board with lifecycle badges and permissioned actions.
- Add shortage-driven requisition generation from reserved demand + reorder signals.
- Ensure real data binding only; remove placeholder controls and fake rows.
- Add end-to-end validation: requisition -> PO -> receipt -> inspect -> release, plus repairable return/scrap branches.
- Add assignment-readiness UX hooks: WO assignee selector must display real-time part readiness + personnel availability context before confirmation.
- Add tests for reservation contention during WO reassignment and shift changes (no silent over-issue or stale reservation carryover).

**Cursor Prompt (S3)**
```text
Deliver procurement and repairable UI workflow end-to-end with reservation-aware demand sourcing, no placeholder data, and complete E2E tests covering happy path and guarded failure transitions.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
