# Conflict Resolution Replay And Recovery Flows

**PRD:** §8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Conflict Taxonomy And Persistence Model
- Implement conflict registry with typed classes (`authority_mismatch`, `stale_row_version`, `delete_update_collision`, `reference_missing`, `policy_denied`, `merge_required`).
- Persist full conflict evidence (local value, inbound value, authority side, checkpoint context, source batch ID, detection timestamp).
- Distinguish auto-resolvable conflicts from operator-required conflicts using deterministic policy rules per entity class.
- Add conflict lifecycle states (`new`, `triaged`, `resolved_local`, `resolved_remote`, `merged`, `dismissed`, `escalated`) with immutable history trail.
- Ensure conflict records link back to source outbox/inbox items so replay and audit remain traceable.

**Cursor Prompt (S1)**
```text
Implement a typed sync-conflict model with evidence-rich records, lifecycle states, and deterministic policy routing between auto-resolution and operator-required review.
```

### S2 - Replay Engine And Recovery Safety
- Build replay service that can reprocess selected failed batches or entity slices without advancing checkpoint prematurely.
- Support replay modes (`single_item`, `batch`, `window`, `checkpoint_rollback`) with explicit preconditions and safety guards.
- Enforce idempotent replay behavior so retries never duplicate accepted writes or orphan side effects.
- Add safeguards for destructive replay paths (mandatory confirmation + snapshot checkpoint before replay on critical entity classes).
- Record replay outcomes with reason codes and residual conflict inventory for support and operator follow-up.

**Cursor Prompt (S2)**
```text
Deliver replay and recovery workflows for sync failures with idempotent behavior, checkpoint safety guards, and explicit replay-mode controls for operators.
```

### S3 - Operator Conflict Inbox And Resolution Workflow
- Replace placeholder conflict UI with queue-based operator inbox showing severity, entity context, policy authority, and recommended resolution actions.
- Provide structured resolution actions (`accept_local`, `accept_remote`, `merge_fields`, `retry_later`, `escalate`) with preview before commit.
- Add permission boundaries for conflict operations (read vs resolve vs replay/admin override) and audit every resolution action.
- Add user-facing explanations for unresolved conflicts and blocked sync status to prevent silent backlog accumulation.
- Gate completion on end-to-end tests validating conflict detection, resolution persistence, replay outcome correctness, and checkpoint continuity.

**Cursor Prompt (S3)**
```text
Finalize conflict handling with an operator inbox, structured resolution actions, permission boundaries, and full tests for replay-safe checkpoint continuity after conflict resolution.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
