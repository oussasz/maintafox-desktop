# Sprint 2 — Data Integrity Detectors, Repair Jobs, And Operator Workbench

**PRD:** §8 recovery, §6.5 evidence consistency

**Objective:** Detect **orphan FKs**, **impossible timestamps** (end < start), **negative durations**, **unclosed segments**, and **reference_values** pointing to deactivated codes; provide **privileged repair** with audit trail.

---

## Standards & authoritative references

| Source | Relevance |
|--------|-----------|
| **ISO 14224:2016** ([ISO 14224](https://www.iso.org/standard/64076.html)) | **Data quality control and assurance** (standard scope): integrity checks ensure failure and downtime data remain exchangeable and analysis-grade. |
| **Internal** | `MASTER_RESEARCH_FRAMEWORK.md` — Layer 3 analytical derivation data must not be silently corrupted. |

---

## Schema definition

### New: `data_integrity_findings`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `severity` | TEXT | `error` / `warning` |
| `domain` | TEXT | `wo_closeout`, `downtime`, `failure_coding` |
| `record_class` | TEXT | `work_orders`, `wo_failure_details`, … |
| `record_id` | INTEGER | Local PK |
| `entity_sync_id` | TEXT UUID | When row has stable sync id |
| `finding_code` | TEXT | e.g. `WO_DOWNTIME_NEGATIVE_DURATION`, `FK_ORPHAN_FAILURE_MODE` |
| `details_json` | TEXT | Structured evidence |
| `detected_at` | TEXT ISO-8601 | |
| `cleared_at` | TEXT NULL | |
| `status` | TEXT | `open`, `waived`, `repaired` |

### New: `data_integrity_repair_actions` (audit)

| Column | Type |
|--------|------|
| `id`, `finding_id`, `action` | INTEGER / TEXT |
| `actor_id`, `performed_at` | |
| `before_json`, `after_json` | TEXT |
| `sync_batch_id` | TEXT NULL | Link to sync replay if applicable |

---

## Business logic & validation rules

1. **Detector jobs** (on-demand + post-migration):
   - `wo_failure_details.failure_mode_id` ∈ active `reference_values` for domain `failure_mode`.
   - Σ `downtime_segment` durations ≤ `(actual_end - actual_start)` + tolerance policy.
   - No overlapping downtime segments for same equipment unless `segment_type` allows.

2. **Repair actions:** Only users with `sync.manage` + integrity role (define `integrity.repair` permission) may apply repairs; every repair writes `data_integrity_repair_actions` **before** commit.

3. **Waiver:** `waived` requires reason text + second-person approval if severity `error` (configurable).

---

## Mathematical triggers

Compute **derived duration** for display:  
`duration_hours = (ended_at - started_at)` in hours (DECIMAL), compare to sum of segment rows — flag if `|sum - derived| > ε`.

---

## Sync transport specification

### `entity_type`: `data_integrity_findings`

```json
{
  "id": 0,
  "entity_sync_id": "uuid",
  "row_version": 0,
  "severity": "error",
  "domain": "wo_closeout",
  "record_class": "work_orders",
  "record_id": 0,
  "finding_code": "WO_DOWNTIME_NEGATIVE_DURATION",
  "details_json": "{}",
  "status": "open"
}
```

### `entity_type`: `data_integrity_repair_actions`

```json
{
  "id": 0,
  "entity_sync_id": "uuid",
  "row_version": 0,
  "finding_id": 0,
  "action": "adjust_downtime_segment",
  "actor_id": 0,
  "before_json": "{}",
  "after_json": "{}"
}
```

Repairs that mutate `work_orders` must **also** emit `work_orders` outbox row with bumped `row_version`.

**`[ATTENTION: REQUIRES VPS AGENT INTERVENTION]`** — mirror tables for findings/repairs; optional: server-side re-run of detectors for fleet dashboards.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 06-work-order-closeout-and-data-integrity — Sprint 02 — Data Integrity Detectors, Repair Jobs, And Operator Workbench

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
