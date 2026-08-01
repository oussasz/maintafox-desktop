# Import Export Jobs And Idempotent Processing

**PRD:** §6.22

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Integration Job Architecture And Scheduling
- Implement import/export job orchestration with queue-backed execution for scheduled, manual, webhook, dry-run, and replay modes.
- Add job partitioning by contract and tenant scope to prevent noisy integration streams from starving others.
- Persist job lifecycle states (`queued`, `running`, `completed`, `partial_failed`, `failed`, `suspended`) with operational metadata.
- Add execution windows and dependency constraints for jobs that must follow business cutoffs or posting sequences.
- Ensure job engine supports cancellation and safe resume semantics.

**Cursor Prompt (S1)**
```text
Implement ERP connector job orchestration with queue-based scheduling, lifecycle tracking, tenant/contract partitioning, and safe cancel/resume behavior.
```

### S2 - Idempotent Processing And Replay Safety
- Implement idempotency keys at batch and item level with dedupe-safe persistence and retry behavior.
- Add payload hash and version-token checks to detect duplicate, stale, or reordered records.
- Ensure retries are bounded with backoff and dead-letter routing for persistent failures.
- Support replay mode using same audited processing pipeline rather than custom side paths.
- Add conflict markers when external and local state diverge beyond automatic reconciliation policy.

**Cursor Prompt (S2)**
```text
Deliver idempotent import/export processing with batch-item dedupe, replay-safe retries, dead-letter handling, and divergence markers for unreconciled records.
```

### S3 - Operator Status UX, Telemetry, And Validation
- Build connector status UI showing job health, throughput, failure clusters, and pending replay/repair actions.
- Add drill-down into item-level failures with reason taxonomy and operator remediation paths.
- Add telemetry for queue latency, processing throughput, retry ratio, and dead-letter growth per contract.
- Add tests for idempotency guarantees, retry exhaustion behavior, and status consistency after restart.
- Gate completion on realistic load tests and operator validation for failure triage workflows.

**Cursor Prompt (S3)**
```text
Finalize import/export operations with operator status visibility, item-level diagnostics, telemetry, and tests proving idempotent behavior under retries and restarts.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
