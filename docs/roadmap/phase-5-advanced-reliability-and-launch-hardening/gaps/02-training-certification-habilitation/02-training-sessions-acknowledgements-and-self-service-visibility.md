# Sprint 2 — Training Sessions, Acknowledgements, And Self-Service Visibility

**PRD:** §6.20, §6.19

**Objective:** `training_sessions`, `training_attendance`, linkage to **document acknowledgements** (6.15) for ISO 45001 **awareness** evidence.

---

## Schema definition

### `training_sessions`

| Column | Type |
|--------|------|
| `id`, `entity_sync_id`, `course_code`, `scheduled_start`, `scheduled_end`, `location`, `instructor_id`, `row_version` | |

### `training_attendance`

| Column | Type |
|--------|------|
| `id`, `entity_sync_id`, `session_id`, `personnel_id`, `attendance_status`, `completed_at`, `score`, `row_version` | |

### `document_acknowledgements` (if not exists — else FK)

| Column | Notes |
|--------|-------|
| `personnel_id`, `document_version_id`, `acknowledged_at` | |

---

## Business logic & validation rules

1. **Attendance → certification:** On `completed` + passing `score`, create `personnel_certifications` row via workflow.

2. **Self-service:** User sees own `expires_at` and assigned sessions only.

---

## Sync transport specification

| `entity_type` | |
|---------------|--|
| `training_sessions` | Full |
| `training_attendance` | Full |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 02-training-certification-habilitation — Sprint 02 — Training Sessions, Acknowledgements, And Self-Service Visibility

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
