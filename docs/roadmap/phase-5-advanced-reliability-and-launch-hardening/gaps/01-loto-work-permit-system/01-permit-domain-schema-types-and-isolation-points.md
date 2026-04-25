# Sprint 1 — Permit Domain: Schema, Types, And Isolation Points

**PRD:** §6.23

**Objective:** Persistent **LOTO/PTW** domain: `permit_types`, `work_permits`, `permit_isolations` with **ISO 45001:2018**-aligned hazard/control traceability (risk assessment & operational control).

---

## Standards & authoritative references

| Source | Role |
|--------|------|
| **ISO 45001:2018** | OH&S MS — hazard identification, operational planning and control, emergency preparedness; permits as **controlled** work ([ISO 45001](https://www.iso.org/standard/63754.html)). |
| **UK HSE — Permit-to-work** | Control system attributes (authorization, isolation, communication, hand-back) — operational requirements reflected in lifecycle sprints. |
| **Internal** | `MODULES_6_23_6_25_*` §6.4 recommended tables; PRD §6.23 `permit_types` row. |

---

## Schema definition

### `permit_types`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `entity_sync_id` | TEXT UUID | |
| `code` | TEXT | `loto`, `hot_work`, `confined_space`, … |
| `name`, `description` | TEXT | |
| `requires_hse_approval`, `requires_operations_approval`, `requires_atmospheric_test` | INTEGER | 0/1 |
| `max_duration_hours` | REAL | |
| `mandatory_ppe_ids_json` | TEXT | Array of PPE ref ids |
| `mandatory_control_rules_json` | TEXT | Machine-readable guardrails |
| `row_version` | INTEGER | |

### `work_permits`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `entity_sync_id` | TEXT UUID | |
| `code` | TEXT | Human-readable permit number |
| `linked_work_order_id` | INTEGER FK NULL | |
| `permit_type_id` | INTEGER FK | |
| `asset_id`, `entity_id` | INTEGER | Scope |
| `status` | TEXT | `draft` / `pending_review` / `approved` / `issued` / `active` / `suspended` / `revalidation_required` / `closed` / `handed_back` / `cancelled` / `expired` |
| `requested_at`, `issued_at`, `activated_at`, `expires_at`, `closed_at`, `handed_back_at` | TEXT | |
| `row_version` | INTEGER | |

### `permit_isolations`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `entity_sync_id` | TEXT UUID | |
| `permit_id` | INTEGER FK | |
| `isolation_point` | TEXT | Tag id / valve id |
| `energy_type` | TEXT | `electrical` / `pressure` / `chemical` / … |
| `isolation_method` | TEXT | Lock / blind / disconnect |
| `applied_by_id`, `verified_by_id` | INTEGER | |
| `applied_at`, `verified_at`, `removal_verified_at` | TEXT | LOTO sequence |
| `row_version` | INTEGER | |

---

## Business logic & validation rules

1. **`permit_types.code=loto`:** ≥1 `permit_isolations` row before `status` → `active` (configurable).

2. **Expiry:** `now > expires_at` → auto `expired` transition (job).

3. **WO link:** `linked_work_order_id` must match same `asset_id` boundary as WO equipment.

---

## Mathematical triggers

N/A.

---

## Sync transport specification

| `entity_type` | Core `payload_json` keys |
|---------------|---------------------------|
| `permit_types` | `id`, `entity_sync_id`, `row_version`, `code`, `max_duration_hours`, `mandatory_control_rules_json` |
| `work_permits` | `id`, `entity_sync_id`, `row_version`, `linked_work_order_id`, `permit_type_id`, `status`, `expires_at` |
| `permit_isolations` | `id`, `entity_sync_id`, `row_version`, `permit_id`, `isolation_point`, `energy_type`, `verified_at` |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 01-loto-work-permit-system — Sprint 01 — Permit Domain: Schema, Types, And Isolation Points

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
