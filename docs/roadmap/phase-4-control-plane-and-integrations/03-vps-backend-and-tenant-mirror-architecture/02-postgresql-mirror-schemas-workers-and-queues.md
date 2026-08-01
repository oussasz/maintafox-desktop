# PostgreSQL Mirror Schemas Workers And Queues

**PRD:** §16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Tenant Mirror Schema Model And Migration Baseline
- Implement shared control-plane schema plus one PostgreSQL mirror schema per tenant (`tenant_<uuid>`) with strict isolation semantics.
- Define mirror tables for synchronized domains with stable `sync_id`, `row_version`, `origin_machine_id`, and checkpoint lineage fields.
- Add control-plane tables for checkpoint state, idempotency records, replay metadata, and mirror audit traces without mixing tenant business rows.
- Build migration strategy that supports online expansion (additive first, non-destructive transitions, backward-compatible readers during rollout).
- Add tenant provisioning/deprovisioning routines that create schema, apply baseline migrations, and verify invariants before accepting sync traffic.

**Cursor Prompt (S1)**
```text
Deliver VPS PostgreSQL multi-tenancy with shared control-plane tables and per-tenant mirror schemas. Include migration-safe schema evolution and tenant lifecycle provisioning checks.
```

### S2 - Sync Workers, Queue Semantics, And Replay Safety
- Implement queue partitions for push ingestion, inbound pull materialization, full restore preparation, and replay/repair actions.
- Enforce idempotent processing with unique idempotency-key constraints and dedupe-safe worker handlers.
- Add class-aware processing rules (append-only events, governed configuration snapshots, mutable operational records) instead of one global merge strategy.
- Route unresolved conflicts into explicit review queues with enough context for operator repair (external value, local value, authority, last checkpoint).
- Add retry/backoff and dead-letter handling with bounded attempts, clear failure reasons, and operator-visible requeue commands.

**Cursor Prompt (S2)**
```text
Implement VPS sync workers and queue topology for push/pull/restore/replay with strict idempotency and class-aware merge behavior. Add conflict queue routing plus dead-letter and safe retry controls.
```

### S3 - Worker Operability, Performance, And Validation Gates
- Add worker health telemetry (queue depth, processing latency, retry rate, dead-letter growth, per-tenant lag).
- Add performance guardrails for batch size, concurrency ceilings, and tenant fairness to prevent one noisy tenant starving others.
- Add migration + worker integration tests for checkpoint progression, replay safety, duplicate batch suppression, and tenant isolation.
- Verify schema-search-path hardening so worker queries cannot accidentally cross tenant boundary.
- Require completion evidence: successful replay/repair drills, queue-failure recovery proof, and clean test run against realistic sync volumes.

**Cursor Prompt (S3)**
```text
Finalize tenant mirror workers with operability metrics, fairness/performance guardrails, and integration tests proving checkpoint safety, dedupe behavior, conflict routing, and strict tenant isolation.
```

---

*Completion: 2026-04-16, Codex (agent), `cargo check` passed, `pnpm typecheck` passed, VPS mirror worker integration tests passed (`vps::mirror_tests`).*
