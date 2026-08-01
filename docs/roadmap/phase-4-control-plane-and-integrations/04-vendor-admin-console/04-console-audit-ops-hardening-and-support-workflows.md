# Console Audit Ops Hardening And Support Workflows

**PRD:** §16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Immutable Admin Audit And Forensic Traceability
- Implement append-only admin audit ledger for all vendor-side actions (who, when, scope, before/after snapshot, reason, approval context).
- Enforce tamper-evident audit guarantees (immutable records, integrity hashes, protected retention and export format).
- Add action taxonomy covering auth/session events, entitlement changes, machine operations, sync repairs, rollout interventions, and platform overrides.
- Add evidence links from audit records to underlying entities (tenant, entitlement ID, machine ID, sync batch, release ID, incident ID).
- Provide search and filter ergonomics for incident timelines and compliance reporting without direct database querying.

**Cursor Prompt (S1)**
```text
Build an immutable vendor-admin audit trail with tamper-evident records, complete action taxonomy, and forensic drill-through from each action to affected platform entities.
```

### S2 - Support Workflows, Diagnostics, And Customer Evidence Packs
- Integrate support workflow with structured ticket context (tenant, severity, affected module, sync status, app versions, linked incidents).
- Add support-bundle ingestion/export flow for diagnostic artifacts (logs, sync trace, migration failures, release metadata) with sensitive-data redaction rules.
- Support offline-origin ticket reconciliation from desktop queue to vendor console with clear sync state and duplicate suppression.
- Define ticket state workflow (`new`, `triaged`, `waiting_for_vendor`, `waiting_for_customer`, `resolved`, `closed`) with SLA clocks and escalation.
- Add cross-linking between support tickets and admin actions so every intervention is auditable and explainable to customer stakeholders.

**Cursor Prompt (S2)**
```text
Implement support operations in the vendor console with structured ticket workflow, diagnostic bundle handling, offline-ticket reconciliation, and strong linkage between tickets and admin interventions.
```

### S3 - Ops Hardening, Incident Runbooks, And Validation Matrix
- Add privileged-operation guardrails: mandatory reason codes, step-up authentication, and optional dual approval for high-blast-radius actions.
- Define incident runbooks for common control-plane failures (heartbeat outage, sync backlog surge, failed rollout, storage pressure, key-rotation incident).
- Add compliance-grade exports for customer audits (entitlement history, machine-state timeline, rollout actions, support resolution chronology).
- Execute tabletop and live simulation drills validating audit completeness, support handoff quality, and runbook usability under pressure.
- Gate completion on operational readiness matrix with measurable pass criteria for audit integrity, support SLA adherence, and incident response recovery time.

**Cursor Prompt (S3)**
```text
Finalize console hardening with privileged-action safeguards, incident runbooks, compliance exports, and drill-based validation of audit integrity and support response quality.
```

---

*Completion: 2026-04-16 — `vps::audit_support_hardening` (immutable audit preimage/chain, support & hardening types); `AuditOpsWorkspace` tabs (ledger, support, hardening); `shared/ipc-types` + `src/services/vendor-audit-support-contracts.ts`. Verify: `cargo test audit_support_hardening vendor_admin_console --lib`, `pnpm typecheck`, `pnpm exec vitest run src/services/__tests__/vendor-audit-support-contracts.test.ts`.*
