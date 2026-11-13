# Sync Strategy & Schema Standard

**Status:** every registered table syncs through the generic engine (Phase 2.4, 2026-10-11); proven by the multi-device harness (Phase 2.5)  
**PRD:** §8 Sync Layer — Hybrid Cloud  
**Agent rule:** `.cursor/rules/universal-entity-sync.mdc`

## 1. Goal

All trusted devices in the same tenant share the same operational truth. Desktop machines receive the full tenant operational replica. RBAC and org scope are enforced at command/query time — not by filtering rows out of desktop sync.

Mobile (later) may use a device-class subscription filter. Synced records remain byte-compatible with desktop payloads.

## 2. Universal Entity Sync Standard

Every primary domain entity MUST carry:

| Column | Type | Rules |
|---|---|---|
| `entity_sync_id` | TEXT UUID | UNIQUE, never reused, merge key across machines. Given at insert: by the code (`sync::identity::new_entity_sync_id()`; `seeded_entity_sync_id(entity_type, key)` for rows every device creates itself, see §2.2) or, when the INSERT has none, by the capture trigger (random UUID v4). Never derived later. |
| `row_version` | INTEGER | Monotonic; increment on every durable mutation (local optimistic concurrency) |
| `updated_at` | TEXT RFC3339 UTC | Always set on mutation |

Changes are captured **by SQLite triggers inside the business transaction** (§2.1): a write that
commits is captured, a write that rolls back leaves nothing to send. Business code calls no stager.

No table may ship without:

1. An entry in the sync entity registry (`src-tauri/src/sync/registry.rs`): table, scope
   (`tenant` | `user` with owner column), conflict policy, parents. A table that must not
   replicate is listed in `LOCAL_ONLY_TABLES` with a category. The CI test
   `registry_covers_every_table` fails on any table in neither list.
2. A row spec in `src-tauri/src/sync/row_spec.rs` (§2.1). The test
   `registry_flags_match_stager_and_materializer_dispatch` fails for a registered type without one,
   and `every_row_spec_has_a_valid_layout_and_capture_triggers` fails for an INTEGER `*_id` column
   (or a foreign key to a local-only table) that has no rule.
3. A place in a domain round trip (`sync/row_sync_tests.rs`); `every_registered_type_round_trips`
   fails for a registered type in none.

### 2.1 Generic capture and apply (Phase 2.4)

- **Capture** (`sync/capture.rs`): `install_capture` (startup, before the worker; after migrations
  and seeders, which run with capture suspended) creates `sync_cap_<table>_ai|_au|_ad` triggers
  that write `sync_capture_journal` rows in the business transaction. An update is captured only
  when a synchronised column changed (device-local, derived and `row_version`-only updates stay
  local). The journal is drained into the outbox before every push, inside every inbound apply and
  before a conflict decision; a group that cannot be staged stays in the journal and becomes a
  `STAGE_ERROR` conflict. `sync_capture_suppression` (one row, set inside a transaction only)
  switches capture off for inbound apply and for data each device generates itself.
- **Payload `row.v1`** (`row_codec.rs`): every column as a sorted JSON object plus
  `_sync_format: "row.v1"`. References travel as `<column without _id>_sync_id`, app catalog
  references as `<column>_key`, BLOBs as base64. Not sent: `id`, `entity_sync_id` (in the
  envelope), `row_version` (bumped locally on apply), device-local and derived columns.
- **Rules per table** (`RowSpec`): `r` (reference), `soft` (cycle back edge, or a link to another
  person's row: written empty and completed when the target arrives, `sync_pending_refs`), `hist`
  (history: empty if the target was deleted), `poly` / `poly_hist` / `poly_soft` (target chosen by a
  type column; `NOT_A_ROW` = value sent as is, e.g. a role name; `LOCAL_ROW` = target that stays on
  each device, sent empty), `catalog` (local app catalog, sent by natural key), `unmapped` (must
  stay empty), `value_columns`, `device_local`, `derived` (column + SQL initial value, recomputed by
  `apply_hooks.rs`), `row_filter` (only matching rows sync, e.g. tenant settings), `skip_baseline`,
  `json_refs` (`json_ref(column, path, entity)`, paths `$`, `[*]`, `[*].field`, `.field`, and
  `?param` for a URL query parameter; `json_ref_hist` sends a deleted target empty).
- **Apply** (`row_codec::apply_row`): a missing parent defers the row (`MISSING_PARENT`, retried on
  every run); a missing required column fails the insert (`MATERIALIZE_ERROR` conflict); an
  append-only row that already exists is unchanged; deletes run after upserts, children first.
  Local consequences run in the same transaction (`apply_hooks.rs`): stock balances of a received
  movement, org paths, a dismissal's scope key, a report schedule's next run.
- **Baseline:** once per entity type, existing rows with a random identity, no
  `sync_entity_state` and no live outbox entry are queued (`sync_capture_baseline`).

### 2.2 Rows every device creates itself

- **Seeded rows** (catalogs, settings, roles, the admin account, the bootstrap org):
  `sync/seeded_identity.rs` gives them `seeded_entity_sync_id(entity_type, natural key)` (UUID v8),
  the same on every device (migration 162 for existing rows, `install_capture` for new ones). They
  are never baselined; an edit is sent like any change.
- **Generated per device, kept local** (generators run with capture suppressed; deterministic ids
  from sync ids, so a person's later action on one device applies to the same row elsewhere): PM
  occurrences, schedule candidates, data-integrity findings, notifications of PM due / DI SLA
  breach / escalations, un-snooze and expiry of notifications.
- **Local-only projections:** `scheduling_conflicts`, `personnel_readiness_snapshots`,
  `training_expiry_alert_events`, `inspection_reliability_signals`, `reliability_kpi_snapshots`.
- **Per user, one row per person and name:** notification preferences, saved activity filters and
  RAMS dismissals get a seeded id from the user's sync id and the name
  (`identity::user_keyed_entity_sync_id`), so the same choice made on two devices is one row.

### 2.3 Adding a table

1. Migration: `entity_sync_id TEXT` + unique index (or a natural primary key).
2. Registry entry (scope, owner column, conflict policy, parents) and a `RowSpec` with a rule for
   every local id (columns, JSON paths, URLs).
3. Add the type to a list in `row_sync_tests.rs`; run
   `cargo test --lib -- sync::registry_tests sync::row_sync_tests` with `MAINTAFOX_WRITE_SYNC_REGISTRY=1`.
4. `node scripts/sync-allowlist-append.mjs` (appends to the vendor console registry and refreshes
   `shared/sync/server-entity-allowlist.v2.json`), then the vendor test
   `pnpm --filter maintafox-control-api run test:sync-v2`.

The registry is exported to `shared/sync/entity-registry.json` (test
`registry_export_matches_shared_file`; regenerate with `MAINTAFOX_WRITE_SYNC_REGISTRY=1`). The
control-plane allowlist is vendored in `shared/sync/server-entity-allowlist.v2.json`; every staged
entity must be in it (test `staged_entities_are_in_the_server_allowlist`).

Local INTEGER PKs are device-local. Rename legacy `sync_id` → `entity_sync_id` (no dual-read aliases).

## 3. Protocol

- **Version:** `v2` only since Phase 2.3 (contract: `maintafox-vendor-console/deploy/SYNC_PROTOCOL_V2.md`).
  The server keeps v1 for desktops that are not updated; nothing in this app speaks v1 any more
  (`apply_sync_batch` still accepts v1 input for the replay/repair tools and old fixtures).
- **Exchange:** `POST /api/v1/sync/exchange`; **bootstrap:** `POST /api/v1/sync/snapshot` (paged);
  **media:** `POST /api/v1/sync/media/presign-put` then `PUT` to the presigned URL (upload),
  `POST /api/v1/sync/media/presign-get` then `GET` (download). Control API calls carry
  `Authorization: Bearer <activation token>`; the presigned URL is the only credential for object
  storage.
- **Envelope:** `entity_type`, `entity_sync_id`, `operation` (`create`\|`update`\|`delete`\|`upsert`\|`archive`), `row_version`, `base_row_version` (explicit `null` for a create), `owner_user_sync_id` (user scope), `payload_json`, `payload_hash` (SHA-256 hex)
- **Checkpoint:** signed and opaque (`v2.…`); stored by `apply_sync_batch` in the same transaction
  that writes the answer to the outbox/inbox, i.e. only once the answer is durable. A v1 token
  (`cp-…`) is treated as "no checkpoint" (bootstrap). v2 item failures never hold the cursor: an
  invalid inbound envelope becomes an `INVALID_INBOUND_PAYLOAD` conflict and the token advances.

### Transport: the Rust sync worker (Phase 2.3)

`src-tauri/src/sync/transport/` (`worker.rs`, `client.rs`, `store.rs`, `wire.rs`) runs as background
task `sync` in `BackgroundTaskSupervisor`, started at the end of the startup sequence. Closing the
main window only hides it in the tray (`window.rs`), so the process and the worker keep running; the
worker stops when the app quits (tray "Quit") through the supervisor shutdown. HTTP: `reqwest` with
rustls/ring (same build as the updater; no OpenSSL/native-tls) and the system proxy settings. Base URL: `VITE_ADMIN_API_BASE_URL`
resolved at build time by `src-tauri/build.rs` like Vite does for the webview, else
`shared/control-plane.json` (the webview reads the same file). `http://` is accepted only for
localhost.

One cycle:

1. Stop when paused (`sync_transport_state.paused_until`), when the entitlement does not allow the
   `sync` capability, when the activation is not active, or when the server refused this activation
   token (its SHA-256 is stored, never the token; a new token, e.g. after the licence check
   re-claims, resumes sync).
2. No v2 checkpoint → scope `all` snapshot (pages of `SYNC_SNAPSHOT_PAGE_SIZE`; each page is stored
   in the inbox; the last page's checkpoint is stored with the last page; then the whole inbox is
   materialized once, parents first). A refused page token restarts the snapshot (bounded).
3. Exchange loop. A new request is built from `get_sync_push_payload` (≤ `SYNC_OUTBOX_BATCH_SIZE`
   items), `current_user_sync_ids`, `device_class: "desktop"`, `tenant_config` and the media
   manifest (entries outside the activated tenant are left out: the server would refuse the whole
   request), then **stored** (`idempotency_key`, exact body, outbox ids) before it is sent. A resend
   (failure, crash, restart) sends the stored key and body unchanged. On success the answer is applied
   (acks store `server_seq` and `duplicate`; `conflict_items` become `SERVER_CONFLICT`), the stored
   request is cleared, the inbox is materialized and users in `user_snapshot_required` get a scope
   `user` snapshot with the same user list (its checkpoint is ignored). The loop continues while
   `has_more` or the batch was full.
4. Media: before each exchange, objects recorded from earlier manifests (`sync_media_uploads`) are
   uploaded: local file under the app data dir, hash verified, presign, `PUT`. Failures never block the
   exchange; they are retried and give up after `OUTBOX_MAX_DELIVERY_ATTEMPTS` (shown in the status).
5. At the end of a cycle that synced, files received from other devices (`sync_media_downloads`)
   are downloaded (§6).

Failures (`classify` in `worker.rs`):

| Answer | Outbox items | Worker |
|---|---|---|
| No connection (DNS, refused, connect timeout) | back to `pending`, attempt not counted | `offline`; backoff; a webview `online` event wakes it (not sooner than `SYNC_NETWORK_WAKE_MIN_SPACING`) |
| 429 | back to `pending`, attempt not counted | `retrying`; waits `Retry-After` (capped at 1 h) |
| 5xx, 408, interrupted, malformed 2xx | `record_outbox_delivery_failure` (counted; `dead_letter` + conflict after `OUTBOX_MAX_DELIVERY_ATTEMPTS`) | `retrying`; same key and body next time; rebuilt without dead-lettered items |
| 401 / 403 | back to `pending`; stored request kept | `blocked` (`activation_refused`); the UI starts a licence check |
| 409 `tenant_config_mismatch` | back to `pending` | `blocked` until the activation changes |
| 400 `invalid_checkpoint_token`, 409 `checkpoint_ahead_of_server` | back to `pending` | checkpoint cleared, snapshot, exchange again (once per cycle) |
| 409 `idempotency_key_reused` | back to `pending` | logged as an error (bug signal), shown; rebuilt with a new key after the backoff |
| other 4xx (`validation_error`, …) | counted as a delivery attempt | `error`; the request is rebuilt after the backoff, items dead-letter after the limit |

Backoff: `SYNC_RETRY_BASE_DELAY` (2 s) doubled per consecutive failure, capped at
`SYNC_RETRY_MAX_DELAY` (5 min), ±25 % jitter; there is no retry budget to exhaust. Idle interval
`SYNC_IDLE_INTERVAL` (60 s). All constants are in `worker.rs`.

Status: `get_sync_worker_status` and the `sync-worker-status` event (state, blocker, last success,
last attempt, next attempt, pending/dead-letter/conflict counts, media counts, last error). Commands:
`run_sync_now` (`sync.view`), `pause_sync` / `resume_sync` (`sync.manage`, pause 1–1440 min, persisted),
`report_network_online` (no data, no session needed). The activation token is read in Rust
(`sync_activation_bearer_token`) and no command returns it for sync.

### Outbox versioning (protocol v2 rules, applied locally since Phase 2.2)

`stage_outbox_item` assigns, for every new envelope:

- `idempotency_key`: a fresh UUID (unique per tenant forever; retries resend the same envelope);
- `base_row_version`: the version of the previous unsent change of the entity, else the version the
  server is known to hold (`sync_entity_state`, updated on ack and on inbound apply), else `NULL`
  (the row is created here);
- `row_version` = `base_row_version + 1`, or `1` for a create;
- `owner_user_sync_id` for user-scoped entities (from the registry owner column);
- `origin_machine_id` = `sync_replica_identity.replica_id` (one id per local database).

Restaging an unchanged payload while the previous change is unsent returns that change.
Outbox status: `pending` → `in_flight` (sent, not acknowledged; sent again on the next exchange) →
`acked` | `rejected` | `conflict` (v2 server conflict, waits for a person) | `dead_letter` (after
`OUTBOX_MAX_DELIVERY_ATTEMPTS` transport failures, listed as a conflict) | `superseded` (replaced by
a person's conflict decision).

### Ingest vs apply

1. Exchange inserts inbound items into `sync_inbox` with `apply_status = 'pending'`
2. `materialize_inbox` applies them and sets (pending rows are ordered parents first by
   `registry::apply_priority` once per call and applied in chunks of `SYNC_MATERIALIZE_BATCH`, one
   transaction per chunk with a savepoint per row; deferred rows are then swept the same way until
   a sweep applies nothing; reference lookups are cached for the chunk):
   - `applied`: written, or already current (own change echoed back, same version and content);
   - `deferred`: unknown entity type (another app version) or a parent not received yet
     (`MISSING_PARENT`, also for ids inside JSON/URL columns); deferred rows are retried on every run
     and applied once possible;
   - `conflict`: concurrent edit, stale or divergent version, or materializer error; a
     `sync_conflicts` record is written in the same transaction.
3. Never mark `applied` on insert alone, and never for a row that was not applied.

## 4. Apply order

The apply order is computed from the registry `parents` (an entity is applied after all of its
parents; `registry::apply_priority`). The CI tests check that the parent graph has no cycle and that
every declared SQLite foreign key between replicated tables is listed as a parent. A child whose
parent has not arrived yet is deferred and retried, so cross-batch order does not lose links.

## 5. Conflict policy

Owner decision (2026-10-10): a person always decides; last-write-wins is never applied silently.

| Class (`ConflictPolicy`) | Policy |
|---|---|
| `operator_review` (mutable rows) | Inbound change while a local change is unsent → `CONCURRENT_EDIT`; older than applied → `STALE_INBOUND`; same version, other content → `EQUAL_VERSION_DIVERGENCE`; v2 server refusal → `SERVER_CONFLICT`. All go to the Sync Center. |
| `append_only` (history rows, stock movements, attachments) | Never overwritten; a replayed row is a no-op |

Resolution acts on data in the same transaction that closes the conflict:

- **Keep mine** (`accept_local`): the server version becomes the base, unsent local changes are
  superseded, and the current local row is staged again (`base_row_version` = server version).
- **Take theirs** (`accept_remote`): unsent local changes are superseded and the server payload is
  applied through the materializer. Not available when the conflict carries no server payload
  (v1 rejections).
- `dismiss`, `retry_later`, `escalate` only change the conflict status.

## 6. Media

- Entity type: `media_attachments` (one row per stored file; parent = the record it belongs to:
  equipment photo, WO/DI attachment, library document)
- Columns that travel: parent type and sync id, `content_sha256`, `mime_type`, `byte_size`,
  `object_key` (`{env}/media/{tenant}/{parent type}/{parent sync id}/{sha256}`), `file_name`,
  `relative_path`, `capture_meta_json`. `relative_path` is the file's place under the app data
  directory (`wo_attachments/…`, `di_attachments/…`, `asset_photos/…`, `library_documents/…`, unique
  by a UUID); the attachment row names the same path, so the file resolves on every device.
- JSON outbox never carries file bytes.
- **Upload** (device that stored the file): the media row is announced in the exchange's
  `media_manifest` (`sync_media_uploads`), the worker uploads the bytes through `presign-put`.
- **Download** (every other device, Phase 2 leftover 1, migration 165): applying an inbound
  `media_attachments` row queues it in `sync_media_downloads` (`apply_hooks`). At the end of a cycle
  that synced, the worker takes up to `SYNC_MEDIA_DOWNLOADS_PER_CYCLE` due files: `presign-get`,
  `GET` streamed to a temporary file next to the target, size and SHA-256 checked while reading
  (more bytes than announced stops the download), then the file is moved into place. A file that
  is already present with the right hash counts as downloaded; a different file at that place is
  never overwritten (`failed`). Only paths inside the four media directories are written
  (`media::media_storage_path`).
- **Failures:** no connection or a refused activation → stop, nothing counted; object not on the
  server yet (404: the other device has not uploaded it) → `pending`, retried with backoff
  (`SYNC_MEDIA_RETRY_BASE_DELAY` 30 s doubling to `SYNC_MEDIA_RETRY_MAX_DELAY` 1 h), never given up;
  other errors (5xx, interrupted, size or hash mismatch) → retried with the same backoff and
  `failed` after `OUTBOX_MAX_DELIVERY_ATTEMPTS`; impossible cases (path outside the media
  directories, no storage location from an older app version) → `failed` at once.
- **UI** (`components/sync/MediaFileStatus.tsx`, FR/EN): attachment lists, the photo gallery and
  the document library show "not downloaded yet", "download failed" with the reason and a retry
  button (`retry_media_download`), or "file not on this device", instead of a broken file
  (`get_media_file_states`). The Sync Center status shows the download counts.
- **Document library** syncs since this change (`library_documents` row + its media row; existing
  documents get their media row at startup, `media::backfill_library_document_media`).
  **Archives** (`archive_*`) stay device-local: no production code writes them yet and their
  snapshots hold local ids.
- Harness: the multi-device harness runs a local S3-compatible object store (path-style bucket,
  presigned PUT/GET; it checks the SigV4 query fields, the access key and the expiry, but does not
  recompute the signature). Real MinIO is used only on the server.

## 6a. Record numbers across devices (Phase 2 leftover 2)

Work orders (`OT-…`), intervention requests (`DI-…`), personnel (`PER-…`) and permits (`PTW-…`)
are numbered on the device that creates them, offline. New numbers carry a 4-character device
code: `OT-7KQ2-0001`, `DI-7KQ2-0001`, `PER-7KQ2-0001`, `PTW-2026-7KQ2-000001` (`sync/device_code.rs`).

- Device code: first 20 bits of the SHA-256 of the database's replica id
  (`sync_replica_identity`) in Crockford base 32 (no I, L, O, U). Stable for the life of the
  database, no network needed. A reset of local data makes a new replica, hence a new code.
- The sequence of a device counts only numbers with its own prefix, so two devices with different
  codes never produce the same number. About 1 chance in 2 400 that two of 30 devices share a code
  (then the clash check below catches it).
- Numbers made before this version keep their form (`OT-0012`): printed sheets and QR codes stay
  valid. Codes typed by a person (personnel) are not changed.
- **Clash check** (`sync/code_clash.rs`): before an inbound create/update of a numbered table is
  applied, its number is looked up locally. Used by another record → `CODE_CLASH` conflict (nothing
  renamed). Sync Center (FR/EN) explains it and offers one action, "renumber the record on this
  device" (`resolve_sync_conflict` action `renumber_local`): the local holder gets a new number of
  this device (sent as a normal edit, recorded in `audit_events` as `sync.code_clash.renumbered`),
  then the received record is applied with its own number. If the received record's newer version
  (already renumbered on its device) arrives, the older clash closes itself; nothing else is
  decided automatically.
- Migration 166 re-types the unique-number failures already recorded as `MATERIALIZE_ERROR` into
  `CODE_CLASH`.

## 6b. Ids inside history rows (Phase 2 leftover 3)

History rows (audit, activity, admin changes, request changes, schedule changes, notification
events) hold local ids in text and JSON columns. Since migration 167 those ids travel as sync ids
through the row rules and each device stores its own local id, so the entries read correctly
everywhere:

- `audit_events.target_id` by `target_type` (user, role assignment, work order; device, licence,
  activation, export, repair, import and sync-conflict targets stay on their device and are sent
  empty); `details_json` `.detail.role_id`, `.detail.assignment_id`, `.filter.actor_id`.
- `activity_events.summary_json` (role, assignment, break-in, commitment, assignee, team, PM
  occurrence and plan, linked work order); `correlation_id` of new PM and break-in events uses the
  record's sync id (grouping no longer mixes devices).
- `admin_change_events.scope_reference` by `scope_type`; `diff_json` `.role_id`, `.assignment_id`.
- `di_change_events.details_json.wo_id`; `schedule_change_log.details_json` ids;
  `notification_events.payload_json` ids; `notification_events.dedupe_key` is device-local (it is
  built from local ids and would suppress unrelated events on another device).
- `inventory_state_events` of articles (`ARTICLE`) were not sendable at all (unmapped type): fixed.
- **Old rows:** rows received from another device before this version keep that device's ids,
  which cannot be mapped back. They are listed in `sync_history_foreign_rows` and the audit log,
  activity feed and admin history show "from another device" instead of a wrong reference.
  Unsent old-format history changes are superseded and requeued in the new format.
- **Not translatable (free text):** ids written inside sentences (some summaries, notification
  bodies, schedule `old_value`/`new_value`), `wo_action_events` payloads (two writers store plain
  text), `data_integrity_repair_actions` snapshots and `org_change_events` before/after (nested,
  polymorphic), `budget_alert_events.dedupe_key`. None of these is used as a link.

## 7. Coverage (Phase 2.4)

Every table in the registry syncs; the rules are in `row_spec.rs`, in this order:

| Domain | Types (examples) | Notes |
|---|---|---|
| Foundation | org model/nodes, users, roles, permissions, scopes, personnel, teams, equipment, reference catalogs, media metadata | Seeded rows share identities; password hashes travel, PIN/lockout/login times stay device-local |
| Inventory / procurement | articles, warehouses, locations, movements, reservations, requisitions, POs, receipts, repairables, counts | A received movement updates `stock_balances` on apply |
| Work orders / requests | DI, WO and their tasks, parts, labour, delays, downtime, closeout, attachments | SLA "notified" stamps device-local |
| PM / planning | plans, versions, occurrences, executions, findings, schedule commitments, break-ins | Occurrences and candidates generated per device (deterministic ids) |
| Finance | cost centers, budgets, actuals, forecasts, alerts, exports | Alert → notification link is soft |
| Personnel / qualification | availability, skills, certifications, qualification profiles | Readiness snapshots and expiry alerts local-only |
| Inspections / permits | rounds, checkpoints, results, anomalies, permits, isolations | Checkpoint packages translate asset ids |
| RAMS | failure hierarchies/codes/events, runtime exposure, FMECA, RCM, FTA, RBD, event trees, Markov, Monte Carlo, Weibull, sign-offs, Ishikawa, guardrails, benchmarks, dashboard policies, data integrity findings and repairs, analytics contract | KPI snapshots local-only; findings recomputed per device |
| Settings / history / notifications / reports | tenant app settings, policy snapshots, retention policies, activity/audit/admin/settings events, notification categories, rules, escalation, events, notifications, acknowledgements, report templates and schedules | Device settings stay local; notifications per recipient |
| Personal items | notification preferences, saved activity filters, dashboard layouts, RAMS dismissals | Only to the devices where the owner is signed in or trusted |

## 8. Non-sync (local-only)

`LOCAL_ONLY_TABLES` in `registry.rs`, by category: sync machinery and the capture tables; sessions,
trusted devices, device configuration; backups; licence and entitlement state; secrets and
connection profiles; import staging; job queues, report runs and artifacts, notification
deliveries; derived projections (§2.2); app catalogs; legacy stubs; archives and event links
(`archive_*`, `event_links`: no production writer yet; their snapshots and links hold local ids).

## 9. Client vs VPS ownership

| Layer | Owner |
|---|---|
| SQLite schema, outbox staging, materializer | Desktop (`src-tauri/src/sync/`) |
| Sync transport (exchange, snapshot, media upload) | Desktop Rust worker (`src-tauri/src/sync/transport/`) |
| Sync status, pause/resume, conflict UI | Desktop webview (`src/stores/sync-orchestrator-store.ts`, `src/components/sync/`) |
| Route contracts / object taxonomy | Desktop contracts (`src-tauri/src/vps/`) |
| Postgres mirror, Fastify exchange, MinIO | Control-plane VPS repo (implements same envelopes) |

See also: [`VPS_SYNC_EXCHANGE.md`](./VPS_SYNC_EXCHANGE.md) for the frozen Fastify request/response contract.

## 10. Verification

- Round trips per domain and for the whole registry (`sync/row_sync_tests.rs`, in-process server
  with the v2 rules in `sync/test_support.rs`): `cargo test --lib -- sync::row_sync_tests`.
- Registry, layouts, triggers, seeded identities, allowlist: `cargo test --lib -- sync::registry_tests`.
- **Multi-device harness (Phase 2.5):** `pnpm sync:harness`. It builds the control API of
  `../maintafox-vendor-console`, starts embedded PostgreSQL 16 and the API on 127.0.0.1, activates
  three machines, and runs `sync::harness_tests` (three desktop databases, real worker, real HTTP):
  create, concurrent edit resolved both ways, offline then reconnect, per-user rows, delete,
  snapshot bootstrap of a new device, lost answer then resend applied once, backup restore then
  snapshot, library files through object storage (also after a delayed upload), revoked device. A few minutes with a warm Rust build (`--skip-api-build` reuses the API
  build).
- **Mobile sync proof (Android field app, step 0):** `pnpm sync:harness --mobile-proof`, with
  `MAINTAFOX_VENDOR_CONSOLE_DIR` on a control API that has the mobile device class. Licence with
  `max_machines: 2, max_mobile_devices: 1`; API on 127.0.0.1:47831 (`MAINTAFOX_MOBILE_PROOF_PORT`),
  reached by a USB phone through `adb reverse tcp:47831 tcp:47831`. `sync::mobile_proof_tests` is the
  desktop (encrypted database file, production sync access): activation, plant join (first pull,
  then first administrator), a Maintenance Technician account (generated password, stdout only),
  one equipment, one work order assigned to the technician, then `MOBILE_PROOF_READY`. It waits
  for a request reported on the phone (`MAINTAFOX_MOBILE_PROOF_WAIT_SECS`, default 1200 s), prints
  `MOBILE_PROOF_DI` with its uplink time (phone `created_at` to arrival, so phone clock included),
  marks the work order ready and prints `MOBILE_PROOF_DOWNLINK`; the API stays up
  `MAINTAFOX_MOBILE_PROOF_HOLD_SECS` (default 300 s) for the phone to pull it. The downlink arrival
  is checked on the phone.
- Golden fixtures: `src-tauri/src/sync/fixtures/exchange_request_v1.json`, `exchange_response_v1.json`
- `cargo check`, `pnpm typecheck`, `cargo test --lib sync::` / `vps::`
- Transport: `cargo test --lib -- sync::transport` (worker against an in-process HTTP stub: push/pull,
  paging, crash resend, 409, checkpoint refusal → snapshot, conflicts, 429/5xx backoff, offline →
  resume, refused activation, dead letter, per-user fields, pause, media upload, media download
  with not-yet-uploaded / wrong-content / outside-path cases)

## 11. Backup restore

A restored database is older than the server. After swapping a restore in, startup
(`startup::run_startup_sequence`) calls `transport::store::forget_server_position`: the checkpoint
and the unanswered request are dropped, so the worker bootstraps from a full snapshot and gets back
the rows this device wrote after the backup (the server never echoes a device's own changes).
Unsent changes of the backup stay queued; the server acknowledges the ones it already has as
duplicates. Harness scenario 8 checks it.

## 12. Large tenants (Phase 2 leftover 4)

Measured with `pnpm sync:harness --large N` (`sync::harness_tests::large_tenant`), on the
development PC (Windows 11, local embedded PostgreSQL 16, control API on 127.0.0.1): device A
creates one work order with its request, tasks, labour, part and history (20 rows with their
shared parents' children), the harness clones them N times in the server's current state (new
sync ids and numbers), then a new device C — an encrypted database file opened like the app opens
it — bootstraps from the snapshot. "Rows" are rows of the cloned types; parents (equipment,
people, catalogs) are shared.

**Before** (the code at the start of this work):

| Tenant | Result |
|---|---|
| 1 206 work orders, 4 234 rows | Snapshot received, but 4 202 rows were never applied: rows waiting for a parent kept the first 500 places of the inbox queue and every pass re-read them, so later rows (their parents included) were never reached. |
| > 60 000 rows | The snapshot could never finish: past ~120 pages the server's per-device limit (120 requests/min) answers 429 and the next cycle restarted the snapshot from page 1. |
| 1 206 work orders, page by page fixed but applied per page | 61.8 s (each row tried about 3 times: pages come in entity-type order, children before parents). |

**After:**

| Tenant | Rows | Bootstrap | Of which 429 waits | Received | Peak memory | Device DB file | Conflicts / rows left |
|---|---|---|---|---|---|---|---|
| 1 206 WO | 4 234 | 10.1 s | 0 | 8 MB | 120 MB | 29 MB | 0 / 0 |
| 12 006 WO | 40 234 | 91 s | 0 | 80 MB | 134 MB | 187 MB | 0 / 0 |
| 48 006 WO | 160 234 | 7.0 min | 21 s | 322 MB | 144 MB | 712 MB | 0 / 0 |
| **100 008 WO** | **333 574** | **15.5 min** | 47 s | 671 MB | 202 MB | 1 473 MB | 0 / 0 |

- **Server:** building the 333 574-row tenant: 97 s (bulk insert); current-state table 914 MB
  (530 MB of payloads). Snapshot page query (`listSnapshotPage`, 1 000 rows): 6.5 ms first page,
  6.1 ms middle page (primary-key range scan; 0.5–1 ms at 40 000 rows). No server change needed.
- **Steady state** on the 100 008-WO database: one change pushed by A in 49 ms, received and
  applied by C in 35 ms (25–70 ms at every size).
- **What changed:** snapshot pages are stored first and applied once after the last page, parents
  first, each row tried once; pending and waiting rows are ordered once per call and read by id
  chunks (no inbox scans); one transaction per 500-row chunk with a savepoint per row; reference
  lookups cached per chunk; indexes on `sync_inbox(apply_status, id)` and
  `sync_pending_refs(target_entity_type, target_sync_id)` (each applied row looked for waiting
  links by a full table read); the number-clash bookkeeping reads its (usually empty) conflicts
  first; snapshot pages of 1 000 rows (server maximum); the position of an unfinished snapshot is
  kept (`sync_transport_state.snapshot_*`, migration 168), so a 429, a restart or a crash
  continues from the next page.
- **Not improved here:** the received copy of every applied row stays in `sync_inbox`, which
  roughly doubles the device database during a bootstrap (1.47 GB for 100 000 work orders).
  Compacting applied inbox rows is a follow-up. Apply rate is ~400 rows/s on this PC with
  SQLCipher; a slower field PC will take proportionally longer (the worker runs in the
  background and the app stays usable).


## 13. Work-order lifecycle: 8 stored phases, 12 derived states (Phase 6.4)

PRD §6.5 describes 12 work-order **statuses**. The desktop deliberately stores 8 phases
(`draft`, `planning`, `ready`, `in_progress`, `on_hold`, `completed`, `closed`, `cancelled`;
migration 130, "Option B") and derives the PRD's 12 states from facts. This deviates from the
PRD wording on purpose:

- One stored status per phase keeps the state machine, permissions and sync payloads simple; the
  finer PRD states are facts of the work order (approval, schedule, assignee, verification, kind
  of hold), and storing them twice would let the two disagree after an offline edit.
- The derived state depends **only on the work order's own row and its delay segments** — never on
  tenant settings or other modules (inventory, permits…). It therefore only changes when that
  work order is edited, and it never causes synchronised writes by itself.
- Nothing derived is synchronised: `wo::lifecycle::derive` (Rust) computes it on read; the SQL
  mirror (`SQL_CASE`, list filters and dashboard counts) and the TypeScript mirror
  (`src/utils/wo-lifecycle.ts`) are checked against it for all 768 fact combinations
  (`shared/wo/lifecycle-cases.json`). The transition log keeps the state reached at each
  transition as history (`wo_state_transition_log.lifecycle_state`, a snapshot, never re-derived).

Facts added by migration 175 (synchronised with their tables):

- `work_orders.planning_approval_required`: copied from the tenant setting
  `wo_planning_approval_required` when the work order enters planning. Changing the setting later
  only affects work orders that enter planning afterwards (intended: no mass re-derivation).
- `work_order_delay_segments.hold_kind` (`pause` | `prerequisite`, never NULL): written by the
  pause and hold actions. The materializer refuses any other value (conflict `INVALID_VALUE`) and
  defers a row without it (`MISSING_FIELD`: sent by an older app version) rather than guessing,
  because the kind decides between "paused" and "waiting for prerequisite".
