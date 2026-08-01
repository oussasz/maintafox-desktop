# Sprint 3 — Regression Tests And Analytics Contract Freeze

**PRD:** §6.5, §6.10, §6.11

**Objective:** Lock **close-out → reliability eligibility** behavior with automated tests and a **frozen analytics contract** document so Gaps `05` and RAMS cannot drift.

---

## Standards & authoritative references

| Source | Relevance |
|--------|-----------|
| **ISO 14224:2016** | Minimum data for failure and maintenance categories; contract lists which Maintafox fields satisfy each category for **MTBF/MTTR**-eligible events. |
| **Internal** | `MODULE_6_10_RELIABILITY_ENGINE.md` §8 calculation governance. |

---

## Schema definition

### Artifact: `docs/contracts/analytics_closeout_contract_v1.md` (or adjacent path)

Sections (non-normative markdown, **versioned**):

1. WO → `failure_events` eligibility matrix (references `work_orders`, `wo_failure_details` columns).
2. **ISO 14224 field mapping table:** Maintafox column → ISO concept (failure mode / cause / mechanism / consequence / downtime).

No new tables required unless storing contract hash:

### Optional: `analytics_contract_versions`

| Column | Type |
|--------|------|
| `id`, `contract_id`, `version_semver`, `content_sha256`, `activated_at` | |

---

## Business logic & validation rules

1. **CI gate:** `cargo test` includes modules:
   - Close corrective WO **without** failure mode → **must fail** close under default policy.
   - Close with full coding → **must succeed**.
   - Integrity detector injects bad downtime → **must** raise `WO_DOWNTIME_NEGATIVE_DURATION`.

2. **Contract freeze:** Any PR changing validation rules **must** bump `analytics_contract_versions.content_sha256` or semver.

---

## Mathematical triggers

Document in contract:

- **MTBF-eligible event:** `eligible = (unplanned_failure) AND (failure_mode_id NOT NULL) AND (NOT excluded_by_policy)`.

---

## Sync transport specification

No new entity types unless `analytics_contract_versions` is replicated:

### `entity_type`: `analytics_contract_versions` (optional)

```json
{
  "id": 0,
  "entity_sync_id": "uuid",
  "row_version": 0,
  "contract_id": "closeout_to_reliability_v1",
  "version_semver": "1.0.0",
  "content_sha256": "hex64"
}
```

Typically **tenant-local**; sync only if VPS must enforce same contract per tenant for regulated fleets.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 06-work-order-closeout-and-data-integrity — Sprint 03 — Regression Tests And Analytics Contract Freeze

**Read Only:**
- @docs/PRD.md §6.5
- @src-tauri/src/
- @src-tauri/src/sync/domain.rs
- @src/pages/WorkOrdersPage.tsx

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
