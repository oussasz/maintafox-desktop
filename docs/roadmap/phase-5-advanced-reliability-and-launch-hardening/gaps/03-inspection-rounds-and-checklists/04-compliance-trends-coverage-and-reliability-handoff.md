# Sprint 4 — Compliance Trends, Coverage, And Reliability Handoff

**PRD:** §6.25, §6.10 (inputs)

**Objective:** Reporting views for **round completion %**, **missed rounds**, **anomaly rate**, and **handoff joins** to `failure_events` / KPI layer (read-only feeds to Gaps `05`).

---

## Standards & authoritative references

| Source | Relevance |
|--------|-----------|
| **ISO 14224:2016** | **Plant production availability** and **equipment availability** use cases (Clause 9 introduction) — inspection compliance is a **leading** indicator of unmanaged **failure mechanisms**. |
| **Internal** | `MODULE_6_10_RELIABILITY_ENGINE.md` §12 integration with inspections. |

---

## Schema definition (read models)

### Optional materialized: `inspection_reliability_signals` (per asset / period)

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `equipment_id` | INTEGER | |
| `period_start`, `period_end` | TEXT | |
| `warning_count` | INTEGER | From `result_status=warning` |
| `fail_count` | INTEGER | |
| `anomaly_open_count` | INTEGER | |
| `checkpoint_coverage_ratio` | REAL | Completed / scheduled |
| `row_version` | INTEGER | If synced |

---

## Business logic & validation rules

1. **Coverage:** `coverage = completed_rounds / scheduled_rounds` for sliding window.

2. **Handoff to RAMS:** Export view or RPC returning `(equipment_id, period, fail_rate, warning_rate)` for **join** in `reliability_kpi_snapshots` companion fields (see Gaps `05-03`).

---

## Mathematical triggers

- **Anomaly density:** `anomaly_density = anomaly_count / asset_hours` if asset hours from `runtime_exposure_logs`.

---

## Sync transport specification

| `entity_type` | Notes |
|---------------|--------|
| `inspection_reliability_signals` | If persisted as table, full payload JSON per row per period |

Alternatively **compute on read** only — **no sync** for derived aggregates if VPS recomputes from mirrored inspection tables (document choice).

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 03-inspection-rounds-and-checklists — Sprint 04 — Compliance Trends, Coverage, And Reliability Handoff

**Read Only:**
- @docs/PRD.md §6.25
- @src-tauri/src/migrations/
- @src-tauri/src/sync/domain.rs
- @src/pages/InspectionsPage.tsx

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
