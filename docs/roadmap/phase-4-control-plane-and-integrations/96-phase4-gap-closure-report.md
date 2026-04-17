# Phase 4 Gap Closure Report (RC)

**Date:** 2026-04-17  
**Scope:** Control-plane production paths (API, vendor console, desktop enforcement)  
**Objective:** prove zero production placeholder/mock/hardcoded workflow behavior in critical paths before RC promotion.

---

## 1) Evidence sources

- Contract freeze: `91-control-plane-v1-contract-freeze.md`
- Placeholder inventory: `92-placeholders-and-hardcodes-inventory.md`
- Sprint 8 VPS hardening evidence (TLS + host controls + secret rotation)
- Sprint 9 observability evidence (SLO endpoint + structured logs + drill record)
- Sprint 10 UAT + RC closeout bundle:
  - `/root/sprint10-uat-20260417T130406Z-postfix/uat-results.json`
  - `/root/sprint10-uat-20260417T130406Z-postfix/uat-results.md`
  - `/root/sprint10-rc-closeout-20260417T130406Z.tar.gz`
- CI guard:
  - `pnpm run guard:phase4:fixtures`
  - implemented by `scripts/check-control-plane-fixture-imports.ts`

---

## 2) Production placeholder/mock closure status

- **Vendor ops dashboards** (`sync`, `rollout`, `platform`) use live API telemetry.  
- **Audit/support surfaces** use API-backed immutable/exportable flows.  
- **Customer/license/machine workflows** are API-backed with typed validation.  
- **Desktop activation state machine** enforces online/offline reconciliation and explicit deny states.  
- **Force-update governance** is policy-driven end-to-end (tenant/cohort -> claim response -> desktop gate).

### Dev-only bypass hardening

- `VITE_DEV_MOCK_AUTH` paths are now constrained to **DEV mode only**:
  - `maintafox-vendor-console/src/components/auth/AuthGuard.tsx`
  - `maintafox-vendor-console/src/services/admin-permissions.ts`
  - `maintafox-vendor-console/src/pages/LoginPage.tsx`

This prevents accidental production bypass activation.

---

## 3) Hardcoded workflow data verification

- API seed behavior is production-safe (`SEED_BEHAVIOR` defaults to `disabled` in production).
- Privileged actions require auditable headers in hardened modes (reason + step-up).
- Remaining constants are operational defaults (non-placeholder), not mock workflow substitutions.

---

## 4) Automated checks run

- Desktop repo:
  - `pnpm run guard:phase4:fixtures` -> pass
  - `pnpm exec tsc --noEmit` -> pass
- Control API repo:
  - `pnpm run build` -> pass
  - `pnpm run test:force-update` -> pass
  - `pnpm run test:observability` -> pass

---

## 5) Conclusion

Control-plane code paths are closed against placeholder/mock workflow behavior for Phase 4 critical scope.  
Release-candidate promotion is approved with joint runtime evidence, completed UAT (`15/15`), and RC closeout package captured.

### As-deployed runtime identifiers

- API image digest: `sha256:49d57a8b35d146c3237f548fc1918d988edb8fced1845f4b0c57dd0b53fc3ce0`
- Edge image digest: `sha256:65645c7bb6a0661892a8b03b89d0743208a18dd2f3f17a54ef4b76fb8e2f2a10`
- API source SHA (`api/src/index.ts`): `a08fe6a9f5174fa37c35624f30bbe739513efd62`

