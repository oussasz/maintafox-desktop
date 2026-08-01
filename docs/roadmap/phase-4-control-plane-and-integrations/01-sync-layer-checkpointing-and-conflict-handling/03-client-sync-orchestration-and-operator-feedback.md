# Client Sync Orchestration And Operator Feedback

**PRD:** §8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Sync Orchestrator Runtime And Mode Handling
- Implement dedicated async orchestrator lifecycle for opportunistic background sync, manual sync, bootstrap restore, replay recovery, and heartbeat refresh.
- Add bounded concurrency and jittered exponential backoff to prevent thundering retries during VPS/network instability.
- Ensure orchestrator respects policy controls (offline grace, suspension, paused sync windows, bandwidth-aware mode) before enqueueing work.
- Define durable orchestrator state machine (`idle`, `scheduled`, `running`, `blocked`, `degraded`, `error`, `paused`) persisted for restart continuity.
- Add cancellation and graceful shutdown behavior so app exit/update does not corrupt in-flight sync transitions.

**Cursor Prompt (S1)**
```text
Implement a resilient client sync orchestrator with explicit mode handling, persisted runtime states, bounded retries, and policy-aware scheduling controls.
```

### S2 - Operator Feedback Surface And UX Contracts
- Expose global sync status indicators in settings and status bar with clear states, last-success time, pending backlog count, and blocker reason.
- Add detailed sync activity timeline for operators/support (batch start, ack summary, conflict count, replay attempts, policy refresh outcomes).
- Implement actionable UX for degraded states (retry now, view conflicts, export diagnostics, contact support flow).
- Align notification semantics with severity and avoid alert fatigue via dedupe/coalescing for repetitive transient failures.
- Provide offline-first language explaining what still works locally vs what is currently delayed due to sync conditions.

**Cursor Prompt (S2)**
```text
Build operator-facing sync feedback UX with explicit health states, timeline diagnostics, and actionable controls for retries, conflict review, and support handoff.
```

### S3 - Diagnostics Export, Telemetry, And Validation
- Implement support-grade diagnostics export with bounded log window, correlation IDs, machine context, checkpoint summary, and redaction of sensitive secrets.
- Add tracing spans/metrics for push/pull duration, retry counts, conflict generation rate, and checkpoint advancement latency.
- Add chaos-style test scenarios for network drops, partial response payloads, suspended entitlement, and interrupted app shutdown.
- Validate orchestrator restart behavior from each persisted state and ensure idempotent resume without duplicate writes.
- Gate completion on integration and UX acceptance tests proving operator messages and remediation actions map to actual runtime behavior.

**Cursor Prompt (S3)**
```text
Finalize client sync orchestration with diagnostics export, telemetry, and resilience tests covering restart safety, network chaos, and entitlement-driven sync blocking behavior.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
