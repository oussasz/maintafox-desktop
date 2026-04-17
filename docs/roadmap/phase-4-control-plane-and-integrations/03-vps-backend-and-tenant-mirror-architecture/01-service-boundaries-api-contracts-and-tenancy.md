# Service Boundaries API Contracts And Tenancy

**PRD:** §16 / §5

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - VPS Service Boundaries And Contract Ownership
- Split VPS backend into explicit API families aligned with PRD §16.3: License/Activation, Sync, Updates, Admin, and Relay endpoints.
- Define contract ownership per family (request/response DTOs, error taxonomy, idempotency headers, and audit metadata expectations).
- Keep desktop runtime authority clear: VPS APIs coordinate/mirror state and policy, but never become the source of truth for day-to-day local execution.
- Add strict route versioning (`/api/v1/*`, `/admin/v1/*`) and deprecation policy so desktop/VPS compatibility remains controlled during phased rollout.
- Enforce correlation IDs and structured request logging at boundary entry for all externally reachable routes.

**Cursor Prompt (S1)**
```text
Refactor VPS API surface into clear contract families (license, sync, updates, admin, relay) with explicit ownership, versioning, and typed error boundaries. Keep local desktop authority intact while making VPS coordination contracts production-safe.
```

### S2 - Multi-Tenancy Resolution, Isolation, And Auth Context
- Implement tenant-resolution middleware that binds every request to validated tenant context (token claims + machine trust + endpoint policy).
- Separate trust models for machine/service routes vs vendor-admin routes (no shared auth session between tenant runtime and vendor console operations).
- Enforce per-tenant data isolation contracts before handler execution, including deny-by-default guards for missing tenant context.
- Define admin auth/session profile for control-plane operations (short-lived sessions, refresh-cookie flow, step-up requirement for dangerous actions).
- Add request-scope permission matrix for VPS admin domains (`customers`, `entitlements`, `machines`, `sync_ops`, `rollout_ops`, `platform_health`, `audit`).

**Cursor Prompt (S2)**
```text
Implement tenancy-aware request context and split auth boundaries for tenant runtime APIs versus vendor admin APIs. Enforce deny-by-default tenant isolation and permission-scoped access in every VPS route family.
```

### S3 - Shared Contracts, Policy Delivery, And Readiness Gates
- Publish shared TypeScript contracts used by admin UI and desktop integration clients for license heartbeat, sync push/pull, and rollout metadata.
- Add policy payload contract for heartbeat refresh (entitlement state, offline grace, trusted-device policy, channel assignment, urgent notices).
- Add idempotency and replay-protection contract requirements (`idempotency_key`, checkpoint token, acknowledged items) across sync and activation writes.
- Add rate limiting and abuse controls for sensitive endpoints (activation, heartbeat, admin mutate actions) with explicit typed throttling errors.
- Gate slice completion on integration tests that verify tenant isolation, auth boundary separation, and contract compatibility across API versions.

**Cursor Prompt (S3)**
```text
Finalize VPS API contracts as shared typed interfaces with policy delivery, idempotency requirements, and replay-safe semantics. Add readiness tests for tenant isolation, split auth boundaries, and version-compatibility behavior.
```

---

*Completion: 2026-04-16, Codex (agent), `cargo check` passed, `pnpm typecheck` passed, readiness tests passed for VPS contract guards (tenant isolation, split auth boundaries, version compatibility).*
