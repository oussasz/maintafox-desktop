# License Enforcement Flows And Admin Actions

**PRD:** §10

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Enforcement Matrix And Safe Degradation Behavior
- Implement enforcement matrix mapping entitlement state to allowed operations per capability class (full, read-only, blocked write, blocked activation).
- Enforce runtime checks in backend command handlers so blocked actions cannot execute even if UI is stale.
- Apply graceful degradation for operational continuity where policy allows (historical read access, diagnostics, support export).
- Separate commercial enforcement from emergency security lock posture for incident-response scenarios.
- Add rejection error taxonomy that clearly distinguishes entitlement block, trust block, and policy-sync pending block.

**Cursor Prompt (S1)**
```text
Implement a strict license-enforcement matrix with backend command gating, safe module degradation behavior, and typed rejection reasons for entitlement and trust violations.
```

### S2 - Admin Actions And Client Reconciliation
- Implement admin-triggered actions (`suspend`, `revoke`, `reactivate`, slot release, channel change) with signed policy propagation to clients.
- Add client reconciliation flow to apply remote admin actions atomically and avoid partial local state mismatch.
- Ensure revocation and suspension actions trigger immediate runtime guard updates after heartbeat or forced refresh.
- Add audit linkage between admin action event and local enforcement transition for traceability.
- Add conflict handling when admin actions race with local pending writes (block/queue/resolve rules by domain criticality).

**Cursor Prompt (S2)**
```text
Deliver admin-to-client license action reconciliation so suspension/revocation/reactivation updates apply atomically, auditable, and safely during concurrent local activity.
```

### S3 - Operator UX, Messaging, And Validation
- Build license status panel in settings showing state, grace countdown, slot posture, channel, last policy sync, and required user actions.
- Add proactive warnings for grace expiration and pending revocation application on next reconnect.
- Provide remediation paths in UX (retry heartbeat, open support bundle flow, contact admin guidance) instead of dead-end errors.
- Add tests for enforcement transition timing, race conditions with unsynced work, and post-reactivation recovery.
- Gate completion on UAT validation that operators can understand and recover from license-state changes without unsafe manual workarounds.

**Cursor Prompt (S3)**
```text
Finalize license enforcement UX and reliability with actionable status messaging, transition-timing tests, and validated recovery paths for suspended, revoked, and reactivated states.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
