# Machine Activation Binding And Offline Policy

**PRD:** §10

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Trusted Device Binding And Activation Contracts
- Implement machine activation flow binding entitlement slot to privacy-preserving device fingerprint anchored by install-local secret + stable OS/hardware anchors.
- Define activation payload and response contracts (`machine_id`, trust score, slot assignment, policy snapshot, issued-at/expiry bounds).
- Add machine-binding tolerance policy for expected hardware maintenance without unnecessary forced reactivation.
- Persist activation lineage locally (first activation, latest heartbeat, rebind reason, revocation state) for support diagnostics.
- Enforce slot assignment consistency between local state and VPS to prevent orphaned or duplicate activations.

**Cursor Prompt (S1)**
```text
Implement machine activation contracts with trusted-device binding, resilient fingerprint tolerance policy, and auditable slot assignment consistency across local and VPS states.
```

### S2 - Offline Grace Policy And Reconnect Enforcement
- Implement offline eligibility checks requiring prior online bootstrap, trusted device status, valid signed entitlement snapshot, and non-expired grace window.
- Add policy-driven grace behavior for suspended/expired/revoked states with clear reconnect requirements and blocked-action semantics.
- Separate unlock/session behavior from license validity so offline access controls remain precise and auditable.
- Add reconnect enforcement rules that apply pending revocations or policy changes immediately after connectivity restoration.
- Surface explicit user-facing messages for offline denial reasons (untrusted device, grace expired, policy revoked, missing bootstrap).

**Cursor Prompt (S2)**
```text
Deliver offline policy enforcement for machine activation with strict eligibility checks, reconnect-time revocation application, and clear operator-facing denial explanations.
```

### S3 - Secret Handling, Rotation, And Validation Coverage
- Store machine trust secrets and activation tokens only through OS-managed secure storage interfaces (no plaintext persistence in SQLite/config).
- Add secret-rotation and rebind workflows for compromised device trust material with controlled downtime impact.
- Add tests for fingerprint drift thresholds, slot exhaustion, replayed activation responses, and offline grace edge timing.
- Verify activation and offline policies remain consistent during time-skew and intermittent network conditions.
- Gate completion on end-to-end activation/offline/reconnect test matrix with security checks and support runbook readiness.

**Cursor Prompt (S3)**
```text
Finalize machine activation and offline policy security with keychain-backed secret handling, rebind/rotation controls, and full validation of drift, replay, slot, and reconnect edge cases.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
