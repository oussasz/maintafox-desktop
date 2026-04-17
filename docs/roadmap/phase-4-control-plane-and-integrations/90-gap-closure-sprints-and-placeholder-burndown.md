# Phase 4 Gap Closure Sprint Plan (Placeholder/Hardcode Burn-down)

**Source alignment:** `docs/PRD.md` (v3.0), `docs/research/MASTER_RESEARCH_FRAMEWORK.md`, and all Phase 4 roadmap slices under this folder.

**Objective:** close all known implementation gaps in the control plane and remove placeholder/hardcoded behavior from production paths with enterprise-grade execution discipline.

---

## 1) Current-State Gap Summary (Code + Roadmap)

### Confirmed high-priority gaps
- Vendor console still contains mock-backed operational panels (sync, rollout, platform health, audit/support hardening) and fixture datasets.
- Vendor console auth path has dev/mock bypass surfaces (`VITE_DEV_MOCK_AUTH`) and minimal token handling that must be production-constrained.
- Control API is now operational (JWT + tenants/licenses/activation claim), but still requires enterprise hardening (idempotency, pagination, audit depth, policy enforcement breadth).
- Desktop product-license gate supports offline-first fallback, but online reconciliation/refresh/revocation handling is not fully closed loop.
- Force-update governance exists conceptually, but end-to-end tenant/channel policy to desktop enforcement is not fully complete.
- Production operations are running on HTTP and require HTTPS hardening, secret rotation, and incident-grade observability posture.

### Professional execution principles (large-company standard)
- No placeholder data in production workflows.
- API contract-first delivery with backward-compatible versioning.
- Security and audit controls are release gates, not post-release tasks.
- Every sprint ends with objective acceptance checks (functional, security, reliability, operability).

---

## 2) Sprint Ownership Model

- **Cursor Agent** = codebase implementation in local repo(s): frontend, desktop, API, contracts, tests, docs.
- **VPS Agent** = server-side execution only: deploy, env/secrets, DNS/TLS, container/runtime health, smoke tests.
- **Joint** = Cursor Agent delivers code and runbook; VPS Agent executes rollout and evidence capture.

---

## 3) Sprint Plan (Detailed)

> Sprint cadence suggestion: 1 week per sprint (or 5 business days), with daily integration checks.

### Sprint 1 — **[Executor: Cursor Agent]** Contract Freeze + Placeholder Inventory
**Goal:** establish one source of truth for all control-plane contracts and produce a complete placeholder/hardcode inventory.

**Scope**
- Freeze and publish canonical API contracts for:
  - auth/session (`/api/v1/auth/login`, token lifecycle)
  - tenants/licenses/activation
  - sync monitoring, rollout, platform health
  - audit/support workflows
- Generate a tracked inventory of:
  - mock fixture usage
  - hardcoded defaults/seeds in runtime paths
  - placeholder UI states that currently mimic production behavior

**Deliverables**
- `docs/contracts/control-plane-v1.md` (or equivalent in roadmap docs).
- `docs/roadmap/phase-4-control-plane-and-integrations/placeholders-and-hardcodes-inventory.md`.
- CI check ensuring no production code imports fixture modules outside test/dev paths.

**Sprint 1 Artifacts (frozen)**
- `docs/roadmap/phase-4-control-plane-and-integrations/91-control-plane-v1-contract-freeze.md`
- `docs/roadmap/phase-4-control-plane-and-integrations/92-placeholders-and-hardcodes-inventory.md`

**Acceptance**
- Every Phase 4 screen maps to at least one typed API contract.
- Placeholder inventory has owner + target sprint for each entry.

**Sprint Prompt**
```text
Create and freeze the control-plane v1 contracts, then inventory all placeholder/mock/hardcoded production paths with owner and removal sprint mapping.
```

---

### Sprint 2 — **[Executor: Cursor Agent]** Customer + License + Activation Productionization
**Goal:** close all customer/license/machine flows without placeholder values.

**Scope**
- Replace remaining read-only/hardcoded values in customer and entitlement screens with live API bindings.
- Add robust UX states: loading, empty, error, retry, optimistic mutation feedback.
- Implement activation claim diagnostics in UI with strict validation and incident-safe errors.

**Deliverables**
- No hardcoded `defaultValue` production metadata in customer/license critical forms.
- API-integrated tenant/license lifecycle matrix behavior with clear action constraints.
- Contract tests for issue/list/revoke/activation paths.

**Acceptance**
- Operator can create tenant, issue license, revoke license, and execute activation claim entirely through UI.
- No mock fixture imports in customer/license/machine production components.

**Sprint Prompt**
```text
Remove remaining placeholders in customer/license/machine workflows and complete production-grade API wiring with resilient UX and contract validation.
```

---

### Sprint 3 — **[Executor: Cursor Agent]** Sync/Rollout/Platform Health De-mock
**Goal:** remove mock operational telemetry and connect real API telemetry surfaces.

**Scope**
- Replace mock sync lag/checkpoint/repair queue datasets with live API.
- Replace rollout mock impact/stage/diagnostic data with real release control endpoints.
- Replace platform service/pressure mock data with real health telemetry.

**Deliverables**
- Live telemetry adapters with typed decoding and fallback behavior.
- Unified severity model (`info`, `warn`, `critical`) from API.
- Error-budget aware refresh strategy (polling/backoff) for operator dashboards.

**Acceptance**
- All ops hub cards and drill-down pages run against real data.
- No `MOCK_*` imports in sync/rollout/platform production components.

**Sprint Prompt**
```text
Migrate all sync, rollout, and platform-health dashboards from fixture data to real API telemetry with typed contracts and operator-safe failure handling.
```

---

### Sprint 4 — **[Executor: Cursor Agent]** Audit/Support Hardening Productionization
**Goal:** replace mock audit/support flows with immutable evidence-grade operations.

**Scope**
- Wire audit ledger search/filter/export to server-side append-only data.
- Implement support ticket lifecycle with SLA timestamps and escalation path.
- Bind diagnostic bundle manifests and redaction metadata to real endpoints.

**Deliverables**
- Immutable-chain verification endpoint consumption in UI.
- Support timeline links between ticket actions and privileged operations.
- Export format for compliance evidence packs.

**Acceptance**
- Audit and support tabs run without fixture data.
- Forensic drill-through works from action -> entity -> ticket.

**Sprint Prompt**
```text
Replace audit/support mock workflows with immutable, searchable, exportable production flows and full ticket-to-intervention traceability.
```

---

### Sprint 5 — **[Executor: Cursor Agent]** API Hardening + Governance Controls
**Goal:** elevate API to production reliability/security standards.

**Scope**
- Add pagination/filtering/sorting for list endpoints.
- Add idempotency keys for mutating admin endpoints.
- Add structured audit event logging on every privileged action.
- Add request validation taxonomy and consistent error envelopes.
- Add seed/hardcoded behavior safety rails (disabled in production mode).

**Deliverables**
- API middleware stack: authn/authz, idempotency, request-id, audit hooks.
- OpenAPI-like schema doc for v1 endpoints.
- Security tests for token misuse, unauthorized scopes, replay attempts.

**Acceptance**
- API passes regression + security checks.
- Hardcoded seed behavior only runs explicitly in bootstrap/dev modes.

**Sprint Prompt**
```text
Harden control-plane API with idempotency, pagination, strict validation, auditable privileged actions, and production-safe seed behavior controls.
```

---

### Sprint 6 — **[Executor: Cursor Agent]** Desktop Activation Enforcement Completion
**Goal:** complete online/offline activation policy loop in desktop runtime.

**Scope**
- Persist activation claim + reconciliation state machine locally.
- Add scheduled revalidation when online.
- Enforce explicit deny states (revoked/expired/slot-limit/force-update-required).
- Add support diagnostics for activation incidents.

**Deliverables**
- Activation state diagram documented and implemented.
- Retry/backoff policy for API outage scenarios.
- UI messaging matrix for all deny and degraded states.

**Acceptance**
- First boot, offline-first continuity, and revalidation policies behave per PRD licensing intent.
- No local-only “success” path that bypasses eventual server reconciliation.

**Sprint Prompt**
```text
Finalize desktop activation as a policy-driven online/offline state machine with periodic reconciliation, explicit deny states, and operator diagnostics.
```

---

### Sprint 7 — **[Executor: Cursor Agent]** Force-Update Policy End-to-End
**Goal:** enable tenant/channel-driven mandatory update enforcement.

**Scope**
- Add API policy fields for min app version and force-update mode.
- Add vendor console controls for per-tenant and per-cohort update constraints.
- Integrate desktop updater decision gates using resolved policy.

**Deliverables**
- API + UI + desktop updater integration tests.
- Emergency override and rollback controls with audit requirements.

**Acceptance**
- Chosen client/tenant can be put into force-update-required state and desktop enforces policy correctly.

**Sprint Prompt**
```text
Deliver full force-update governance from vendor controls to desktop enforcement, including tenant/cohort targeting, rollback safety, and auditability.
```

---

### Sprint 8 — **[Executor: VPS Agent]** Production Infrastructure Hardening (HTTP -> HTTPS)
**Goal:** move deployed control plane to secure production posture.

**Scope**
- Enable TLS for `console.maintafox.systems` and `api.maintafox.systems`.
- Re-enable/verify host security controls (`fail2ban`, firewall policy, SSH hardening).
- Rotate exposed credentials/secrets and verify secret hygiene.

**Deliverables**
- HTTPS live on both domains.
- Security checklist evidence (ports, cert renewal, service ownership).

**Acceptance**
- Public endpoints pass TLS checks and security baseline.
- No plaintext production login traffic.

**Sprint Prompt**
```text
Harden VPS deployment to production security baseline: TLS, firewall, fail2ban, secret rotation, and validated secure public routing.
```

---

### Sprint 9 — **[Executor: Joint — Cursor Agent + VPS Agent]** Observability + SLO + Runbooks
**Goal:** enterprise-grade operational readiness.

**Scope**
- Add structured logs and metrics for API, edge, and key desktop sync/activation events.
- Define SLOs for API health, auth latency, activation success, and sync operator surfaces.
- Publish incident runbooks for top failure classes.

**Deliverables**
- Runbook pack + on-call escalation matrix.
- Dashboard + alert thresholds tied to SLOs.

**Acceptance**
- Simulated incident drill completed with evidence and postmortem template.

**Sprint Prompt**
```text
Implement control-plane observability and SLO-driven operations with actionable runbooks and validated incident drill readiness.
```

---

### Sprint 10 — **[Executor: Joint — Cursor Agent + VPS Agent]** UAT, Compliance, Release Candidate
**Goal:** close all Phase 4 gaps and promote RC to production.

**Scope**
- End-to-end UAT against real environment:
  - auth
  - tenant/license lifecycle
  - machine activation and revalidation
  - force-update policy
  - sync/rollout/platform health
  - audit/support exports
- Compliance evidence pack generation.

**Deliverables**
- RC sign-off checklist.
- Gap closure report showing zero production placeholders/hardcoded workflow data.

**Acceptance**
- Product/Engineering/Security sign-off complete.
- Release promotion approved.

**Sprint Prompt**
```text
Run full UAT and compliance validation on real infrastructure, close all remaining control-plane gaps, and produce release-candidate sign-off evidence.
```

---

## 4) Governance And Quality Gates Per Sprint

- **Definition of Ready (DoR):**
  - contract for scope finalized
  - test cases enumerated
  - rollback plan prepared
- **Definition of Done (DoD):**
  - code + tests + docs updated
  - zero unresolved high-severity lints/type errors
  - security and audit implications reviewed
  - deployment/runbook instructions updated
- **No-go conditions:**
  - placeholder or hardcoded production values in critical flow
  - missing audit trail for privileged action
  - unversioned contract changes

---

## 5) Recommended Execution Order

1. Sprint 1 -> 2 -> 3 -> 4 (product gaps and de-mock).
2. Sprint 5 -> 6 -> 7 (hardening and licensing/update enforcement completion).
3. Sprint 8 -> 9 -> 10 (infrastructure hardening, SLO readiness, production RC).

---

## 6) Notes For Cursor Agent Routing

- Any sprint titled **Executor: Cursor Agent** is implemented in repo(s) directly.
- Any sprint titled **Executor: VPS Agent** is executed on server infrastructure only.
- Joint sprints require Cursor-delivered artifacts first, then VPS rollout and evidence capture.

---

## 7) Execution Status Snapshot (2026-04-17)

- **Completed (Cursor Agent):** Sprint 1, 2, 3, 4, 5, 6, 7.
- **Completed (Joint):** Sprint 8 baseline delivered by VPS Agent; Sprint 9 artifacts + observability instrumentation delivered by Cursor Agent.
- **Completed (Joint):** Sprint 10 UAT rerun passed (`15/15`) on real infrastructure; RC decision `GO`.
- **Prepared artifacts (Cursor Agent):**
  - `93-observability-slo-and-runbooks-pack.md` (runbooks, on-call matrix, dashboard/alerts)
  - `95-incident-drill-evidence-and-postmortem-template.md` (drill evidence + reusable template)
  - `94-uat-compliance-and-rc-signoff-pack.md` (Sprint 10 sign-off checklist)
  - `96-phase4-gap-closure-report.md` (zero-placeholder closure evidence)
- **New quality gate active:** CI fixture/mock import guard (`pnpm run guard:phase4:fixtures`).

### Track alignment note

- This file is the full Phase 4 master plan (10-sprint closure model).
- `97-tenant-activation-auth-onboarding-sprints.md` is a focused onboarding stream view (9-sprint sequence) and is considered aligned with this master status at RC `GO`.

### Sprint 10 runtime evidence (VPS)

- UAT pass bundle: `/root/sprint10-uat-20260417T130406Z-postfix/`
- RC closeout bundle: `/root/sprint10-rc-closeout-20260417T130406Z/`
- Frontend hardening evidence bundle: `/root/logs/frontend-hardening-20260417T145601Z.tar.gz`
- Frontend hardening bundle SHA-256: `a6a9ef2aa047e6c5d5ab5f1690f627d658c172261c9db51970d8d1a184b96e2b`
- API image digest: `sha256:49d57a8b35d146c3237f548fc1918d988edb8fced1845f4b0c57dd0b53fc3ce0`
- Edge image digest: `sha256:65645c7bb6a0661892a8b03b89d0743208a18dd2f3f17a54ef4b76fb8e2f2a10`

