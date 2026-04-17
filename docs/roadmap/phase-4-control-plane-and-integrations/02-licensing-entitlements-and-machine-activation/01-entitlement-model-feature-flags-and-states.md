# Entitlement Model Feature Flags And States

**PRD:** §10

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Entitlement Envelope Model And Local Cache Contracts
- Implement signed entitlement-envelope schema in local cache with claim fields for tier, feature flags, machine slots, offline policy, channel, and validity timestamps.
- Add signature verification and issuer validation pipeline separated from local session-token logic.
- Persist entitlement snapshots with version lineage and last-verified metadata for audit and troubleshooting.
- Define local fallback behavior when VPS unreachable (use last valid signed snapshot within policy constraints).
- Add migration path for future entitlement claim expansion without breaking older clients.

**Cursor Prompt (S1)**
```text
Implement signed entitlement-envelope persistence and verification with robust local cache semantics, version lineage, and policy-safe offline fallback behavior.
```

### S2 - Feature Flag Runtime Gating And State Semantics
- Map entitlement feature flags to strongly typed runtime capability gates in Rust and frontend navigation guards.
- Add explicit entitlement states (`active`, `grace`, `expired`, `suspended`, `revoked`) with deterministic local behavior for each.
- Ensure gated modules degrade safely (read-only where allowed, blocked writes where required, clear operator messages).
- Add heartbeat-refresh integration to update state and flags atomically without partial gate drift.
- Prevent hidden bypasses by enforcing gate checks in backend command handlers, not UI only.

**Cursor Prompt (S2)**
```text
Deliver entitlement-driven feature gating with explicit state semantics, atomic policy refresh behavior, and backend-enforced capability checks across modules.
```

### S3 - IPC Exposure, UX Transparency, And Validation
- Add typed IPC contracts exposing entitlement summary, feature availability map, and policy expiry information to UI surfaces.
- Implement entitlement diagnostics view in settings for support triage (current state, expiry, last heartbeat, channel, machine-slot posture).
- Add tests for state transitions and boundary edges (grace expiry, suspension recovery, revoked reconnection, malformed claim rejection).
- Validate that module visibility and action-level permissions remain consistent during entitlement changes mid-session.
- Gate completion on integrated tests proving consistent enforcement between local cache, runtime checks, and UI contract exposure.

**Cursor Prompt (S3)**
```text
Finalize entitlement modeling with typed IPC visibility, support-grade diagnostics, and tests that verify consistent enforcement across state transitions and mid-session policy changes.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
