# Sprint 2 — Permit Lifecycle: Issuance, Activation, Suspension, Handback

**PRD:** §6.23

**Objective:** State machine for `work_permits.status` with **suspension** and **revalidation**; audit events for ISO 45001 **continual improvement** evidence.

---

## Schema definition

### `permit_suspensions`

| Column | Type |
|--------|------|
| `id` | INTEGER PK |
| `entity_sync_id` | TEXT |
| `permit_id` | INTEGER FK |
| `reason` | TEXT |
| `suspended_by_id`, `suspended_at` | |
| `reinstated_by_id`, `reinstated_at` | NULL until reinstated |
| `reactivation_conditions` | TEXT |
| `row_version` | INTEGER |

### `permit_handover_logs`

| Column | Type |
|--------|------|
| `id` | INTEGER PK |
| `entity_sync_id` | TEXT |
| `permit_id` | INTEGER FK |
| `handed_from_role`, `handed_to_role` | TEXT |
| `confirmation_note` | TEXT |
| `signed_at` | TEXT |
| `row_version` | INTEGER |

---

## Business logic & validation rules

Valid transitions (subset — encode as Rust enum + match):

| From | To | Guard |
|------|-----|-------|
| `draft` | `pending_review` | fields complete |
| `pending_review` | `approved` | approvals per `permit_types` |
| `approved` | `issued` | issuer role |
| `issued` | `active` | isolations verified |
| `active` | `suspended` | always allowed with reason |
| `suspended` | `revalidation_required` | policy |
| `revalidation_required` | `active` | revalidation tests pass |
| `active` | `closed` | mechanical complete |
| `closed` | `handed_back` | operations sign-off |

**Illegal:** `handed_back` → any except `cancelled` correction.

---

## Sync transport specification

| `entity_type` | Payload |
|---------------|---------|
| `work_permits` | Must include `status`, all timestamps |
| `permit_suspensions` | Full row |
| `permit_handover_logs` | Full row |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 01-loto-work-permit-system — Sprint 02 — Permit Lifecycle: Issuance, Activation, Suspension, Handback

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
