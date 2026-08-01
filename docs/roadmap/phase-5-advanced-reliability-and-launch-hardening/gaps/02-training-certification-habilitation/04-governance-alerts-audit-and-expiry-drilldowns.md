# Sprint 4 — Governance Alerts, Audit, And Expiry Drilldowns

**PRD:** §6.20, §6.14

**Objective:** Notification rules for **expiry** (`notification_rules` pattern); supervisor drilldowns by org node.

---

## Schema definition

Uses `notification_rules` + `personnel_certifications` — no new tables unless:

### `training_expiry_alert_events`

| Column | Type |
|--------|------|
| `id`, `certification_id`, `alert_dedupe_key`, `fired_at`, `entity_sync_id`, `row_version` | |

---

## Business logic & validation rules

1. **Dedupe:** `dedupe_key = hash(personnel_id + certification_type + week_bucket)`.

2. **Escalation:** If expired + assigned to active WO → **critical** severity.

---

## Sync transport specification

| `entity_type` | |
|---------------|--|
| `training_expiry_alert_events` | If table exists |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 02-training-certification-habilitation — Sprint 04 — Governance Alerts, Audit, And Expiry Drilldowns

**Read Only:**
- @docs/PRD.md §6.20
- @src/pages/PersonnelPage.tsx
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
