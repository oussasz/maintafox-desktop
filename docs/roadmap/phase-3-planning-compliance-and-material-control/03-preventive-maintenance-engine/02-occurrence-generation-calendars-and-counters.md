# Occurrence Generation Calendars And Counters

**PRD:** §6.9

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Deterministic Occurrence Engine And Trigger Evidence
- Implement deterministic due-generation service for active PM versions across fixed, floating, meter, event, and condition triggers.
- Deliver generation as explicit command first (manual/run-on-demand) before introducing background scheduler loops; this keeps Phase 3 reproducible and testable.
- Record auditable trigger evidence in `pm_trigger_events` for every evaluated trigger, including measured value, threshold, generation decision, and source reference.
- Reuse current meter backbone (`asset_meters`, `meter_readings`) as the default meter trigger source; treat IoT/condition feeds as optional adapters where available.
- Add idempotency safeguards (duplicate-generation protection for same plan/version/due basis window).

**Cursor Prompt (S1)**
```text
Build a deterministic PM occurrence generator with auditable trigger events across fixed/floating/meter/event/condition logic. Start with command-driven generation and robust idempotency before adding any background loop.
```

### S2 - Occurrence Lifecycle Governance And WO Bridging
- Implement PM occurrence lifecycle states and guarded transitions: `forecasted -> generated -> ready_for_scheduling -> scheduled -> in_progress -> completed`, with `deferred`, `missed`, and `cancelled` side paths.
- Enforce coded deferral/miss reasons and retain actor/timestamp evidence for every non-happy-path transition.
- Add optional draft WO creation from occurrence via existing WO command surface, using preventive WO classification and explicit PM occurrence linkage fields.
- Preserve inventory/parts provenance by keeping PM-generated WO stock events source-tagged (`PM_WO`) so reservations/issues remain traceable in current inventory logic.
- Keep planning-safe queue exposure: if full planning engine is not yet implemented, provide `ready_for_scheduling` APIs instead of fake calendar commitments.

**Cursor Prompt (S2)**
```text
Implement PM occurrence lifecycle transitions with strict reason coding and optional preventive WO generation. Keep every generated WO and stock movement traceable back to its originating PM occurrence without introducing placeholder planning states.
```

### S3 - Forecast Calendar UX, Dashboard Hooks, And Validation
- Replace PM placeholder view with occurrence forecast and due-work list surfaces (weekly/monthly horizon, grouped by asset/family/entity/criticality).
- Expose real PM due/overdue counts for dashboard/report consumers from occurrence tables (no static mock counters).
- Add availability/readiness hint fields as non-blocking signals for future planning integration (skill/parts/permit windows) without pretending full scheduler exists.
- Cover edge-case tests: floating recalculation on late completion, meter rollback/reset behavior, event replay idempotency, and overdue rollover.
- Add permission tests proving `pm.view` can query occurrences while create/edit/delete actions remain isolated.

**Cursor Prompt (S3)**
```text
Deliver PM occurrence visibility end-to-end by replacing placeholder UI with real forecast data, wiring dashboard-ready due metrics, and adding tests for floating, meter, idempotency, and permission boundaries.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
