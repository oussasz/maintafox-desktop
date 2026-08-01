# Sprint 2 — Governed Failure Events: Eligibility Rules And WO Ingestion

**PRD:** §6.10.1 `failure_events`

**Objective:** Generate **`failure_events`** from closed WOs using **ISO 14224 failure data** semantics (mode, cause, mechanism, consequence, downtime) and **explicit eligibility** for MTBF/MTTR numerators.

---

## Standards & authoritative references

| ISO 14224 category | Maintafox column |
|--------------------|------------------|
| Failure data — **time of failure** / detection | `detected_at`, `failed_at` |
| **Failure mode** | `failure_mode_id` → `failure_codes.code_type=mode` |
| **Failure cause** | `failure_cause_id` |
| **Failure mechanism** (optional) | `failure_mechanism_id` OR child under cause |
| **Consequence** | `failure_effect_id`, `production_impact_level`, `safety_impact_level` |
| Maintenance data — **down time** | `downtime_duration_hours`, segments from WO |

Source: [ISO 14224:2016](https://www.iso.org/standard/64076.html) Clauses 3, 9.

---

## Schema definition

### `failure_events`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `entity_sync_id` | TEXT UUID | |
| `source_type` | TEXT | `work_order` / `inspection` / … |
| `source_id` | INTEGER | WO id |
| `equipment_id` | INTEGER | ISO equipment boundary |
| `component_id` | INTEGER NULL | |
| `detected_at`, `failed_at`, `restored_at` | TEXT | |
| `downtime_duration_hours` | REAL | Sum of **unplanned** downtime segments or attested total |
| `active_repair_hours` | REAL | From labor segments |
| `waiting_hours` | REAL | Delay segments |
| `is_planned` | INTEGER | **Exclude from MTBF** if planned |
| `failure_class_id`, `failure_mode_id`, `failure_cause_id`, `failure_effect_id` | INTEGER FK | **Mode mandatory** for `eligible_unplanned` |
| `failure_mechanism_id` | INTEGER NULL | ISO §3.29 |
| `cause_not_determined` | INTEGER | If 1 → `eligible_for_strict_mtbf=0` |
| `eligible_flags_json` | TEXT | Machine-readable eligibility |
| `row_version` | INTEGER | |

---

## Business logic & validation rules

1. **Insert trigger:** On WO → `Closed`, if `maintenance_type` ∈ corrective set:
   - Insert `failure_events` row **idempotent** on `(source_type, source_id)` UNIQUE.

2. **Eligibility:**
   - `eligible_unplanned_mtbf = NOT is_planned AND failure_mode_id IS NOT NULL AND NOT cause_not_determined` **OR** policy B allows cause_not_determined with reduced weight.

3. **Inspection linkage (later):** `source_type=inspection` when anomaly confirms **functional failure** — separate sprint.

---

## Mathematical triggers

N/A (generation only). Durations:

- `downtime_duration_hours = SUM(ended_at - started_at)` for `work_order_downtime_segments` where `segment_type='unplanned'` (or tenant default).

---

## Sync transport specification

| `entity_type` | `payload_json` |
|---------------|----------------|
| `failure_events` | Full row including `eligible_flags_json`, ISO-linked FK ids, `entity_sync_id`, `row_version` |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 05-reliability-data-foundation-iso-14224 — Sprint 02 — Governed Failure Events: Eligibility Rules And WO Ingestion

**Read Only:**
- @docs/PRD.md §6.10.1
- @docs/research/MODULE_6_10_RELIABILITY_ENGINE.md
- @src-tauri/src/migrations/
- @src-tauri/src/sync/domain.rs

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
