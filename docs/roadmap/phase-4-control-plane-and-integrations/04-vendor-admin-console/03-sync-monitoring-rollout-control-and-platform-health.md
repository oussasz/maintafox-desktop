# Sync Monitoring Rollout Control And Platform Health

**PRD:** §8 / §11

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Tenant Sync Health Monitoring And Failure Visibility
- Build sync monitoring dashboards with tenant-level lag, checkpoint freshness, rejection rate, retry pressure, and dead-letter indicators.
- Add drill-down from aggregate metrics into specific batches/items (entity type, failure reason, idempotency key, last processing attempt).
- Surface heartbeat-policy anomalies that affect sync posture (expired grace, revoked trust, stale machine policy snapshot).
- Provide operator-friendly conflict/repair queues with explicit action affordances (replay, requeue, acknowledge, escalate).
- Add sync-health severity model (info/warn/critical) tied to operational impact and support triage routing.

**Cursor Prompt (S1)**
```text
Implement tenant sync-health monitoring with lag/checkpoint/retry visibility, drill-down diagnostics, and actionable repair queue workflows for operators.
```

### S2 - Rollout Control Plane And Release Safety Governance
- Add rollout workspace for channel and cohort management (`stable`, `pilot`, `internal`) with progressive exposure controls.
- Implement staged rollout policy by tenant cohort and machine segment, with pause/recall controls for bad release containment.
- Show rollout impact preview before activation (affected tenants, machine count, entitlement/channel compatibility, known blockers).
- Add release diagnostics view combining download failures, signature verification failures, migration failures, and post-update heartbeat drop.
- Enforce approval workflow for recalls and emergency rollback policies to avoid unilateral high-blast-radius actions.

**Cursor Prompt (S2)**
```text
Deliver update rollout control with cohort-aware staging, pause/recall governance, and integrated diagnostics for signature, migration, and post-deploy health failures.
```

### S3 - Platform Health, Alerting, And Operational Readiness
- Add platform-health domain that aggregates service status for API, workers, PostgreSQL, Redis, object storage, and admin-ui dependencies.
- Surface infrastructure pressure indicators (DB latency, storage growth, queue saturation, worker backlog age) with trend and threshold context.
- Add alert acknowledgment, ownership, and timeline notes so incident handling is auditable and shift-safe.
- Provide tenant-safe drill-through from platform incidents to affected sync/licensing/rollout domains without exposing unrelated customer data.
- Gate completion on full operator run-through covering sync degradation, failed rollout recall, and infrastructure pressure incident response.

**Cursor Prompt (S3)**
```text
Finalize vendor operations dashboards by unifying sync, rollout, and platform-health telemetry with alert ownership workflows and tenant-safe incident drill-through.
```

---

*Completion: 2026-04-16 — Desktop vendor console: `SyncHealthPanel`, `RolloutOpsPanel`, `PlatformHealthOpsPanel`, `OpsAlertsPanel`, `VendorConsoleOpsHubCard`; contracts `vps::sync_rollout_platform_ops` + `shared/ipc-types` vendor-ops types + `src/services/vendor-ops-contracts.ts`. Verify: `cargo test sync_rollout_platform_ops vendor_admin_console --lib`, `pnpm typecheck`, `pnpm exec vitest run src/services/__tests__/vendor-ops-contracts.test.ts`.*

*Operational validation: 2026-04-17 — real-infra Sprint 10 rerun passed (`15/15`), with route verification and observability conformance evidence captured in `/root/sprint10-rc-closeout-20260417T130406Z/`.*
