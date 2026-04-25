# Phase 4 Observability, SLO, And Runbooks Pack

**Owner:** Cursor Agent (artifacts) + VPS Agent (deployment/evidence)  
**Related sprint:** Sprint 9

---

## 1) SLO Definitions (v1)

- **API availability SLO:** `>= 99.9%` monthly for `GET /health` and auth/licensing endpoints.
- **Auth latency SLO:** `p95 < 300ms`, `p99 < 700ms` for `/api/v1/auth/login`.
- **Activation claim success SLO:** `>= 99.5%` successful claim responses excluding explicit policy denials.
- **Sync operator freshness SLO:** `>= 99%` of tenant telemetry rows updated within `<= 5m`.

### Error budgets

- Availability budget at 99.9%: `~43m 49s` downtime / month.
- Activation success budget at 99.5%: `0.5%` failed non-policy claim attempts / month.

---

## 2) Required Metrics And Logs

- **API metrics**
  - request count/rate by route/status
  - request duration histogram (p50/p95/p99)
  - 4xx/5xx ratio
  - privileged action count and guard failures
- **Licensing metrics**
  - activation claim success/fail by reason
  - deny-state counts (`revoked`, `expired`, `slot_limit`, `force_update_required`, `invalid`)
  - reconciliation retry backlog
- **Sync/rollout/platform metrics**
  - checkpoint lag max/p95
  - dead-letter queue totals
  - rollout cohort mismatch counts
  - platform alert severity totals
- **Structured logs (JSON)**
  - correlation id
  - tenant id (when available)
  - action code / audit record id for privileged actions
  - error code taxonomy

### Implemented observability surfaces

- **API structured logs**
  - `api_request_complete`
  - `auth_login_success` / `auth_login_failed`
  - `activation_claim_success` / `activation_claim_failed`
- **API SLO metric endpoint**
  - `GET /api/v1/ops/observability/slo`
  - includes API availability/error-rate, auth p50/p95/p99, activation success/failure by reason, sync stale ratio, and alert states.
- **Desktop structured logs**
  - `desktop_activation_submit`
  - `desktop_activation_reconciliation`
  - `desktop_sync_stage_outbox`
  - `desktop_sync_apply_batch`
  - `desktop_sync_replay_failures`
  - `desktop_sync_execute_repair`
  - `desktop_sync_observability_report`
- **Edge log contract**
  - retain JSON or key-value access log fields for `host`, `status`, `request_time`, `upstream_response_time`, `request_id`.
  - enforce correlation propagation via `X-Request-Id`.

---

## 3) Alert Thresholds

- **Critical**
  - API availability burn-rate > 2x for 10m
  - activation claim failure ratio > 5% for 10m (excluding explicit policy denies)
  - sync lag max > 15m for any production tenant
- **Warning**
  - auth p95 > 300ms for 15m
  - DLQ size growth > 20% in 30m
  - privileged guard failures > baseline + 3 sigma

### Dashboard panels (minimum set)

- **Control-plane SLO board**
  - API availability %
  - API 5xx rate %
  - auth latency p95/p99
  - activation success rate %
  - activation failures by reason
  - sync stale ratio %
- **Ops diagnostics board**
  - dead-letter total
  - repair queue volume
  - privileged action guard failures
  - force-update denied activations

---

## 4) Runbook Set (Operator-ready)

- **RB-CP-01:** Activation outage / API unreachable
  - verify API health and DNS/TLS route
  - inspect reconciliation retry spikes
  - declare degraded mode and notify support
- **RB-CP-02:** Force-update policy incident
  - verify tenant/cohort policy source
  - apply emergency override (with reason + step-up)
  - rollback to last known-good policy if blast radius expands
- **RB-CP-03:** Audit guard failure surge
  - inspect missing reason/step-up headers
  - validate operator workflow and token issuance path
  - capture audit trail for postmortem
- **RB-CP-04:** Sync backlog surge
  - inspect repair queue and DLQ
  - pause risky rollout interventions
  - recover from oldest-first batches with evidence capture

### On-call escalation matrix

- **L1 (Support Operator, 24/7)**
  - owns first response, incident declaration, ticket hygiene.
  - SLA: acknowledge <= 10 min.
- **L2 (Platform Engineer)**
  - owns API/edge recovery, rollout/sync interventions, policy rollback.
  - SLA: engage <= 20 min after L1 escalation.
- **L3 (Security + Staff Engineer)**
  - owns credential compromise, audit-chain integrity, emergency change approvals.
  - SLA: engage <= 30 min for critical security/sev1.
- **Comms owner (Product/Ops)**
  - customer comms cadence and ETA updates every 30 min during sev1.

---

## 5) Evidence Checklist (Sprint 9 Exit)

- dashboard screenshots or exported panel data for all SLOs
- alert test evidence (trigger + resolve)
- one completed incident drill with timeline
- postmortem template filled with owner/action items

