# Customer License And Machine Management Flows

**PRD:** §10

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Customer Workspace And Entitlement Lifecycle Governance
- Build customer-management workspace covering tenant identity, deployment metadata, rollout cohort, support posture, and environment flags.
- Implement entitlement lifecycle forms for issue, renew, suspend, revoke, and emergency lock with clear state-transition guards.
- Add signed entitlement-envelope preview (tier, feature flags, machine slots, offline grace, update channel, validity window) before activation.
- Enforce dual-confirmation and mandatory reason codes for destructive actions (revocation, immediate expiry, machine-slot reduction).
- Keep all state changes auditable with actor, timestamp, previous claim snapshot, and change rationale.

**Cursor Prompt (S1)**
```text
Implement vendor console customer and entitlement management with state-governed lifecycle actions, signed-claim preview, and auditable approval controls for destructive changes.
```

### S2 - Machine Activation Monitor And Policy Controls
- Add machine activation table with heartbeat freshness, app version, trusted-device status, activation source, and anomaly flags.
- Implement operator actions for slot release, device rebind, soft suspend, and policy refresh trigger with guardrails against accidental tenant lockout.
- Add offline-policy controls (grace window, trust revocation behavior, reconnect enforcement) aligned with licensing policy model.
- Surface machine-fingerprint confidence and change-threshold decisions so hardware refresh does not trigger unnecessary support churn.
- Expose machine/event timeline for activation failures, repeated rebind attempts, and post-revocation reconnect behavior.

**Cursor Prompt (S2)**
```text
Deliver machine activation operations in the vendor console with heartbeat visibility, trusted-device lifecycle actions, and policy-driven offline controls that avoid accidental tenant disruption.
```

### S3 - Bulk Operations, Safety Nets, And Contract Validation
- Add safe bulk operations for renewals/channel reassignment with dry-run impact preview and per-tenant failure reporting.
- Integrate update-channel assignment logic so license policy and rollout policy remain consistent (`stable`, `pilot`, `internal`).
- Add typed admin API contracts and optimistic-concurrency checks to prevent stale overwrite during multi-operator sessions.
- Add test coverage for entitlement state matrix (`active`, `grace`, `expired`, `suspended`, `revoked`) and machine-slot boundary conditions.
- Gate completion on UAT checklist proving support operators can resolve common activation/license incidents without direct database intervention.

**Cursor Prompt (S3)**
```text
Finalize customer, entitlement, and machine-management flows with bulk safety controls, channel-policy consistency, concurrency protections, and full validation of state transitions and slot limits.
```

---

*Completion: 2026-04-16 — `vps::customer_entitlement_machine` (lifecycle matrix, signed preview, bulk/concurrency, machines), TS `customer-entitlement-machine-contracts.ts`, IPC types, vendor UI (`CustomerWorkspace`, `EntitlementLifecyclePanel`, `MachineActivationPanel`, `/vendor-console/machines`); tests: `cargo test customer_entitlement_machine_tests vendor_admin_console_tests --lib`, `pnpm exec vitest run src/services/__tests__/customer-entitlement-machine-contracts.test.ts`; `pnpm typecheck` clean.*
