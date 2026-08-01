# Sprint 2 — ERP Export, Reconciliation, And Posting Integrity

**PRD:** §6.24, §6.22

**Objective:** Harden `exportPostedActualsForErp` / `exportApprovedReforecastsForErp` — idempotent **JSONL** or **contract JSON** with **reconciliation status** fields.

---

## Standards & authoritative references

| Source | Role |
|--------|------|
| **ISO 14224** | Not used for cost export — cite exclusion to avoid mixing RM exchange with ERP finance. |
| **Internal** | `MODULE_6_22_ERP_AND_EXTERNAL_SYSTEMS_CONNECTOR.md`; Phase 4 ERP connector roadmap. |

---

## Schema definition

### `integration_exceptions` (PRD §16 patterns)

Ensure posts reference:

| Column | Use |
|--------|-----|
| `maintafox_value_snapshot` | Pre-image |
| `external_value_snapshot` | ERP echo |
| `resolution_status` | `open` / `merged` / … |

---

## Business logic & validation rules

1. **Idempotency:** Export batch carries `batch_uuid`; ERP ack updates **posting** row — no double post.

2. **Error taxonomy:** Map ERP HTTP codes to `rejection_code` for operator retry.

---

## Mathematical triggers

**Posted total:** \(\sum \text{labor} + \sum \text{parts}\) from WO cost tables — must match export `total_posted`.

---

## Sync transport specification

ERP payloads often **not** the same as SQLite sync — document:

| Channel | Payload |
|---------|---------|
| **Desktop → VPS ERP relay** | `{ "batch_id", "tenant_id", "lines": [...], "signature": "..." }` |
| **SQLite sync** | `integration_exceptions`, `posted_export_batches` if added |

**`[ATTENTION: REQUIRES VPS AGENT INTERVENTION]`** — Fastify route + auth.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- HTTP callback route + auth for ERP posting acknowledgements (see Phase 4 API patterns).
- Mirror posted_export_batches / integration_exceptions; reconcile with desktop batches.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 04-budget-cost-center-closure — Sprint 02 — ERP Export, Reconciliation, And Posting Integrity

**Read Only:**
- @docs/PRD.md §6.24
- @src/pages/BudgetPage.tsx
- @src-tauri/src/sync/domain.rs
- @src/services/sync-vps-transport-service.ts

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** ERP payloads often **not** the same as SQLite sync — document: Channel Payload --------- --------- **Desktop → VPS ERP relay** `{ "batch_id", "tenant_id", "lines": [...], "signature": "..." }` **SQLite sync** `integration_exceptions`, `posted_export_batches` i

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
