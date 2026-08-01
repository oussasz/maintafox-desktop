# Ingestion Buffering And Rule Evaluation Pipeline

**PRD:** §6.21

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Ingestion Buffering, Ordering, And Data Quality
- Implement ingestion pipeline stages for receive, normalize, dedupe, timestamp alignment, quality tagging, and persistence.
- Add bounded buffering strategy for bursty sensor traffic with backpressure behavior and loss-prevention priorities.
- Enforce idempotent ingestion keys to suppress duplicate frames from unreliable gateway retransmissions.
- Apply data-quality flags (late, out-of-order, invalid unit, stale source, missing timestamp) before rule evaluation.
- Add retention and compaction policy for raw vs normalized signal history aligned with analytics and storage constraints.

**Cursor Prompt (S1)**
```text
Implement IoT ingestion pipeline with buffered normalization, idempotent dedupe, ordering safeguards, and explicit signal-quality tagging before downstream processing.
```

### S2 - Rule Evaluation Engine And Event Emission
- Implement rule engine supporting threshold, duration-over-limit, rate-of-change, and compound-condition evaluation patterns.
- Add rule versioning with staged publish/rollback so production alert semantics stay governed.
- Support per-asset and per-signal rule overrides with fallback to global defaults.
- Emit typed internal events carrying signal context, rule version, confidence flags, and suppression metadata.
- Add suppression/cooldown controls to prevent noisy alert storms from unstable telemetry sources.

**Cursor Prompt (S2)**
```text
Deliver governed IoT rule evaluation with versioned rule publish controls, rich event payloads, and alert suppression mechanisms for noisy telemetry.
```

### S3 - Observability, Performance, And Resilience Testing
- Add pipeline metrics for ingest latency, drop rate, duplicate suppression ratio, rule-eval latency, and event throughput.
- Define failure handling for adapter outage, queue saturation, malformed payload spikes, and rule-engine exceptions.
- Add resilience tests with synthetic high-volume telemetry and degraded network conditions.
- Validate replay behavior when buffered signals are reprocessed after temporary gateway downtime.
- Gate completion on performance and correctness thresholds proving reliable event generation under realistic plant signal loads.

**Cursor Prompt (S3)**
```text
Finalize ingestion and rule pipeline with observability, failure-mode handling, and high-volume resilience testing to prove reliable event generation under load.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
