# Signal Model Device Binding And Protocol Adapters

**PRD:** §6.21

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - IoT Signal And Device Binding Data Model
- Define canonical IoT domain model for gateways, devices, signals, quality flags, and asset bindings with stable IDs and versioned metadata.
- Persist binding lifecycle (`discovered`, `mapped`, `active`, `suspended`, `retired`) with audit fields and effective-dating for history safety.
- Enforce governed signal semantics (unit, type, aggregation mode, expected cadence, anomaly thresholds) to protect analytics integrity.
- Add compatibility with asset/equipment registry identities so bindings survive normal asset metadata updates.
- Add validation rules preventing duplicate active bindings for same device-signal to conflicting asset contexts.

**Cursor Prompt (S1)**
```text
Implement IoT signal and device-binding model with governed semantics, lifecycle history, and safe integration with stable asset identities.
```

### S2 - Protocol Adapter Architecture And Runtime Boundaries
- Implement protocol adapter interface for MQTT/OPC-UA/HTTP/other connectors with clear contract for normalize-validate-forward behavior.
- Isolate adapter failures so one protocol outage does not stop all ingestion paths.
- Add adapter configuration schema with secure secret references and connection-health visibility.
- Define adapter capability metadata (batch support, QoS behavior, security profile) for operator planning.
- Add adapter lifecycle controls (`enabled`, `degraded`, `paused`, `disabled`) with reason tracking and operational events.

**Cursor Prompt (S2)**
```text
Deliver a pluggable IoT protocol-adapter architecture with failure isolation, secure configuration contracts, and operator-visible adapter lifecycle controls.
```

### S3 - Security Baseline, Binding UX, And Validation
- Store IoT credentials via secure secret handling workflows and never expose plaintext in runtime diagnostics.
- Add binding UI/operator workflows for mapping signals to assets, reviewing mapping confidence, and approving risky remaps.
- Add tests for duplicate binding prevention, schema validation, adapter failure isolation, and secure credential retrieval paths.
- Ensure IoT bindings and signal metadata changes emit auditable events for compliance and troubleshooting.
- Gate completion on end-to-end validation from device ingest to bound asset signal visibility with security controls enabled.

**Cursor Prompt (S3)**
```text
Finalize IoT device-binding delivery with secure credential handling, operator mapping workflows, and full validation of adapter isolation and binding integrity.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
