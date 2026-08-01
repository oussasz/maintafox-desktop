# Analytics close-out contract v1.0.0

**Contract id:** `closeout_to_reliability_v1`  
**Version:** 1.0.0  
**Purpose:** Freeze the mapping from work-order close-out data to reliability analytics and ISO-style reporting so Gaps `05` / RAMS cannot drift silently.

---

## 1. WO → `failure_events` eligibility matrix

| Condition | Required Maintafox state | Eligible for reliability failure_event ingestion |
|-----------|----------------------------|--------------------------------------------------|
| Unplanned corrective / emergency work | `work_orders` linked to `work_order_types` with `corrective` or `emergency` | Baseline |
| Failure taxonomy present | `work_order_failure_details.failure_mode_id` → `failure_codes` (`code_type = mode`), active | Yes for MTBF-oriented views |
| Cause known | `cause_not_determined = 0` and `failure_cause_id` populated unless policy `allow_close_with_cause_mode_only` | Yes for full ISO 14224 coding |
| Close-out validated | `work_orders.closeout_validation_passed = 1` at closure | Gate for post-close ingestion |

**Formula (normative summary):**

`eligible = (unplanned_failure) AND (failure_mode_id NOT NULL) AND (NOT excluded_by_policy)`

Where `excluded_by_policy` includes: cancelled WO, corrective without required coding per `closeout_validation_policies`, or explicit data-quality dismissal per RAM views.

---

## 2. ISO 14224 field mapping (Maintafox → concepts)

| ISO 14224 concept | Maintafox source |
|-------------------|------------------|
| Failure mode | `failure_codes` row referenced by `work_order_failure_details.failure_mode_id` (`code_type = mode`) |
| Failure cause / mechanism | `failure_codes` via `failure_cause_id` (`code_type` in `cause`, `mechanism`) |
| Failure effect / consequence | `failure_codes` via `failure_effect_id` (`code_type = effect`) |
| Symptom | `reference_values` / taxonomy via `symptom_id` where applicable |
| Downtime duration | Sum of `(ended_at - started_at)` from `work_order_downtime_segments` in hours; must reconcile with `actual_start` / `actual_end` window per integrity rules |

---

## 3. Change control

Any PR that changes close-out validation rules, failure ingestion, or this matrix must:

1. Bump `version_semver` or update `content_sha256` in `analytics_contract_versions`.
2. Add or update regression tests under `wo::gap06_regression_tests` (or successor module).
