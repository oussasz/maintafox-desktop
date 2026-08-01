# Sprint 3 — Planning Integration: Permit Eligibility And Skill Gaps

**PRD:** §6.20, §6.16, §6.23

**Objective:** Read model **`personnel_readiness`** for WO assignment: **blocked** if certification missing for permit type on linked WO.

---

## Schema definition (view or RPC)

### `v_personnel_readiness` (materialized optional)

| Column | Notes |
|--------|-------|
| `personnel_id`, `permit_type_code`, `is_qualified`, `blocking_reason`, `expires_at` | |

---

## Business logic & validation rules

1. **Query:** Join `qualification_requirement_profiles` → `personnel_certifications` on type; compare `expires_at`.

2. **Planning:** Return list of **skill gaps** for selected crew vs WO permit requirement.

---

## Mathematical triggers

**Gap count:** \(\sum \mathbb{1}_{\neg qualified}\) per shift roster.

---

## Sync transport specification

Derived views typically **not** synced — **recompute** on VPS from mirrored base tables. If materialized, `entity_type`: `personnel_readiness_snapshots` with `period`, `payload_json`.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 02-training-certification-habilitation — Sprint 03 — Planning Integration: Permit Eligibility And Skill Gaps

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
