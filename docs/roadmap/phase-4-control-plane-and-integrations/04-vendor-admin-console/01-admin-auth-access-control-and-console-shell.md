# Admin Auth Access Control And Console Shell

**PRD:** §16

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - Console Shell Foundation And Domain Navigation
- Build vendor console as a dedicated surface (`console.maintafox.systems`) separate from desktop runtime UI and tenant-auth contexts.
- Define shell layout with persistent tenant/search context, alert banner, environment badge, and fast drill-down to customer/license/sync/update domains.
- Add role-aware navigation where hidden domains remain non-discoverable for unauthorized admin roles.
- Include evidence-first UX patterns: each summary card links to underlying machine, batch, rollout, or audit records.
- Add operational state indicators for offline integrations, degraded queues, and pending high-risk actions.

**Cursor Prompt (S1)**
```text
Implement the vendor admin console shell as a dedicated control-plane UI with role-aware navigation, operational context, and evidence drill-through patterns.
```

### S2 - Admin Auth, Session Security, And Permission Boundaries
- Implement admin authentication model aligned with PRD §16.5: short-lived sessions, refresh cookie, mandatory TOTP, optional IP allowlist or VPN-only gate.
- Add step-up reauthentication for dangerous operations (mass machine revoke, entitlement suspension, forced rollback, tenant restore actions).
- Define granular console permissions (`console.view`, `customer.manage`, `entitlement.manage`, `sync.operate`, `rollout.manage`, `platform.observe`, `audit.view`).
- Enforce backend authorization for every admin route (UI gating is convenience only, not security boundary).
- Add session/audit events for login, failed MFA, step-up prompts, privileged action attempts, and explicit sign-out.

**Cursor Prompt (S2)**
```text
Deliver hardened admin auth and authorization for the vendor console with mandatory MFA, step-up for risky actions, and server-enforced permission boundaries on every admin endpoint.
```

### S3 - Console Runtime Resilience, Accessibility, And Acceptance
- Add typed frontend contracts with backend admin APIs (no ad-hoc payload parsing for critical control actions).
- Implement resilient loading/error states for high-latency operational screens and preserve context during refresh or session renewal.
- Add accessibility and operator ergonomics checks for dense operational views (keyboard navigation, focus order, critical-action clarity).
- Add end-to-end tests for admin sign-in, MFA, role-scoped navigation, and blocked unauthorized action paths.
- Gate completion on security review checklist (session expiry behavior, token refresh edge cases, and audit event completeness).

**Cursor Prompt (S3)**
```text
Finalize vendor console shell with typed API contracts, resilient operational UX, and end-to-end validation for MFA/session security and role-scoped admin action boundaries.
```

---

*Completion: 2026-04-16 — Vendor console route tree (`/vendor-console`), shell (`src/vendor-console/`), RBAC migration `m20260511_000066_vendor_console_permissions`, VPS contracts `vps::vendor_admin_console`, Zod contracts `vendor-admin-console-contracts.ts`, IPC types in `shared/ipc-types.ts`; tests: `cargo test vendor_admin_console_tests --lib`, `pnpm exec vitest run src/vendor-console src/services/__tests__/vendor-admin-console-contracts.test.ts`; `pnpm typecheck` clean.*
