# Control-Plane Placeholder / Mock / Hardcode Inventory

**Status:** Active inventory (Sprint 1 deliverable)  
**Last updated:** 2026-04-17  
**Scope:** Phase 4 control-plane surfaces (vendor console, control API, desktop activation handshake).

---

## 1) Classification

- **MOCK_FIXTURE:** production component imports fixture/mock datasets.
- **DEV_BYPASS:** runtime bypass intended for local dev/testing.
- **HARDCODED_DEFAULT:** static value in production path that should come from API/config/policy.
- **TRANSITIONAL_FALLBACK:** temporary fallback accepted now but must be closed by later sprint.

---

## 2) Inventory With Owner And Removal Sprint

| ID | Path | Type | Current Behavior | Owner | Removal / Closure Sprint |
|---|---|---|---|---|---|
| PH4-001 | `maintafox-vendor-console/src/vendor-console/components/SyncHealthPanel.tsx` | MOCK_FIXTURE | Uses `MOCK_SYNC_*` and repair queue fixtures for operational health. | Cursor Agent | Sprint 3 |
| PH4-002 | `maintafox-vendor-console/src/vendor-console/components/RolloutOpsPanel.tsx` | MOCK_FIXTURE | Uses `MOCK_ROLLOUT_*` for rollout staging and diagnostics. | Cursor Agent | Sprint 3 |
| PH4-003 | `maintafox-vendor-console/src/vendor-console/components/PlatformHealthOpsPanel.tsx` | MOCK_FIXTURE | Uses `MOCK_PLATFORM_SERVICES`, pressure, alerts fixtures. | Cursor Agent | Sprint 3 |
| PH4-004 | `maintafox-vendor-console/src/vendor-console/components/VendorConsoleOpsHubCard.tsx` | MOCK_FIXTURE | Aggregate cards derive from fixture constants, not live telemetry. | Cursor Agent | Sprint 3 |
| PH4-005 | `maintafox-vendor-console/src/vendor-console/components/SupportOperationsPanel.tsx` | MOCK_FIXTURE | Support tickets and diagnostic bundle lists are fixture-backed. | Cursor Agent | Sprint 4 |
| PH4-006 | `maintafox-vendor-console/src/vendor-console/components/AuditLedgerPanel.tsx` | MOCK_FIXTURE | Ledger and chain verification are fixture-backed in production UI. | Cursor Agent | Sprint 4 |
| PH4-007 | `maintafox-vendor-console/src/vendor-console/components/ConsoleHardeningPanel.tsx` | MOCK_FIXTURE | Runbook/readiness data uses fixtures. | Cursor Agent | Sprint 4 |
| PH4-008 | `maintafox-vendor-console/src/vendor-console/components/EntitlementLifecyclePanel.tsx` | MOCK_FIXTURE | Signed claim preview still references `MOCK_SIGNED_CLAIM_PREVIEW`. | Cursor Agent | Sprint 2 |
| PH4-009 | `maintafox-vendor-console/src/vendor-console/components/MachineActivationPanel.tsx` | MOCK_FIXTURE | Machine table rows still include fixture source (`MOCK_MACHINE_ROWS`). | Cursor Agent | Sprint 2 |
| PH4-010 | `maintafox-vendor-console/src/vendor-console/components/CustomerWorkspace.tsx` | HARDCODED_DEFAULT | Metadata block still contains read-only demo defaults (`demo-org`, `stable-main`). | Cursor Agent | Sprint 2 |
| PH4-011 | `maintafox-vendor-console/src/services/admin-permissions.ts` | DEV_BYPASS | `VITE_DEV_MOCK_AUTH=true` returns `MOCK_VENDOR_PERMISSIONS`. | Cursor Agent | Sprint 5 (restrict to dev build only) |
| PH4-012 | `maintafox-vendor-console/src/components/auth/AuthGuard.tsx` | DEV_BYPASS | Mock-auth bypass allows route access in dev mode. | Cursor Agent | Sprint 5 (policy-gated) |
| PH4-013 | `maintafox-vendor-console/src/pages/LoginPage.tsx` | DEV_BYPASS | Dev-mode “Continue” path bypasses real login. | Cursor Agent | Sprint 5 (non-prod only) |
| PH4-014 | `maintafox-vendor-console/api/src/index.ts` | HARDCODED_DEFAULT | Auto-seeds default tenant (`slug: default`) when empty DB. | Cursor Agent | Sprint 5 (bootstrap/admin init flow) |
| PH4-015 | `maintafox-vendor-console/api/src/index.ts` | HARDCODED_DEFAULT | License key generation format fixed in process (`MFX-...`) with no policy config. | Cursor Agent | Sprint 5 |
| PH4-016 | `maintafox-vendor-console/api/src/index.ts` | HARDCODED_DEFAULT | `CORS_ORIGIN` default is fixed to console URL if env missing. | Cursor Agent | Sprint 5 |
| PH4-017 | `maintafox-desktop/src/components/auth/ProductLicenseGate.tsx` | HARDCODED_DEFAULT | Activation claim uses static `app_version: "0.1.0-dev"` and `machine_label: "desktop-client"`. | Cursor Agent | Sprint 6 |
| PH4-018 | `maintafox-desktop/src/components/auth/ProductLicenseGate.tsx` + `src/services/product-license-service.ts` | TRANSITIONAL_FALLBACK | Local save continues if activation API unreachable (`pending_online_validation`). | Cursor Agent | Sprint 6 (close with scheduled reconciliation + deny-state enforcement) |
| PH4-019 | VPS runtime (`http://console...`, `http://api...`) | HARDCODED_DEFAULT | Production still allowed over HTTP; TLS not mandatory yet. | VPS Agent | Sprint 8 |
| PH4-020 | VPS host security controls | TRANSITIONAL_FALLBACK | Temporary fail2ban stop/disable events during bring-up must be normalized. | VPS Agent | Sprint 8 |

---

## 3) Non-Goals For This Inventory

- Placeholder pages in non-control-plane modules (`src/pages/placeholder/*`) are out of Phase 4 scope and tracked in other phase plans.
- Test-only mocks under `__tests__` are intentionally excluded.

---

## 4) Mandatory Closure Evidence Per Item

Each item is considered closed only when all are true:
- production code no longer imports fixture/mock data for that behavior;
- contract tests and UI tests cover the real path;
- deployment runbook updated where behavior changed;
- item status updated in this file with commit reference.

---

## 5) Status Tracking Convention

Append one of:
- `OPEN` (default)
- `IN_PROGRESS`
- `CLOSED (commit: <sha>)`

---

## 6) Closure Snapshot (2026-04-17)

- **CLOSED (pending commit reference update):** PH4-001 -> PH4-010, PH4-014 -> PH4-018.
- **OPEN / verify in vendor-console branch history:** PH4-011, PH4-012, PH4-013.
- **IN_PROGRESS (VPS Agent / Sprint 8):** PH4-019, PH4-020.

### Automated guard now active

- CI now runs `pnpm run guard:phase4:fixtures` from `.github/workflows/ci.yml`.
- The guard fails CI when production files under `src/` import fixture/mock modules intended for test/dev paths.

