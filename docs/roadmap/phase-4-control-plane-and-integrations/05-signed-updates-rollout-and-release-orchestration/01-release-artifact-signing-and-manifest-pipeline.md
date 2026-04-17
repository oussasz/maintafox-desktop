# Release Artifact Signing And Manifest Pipeline

**PRD:** §11

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Release Artifact Build And Signing Chain
- Implement CI/CD pipeline that builds platform artifacts deterministically and signs each bundle with dedicated updater-signing key.
- Enforce key separation from licensing/session secrets and lock signing operations to controlled release environment.
- Add artifact metadata generation (build ID, source revision, platform target, compatibility flags, migration marker).
- Verify signatures and checksums as part of release pipeline before publication.
- Store build provenance for traceability and rollback forensics.

**Cursor Prompt (S1)**
```text
Implement secure release artifact pipeline with deterministic builds, dedicated updater signing, provenance tracking, and mandatory signature/checksum verification before publish.
```

### S2 - Manifest Contract And Publication Governance
- Define manifest schema with required fields (`version`, `pub_date`, `notes`, `url`, `signature`, `channel`, minimum supported version, migration guard flags).
- Version manifest contract and enforce validation in publisher and client before rollout eligibility.
- Add channel-aware publishing workflow so artifacts can be staged independently for `internal`, `pilot`, and `stable`.
- Publish manifests and artifacts to object storage with immutable object/version policy and integrity metadata.
- Add pre-publication checks for compatibility and migration safety constraints.

**Cursor Prompt (S2)**
```text
Deliver versioned update manifest contracts with strict validation, channel-aware publication workflow, and immutable object-storage publication guarantees.
```

### S3 - Pipeline Security, Observability, And Acceptance
- Add release-pipeline observability for sign/publish failures, manifest validation errors, and artifact upload drift.
- Implement break-glass procedure for signing key incident without bypassing mandatory signature policy.
- Add automated tests for malformed manifests, missing signatures, and channel mismatch rejection behavior.
- Add release checklist requiring artifact integrity proof and manifest/client compatibility evidence.
- Gate completion on end-to-end dry run from build to published manifest validated by client-side verification flow.

**Cursor Prompt (S3)**
```text
Finalize artifact signing pipeline with observability, incident-safe key handling, and end-to-end validation that clients reject malformed or unsigned update contracts.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
