# Object Storage Backups And Operations Baseline

**PRD:** §16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Object Storage Layout, Integrity, And Retention Rules
- Define bucket/prefix taxonomy for updater artifacts, backup snapshots, tenant restore bundles, and support evidence exports.
- Enforce environment and tenant boundary naming conventions so operational data cannot be misrouted between customers.
- Add object integrity controls (checksums/signatures on backup manifests, immutable metadata for release artifacts).
- Configure lifecycle and retention policies by data class (short-lived rollout files vs long-lived compliance backups).
- Ensure secrets for object storage access are managed through secure reference handling and rotation workflow.

**Cursor Prompt (S1)**
```text
Implement production object-storage baseline for updates and backups with strict naming boundaries, integrity metadata, and retention-by-data-class policies.
```

### S2 - Backup Orchestration For Control Plane And Tenant Mirrors
- Schedule PostgreSQL backups for both shared control-plane metadata and tenant mirror schemas with independent restore scope.
- Add backup catalog records (snapshot ID, tenant scope, timestamp, checksum, encryption context, retention class, verify status).
- Support point-in-time recovery strategy where feasible (base snapshots + WAL/archive continuity policy).
- Add periodic integrity verification jobs that rehydrate sample backups in isolated restore targets and confirm schema/version compatibility.
- Prevent backup pipelines from overloading sync workers by isolating resource windows and prioritizing operational queue health.

**Cursor Prompt (S2)**
```text
Deliver backup orchestration for shared control-plane data and tenant mirror schemas, including snapshot cataloging, integrity verification, and restore-scope isolation.
```

### S3 - Restore Drills, Runbooks, And Operational Acceptance
- Define tenant-scoped and platform-scoped restore runbooks with clear prerequisites, expected RPO/RTO, and verification checkpoints.
- Add quarterly restore-drill cadence with evidence capture (time to restore, data-consistency checks, residual issue log).
- Include emergency procedures for accidental rollout artifact deletion, corrupted mirror snapshot, and expired storage credentials.
- Add operator checklists for post-restore validation: entitlement heartbeat health, sync checkpoint continuity, admin audit continuity, and update manifest integrity.
- Gate completion on successful drill execution and documented remediation for any restore mismatch or runbook ambiguity.

**Cursor Prompt (S3)**
```text
Finalize backup and recovery readiness with tested tenant/platform restore runbooks, drill evidence, and post-restore validation checks for sync, entitlement, and admin operations continuity.
```

---

*Completion: 2026-04-16 — VPS object-storage contract (`vps::object_storage`), unit tests (`cargo test object_storage_tests --lib`), TS types in `shared/ipc-types.ts`; `cargo check --lib` and `pnpm typecheck` clean.*
