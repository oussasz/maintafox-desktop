# Sync Testing Observability And Repair Tooling

**PRD:** §8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Sync Test Matrix And Deterministic Fixtures
- Build deterministic sync integration harness using SQLite fixtures + mock VPS contract emulator for push/pull/checkpoint scenarios.
- Cover normal and edge cases: duplicate outbox submissions, partial batch acceptance, stale checkpoint tokens, and multi-entity dependency ordering.
- Add fixture classes for append-only, governed, and mutable records so merge-policy differences are tested explicitly.
- Add restart/recovery tests ensuring in-progress sync sessions recover safely after crash, forced close, or upgrade restart.
- Define baseline performance assertions for batch throughput and conflict-generation behavior under realistic tenant volumes.

**Cursor Prompt (S1)**
```text
Implement a deterministic sync integration test matrix with mock VPS contracts, covering replay, partial acceptance, checkpoint staleness, and restart safety across record classes.
```

### S2 - Repair Tooling And Operator Safety Controls
- Implement sync repair commands for reindex, rebuild checkpoint state, replay failed window, and conflict queue reconciliation.
- Add strict guardrails so repair actions are scoped and reversible where possible (snapshot before destructive repair modes).
- Provide operator permission boundaries for repair commands and require explicit reason logging for high-impact actions.
- Surface repair previews showing expected impact (pending count changes, replay window scope, affected entities) before execution.
- Ensure repair workflows do not require full database reset as default support answer.

**Cursor Prompt (S2)**
```text
Deliver safe sync repair tooling with scoped commands, preview-and-confirm guardrails, auditable operator actions, and no-default destructive reset path.
```

### S3 - Observability, Alerting, And Validation Checklist
- Add metrics and tracing for sync phase durations, checkpoint drift, rejection ratio, dead-letter growth, and repair-tool usage frequency.
- Define alert thresholds for sync health degradation and operator-visible warning escalation paths.
- Add support runbook checklist tied to observable metrics so triage actions are consistent across incidents.
- Add end-to-end validation script that exercises sync, induced failure, repair action, and post-repair convergence proof.
- Gate completion on clean test suite plus documented sync SLO dashboard signals and repair procedure sign-off.

**Cursor Prompt (S3)**
```text
Finalize sync reliability with observability and repair validation: add health metrics/alerts, runbook-linked diagnostics, and end-to-end failure-to-recovery proof workflows.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
