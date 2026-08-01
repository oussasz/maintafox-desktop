# Sprint 2 — Field Execution: Results, Evidence, And Offline Queue

**PRD:** §6.25

**Objective:** Persist **`inspection_results`**, **`inspection_evidence`**, and auto-generated **`inspection_anomalies`** — this completes the **condition data plane** for RAMS (feeds Gaps `05-03`).

---

## Standards & authoritative references

| Source | Relevance |
|--------|-----------|
| **ISO 14224:2016** | Failure data category includes operating context; **inspection readings** provide **precursor / condition** signals for **failure mechanisms** (§3.29) when trendable. |
| **IEC 60050-192** | Vocabulary alignment for “failure” vs “degraded state” — map `result_status=warning` to **incipient** condition, not necessarily a counted failure event. |
| **Internal** | `MODULES_6_23_6_25_*` §5.4 `inspection_results`, `inspection_evidence`, `inspection_anomalies`. |

---

## Schema definition

### `inspection_results`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `entity_sync_id` | TEXT UUID | |
| `round_id` | INTEGER | FK → `inspection_rounds` |
| `checkpoint_id` | INTEGER | FK → `inspection_checkpoints` |
| `result_status` | TEXT | `pass` / `warning` / `fail` / `not_accessible` / `not_done` |
| `numeric_value` | REAL | nullable |
| `text_value` | TEXT | nullable |
| `boolean_value` | INTEGER | 0/1 nullable |
| `comment` | TEXT | |
| `recorded_at` | TEXT | |
| `recorded_by_id` | INTEGER | |
| `row_version` | INTEGER | |

### `inspection_evidence`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `result_id` | INTEGER | FK |
| `evidence_type` | TEXT | `photo` / `file` / `reading_snapshot` / `signature` |
| `file_path_or_value` | TEXT | Local path or hash reference |
| `captured_at` | TEXT | |
| `entity_sync_id` | TEXT | |
| `row_version` | INTEGER | |

### `inspection_anomalies`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `round_id` | INTEGER | |
| `result_id` | INTEGER | nullable |
| `anomaly_type` | TEXT | e.g. `threshold_breach`, `missing_mandatory_photo` |
| `severity` | INTEGER | 1–5 |
| `description` | TEXT | |
| `linked_di_id`, `linked_work_order_id` | INTEGER | nullable |
| `requires_permit_review` | INTEGER | 0/1 |
| `resolution_status` | TEXT | `open` / `triaged` / `closed` |
| `entity_sync_id` | TEXT | |
| `row_version` | INTEGER | |

### Offline queue (optional table or reuse outbox)

| `inspection_offline_queue` | `id`, `payload_json`, `local_temp_id`, `sync_status` | Until pushed |

---

## Business logic & validation rules

1. **Threshold evaluation:** For `check_type=numeric`, compute:
   - `pass` if `normal_min ≤ value ≤ normal_max`
   - `warning` if inside `warning_*` bands but outside normal
   - `fail` if outside warning bands or beyond fail policy

2. **Anomaly generation:** On `fail` or `warning` per `escalation_rules_json`, insert `inspection_anomalies`.

3. **Evidence:** If `requires_comment_on_exception` and `result_status ≠ pass`, require `comment` or `inspection_evidence` row.

---

## Mathematical triggers

**Deviation score (optional helper for RAMS):**  
`deviation_ratio = (value - center) / (normal_max - normal_min)` where `center = (normal_max+normal_min)/2` — store in `details_json` for trend charts (not for ISO compliance per se).

---

## Sync transport specification

| `entity_type` | Notes |
|---------------|--------|
| `inspection_results` | Include `round_id`, `checkpoint_id`, `result_status`, numeric/boolean/text fields, `row_version` |
| `inspection_evidence` | Include `result_id`, `evidence_type`, `file_path_or_value` **or** blob hash after upload |
| `inspection_anomalies` | Full row |

**Large files:** `[ATTENTION: REQUIRES VPS AGENT INTERVENTION]` — if evidence uses object storage, define **separate upload URL flow**; `payload_json` carries **content hash + size + MIME**, not raw bytes.

Example `inspection_results` payload:

```json
{
  "id": 0,
  "entity_sync_id": "uuid",
  "row_version": 3,
  "round_id": 0,
  "checkpoint_id": 0,
  "result_status": "warning",
  "numeric_value": 82.4,
  "recorded_at": "2026-04-17T14:00:00Z",
  "recorded_by_id": 0
}
```

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 03-inspection-rounds-and-checklists — Sprint 02 — Field Execution: Results, Evidence, And Offline Queue

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
