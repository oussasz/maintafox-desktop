# Sprint 3 — Isolation Verification And Work Order Gating

**PRD:** §6.23, §6.5

**Objective:** Rust-enforced **WO state guards**: cannot enter `In Progress` on hazardous work without **active** permit + verified isolations.

---

## Schema definition (references)

- `work_orders.status`, `work_orders.id`
- `work_permits` (`linked_work_order_id`, `status`, `permit_type_id`)
- `permit_isolations` (`verified_at NOT NULL`)

---

## Business logic & validation rules

1. If `work_orders.requires_permit=1` (new column **or** derived from WO hazard class):
   - `transition_wo(in_progress)` requires ∃ `work_permits` where `linked_work_order_id = wo.id` AND `status = active` AND all mandatory isolations `verified_at IS NOT NULL`.

2. **Local-first:** Rules apply offline; permit status must be readable from local DB.

3. **Resume after suspend:** Block `in_progress` if permit `suspended`.

---

## Mathematical triggers

N/A.

---

## Sync transport specification

Uses existing `work_permits` + `work_orders` payloads; transition may emit **two** outbox rows in one transaction — same `server_batch_id` on apply.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 01-loto-work-permit-system — Sprint 03 — Isolation Verification And Work Order Gating

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

**Sync JSON:** Uses existing `work_permits` + `work_orders` payloads; transition may emit **two** outbox rows in one transaction — same `server_batch_id` on apply.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
