# VPS Deployment Observability And Recovery Validation

**PRD:** §16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Production Deployment Baseline And Security Posture
- Define production Docker Compose topology for `nginx`, `api`, `worker`, `postgres`, `redis`, `admin-ui`, object storage integration, and observability stack.
- **DNS and public hostnames:** register A/AAAA (or CNAME) for production domains (e.g. `console.maintafox.systems` for vendor admin UI, `api.maintafox.systems` or equivalent for tenant/runtime API) pointing at the VPS; keep **tenant runtime API** and **vendor admin** on separate hostnames to align with split auth boundaries from `01-service-boundaries-api-contracts-and-tenancy.md`.
- **TLS edge:** terminate HTTPS at the reverse proxy with valid certificates (e.g. ACME/Let’s Encrypt), HTTP→HTTPS redirect, HSTS where appropriate; document CAA and renewal runbook (see §S3 in this file for cert ops).
- Add hardened network model (public exposure only for HTTPS edge, internal service network segmentation, restricted SSH operations).
- Implement secret injection strategy using environment references and runtime secret stores (no long-lived plaintext in compose files).
- Add deployment profiles for pilot vs shared production vs growth sizing aligned with PRD §16.1.
- Define release and rollback deployment workflow with explicit preflight checks (DB migration readiness, queue drain behavior, artifact integrity).
- **VPS access:** SSH and cloud credentials are **operational secrets** used only during deployment and maintenance — not embedded in the desktop repo; rotate after any exposure.

**Cursor Prompt (S1)**
```text
Implement secure production deployment baseline for VPS services with hardened network boundaries, secret handling, sizing profiles, and safe deploy/rollback workflow.
```

### S2 - Observability Contracts, SLOs, And Alert Routing
- Add structured logs with correlation IDs across API, worker, and admin actions for traceability between tenant events and platform operations.
- Publish service metrics for heartbeat success rate, activation latency, sync queue lag, update download failures, and worker retry/dead-letter rates.
- Define SLOs and alert thresholds for control-plane-critical paths (license heartbeat availability, sync throughput, rollout service health, storage pressure).
- Add tenant-level health views so support can isolate degraded customers without exposing cross-tenant data.
- Add on-call alert routing and acknowledgment workflow with incident severity mapping tied to platform/customer impact.

**Cursor Prompt (S2)**
```text
Deliver VPS observability contracts with structured logs, control-plane SLOs, and tenant-level health indicators for heartbeat, sync, rollout, and worker reliability.
```

### S3 - Disaster Recovery Validation And Operational Sign-off
- Execute regular disaster-recovery drills covering control-plane metadata restore, tenant mirror restore, and update artifact recovery.
- Validate post-restore continuity for entitlement checks, machine activation, sync checkpoint progression, and admin audit retrieval.
- Add failure-injection scenarios (worker outage, Redis pressure, PostgreSQL failover, object-store unavailability) with documented operator response.
- Maintain operational runbooks for certificate renewal, signing-key rotation, and emergency access hardening updates.
- Gate completion on evidence-backed DR test results, remediation closure, and clean operational readiness checklist.

**Cursor Prompt (S3)**
```text
Finalize VPS deployment readiness by validating DR and failure-injection scenarios, confirming entitlement/sync continuity after restore, and codifying runbooks for keys, certs, and incident response.
```

---

*Completion: 2026-04-16 — `vps::deployment_observability` (S1–S3 contracts, runbooks, DR evidence types), tests (`cargo test deployment_observability_tests --lib`), TS mirrors in `shared/ipc-types.ts`; `pnpm typecheck` clean.*
