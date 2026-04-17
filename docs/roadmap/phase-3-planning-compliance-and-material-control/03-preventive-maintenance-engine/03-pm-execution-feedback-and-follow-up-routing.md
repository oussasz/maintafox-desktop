# PM Execution Feedback And Follow Up Routing

**PRD:** §6.9

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - PM Completion Evidence And Follow-Up Object Links
- Implement execution write model (`pm_executions`, `pm_findings`) tied to PM occurrences and optional linked WO closeout records.
- Capture completion outcomes explicitly (`completed_no_findings`, `completed_with_findings`, `deferred`, `missed`, `cancelled`) with actor/time provenance.
- Support follow-up routing targets from findings: create linked DI and/or linked WO while preserving source linkage back to occurrence + execution.
- Persist planned-vs-actual work signals using existing WO duration/labor fields when execution is WO-backed; do not duplicate labor truth in PM tables.
- Enforce guardrails so findings cannot be created for invalid execution states and follow-up links cannot point to missing records.

**Cursor Prompt (S1)**
```text
Implement PM execution and finding persistence with strong provenance and guarded follow-up creation into DI/WO. Reuse existing WO actuals as the source of labor-duration truth when a PM execution is WO-backed.
```

### S2 - Deferral/Miss Governance And Notification Coupling
- Add coded deferral and miss reason workflows on occurrences with mandatory reason evidence for non-completion outcomes.
- Emit PM due/missed/deferred/follow-up-created events into the existing notification engine (module 6.14) using governed category/severity mapping.
- Mirror key PM state changes into activity feed/audit flows (module 6.17) so supervisors can reconstruct end-to-end PM decision history.
- Add duplicate-notification suppression keys for unresolved PM misses to avoid noisy repeated alerts.
- Keep source-of-truth discipline: notification acknowledgement never mutates PM record state directly.

**Cursor Prompt (S2)**
```text
Wire PM execution outcomes into notification and activity pipelines with coded deferral/miss reasons, dedupe-safe alerting, and strict separation between notification acknowledgement and PM state transitions.
```

### S3 - Execution UX, Pattern Detection, And Reliability Handoff
- Extend PM UI with completion form, finding capture panel, and follow-up creation actions backed by real commands.
- Add recurring-finding detection queries (asset + finding type + interval window) to surface PM effectiveness risks for planners/reliability analysts.
- Expose PM execution feedback outputs needed by reliability module (`follow_up_ratio`, repeated findings, late completion patterns) without inventing future analytics tables.
- Add integration tests across full chain: occurrence -> execution -> finding -> follow-up DI/WO -> notification event.
- Add permission tests ensuring execution/finding actions are blocked without `pm.edit` (view-only users can read but not mutate).

**Cursor Prompt (S3)**
```text
Deliver PM execution feedback end-to-end with real UI flows, recurring-finding detection, and integration tests from occurrence through follow-up routing and notifications while preserving pm.view vs pm.edit boundaries.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
