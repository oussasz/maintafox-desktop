# Sprint 1 — Failure Taxonomy: Hierarchies And Stable Reference Binding

**PRD:** §6.10.1

**Objective:** Implement **ISO 14224-aligned** failure **taxonomy** in SQLite: **failure mode** (manner of failure), **failure cause** (root cause), **failure mechanism** (physical/chemical process), and **failure consequence** — mapped to Maintafox `failure_hierarchies` / `failure_codes` with `code_type` discriminant.

---

## Standards & authoritative references

| Concept | ISO 14224:2016 | IEC 60050-192 (via ISO 14224) |
|---------|----------------|--------------------------------|
| **Failure mode** | §3.30 — *manner in which failure occurs*; normative thesaurus **Annex B.2.6** (per-class failure modes) | Aligns with IEC failure mode |
| **Failure cause** | §3.24 — *root cause*, circumstances leading to failure | |
| **Failure mechanism** | §3.29 — *process* leading to failure; **Table B.2** links mechanisms to equipment classes | Store as `code_type = mechanism` or child of cause |
| **Failure consequence / effect** | Data category **(b) failure data** — failure consequence | Maps to `failure_effect` in PRD |

**Official standard:** [ISO 14224:2016](https://www.iso.org/standard/64076.html) — *Collection and exchange of reliability and maintenance data for equipment*.

**Internal:** `MODULE_6_10_RELIABILITY_ENGINE.md` §7; PRD §6.10.1 entity list.

---

## Schema definition

### `failure_hierarchies`

| Column | Type |
|--------|------|
| `id` | INTEGER PK |
| `entity_sync_id` | TEXT UUID UNIQUE |
| `name` | TEXT |
| `asset_scope` | TEXT | JSON: equipment class filters |
| `version_no` | INTEGER |
| `is_active` | INTEGER |
| `row_version` | INTEGER |

### `failure_codes`

| Column | Type | ISO mapping |
|--------|------|-------------|
| `id` | INTEGER PK | |
| `hierarchy_id` | INTEGER FK | |
| `parent_id` | INTEGER NULL | Tree: mode → mechanism → cause |
| `code` | TEXT | Stable code e.g. `FM-LEAK` |
| `label` | TEXT | |
| `code_type` | TEXT | `class` / `mode` / **`mechanism`** / `cause` / `effect` / `remedy` |
| `iso_14224_annex_ref` | TEXT NULL | Traceability e.g. `B.2.6 / Pump` |
| `is_active` | INTEGER | |
| `row_version` | INTEGER | |

**Rule:** `failure_mode` rows **must** be leaves or defined level per tenant policy; **mechanism** optional but recommended for analytics matching ISO §3.29.

---

## Business logic & validation rules

1. **Deactivate, don’t delete:** Codes in use by `wo_failure_details` or `failure_events` → `is_active=0` + successor `code` mapping.

2. **Uniqueness:** `(hierarchy_id, code)` UNIQUE.

3. **WO close-out binding:** Existing `wo_failure_details.failure_mode_id` must reference `failure_codes.id` where `code_type=mode` (migrate from generic `reference_values` if needed via compatibility view).

---

## Mathematical triggers

N/A.

---

## Sync transport specification

| `entity_type` | `payload_json` |
|---------------|----------------|
| `failure_hierarchies` | `{ "id", "entity_sync_id", "row_version", "name", "version_no", "is_active" }` |
| `failure_codes` | `{ "id", "entity_sync_id", "row_version", "hierarchy_id", "parent_id", "code", "code_type", "iso_14224_annex_ref", "is_active" }` |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 05-reliability-data-foundation-iso-14224 — Sprint 01 — Failure Taxonomy: Hierarchies And Stable Reference Binding

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

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
