# Licensing Security Validation And Auditability

**PRD:** §10

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Licensing Threat Model And Control Implementation
- Document licensing threat model for forged entitlements, replayed activation payloads, slot abuse, time manipulation, and offline policy bypass attempts.
- Enforce cryptographic verification controls for entitlement signatures, issuer trust, and claim integrity before runtime acceptance.
- Add anti-replay controls on activation/heartbeat responses (nonce or token freshness, signed timestamp windows, idempotency context).
- Separate key material by purpose (session secrets vs entitlement verification keys vs updater signing trust chain).
- Define incident-response behavior for suspected key compromise or mass revocation events.

**Cursor Prompt (S1)**
```text
Implement licensing security controls from threat model to runtime enforcement, including signature trust, anti-replay protection, key separation, and compromise-response behavior.
```

### S2 - Auditability And Forensic Evidence
- Record immutable audit events for entitlement issuance, activation, slot release, suspension/revocation, reactivation, and policy refresh outcomes.
- Include correlation IDs linking VPS admin action, API transaction, and local enforcement result for end-to-end traceability.
- Persist audit metadata needed for customer dispute handling (claim snapshot hash, actor, timestamp, machine context, reason code).
- Add export-friendly audit query surfaces for support/compliance investigations.
- Ensure audit retention and tamper-evidence expectations align with control-plane governance standards.

**Cursor Prompt (S2)**
```text
Deliver full licensing auditability with immutable event traces that connect admin actions, API exchanges, and local enforcement outcomes for compliance and support investigations.
```

### S3 - Adversarial Testing, Time Edge Cases, And Sign-off
- Add adversarial tests for tampered tokens, invalid signatures, claim mutation attempts, replayed responses, and stale activation grants.
- Add time-skew and clock-drift tests validating grace calculations and expiry handling under non-ideal system time conditions.
- Validate behavior when network flaps during policy refresh to ensure no unsafe temporary bypass states.
- Maintain security validation checklist in file footer for release gating and regression tracking.
- Gate completion on passing security tests, reviewed threat notes, and verified audit completeness across all licensing transitions.

**Cursor Prompt (S3)**
```text
Finalize licensing security readiness with adversarial validation, clock-skew resilience tests, and release-gate evidence proving audit-complete, bypass-resistant enforcement behavior.
```

---

## Licensing Security Validation Checklist

- [x] Tampered or unsigned entitlement envelopes are rejected before any runtime gate update.
- [x] Replayed activation/heartbeat responses are detected and blocked by freshness/idempotency controls.
- [x] Grace and expiry calculations stay correct under bounded clock skew and reconnect timing edges.
- [x] Revocation and suspension apply deterministically on reconnect without stale local bypass windows.
- [x] All admin and client-side licensing transitions produce immutable, correlation-linked audit entries.

---

*Completion: 2026-04-16, Codex (agent), `cargo check` passed, `pnpm typecheck` passed, targeted tests passed (`entitlements::tests`, `activation::tests`, `license::tests`).*
