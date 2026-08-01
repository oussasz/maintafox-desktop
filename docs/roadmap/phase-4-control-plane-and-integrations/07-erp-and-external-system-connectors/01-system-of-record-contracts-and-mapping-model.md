# System Of Record Contracts And Mapping Model

**PRD:** §6.22

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - System-Of-Record Contracts And Domain Boundaries
- Define explicit source-of-record contracts per integration domain (master data, transactional posting, acknowledgments, archival signals).
- Document field-level authority rules and mutation ownership to avoid accidental bidirectional ambiguity.
- Implement versioned connector contracts with schema compatibility checks and change-management workflow.
- Add contract activation lifecycle (`discover`, `preview`, `test`, `activate`, `monitor`, `retire`) aligned with PRD integration governance.
- Ensure contracts include idempotency scope, replay policy, and required audit metadata.

**Cursor Prompt (S1)**
```text
Implement ERP/external source-of-record contract governance with field authority boundaries, versioned schemas, and lifecycle controls from test to active monitoring.
```

### S2 - Mapping Model And External Identity Management
- Implement mapping model for external keys, local record links, version tokens, and sync-state lifecycle fields.
- Support mapping revisions with effective dating so historical synchronization remains interpretable after mapping changes.
- Add transformation and canonicalization rules for codes, units, statuses, and reference-domain alignment.
- Validate mapping publish safety against governed domains to prevent breaking analytical semantics.
- Add tooling for mapping drift detection and impact preview before publishing changes to active contracts.

**Cursor Prompt (S2)**
```text
Deliver external identity and mapping model with versioned transformations, publish-safe governance, and drift detection before active contract changes.
```

### S3 - Contract Validation, Operator Visibility, And Acceptance
- Add typed contract inspection APIs and UI views so operators can review active authority/mapping rules by domain.
- Validate incoming/outgoing payloads against contract versions with clear rejection reasons and remediation hints.
- Add tests for authority-rule enforcement, mapping-version transitions, and schema evolution compatibility.
- Ensure each synchronized record remains traceable to contract version and mapping revision used at processing time.
- Gate completion on integration acceptance tests proving no domain crosses source-of-record boundaries unintentionally.

**Cursor Prompt (S3)**
```text
Finalize connector contract governance with operator-visible rule inspection, strict payload validation, and tests proving source-of-record boundaries hold across mapping/version changes.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
