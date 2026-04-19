# Sprint 3 — Anomaly Routing: Follow-Up DI And WO Generation

**PRD:** §6.25, §6.4, §6.5

**Objective:** Link **`inspection_anomalies`** to **intervention requests** and **work orders** with stable FKs for downstream **RAMS** (repeat-condition analysis).

---

## Standards & authoritative references

| Source | Relevance |
|--------|-----------|
| **ISO 14224:2016** | Follow-up work generates **maintenance data** (corrective action) tied to equipment — must reference originating **condition evidence** for traceability. |
| **Internal** | Research §5.6 follow-up routing; PRD §6.25 reviewed state. |

---

## Schema definition

### `inspection_anomalies` (columns added or linked)

| Column | Notes |
|--------|-------|
| `linked_di_id` | FK → `intervention_requests` or tenant DI table |
| `linked_work_order_id` | FK → `work_orders` |
| `routing_decision` | TEXT | `di` / `wo` / `defer` |

### `intervention_requests` / `work_orders` (existing)

Ensure optional FK:

| Column | Notes |
|--------|-------|
| `source_inspection_anomaly_id` | INTEGER NULL — **provenance** for RAMS |

---

## Business logic & validation rules

1. **Idempotent routing:** Same `inspection_anomalies.id` cannot spawn two open WOs — unique partial index on `(anomaly_id)` where WO status not closed/cancelled.

2. **Severity:** `severity ≥ threshold` may **require** WO vs DI per tenant policy.

3. **Permit:** If `requires_permit_review=1`, block WO **In Progress** until permit linked (Gaps `01`).

---

## Mathematical triggers

**Repeat condition index (for RAMS handoff):**  
`repeat_count = COUNT(anomalies)` per `(asset_id, checkpoint_id)` rolling 90d — surface in Gaps `05` KPI companion table or materialized query.

---

## Sync transport specification

| `entity_type` | Payload must include |
|---------------|----------------------|
| `inspection_anomalies` | Updated `linked_di_id`, `linked_work_order_id`, `resolution_status`, `row_version` |
| `intervention_requests` | `source_inspection_anomaly_id` when created from anomaly |
| `work_orders` | `source_inspection_anomaly_id` + standard WO fields |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 03-inspection-rounds-and-checklists — Sprint 03 — Anomaly Routing: Follow-Up DI And WO Generation

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
