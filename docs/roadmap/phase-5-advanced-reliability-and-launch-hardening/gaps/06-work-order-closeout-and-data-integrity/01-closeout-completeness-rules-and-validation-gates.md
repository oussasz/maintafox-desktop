# Sprint 1 — Closeout Completeness Rules And Validation Gates

**PRD:** §6.5 (close-out evidence), §6.10 (governed failure data), ISO 14224 data categories (failure + maintenance + downtime)

**Objective:** Enforce **progressive mandatory fields** at WO closure so operational evidence satisfies ISO 14224-style **failure data** (failure mode, cause, mechanism/cause chain, consequence) and **maintenance data** (actions, resources, downtime) before a corrective WO is **Closed**.

---

## Standards & authoritative references

| Source | Relevance |
|--------|-----------|
| **ISO 14224:2016** — *Petroleum, petrochemical and natural gas industries — Collection and exchange of reliability and maintenance data for equipment* ([ISO 14224:2016](https://www.iso.org/standard/64076.html)) | Clause 9 / data categories: **(b) failure data** — failure cause, failure consequence; **(c) maintenance data** — maintenance action, resources, **down time**. Normative definitions: **failure mode** (manner of failure, §3.30), **failure cause** / root cause (§3.24), **failure mechanism** (process leading to failure, §3.29; linked to Table B.2 in the standard’s thesaurus). Maintafox maps **mode/cause/effect** to ISO **mode/cause/consequence** for analytics. |
| **IEC 60050-192** | ISO 14224 explicitly references IEC vocabulary for failure and maintenance terms; align field semantics with IEC 60050-192 where Maintafox labels match. |
| **Internal** | `docs/research/MODULES_6_4_6_5_REQUEST_TO_WORK_ORDER_LIFECYCLE.md` (§5.4–5.5 progressive mandatory fields); `MODULE_6_10_RELIABILITY_ENGINE.md` (governed failure events). |

---

## Schema definition (SQLite) — tables & critical columns

Implementation may extend existing migrations (`work_orders`, `wo_failure_details`, `work_order_downtime_segments`, etc.). Minimum **logical** contract:

### `work_orders` (existing — columns enforced at close)

| Column | Role |
|--------|------|
| `id`, `row_version`, `status`, `maintenance_type_code` (or equivalent) | State + classification for conditional rules |
| `closed_at`, `actual_start`, `actual_end` | Time basis for MTTR segments |
| `asset_id` / equipment link | ISO 14224 equipment boundary |

### `wo_failure_details` (existing pattern — PRD §6.5 / close-out)

| Column | Type | ISO 14224 mapping |
|--------|------|-------------------|
| `work_order_id` | FK | — |
| `symptom_id` | FK → `reference_values` | Observed condition (supporting) |
| `failure_mode_id` | FK → `reference_values` | **Failure mode** (§3.30) |
| `failure_cause_id` | FK | **Failure cause** (§3.24) |
| `failure_effect_id` | FK | **Failure consequence** (failure data category (b)) |
| `cause_not_determined` | BOOLEAN | When true, **failure_mode_id** may be mandatory but cause may be waived per policy |
| `is_temporary_repair`, `is_permanent_repair` | BOOLEAN | Maintenance action characterization |

### `work_order_downtime_segments` (existing)

| Column | Role |
|--------|------|
| `work_order_id`, `equipment_id`, `started_at`, `ended_at`, `segment_type` (planned/unplanned) | **Down time** for ISO 14224 maintenance data category (c); drives MTTR numerator splits |

### New: `closeout_validation_policies` (tenant-scoped)

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `tenant_id` / org scope | FK | As per existing multi-tenant pattern |
| `policy_name` | TEXT | e.g. `default_corrective` |
| `applies_when` | JSON | e.g. `{"maintenance_type":["CM"],"criticality_min":3}` |
| `require_failure_mode_if_unplanned` | BOOLEAN | |
| `require_downtime_if_production_impact` | BOOLEAN | |
| `allow_close_with_cause_not_determined` | BOOLEAN | If true, require documented reason text |
| `row_version` | INTEGER | Sync |

---

## Business logic & validation rules

1. **Corrective / breakdown WO (configurable “failure-related” class):** Cannot transition to **Closed** if:
   - `cause_not_determined = false` **and** `failure_mode_id IS NULL` (ISO **failure mode** is mandatory for reliability counting).
   - `cause_not_determined = false` **and** `failure_cause_id IS NULL` **unless** policy explicitly allows “mode-only” close for a pilot class (document in policy JSON).

2. **If `cause_not_determined = true`:** Require `notes` (or `failure_investigation_summary`) non-empty with minimum length — satisfies traceability while excluding event from strict MTBF numerator if policy says so (feeds Gaps `05` eligibility).

3. **Downtime:** If `maintenance_type` indicates production asset impact **or** `failure_effect` implies production impact level ≥ threshold, require ≥1 `work_order_downtime_segments` row **or** explicit “no downtime” attestation flag with reason (auditable).

4. **Verification:** Cannot close if PRD verification step requires `wo_verifications` row and `return_to_service_confirmed` is false for regulated types.

5. **ISO terminology consistency:** UI labels must distinguish **failure mode** vs **failure cause** vs **failure mechanism** (if `failure_mechanism_id` is added later per ISO §3.29, map to optional FK or subtype under cause taxonomy).

6. **Authoritative validation:** All gates implemented in **Rust** on `close_work_order` / state transition; React only mirrors messages.

---

## Mathematical triggers

Not applicable to this sprint (no KPI computation). Outputs feed **eligibility flags** consumed by Gaps `05` (`failure_events.eligible_for_mtbf`, etc.).

---

## Sync transport specification (`protocol_version: "v1"`)

Outbox rows use `StageOutboxItemInput` (`src-tauri/src/sync/domain.rs`). For this sprint, mutations primarily touch **`work_orders`** (already `entity_type: "work_orders"` in tests).

### `entity_type`: `work_orders`

**`payload_json`** (canonical keys — extend existing mirror contract; must be a strict superset of what VPS already applies):

```json
{
  "id": 0,
  "entity_sync_id": "uuid",
  "row_version": 0,
  "status": "closed",
  "maintenance_type_code": "CM",
  "asset_id": 0,
  "closed_at": "ISO-8601",
  "closeout_validation_profile_id": 0,
  "closeout_validation_passed": true
}
```

### New `entity_type` (if `closeout_validation_policies` introduced): `closeout_validation_policies`

```json
{
  "id": 0,
  "entity_sync_id": "uuid",
  "row_version": 0,
  "policy_name": "default_corrective",
  "applies_when": {},
  "require_failure_mode_if_unplanned": true,
  "require_downtime_if_production_impact": true,
  "allow_close_with_cause_not_determined": false
}
```

**`payload_hash`:** SHA-256 of canonical JSON string (existing sync contract).

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 06-work-order-closeout-and-data-integrity — Sprint 01 — Closeout Completeness Rules And Validation Gates

**Read Only:**
- @docs/PRD.md §6.5
- @src-tauri/src/
- @src-tauri/src/sync/domain.rs
- @src/pages/WorkOrdersPage.tsx

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
