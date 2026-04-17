# Outbox Inbox And Checkpoint Contracts

**PRD:** §8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Sync Envelope And Persistence Contracts
- Implement local sync-control tables (`sync_outbox`, `sync_inbox`, `sync_checkpoint`, `sync_rejections`) with lifecycle fields, FK references, and retry metadata.
- Standardize outbox/inbox envelope shape with required fields (`entity_type`, `entity_sync_id`, `operation`, `row_version`, payload hash, origin machine, created/acked timestamps).
- Add idempotency-key generation and uniqueness constraints per outbound batch to prevent duplicate processing during intermittent connectivity.
- Separate mutable business record storage from sync-control metadata so sync logic can evolve without destabilizing operational tables.
- Define migration baseline and retention policy for sync-control tables (purge windows, audit exceptions, and forensic override mode).

**Cursor Prompt (S1)**
```text
Implement robust local sync persistence contracts for outbox, inbox, and checkpoints with idempotency-safe envelope fields, replay metadata, and migration-ready table design.
```

### S2 - Checkpoint Protocol And VPS Contract Alignment
- Implement replay-safe checkpoint handshake with `checkpoint_token`, `acknowledged_items`, partial-accept responses, and rejection reasons.
- Define strict push/pull contract compatibility between desktop and VPS Fastify endpoints, including typed error taxonomy and transient vs permanent failure classes.
- Enforce apply ordering rules for inbound batches (dependency-safe sequencing and dedupe when same entity appears multiple times in recovery windows).
- Add policy metadata pass-through in sync responses (entitlement status, channel info, urgent notice flags) without coupling to business payload mutation.
- Version sync APIs and DTOs explicitly to support rolling upgrades between desktop clients and VPS environments.

**Cursor Prompt (S2)**
```text
Deliver checkpoint-aware sync protocol contracts between desktop and VPS with idempotent acknowledgments, typed rejections, policy metadata pass-through, and versioned API compatibility.
```

### S3 - Typed IPC Surface, Validation, And Failure Safeguards
- Add shared sync DTOs in `shared/ipc-types.ts` and runtime validation in desktop sync service to reject malformed envelopes before persistence.
- Expose sync state queries and diagnostics through typed IPC commands for UI/operator visibility (`last_checkpoint`, `pending_outbox`, `failed_items`).
- Add guardrails against silent data loss: no checkpoint advancement when non-recoverable inbound failures remain unresolved.
- Add tests for duplicate batch submission, stale checkpoint token handling, partial acceptance, and out-of-order inbound item recovery.
- Gate completion on integration tests using local SQLite + mock VPS with deterministic replay scenarios and crash-restart continuity checks.

**Cursor Prompt (S3)**
```text
Finalize sync envelope contracts with typed IPC exposure, strict validation, and tests for duplicate batches, partial apply behavior, and checkpoint safety under restart/replay scenarios.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
