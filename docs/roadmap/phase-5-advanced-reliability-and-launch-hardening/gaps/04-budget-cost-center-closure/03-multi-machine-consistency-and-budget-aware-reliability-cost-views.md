# Sprint 3 — Multi-Machine Consistency And Budget-Aware Reliability Cost Views

**PRD:** §6.24, §6.10 cost-of-failure

**Objective:** Join **`reliability_kpi_snapshots`** (or `failure_events`) with **WO cost roll-ups** for **cost-of-failure** without double counting.

---

## Standards & authoritative references

| Source | Role |
|--------|------|
| **ISO 14224** | Failure analytics; **not** direct cost — financial numbers from Maintafox cost engine. |
| **Internal** | `MODULE_6_10` §11 cost-of-failure; `MODULE_6_24` roll-up. |

---

## Schema definition

### Read model: `v_cost_of_failure` (view)

| Column | Source |
|--------|--------|
| `equipment_id` | `failure_events` |
| `period` | |
| `total_downtime_cost` | derived from hours × rate **or** WO cost |
| `total_corrective_cost` | sum WO costs in period |
| `currency_code` | |

---

## Business logic & validation rules

1. **Single source of truth:** WO costs authoritative; budget shows **committed vs actual** separately.

2. **Sync order:** Apply `budget_versions` before `budget_lines` children on inbound.

---

## Mathematical triggers

**Cost per failure:**  
\(\frac{\sum \text{corrective cost for failures}}{\text{failure count}}\)

---

## Sync transport specification

| `entity_type` | |
|---------------|--|
| All budget tables | As in Sprint `04-01` |
| `failure_events` | For VPS cost correlation if fleet RAMS |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 04-budget-cost-center-closure — Sprint 03 — Multi-Machine Consistency And Budget-Aware Reliability Cost Views

**Read Only:**
- @docs/PRD.md §6.24
- @src/pages/BudgetPage.tsx
- @src-tauri/src/sync/domain.rs
- @src/services/sync-vps-transport-service.ts

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
