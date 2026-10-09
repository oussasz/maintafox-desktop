# VPS Sync Exchange — Control-Plane Contract

**Companion:** [`SYNC_STRATEGY.md`](./SYNC_STRATEGY.md)  
**Desktop contracts:** `src-tauri/src/vps/guards.rs`, `src/services/sync-vps-transport-service.ts`  
**Server implementation:** `maintafox-vendor-console/api/src/sync/` (Express; not Fastify)

**Status:** Exchange live (Step 2) — fan-out, checkpoint freeze, Postgres mirror optional, MinIO TenantMedia presign.

## Endpoint

`POST /api/v1/sync/exchange`

Headers:

- `Authorization: Bearer <activation JWT>`
- `Idempotency-Key: <uuid>`
- `Content-Type: application/json`

## Request body (v1)

```json
{
  "protocol_version": "v1",
  "idempotency_key": "<uuid>",
  "checkpoint_token": "<optional string>",
  "device_class": "desktop",
  "media_manifest": [],
  "outbox_batch": [
    {
      "idempotency_key": "<string>",
      "entity_type": "equipment",
      "entity_sync_id": "<uuid>",
      "operation": "upsert",
      "row_version": 1,
      "payload_json": "{...}",
      "payload_hash": "<64 hex sha256>"
    }
  ]
}
```

Optional additive fields: `tenant_config`, `device_class`, `media_manifest`.

## Response body

Matches desktop `ApplySyncBatchInput`:

- `protocol_version`: `"v1"`
- `server_batch_id`
- `checkpoint_token`
- `acknowledged_items`
- `rejected_items`
- `inbound_items`
- `policy_metadata_json` (optional)

## Media presign (TenantMedia)

- `POST /api/v1/sync/media/presign-put` — `{ parent_entity_type, parent_entity_sync_id, content_sha256 }`
- `POST /api/v1/sync/media/presign-get` — `{ object_key }`

Object key layout: `{env}/media/{tenant}/{parent_type}/{parent_sync_id}/{content_sha256}`  
Requires `S3_ENDPOINT` / MinIO env on the API. Returns `503 object_storage_unavailable` when unset.

## Server responsibilities (implemented)

1. Tenant isolation from activation JWT (`tenant_id` claim).
2. Idempotent accept of outbox into sync mirror (`DATABASE_URL` Postgres, else SQLite fallback).
3. Inbound fan-out: peer machines’ ingress since per-device cursor (excludes self).
4. Checkpoint advances only when the batch has **zero** permanent validation failures.
5. Never log `payload_json` for `user_accounts` (entity type only in structured logs).
6. Media: metadata in JSON / `media_manifest`; binaries via MinIO/S3 presign — not in envelopes.

## Desktop client apply path

1. `apply_sync_batch` → insert `sync_inbox` with `apply_status = 'pending'`
2. `materialize_sync_inbox` → upsert domain tables → `applied` / `conflict`
3. Transport (`exchangeControlPlaneSyncRound`) fails the round if materialize reports conflicts/errors

## Verification

| Proof | Location |
|---|---|
| Desktop two-SQLite A→B | `cargo test --lib sync::convergence_tests` |
| Golden fixtures | `src-tauri/src/sync/fixtures/exchange_*_v1.json` |
| Control-plane A→VPS→B | `maintafox-vendor-console/api`: `pnpm test:sync-exchange-ab` |

### Desktop smoke checklist (operator)

1. Point desktop `VITE_ADMIN_API_BASE_URL` at the control API (local or `https://api.maintafox.systems`).
2. Activate two machines (or two profiles) on the same tenant license.
3. Mutate a Phase-1 entity on Desktop A (e.g. create equipment) so outbox stages.
4. Run Sync Center / wait for heartbeat on A, then on B.
5. Confirm B shows the business row (code/name); header is not stuck on red « Erreur de sync » after a successful round.
6. Optional: with MinIO configured, exercise media presign put/get for a TenantMedia key.

## Env (control API)

| Variable | Role |
|---|---|
| `DATABASE_URL` | Postgres sync mirror (omit → SQLite sync tables) |
| `S3_ENDPOINT` / `S3_ACCESS_KEY` / `S3_SECRET_KEY` / `S3_BUCKET` | MinIO/S3 TenantMedia |
| `DEPLOYMENT_ENV` | Object key env segment (`dev` / `prod`) |

Compose services: `postgres`, `minio`, `minio-init` in `maintafox-vendor-console/docker-compose.yml`. See `deploy/SYNC_EXCHANGE.md` in that repo.
