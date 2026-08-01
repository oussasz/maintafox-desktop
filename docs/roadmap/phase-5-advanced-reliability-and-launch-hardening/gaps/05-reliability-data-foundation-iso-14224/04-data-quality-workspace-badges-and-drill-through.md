# Sprint 4 — Data Quality Workspace, Badges, And Drill-Through

**PRD:** §6.10.1, §6.11

**Objective:** RAMS UI with **data-quality badges** driven by **`data_quality_score`**, missing **ISO 14224** fields, and weak exposure — drill-through to WO and inspection records.

---

## Standards & authoritative references

| Source | Use |
|--------|-----|
| **ISO 14224:2016** | Data quality control (standard scope); list minimum fields missing for exchange. |
| **Internal** | PRD §6.11 widget rules. |

---

## Schema definition (read-only views)

### View `v_ram_data_quality_issues`

| Exposed column | Source |
|----------------|--------|
| `equipment_id` | |
| `issue_code` | `MISSING_FAILURE_MODE`, `MISSING_EXPOSURE`, `LOW_SAMPLE`, `MISSING_INSPECTION_COVERAGE` |
| `severity` | |
| `remediation_url` | Deep link |

---

## Business logic & validation rules

1. **Badge green:** `data_quality_score ≥ 0.85` and no blocking `issue_code`.

2. **Drill-through:** Click badge → list WOs missing `failure_mode_id` (ISO **failure mode**); list assets with zero `runtime_exposure_logs` in 90d.

---

## Mathematical triggers

Display **confidence interval** optional: Wilson score on repeat rate — **Phase 2**; document placeholder.

---

## Sync transport specification

Typically **no new entities** — UI only. If user dismisses warnings, optional `user_dismissals` table with sync.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- None — no mirror/API work for this sprint per Sync section (desktop/UI/tests only).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 05-reliability-data-foundation-iso-14224 — Sprint 04 — Data Quality Workspace, Badges, And Drill-Through

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

**Sync JSON:** Typically **no new entities** — UI only. If user dismisses warnings, optional `user_dismissals` table with sync.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
