# Sync Strategy & Schema Standard

**Status:** Phase 1 foundation (multi-desktop + VPS domain sync)  
**PRD:** §8 Sync Layer — Hybrid Cloud  
**Agent rule:** `.cursor/rules/universal-entity-sync.mdc`

## 1. Goal

All trusted devices in the same tenant share the same operational truth. Desktop machines receive the full tenant operational replica. RBAC and org scope are enforced at command/query time — not by filtering rows out of desktop sync.

Mobile (later) may use a device-class subscription filter. Synced records remain byte-compatible with desktop payloads.

## 2. Universal Entity Sync Standard

Every primary domain entity MUST carry:

| Column | Type | Rules |
|---|---|---|
| `entity_sync_id` | TEXT UUID | UNIQUE, never reused, merge key across machines |
| `row_version` | INTEGER | Monotonic; increment on every durable mutation |
| `updated_at` | TEXT RFC3339 UTC | Always set on mutation |

Every durable mutation MUST call `stage_outbox_item` with a typed payload.

No domain may ship without:

1. `SYNC_ENTITY_*` registry key
2. Versioned payload with **`entity_sync_id` FKs only** (never local `i64`)
3. Inbox materialization handler that upserts the real domain table

Local INTEGER PKs are device-local. Rename legacy `sync_id` → `entity_sync_id` (no dual-read aliases).

## 3. Protocol

- **Version:** `v1` (additive fields preferred over breaking `v2`)
- **Exchange:** `POST /api/v1/sync/exchange` (Bearer activation JWT + Idempotency-Key)
- **Envelope:** `entity_type`, `entity_sync_id`, `operation` (`create`\|`update`\|`delete`\|`upsert`\|`archive`), `row_version`, `payload_json`, `payload_hash` (SHA-256 hex)
- **Checkpoint:** Replay-safe `checkpoint_token`; do not advance when materialize conflicts remain unresolved

### Ingest vs apply

1. Exchange inserts inbound items into `sync_inbox` with `apply_status = 'pending'`
2. `materialize_inbox_batch` upserts domain tables, then sets `applied` / `conflict`
3. Never mark `applied` on insert alone

## 4. Apply order

Dependency-safe materialization order:

1. Org structure / node types
2. Users / RBAC (roles, permissions bindings, scope)
3. Personnel / teams
4. Equipment (+ meters)
5. Reference catalogs (by code)
6. Inventory (articles, locations, movements)
7. Intervention requests (DI)
8. PM plans / versions / occurrences
9. Work orders + children (tasks, labor, attachments metadata)
10. Inspection / permits / budget / reliability events
11. Media attachment metadata (binaries via object store)

## 5. Conflict policy

| Class | Policy |
|---|---|
| Mutable operational (WO status, DI fields) | Higher `row_version` wins when checkpoint is newer; equal version → operator review via `sync_conflicts` |
| Append-only (labor, attachments, execution events, stock movements) | Never merge-overwrite; insert by `entity_sync_id` |
| Authority mismatch | Route to Sync Center for operator resolution |

## 6. Media

- Entity type: `media_attachments`
- Envelope fields: `entity_type`, `parent_entity_sync_id`, `content_sha256`, `mime_type`, `byte_size`, `object_key`, `capture_meta`
- Object store category: `TenantMedia` under tenant-scoped prefix
- JSON outbox MUST NOT carry file bytes
- Desktop stores files under app data; sync uploads/downloads by content hash

## 7. Phase 1 entity coverage

| Domain | Sync entity key(s) | Notes |
|---|---|---|
| Equipment | `equipment`, `equipment_meters` | Rename `sync_id` |
| Users / RBAC | `user_accounts`, `roles`, `user_role_assignments`, `user_scope_assignments` | Password hashes travel (secret); PIN stays device-local |
| Org | `org_nodes`, `org_structure_models`, `org_node_types` | Required for routing |
| Personnel | `personnel`, `teams`, `personnel_team_assignments` | |
| DI | `intervention_requests`, `di_attachments` (via media) | Full payload |
| WO | `work_orders`, `work_order_tasks`, `work_order_interveners`, `work_order_attachments` (media) | Full graph |
| PM | `pm_plans`, `pm_plan_versions`, `pm_occurrences` | |
| Inventory | `articles`, `warehouses`, `stock_locations`, `stock_movements`, `stock_reservations` | Movements append-only |
| Media | `media_attachments` | Metadata + object store |
| Reference | `reference_values` (or domain codes) | Stable by code |
| Existing | permits, inspections, budget, reliability, … | Keep staging; add materializers |

## 8. Non-sync (not primary entities)

- Sync-control tables (`sync_outbox`, `sync_inbox`, `sync_checkpoint`, …)
- Device-local: `trusted_devices`, PIN hashes, OS keyring secrets, session tokens
- Derived: computation jobs, plot caches, dashboard layouts (unless explicitly promoted)
- High-churn UX: notification read state, activity feed (Phase 1 defer)
- Vendor/admin control-plane only data

## 9. Client vs VPS ownership

| Layer | Owner |
|---|---|
| SQLite schema, outbox staging, materializer | Desktop (`src-tauri/src/sync/`) |
| Exchange HTTP client + Zod | Desktop (`src/services/sync-vps-transport-service.ts`) |
| Route contracts / object taxonomy | Desktop contracts (`src-tauri/src/vps/`) |
| Postgres mirror, Fastify exchange, MinIO | Control-plane VPS repo (implements same envelopes) |

See also: [`VPS_SYNC_EXCHANGE.md`](./VPS_SYNC_EXCHANGE.md) for the frozen Fastify request/response contract.

## 10. Verification

- Two-DB convergence tests: outbox on A → exchange fixture → materialize on B → domain row present (`src-tauri/src/sync/convergence_tests.rs`)
- Golden fixtures: `src-tauri/src/sync/fixtures/exchange_request_v1.json`, `exchange_response_v1.json`
- `cargo check`, `pnpm typecheck`, `cargo test --lib sync::` / `vps::`
- Sync Center / transport: exchange OK is insufficient — materialize conflicts/errors fail the round (`exchangeControlPlaneSyncRound`)

## 11. Staging coverage matrix (Phase 1)

Prefer post-commit `warn_stage` / `foundation_stage::warn_stage` — never fail the business write on staging errors.

| Entity | Stage helper | Primary mutation entry points | Materializer |
|---|---|---|---|
| `equipment` | `wo::sync_stage` / asset paths | asset create/update/photos | `upsert_equipment` |
| `org_nodes` | `stage_org_node_sync` | create / move / deactivate | `upsert_org_node` |
| `user_accounts` / `roles` | `stage_user_account_sync` / `stage_role_sync` | admin user/role create/update | `upsert_user_account` / `upsert_role` |
| `personnel` / `teams` | `stage_personnel_sync` / `stage_team_sync` | personnel/team create/update/deactivate | `upsert_personnel` / `upsert_team` |
| `reference_values` | `stage_reference_value_sync` | reference create/update | `upsert_reference_value` |
| `intervention_requests` | `ops_stage::stage_intervention_request_sync` | DI create/update/review/triage/convert | `upsert_intervention_request` |
| `work_orders` | `wo::sync_stage::stage_work_order_by_id` | create/update/cancel/workflow/execution/closeout/planning sync | `upsert_work_order` |
| `work_order_tasks` | `stage_work_order_task_sync` | `wo/tasks.rs` | `upsert_work_order_task` |
| `work_order_interveners` | `stage_work_order_intervener_sync` | `wo/labor.rs` | `upsert_work_order_intervener` |
| `articles` / `warehouses` / `stock_locations` | `stage_*_sync` | inventory CRUD | matching upserts |
| `inventory_transactions` | `stage_stock_movement_sync` | adjust/reserve/issue/return/transfer/procurement/controls | `upsert_stock_movement` (append-only) |
| `stock_reservations` | `stage_stock_reservation_sync` | reservation create/release | `upsert_stock_reservation` |
| `pm_plans` / versions / occurrences | `stage_pm_plan_sync` / `stage_pm_plan_version_sync` / `stage_pm_occurrence_sync` | PM lifecycle in `pm/queries.rs` | matching upserts |
| `media_attachments` | `sync::media::stage_media_bytes` (+ delete op) | DI/WO/asset photo save/delete | `upsert_media_attachment` |

**Explicitly out of Phase-1 staging (follow-on):** availability blocks, suppliers/procurement graphs beyond stock events, finance/reliability materializer backlog, activity feed, PIN/device secrets.
