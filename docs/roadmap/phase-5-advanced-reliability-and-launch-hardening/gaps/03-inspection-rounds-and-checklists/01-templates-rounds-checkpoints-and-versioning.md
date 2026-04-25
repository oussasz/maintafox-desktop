# Sprint 1 — Templates, Rounds, Checkpoints, And Versioning

**PRD:** §6.25

**Objective:** Persist **inspection templates**, **immutable template versions**, **checkpoints**, and **scheduled round instances** — the **minimum schema** required before RAMS KPI logic can consume **condition-based** inputs (Gaps `05-03` gate).

---

## Standards & authoritative references

| Source | Relevance |
|--------|-----------|
| **ISO 14224:2016** | Equipment taxonomy and **condition monitoring** inputs feed reliability when encoded as structured data (Clause 9 — data categories; operating context). |
| **ISO 45001:2018** | OH&S management — inspection as **monitoring and measurement**; evidence must be audit-capable (cross-ref `MODULES_6_23_6_25_*` §3.3). |
| **Internal** | `docs/research/MODULES_6_23_6_25_WORK_PERMITS_AND_INSPECTION_ROUNDS.md` §5.4–5.7 (exact table list). |

---

## Schema definition (SQLite)

### `inspection_templates`

| Column | Type | Constraints |
|--------|------|----------------|
| `id` | INTEGER | PK |
| `entity_sync_id` | TEXT | UUID UNIQUE NOT NULL |
| `code` | TEXT | UNIQUE per tenant scope |
| `name` | TEXT | NOT NULL |
| `org_scope_id` | INTEGER | FK nullable |
| `route_scope` | TEXT | JSON or enum |
| `estimated_duration_minutes` | INTEGER | |
| `is_active` | INTEGER | 0/1 |
| `current_version_id` | INTEGER | FK → `inspection_template_versions.id` |
| `row_version` | INTEGER | NOT NULL default 1 |

### `inspection_template_versions`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `template_id` | INTEGER | FK |
| `version_no` | INTEGER | Monotonic per template |
| `effective_from` | TEXT | ISO-8601 |
| `checkpoint_package_json` | TEXT | Denormalized snapshot of checkpoints at publish |
| `tolerance_rules_json` | TEXT | Warning/fail bands |
| `escalation_rules_json` | TEXT | |
| `requires_review` | INTEGER | 0/1 |
| `row_version` | INTEGER | |

### `inspection_checkpoints`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `template_version_id` | INTEGER | FK |
| `sequence_order` | INTEGER | NOT NULL |
| `asset_id` | INTEGER | FK nullable (template-level default) |
| `component_id` | INTEGER | nullable |
| `checkpoint_code` | TEXT | NOT NULL |
| `check_type` | TEXT | `numeric` / `boolean` / `observation` / `pass_fail` |
| `measurement_unit` | TEXT | e.g. `°C`, `mm/s` |
| `normal_min`, `normal_max`, `warning_min`, `warning_max` | REAL | nullable |
| `requires_photo`, `requires_comment_on_exception` | INTEGER | |

### `inspection_rounds`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `entity_sync_id` | TEXT | UUID |
| `template_id`, `template_version_id` | INTEGER | FK — **version pinned at schedule time** |
| `scheduled_at` | TEXT | |
| `assigned_to_id` | INTEGER | |
| `status` | TEXT | `scheduled` / `released` / `in_progress` / `completed` / `completed_with_findings` / `reviewed` / `missed` / `cancelled` |
| `row_version` | INTEGER | |

---

## Business logic & validation rules

1. **Version immutability:** `inspection_template_versions` rows are **append-only**; editing checkpoints creates **new** `version_no` + new checkpoint rows FK’d to new version.

2. **Round pins version:** `inspection_rounds.template_version_id` **must** copy `inspection_templates.current_version_id` at schedule time (or explicit version picker).

3. **Checkpoint uniqueness:** `(template_version_id, sequence_order)` UNIQUE.

---

## Mathematical triggers

N/A (no RAMS formulas this sprint). Checkpoint `normal_min/max` define **warning/fail** bands for Sprint `02` result evaluation.

---

## Sync transport specification

| `entity_type` | `payload_json` required keys |
|---------------|------------------------------|
| `inspection_templates` | `{ "id", "entity_sync_id", "row_version", "code", "name", "is_active", "current_version_id" }` |
| `inspection_template_versions` | `{ "id", "entity_sync_id", "row_version", "template_id", "version_no", "effective_from", "checkpoint_package_json", "requires_review" }` |
| `inspection_checkpoints` | `{ "id", "entity_sync_id", "row_version", "template_version_id", "sequence_order", "checkpoint_code", "check_type" }` |
| `inspection_rounds` | `{ "id", "entity_sync_id", "row_version", "template_version_id", "scheduled_at", "status", "assigned_to_id" }` |

**`entity_sync_id`:** Stable UUID per logical entity; `row_version` increments on every mutating write.

**`[ATTENTION: REQUIRES VPS AGENT INTERVENTION]`** — PostgreSQL mirror tables + inbound apply + tests.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 03-inspection-rounds-and-checklists — Sprint 01 — Templates, Rounds, Checkpoints, And Versioning

**Read Only:**
- @docs/PRD.md §6.25
- @src-tauri/src/migrations/
- @src-tauri/src/sync/domain.rs
- @src/pages/InspectionsPage.tsx

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
