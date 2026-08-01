# Sprint 1 — PRD Gap Review And UX Residuals On Budget Surfaces

**PRD:** §6.24

**Objective:** Close UX/logic gaps against PRD using **existing** tables: `budget_versions`, `budget_lines`, `cost_centers`, `budget_alert_configs`, `budget_alert_events` (see `m20260503_000058_budget_controls_alerting_reporting.rs`).

---

## Standards & authoritative references

| Source | Role |
|--------|------|
| **ISO 14224:2016** | Explicitly **excludes direct cost data** from its RM scope — Maintafox cost/budget remains **internal financial** layer; reliability **cost-of-failure** uses WO costs, not ISO 14224 exchange format. |
| **Internal** | `MODULE_6_24_BUDGET_AND_COST_CENTER_MANAGEMENT.md` §4 actuals/commitments/variance. |

---

## Schema definition (existing — verify coverage)

| Table | Critical columns (audit) |
|-------|---------------------------|
| `budget_versions` | Versioning, scenario, `row_version` |
| `budget_lines` | Period, bucket, amount |
| `cost_centers` | Hierarchy FK |
| `budget_alert_configs` | `threshold_pct`, `threshold_amount`, `dedupe_window_minutes`, `row_version` |
| `budget_alert_events` | `dedupe_key`, `current_value`, `threshold_value`, `acknowledged_at`, `row_version` |

---

## Business logic & validation rules

1. **Alert dedupe:** Must respect `dedupe_window_minutes` (default 240) — align UI copy with backend.

2. **Variance review:** PRD §6.24.3 — ensure `budget_variance_reviews` (if present) or equivalent captured.

3. **RAMS handoff:** Cost-of-failure view joins **WO actuals** + `failure_events` — document join keys in gap review checklist.

---

## Mathematical triggers

**Variance %:**  
\(\frac{\text{actual} - \text{budget}}{\text{budget}} \times 100\)** for bucket b.

---

## Sync transport specification

Mirror **budget** entities if multi-device:

| `entity_type` | Include |
|---------------|---------|
| `budget_versions` | `id`, `entity_sync_id`, `row_version`, fiscal keys |
| `budget_lines` | Full line |
| `budget_alert_configs` | Full row |
| `budget_alert_events` | Full row |

**Note:** Confirm existing sync catalog — extend if not registered.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 04-budget-cost-center-closure — Sprint 01 — PRD Gap Review And UX Residuals On Budget Surfaces

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
