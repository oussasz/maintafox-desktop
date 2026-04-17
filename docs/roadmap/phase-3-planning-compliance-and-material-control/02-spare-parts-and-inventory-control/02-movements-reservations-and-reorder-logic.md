# Movements Reservations And Reorder Logic

**PRD:** §6.8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (task list + short Cursor prompts + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Movement Ledger Hardening
- Enforce event-first inventory mutations only (`RESERVE`, `ISSUE`, `RETURN`, `TRANSFER`, `RELEASE`, `ADJUST`), no direct balance edits.
- Keep reservation invariants strict (`reserved >= issued`, `available >= 0`, no negative on-hand).
- Ensure article/location integrity before mutation (active article, active location, governed unit consistency).
- Add migration and query-level guards for stale/missing linked entities.

**Cursor Prompt (S1)**
```text
Implement movement-ledger hardening for inventory: no silent balance edits, strict reservation invariants, and active master-data validation before every stock mutation. Keep all writes transactional in Rust services and return typed validation errors.
```

### S2 - WO/PM Integration And Command Reliability
- Keep WO/PM reservation and issue hooks in Rust execution/closeout services with rollback-safe behavior.
- Ensure `source_type`, `source_id`, `source_ref` are populated for all transaction/reservation events.
- Expose complete command surface with RBAC split (`inv.view`, `inv.manage`) and stable IPC payload contracts.
- Remove placeholders from WO reservation panels; display only real linked reservation rows.
- Add shortage context fields required by procurement handoff (preferred supplier, lead-time risk, reorder-source trace id) so reservations can flow into supplier-facing requisition logic in File 03.

**Cursor Prompt (S2)**
```text
Wire robust WO/PM stock hooks: reserve on planning, issue/return on usage deltas, release on no-parts/closeout, with full source provenance and RBAC-gated commands. Validate IPC contracts end-to-end and keep UI strictly data-driven.
```

### S3 - Reorder Engine, Reconciliation Readiness, And Tests
- Implement deterministic reorder evaluation (reorder-point and min/max) with warehouse scope and suggested quantity rationale.
- Keep transaction rows reconciliation-ready for Phase 4 (`pending`, `posted`, `reconciled`, `failed` compatibility).
- Add concurrency tests for parallel issue/release/transfer and stale-state races.
- Add integration tests for reservation lifecycle and reorder evaluation.

**Cursor Prompt (S3)**
```text
Finalize reorder and integrity validation: implement deterministic recommendation logic, add reconciliation-ready status compatibility, and cover concurrency/integration scenarios for reservation lifecycle and transfer safety.
```

---

*Completion: 2026-04-14, Codex (Cursor). Added migration `m20260418_000043_inventory_movements_reservations`, inventory transaction/reservation/reorder command surface, WO auto-reserve+issue/release hooks via source types (`WO`/`PM_WO`), and WO execution reservation panel. Verification: `cargo check` (pass, with dedicated target dir), `pnpm typecheck` (pass).*
