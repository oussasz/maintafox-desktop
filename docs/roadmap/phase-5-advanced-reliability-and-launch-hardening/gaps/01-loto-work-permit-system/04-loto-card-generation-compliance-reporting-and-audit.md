# Sprint 4 — LOTO Card Generation, Compliance Reporting, And Audit

**PRD:** §6.23 (LOTO cards)

**Objective:** Print-ready **per-isolation-point** LOTO card + compliance KPI queries.

---

## Schema definition

### `loto_card_print_jobs` (optional audit)

| Column | Type |
|--------|------|
| `id`, `permit_id`, `isolation_id`, `printed_at`, `printed_by_id`, `entity_sync_id`, `row_version` | |

---

## Business logic & validation rules

1. Card must list: **equipment**, **energy type**, **isolation point id**, **lock number** (if captured), **verifier signature**, **expiry** (inherits permit).

2. **Report `open_permits`:** `status IN (issued, active, suspended)` with `expires_at > now`.

---

## Mathematical triggers

**Permit compliance rate (30d):**  
\(\frac{\text{handed\_back on time}}{\text{activated}}\)

---

## Sync transport specification

| `entity_type` | Notes |
|---------------|--------|
| `loto_card_print_jobs` | If persisted |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 01-loto-work-permit-system — Sprint 04 — LOTO Card Generation, Compliance Reporting, And Audit

**Read Only:**
- @docs/PRD.md §6.23
- @docs/research/MODULES_6_23_6_25_WORK_PERMITS_AND_INSPECTION_ROUNDS.md
- @src-tauri/src/migrations/
- @src-tauri/src/sync/domain.rs
- @src-tauri/src/commands/

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
