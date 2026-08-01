# Sprint 1 — Qualification Schema: Certificates, Profiles, And Renewal Rules

**PRD:** §6.20

**Objective:** **ISO 45001 competence** records: certifications, requirement profiles, renewal — enabling **permit** and **assignment** gates (research §4–5).

---

## Standards & authoritative references

| Source | Role |
|--------|------|
| **ISO 45001:2018** | Clause on **competence** — training records must evidence ability to perform safe work ([ISO 45001](https://www.iso.org/standard/63754.html)). |
| **Internal** | `MODULE_6_20_TRAINING_CERTIFICATION_AND_HABILITATION.md` §4.1–4.3, §5 workflow. |

---

## Schema definition

### `certification_types`

| Column | Type |
|--------|------|
| `id` | INTEGER PK |
| `entity_sync_id` | TEXT UUID |
| `code`, `name` | TEXT |
| `default_validity_months` | INTEGER |
| `renewal_lead_days` | INTEGER |
| `row_version` | INTEGER |

### `personnel_certifications`

| Column | Type |
|--------|------|
| `id` | INTEGER PK |
| `entity_sync_id` | TEXT |
| `personnel_id` | INTEGER FK |
| `certification_type_id` | INTEGER FK |
| `issued_at`, `expires_at` | TEXT |
| `issuing_body`, `certificate_ref` | TEXT |
| `verification_status` | TEXT | `pending` / `verified` / `rejected` |
| `row_version` | INTEGER |

### `qualification_requirement_profiles`

| Column | Type |
|--------|------|
| `id` | INTEGER PK |
| `entity_sync_id` | TEXT |
| `profile_name` | TEXT |
| `required_certification_type_ids_json` | TEXT |
| `applies_to_permit_type_codes_json` | TEXT | Link to `permit_types.code` |
| `row_version` | INTEGER |

---

## Business logic & validation rules

1. **Expiry:** `expires_at < today` → status `expired` for readiness queries.

2. **Protected hazardous:** Cannot disable profile check for `permit_types` flagged `requires_competence_verify` without audit super-admin (research §6).

---

## Sync transport specification

| `entity_type` | Payload keys |
|---------------|----------------|
| `certification_types` | Full |
| `personnel_certifications` | `personnel_id` as FK id; **no PII in logs** — payload still contains names if mirror stores them encrypted per policy |
| `qualification_requirement_profiles` | JSON arrays |

**Privacy:** `[ATTENTION: REQUIRES VPS AGENT INTERVENTION]` — PII retention, GDPR-style minimization on mirror.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- Create/update tenant mirror tables for qualifications + personnel_qualifications + profiles; PII minimization policy.
- Inbound apply + validation; idempotent upsert on (tenant_id, entity_sync_id).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 02-training-certification-habilitation — Sprint 01 — Qualification Schema: Certificates, Profiles, And Renewal Rules

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
