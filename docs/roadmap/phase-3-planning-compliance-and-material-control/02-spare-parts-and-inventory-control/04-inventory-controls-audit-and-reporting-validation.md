# Inventory Controls Audit And Reporting Validation

**PRD:** §6.8

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (task list + short Cursor prompts + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Count Governance And Variance Integrity
- Add count session lifecycle (`planned`, `in_progress`, `submitted`, `approved`, `posted`) scoped by warehouse/location/article class.
- Require variance reason codes and approval thresholds before posting.
- Enforce step-up or dual-control for critical spare variances above configured tolerance.
- Persist immutable variance evidence (who, when, reason, delta, approval chain).

**Cursor Prompt (S1)**
```text
Implement cycle-count governance with explicit session lifecycle, mandatory variance coding, and approval controls for critical variances. Ensure no variance can post without traceable reviewer evidence.
```

### S2 - Audit Coupling And Ledger Reconciliation
- Emit activity/audit events for all stock mutations, reversals, approvals, and posting failures.
- Add periodic ledger-vs-balance reconciliation checks and drift anomaly flags.
- Keep forensic links from mutation event -> source command -> actor -> before/after quantities.
- Prepare audit payloads for high-integrity export and future ERP reconciliation diagnostics.

**Cursor Prompt (S2)**
```text
Deliver immutable audit coupling and reconciliation health checks for inventory: every mutation must be traceable and daily integrity checks must detect drift between balances and transaction ledger aggregates.
```

### S3 - Reporting, UX, Permissions, And Test Completion
- Add movement ledger export, reservation aging, shortage exposure, count-accuracy, and top-variance reporting views.
- Add `inv.report` permission split readiness from `inv.view`/`inv.manage`.
- Build forensic trace UX from article/location records into mutation timelines and variance decisions.
- Complete automated tests for count posting, reversal logic, reconciliation, and report query correctness/performance.
- Add workforce-impact reporting: shortages and stockout delays tied to affected WO assignees/teams and resulting labor idle-time exposure.
- Add validation for assignment-related reservation integrity when WO assignee/team changes after reservation creation.

**Cursor Prompt (S3)**
```text
Finalize inventory controls with report surfaces, forensic trace UX, permission split readiness, and full automated validation for count posting/reversal/reconciliation correctness under realistic data volumes.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
