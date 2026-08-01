# Tenant -> License -> Activation -> Admin Bootstrap -> User Login (Implementation Sprints)

**Goal:** implement enterprise-grade onboarding workflow in this order:

1. Tenant provisioning (vendor side)
2. License/activation key issuance
3. Desktop activation (tenant binding)
4. First admin account / SSO bootstrap
5. Normal user login

**UX hardening mandate (applies to all Local Agent sprints):**
- No hardcoded/mock/fixture runtime data in production pages.
- No silent, inactive, or no-op buttons in pages, dialogs, or sheets.
- Every CTA must do one of: real mutation, real fetch/refresh, explicit disabled+reason, or explicit read-only label.

---

## Sprint 1 — Tenant Provisioning API + Governance
**Executor:** `VPS Agent`

### Scope
- Create hardened tenant provisioning endpoints and DB writes (idempotent, auditable).
- Enforce required tenant metadata (slug, display name, rollout channel/cohort, policy defaults).
- Add provisioning audit records with actor, reason, request id, and timestamp.
- Add safeguards for duplicate tenant slug and invalid tenant IDs.

### Deliverables
- Production endpoint(s) for tenant creation/update/status.
- Audit log entries for all provisioning actions.
- Provisioning contract and response/error envelope aligned with current API standards.

### Prompt (for Cursor)
```text
Implement production-grade tenant provisioning on the VPS control-plane API: idempotent tenant create/update, strict validation, duplicate protection, and immutable audit events for each provisioning action. Return typed success/error envelopes and keep route behavior compatible with existing v1 contracts.
```

---

## Sprint 2 — Vendor Console Customer UX (List View + Add Flow)
**Executor:** `Local Agent`

### Scope
- Redesign `/vendor-console/customers` for operator-friendly list-first workflow.
- Provide clear customer list view with sorting/filter/search and row click selection.
- Add prominent **Add customer/company** button and guided creation form/modal.
- Keep all data API-backed (tenant list/create/policy reads), no static fallback rows.
- Add explicit empty/loading/error states with retry and field-level validation.
- Ensure all buttons on customer page are active, wired, and user-clear.

### Deliverables
- Customer page list view + add flow optimized for non-technical operators.
- Add-company action with success feedback and immediate row visibility.
- Tests for list/load/error/add flows and disabled-state behavior.

### Prompt (for Cursor)
```text
Refactor the vendor-console customers page into a user-friendly list view with a clear Add button for company creation, real API-backed loading states, validation, and retry UX. Remove any mock/hardcoded runtime elements and ensure every button on this page has an active, meaningful behavior (no no-op/silent actions).
```

---

## Sprint 3 — Entitlements UX (List View + Detail Window + Actions)
**Executor:** `Local Agent`

### Scope
- Add an entitlement/license list view optimized for key management.
- Add clear **Add/Issue key** button for new key issuance.
- Make each key row clickable to open a detail window (sheet/modal/panel) with:
  - key metadata (tenant, issued at, expiry, channel, slot limit, state)
  - action buttons (edit/update where supported, revoke/delete where policy allows)
- Ensure every list/detail action is wired to real API behavior or explicitly marked unavailable with reason.
- Keep zero hardcoded runtime list items/details.

### Deliverables
- Entitlements list + detail interaction model ready for operators.
- Working actions for key lifecycle operations (issue/revoke/update depending on API support).
- Tests for list/detail/actions/error states, including no inactive button regression.

### Prompt (for Cursor)
```text
Implement a user-friendly entitlements workspace with list view, Add key button, clickable key rows, and per-key detail window containing action buttons (edit/revoke/delete where allowed). Use only real API data, remove hardcoded runtime elements, and enforce that no buttons are silent or inactive without explicit reason.
```

---

## Sprint 4 — Cross-Page UX Consistency + No-Inactive-Buttons Gate
**Executor:** `Local Agent`

### Scope
- Run full UI pass across all vendor-console pages/windows:
  - overview, customers, entitlements, machines, sync, rollouts, health, audit/support/hardening
- Standardize user-friendly components:
  - consistent list/table patterns
  - consistent action bar placement
  - clear empty/error/loading states
  - clear success/failure toasts/messages
- Remove/replace any remaining ambiguous action labels.
- Introduce automated/CI check to detect silent/no-op button regressions where feasible.

### Deliverables
- Consistent UX and interaction patterns across the console.
- No non-functional or misleading interactive controls.
- QA checklist and test evidence confirming all visible buttons work or are intentionally disabled with explanation.

### Prompt (for Cursor)
```text
Perform a full vendor-console UX hardening pass to standardize user-friendly views and interactions across all pages and dialogs, eliminate any silent/no-op buttons, and enforce real data rendering only. Add tests/guards so inactive button regressions are detected before release.
```

---

## Sprint 5 — License/Activation Key Issuance Controls
**Executor:** `Local Agent`

### Scope
- Harden vendor-console license issuance UX for newly provisioned tenants.
- Ensure issuance path supports machine-slot limits, update channel selection, expiry, and operator rationale.
- Add export/copy-safe operator workflow for handing activation key to customer.
- Add test coverage for issue/list/revoke lifecycle for newly created tenants.

### Deliverables
- Clear operator workflow in console for tenant -> issue key -> verify key.
- Guardrails on max machines, invalid inputs, and duplicate operations.
- Integration tests for key issuance and retrieval.

### Prompt (for Cursor)
```text
Implement and harden the vendor-console license issuance workflow so operators can select a newly provisioned tenant, issue activation keys with slot/channel/expiry constraints, and safely retrieve/export keys for customer onboarding. Add validation, failure UX, and tests for issue/list/revoke lifecycle.
```

---

## Sprint 6 — Desktop First-Boot Activation-First Gate
**Executor:** `Local Agent`

### Scope
- Reorder desktop onboarding flow to activation-first on uninitialized devices.
- If no local activation state exists: show ProductLicenseGate before app login.
- Persist tenant-bound activation claim and reconciliation state as source of truth.
- Keep deny/degraded state handling (revoked/expired/slot-limit/force-update-required/api-down).

### Deliverables
- New gate order behavior:
  - First boot (unactivated): activation key first.
  - Activated device: normal auth/login flow.
- No tenant-ID manual input required; tenant resolved from activation key claim.
- Regression tests for first boot, offline degraded mode, and deny transitions.

### Prompt (for Cursor)
```text
Refactor desktop onboarding to activation-first for uninitialized devices: require product license activation before presenting normal login, bind tenant context from activation claim, preserve reconciliation/deny/degraded state machine behavior, and add regression tests for first boot and offline/deny paths.
```

---

## Sprint 7 — First Admin Account / SSO Bootstrap
**Executor:** `VPS Agent`

### Scope
- Add bootstrap path for tenant-first admin identity creation (or first SSO binding).
- Support one-time secure bootstrap token/URL with expiration and audit trail.
- Enforce post-bootstrap hardening (password policy and/or SSO required, bootstrap token invalidated).
- Ensure no cross-tenant bootstrap leakage.

### Deliverables
- Bootstrap endpoint + secure token flow.
- First admin creation or SSO binding process per tenant.
- Evidence logs for bootstrap completed/rejected/expired events.

### Prompt (for Cursor)
```text
Implement a secure first-admin bootstrap flow on the VPS control plane after tenant activation: one-time expiring bootstrap token, tenant-scoped identity creation or SSO binding, immediate token invalidation after use, and immutable audit records for all bootstrap outcomes.
```

---

## Sprint 8 — Normal User Login + Tenant-Scoped Runtime Enforcement
**Executor:** `Local Agent`

### Scope
- Finalize day-2/day-n login flow for activated tenants and normal users.
- Ensure runtime data access remains tenant-scoped by token claims and route guards.
- Add UX messaging for mismatch states (activated tenant vs unauthorized account).
- Confirm role/permission routing in desktop/vendor console remains least-privilege.

### Deliverables
- Stable normal login experience after activation.
- Tenant isolation checks validated end-to-end.
- Operator/user runbook for common onboarding/login failure classes.

### Prompt (for Cursor)
```text
Finalize normal user login flow after tenant activation and enforce strict tenant-scoped runtime access using token claims and permission guards. Add clear UX for tenant/account mismatch cases and validate least-privilege routing across desktop and vendor-console surfaces.
```

---

## Sprint 9 — End-to-End UAT + Cutover
**Executor:** `VPS Agent`

### Scope
- Run full UAT for the new onboarding order:
  - tenant provisioned
  - key issued
  - desktop activation-first
  - first admin bootstrap
  - normal user login
- Capture artifacts (JSON/MD evidence, logs, status snapshots, image digests).
- Publish rollback plan and cutover checklist.

### Deliverables
- UAT pass/fail report with defect mapping.
- Signed artifact bundle for release decision.
- Go/No-Go recommendation for production rollout.

### Prompt (for Cursor)
```text
Execute end-to-end UAT and cutover validation for tenant-first onboarding (provision -> key issuance -> activation-first desktop -> admin bootstrap -> normal login), collect release evidence artifacts, and produce a Go/No-Go recommendation with rollback checklist.
```

---

## Execution Notes

- `Local Agent` sprints target repo code (desktop app, vendor console UI/services, tests, docs).
- `VPS Agent` sprints target deployed infrastructure and control-plane runtime behavior.
- Do not promote rollout until Sprint 9 evidence is complete and reviewed.

## Closeout Status (2026-04-17)

- Sprint 1 -> Sprint 9 deliverables are completed with evidence captured.
- UAT/cutover stream concluded with RC decision `GO`.
- Final cross-document artifact pointers are maintained in:
  - `94-uat-compliance-and-rc-signoff-pack.md`
  - `99-phase4-final-artifact-index.md`

