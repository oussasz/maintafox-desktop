# Client Update Installation And Migration Safety

**PRD:** §11

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Client Update Flow Integration And Trust Checks
- Harden Tauri updater integration to enforce mandatory signature validation and channel eligibility before download/install.
- Add client preflight checks (disk space, current state safety, active sync critical section, backup checkpoint requirement).
- Ensure updater flow handles interrupted downloads and resume/retry semantics without corrupted install state.
- Bind updater trust to dedicated public key chain and prevent fallback to insecure transport.
- Add structured update session logs with correlation IDs for support diagnostics.

**Cursor Prompt (S1)**
```text
Implement hardened client updater flow with mandatory trust checks, safe preflight validation, resumable downloads, and support-grade update session logging.
```

### S2 - Migration Safety, Startup Gates, And Recovery Path
- Enforce migration execution before normal UI entry with explicit startup gate when required migrations fail.
- Add idempotent migration safety model with schema-version tracking and pre-migration checkpoint for destructive classes.
- Provide controlled fallback path for failed migration (safe mode diagnostics, rollback guidance, support export).
- Prevent partial upgrade state from entering normal operations when compatibility invariants are broken.
- Validate module-specific migration dependencies that can affect sync/licensing post-update behavior.

**Cursor Prompt (S2)**
```text
Deliver migration-safe update installation with startup gating, idempotent schema progression, and controlled recovery path when post-update migrations fail.
```

### S3 - Update UX, Messaging, And Acceptance Tests
- Implement settings UX for update availability, release notes, scheduled install options, and deferred-install policy constraints.
- Surface clear operator messages for trust failure, migration failure, paused rollout, and forced security update scenarios.
- Add tests for install interruption, failed signature, incompatible manifest, and migration rollback/safe-mode entry.
- Validate that update flow preserves unsynced local work with explicit user warning and deferred path when needed.
- Gate completion on user-facing and integration tests confirming trustworthy install behavior across success and failure paths.

**Cursor Prompt (S3)**
```text
Finalize client update delivery with clear UX and robust failure handling, including signature/migration/incompatibility test coverage and unsynced-work safety protections.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
