# Channel Rollout Policy And Recall Controls

**PRD:** §11

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Rollout Policy Model And Cohort Targeting
- Implement rollout policy model supporting percentage rollout, tenant cohorts, support cohorts, and explicit allow/deny overrides.
- Add channel eligibility rules tied to entitlement tier/policy (`internal`, `pilot`, `stable`) and environment safety constraints.
- Provide dry-run rollout simulation showing affected tenants/machines before policy activation.
- Persist rollout policy versions with activation history and operator attribution.
- Add safety thresholds preventing accidental full-fleet exposure from misconfigured rollout parameters.

**Cursor Prompt (S1)**
```text
Implement channel rollout policy with cohort targeting, dry-run impact simulation, and versioned activation history to prevent accidental fleet-wide exposure.
```

### S2 - Recall Controls And Emergency Containment
- Implement recall controls that immediately stop distribution of specific versions/channels without deleting historical evidence.
- Add emergency pause and policy freeze modes for incident containment while diagnostics are collected.
- Ensure client manifest fetch honors recall decisions deterministically across cache and retry behavior.
- Add rollback eligibility policy specifying when controlled downgrade is permitted and how migration constraints are enforced.
- Require reason codes and approval workflow for high-blast-radius recall/rollback actions.

**Cursor Prompt (S2)**
```text
Deliver recall and emergency containment controls for update rollout with deterministic client behavior, rollback eligibility policy, and auditable approval workflows.
```

### S3 - Rollout API Contracts, Telemetry, And Validation
- Define typed rollout API contracts for policy read, eligibility evaluation, activation, recall, and rollout-status metrics.
- Add telemetry for rollout adoption curve, failure concentration by cohort, and recall execution latency.
- Integrate rollout-state signals into vendor console monitoring so operators can correlate release and sync/license health.
- Add automated tests for cohort matching logic, recall propagation, stale policy cache handling, and rollback safeguards.
- Gate completion on staged rollout rehearsal proving safe progression and rapid recall under simulated bad-release conditions.

**Cursor Prompt (S3)**
```text
Finalize rollout governance with typed policy APIs, adoption/failure telemetry, and rehearsal-tested recall behavior under bad-release simulation.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
