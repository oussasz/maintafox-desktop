# Reconciliation Operator Workflows And Failure Handling

**PRD:** §6.22

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Reconciliation Data Model And Failure Taxonomy
- Implement reconciliation records linking batch, item, contract version, authority rule, and mismatch fields.
- Define failure taxonomy (`mapping_error`, `authority_conflict`, `validation_error`, `external_reject`, `timeout`, `duplicate`, `stale_token`) for actionable triage.
- Preserve before/after snapshots and version tokens required for deterministic operator decisions.
- Add severity and business-impact scoring to prioritize reconciliation workload.
- Ensure reconciliation records are immutable in history while current resolution status remains updatable.

**Cursor Prompt (S1)**
```text
Implement reconciliation model with actionable failure taxonomy, snapshot evidence, and priority scoring so operators can triage integration mismatches effectively.
```

### S2 - Operator Reconciliation Workflow And Recovery Actions
- Build reconciliation workspace with side-by-side diff, authority context, and recommended resolution actions.
- Support structured operator actions (`accept_external`, `keep_local`, `merge_and_retry`, `override_with_approval`, `escalate`) with permission checks.
- Add controlled retry flows scoped to item, batch, or filtered segment with idempotent safeguards.
- Require reason codes and optional dual-approval for high-impact overrides.
- Link reconciled outcomes back to integration job and audit timelines for end-to-end traceability.

**Cursor Prompt (S2)**
```text
Deliver reconciliation operator workflows with diff-based decisioning, structured recovery actions, controlled retries, and permissioned override governance.
```

### S3 - Failure Handling Automation And Validation
- Implement automatic retry/backoff policy for transient failures while routing non-transient failures directly to reconciliation queues.
- Add SLA clocks and escalation triggers for unresolved reconciliation items by severity and business domain.
- Add tests for resolution correctness, retry safety, override approval logic, and stale-token edge cases.
- Validate that reconciliation actions do not violate source-of-record contract authority rules.
- Gate completion on scenario-based UAT proving operators can clear realistic failure backlogs safely and audibly.

**Cursor Prompt (S3)**
```text
Finalize reconciliation and failure handling with automated retry policies, escalation governance, and tested operator workflows that preserve source-of-record integrity.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
