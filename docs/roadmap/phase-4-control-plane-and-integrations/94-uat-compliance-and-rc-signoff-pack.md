# Phase 4 UAT, Compliance, And Release Candidate Sign-off Pack

**Owner:** Joint (Cursor Agent + VPS Agent)  
**Related sprint:** Sprint 10

---

## 1) UAT Scope Matrix

- **Auth/session**
  - login success/failure taxonomy
  - token expiry/re-auth path
- **Tenant/license lifecycle**
  - create tenant
  - issue license
  - revoke license
- **Machine activation and reconciliation**
  - first claim
  - offline continuity
  - scheduled revalidation
  - deny state transitions
- **Force-update governance**
  - cohort policy enforcement
  - tenant emergency override
  - rollback behavior
- **Ops telemetry**
  - sync overview
  - rollout overview
  - platform health
- **Audit/support**
  - audit ledger query/export
  - support intervention linkage
  - diagnostic bundle visibility

### Real-infra UAT runner (operator command)

Use the control API automation runner on the deployed environment:

```bash
BASE_URL=https://api.maintafox.systems \
ADMIN_EMAIL="<admin-email>" \
ADMIN_PASSWORD="<admin-password>" \
OPERATOR_REASON="RC-UAT-S10" \
STEP_UP_TOKEN="<step-up-token>" \
OUTPUT_DIR="/root/sprint10-uat-$(date -u +%Y%m%dT%H%M%SZ)" \
node scripts/sprint10-uat-real-infra.mjs
```

Expected artifacts:
- `uat-results.json`
- `uat-results.md`

---

## 2) Compliance Evidence Pack Checklist

- API contract freeze reference (`91-control-plane-v1-contract-freeze.md`)
- placeholder inventory reference + closure snapshot (`92-placeholders-and-hardcodes-inventory.md`)
- privileged-action audit samples (set policy, rollback, revoke)
- integration test evidence:
  - force-update governance API scenario
  - desktop updater enforcement tests
- security posture evidence:
  - HTTPS on console/api (from Sprint 8)
  - secret rotation log
  - firewall/fail2ban posture
- final artifact pointer index (`99-phase4-final-artifact-index.md`)

---

## 3) Release Candidate Go/No-Go Checklist

- [x] Sprint 1-7 code acceptance satisfied and verified
- [x] Sprint 8 infrastructure baseline verified in production
- [x] Sprint 9 observability/SLO alerts active and tested
- [x] no unresolved high-severity lint/type/test failures
- [x] no known placeholder/mock imports in production paths
- [x] rollback plan validated for:
  - [x] force-update policies
  - [x] API deployment
  - [x] desktop updater package
- [x] sign-off record captured in runtime evidence bundle:
  - [x] Product
  - [x] Engineering
  - [x] Security
  - [x] Support/Operations

---

## 4) Sign-off Record

- **RC version:** `phase4-rc-2026-04-17`
- **Date:** `2026-04-17`
- **Product sign-off:** `Recorded in VPS closeout artifact`
- **Engineering sign-off:** `Recorded in VPS closeout artifact`
- **Security sign-off:** `Recorded in VPS closeout artifact`
- **Support/Operations sign-off:** `Recorded in VPS closeout artifact`
- **Decision:** `GO`
- **Notes / required follow-up:** Runtime evidence package: `/root/sprint10-rc-closeout-20260417T130406Z.tar.gz`

---

## 5) Current RC Posture

- **Technical status:** `GO`
- **Promotion posture:** `APPROVED_FOR_RELEASE_PROMOTION`
- **Primary runtime evidence:**
  - `/root/sprint10-uat-20260417T130406Z-postfix/uat-results.json`
  - `/root/sprint10-uat-20260417T130406Z-postfix/uat-results.md`
  - `/root/phase4-rc/94-uat-compliance-and-rc-signoff-pack.md`
  - `/root/sprint10-route-verification-20260417T000000Z.txt`
  - `/root/logs/frontend-hardening-20260417T145601Z.tar.gz` (`sha256: a6a9ef2aa047e6c5d5ab5f1690f627d658c172261c9db51970d8d1a184b96e2b`)

