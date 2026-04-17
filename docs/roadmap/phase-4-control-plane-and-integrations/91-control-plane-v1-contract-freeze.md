# Control-Plane v1 Contract Freeze

**Status:** Frozen  
**Frozen on:** 2026-04-17  
**Owner:** Cursor Agent (application contracts) + VPS Agent (deployment/runtime conformance)  
**Source of truth:** API implementation in `maintafox-vendor-console/api/src/index.ts` and web/desktop consumers.

---

## 1) Freeze Rules

- No breaking changes to request/response field names, enum/error codes, or auth behavior in `v1`.
- Additive changes are allowed only with backward compatibility.
- Any breaking change requires `v2` route family and migration notes.
- All production consumers (vendor console + desktop activation path) must stay aligned to this freeze.

---

## 2) Authentication And Session Contracts

### `POST /api/v1/auth/login`
**Request**
```json
{ "email": "string(email)", "password": "string" }
```

**Response 200**
```json
{ "access_token": "jwt", "token_type": "Bearer", "expires_in": 43200 }
```

**Errors**
- `400 invalid_body`
- `401 invalid_credentials`

### `GET /api/v1/me/permissions` (Bearer required)
**Response 200**
```json
[
  {
    "name": "console.view",
    "description": "console.view",
    "category": "vendor",
    "is_dangerous": false,
    "requires_step_up": false
  }
]
```

**Errors**
- `401 missing_token`
- `401 invalid_token`

---

## 3) Tenant Contracts (Bearer required)

### `GET /api/v1/tenants`
Returns tenant rows:
- `id: string`
- `slug: string`
- `display_name: string`
- `force_min_app_version: string | null`
- `is_active: number`
- `created_at: string`

### `POST /api/v1/tenants`
**Request**
```json
{
  "slug": "lowercase-slug",
  "display_name": "Tenant Display Name",
  "force_min_app_version": "optional semver or null"
}
```

**Response 201**
```json
{
  "id": "tenant_xxx",
  "slug": "lowercase-slug",
  "display_name": "Tenant Display Name",
  "force_min_app_version": null
}
```

**Errors**
- `400 invalid_body`
- `409 tenant_conflict`

---

## 4) License Lifecycle Contracts (Bearer required)

### `POST /api/v1/licenses`
**Request**
```json
{ "tenant_id": "tenant_xxx", "max_machines": 3, "expires_at": null }
```

**Response 201**
```json
{
  "id": "lic_xxx",
  "tenant_id": "tenant_xxx",
  "tenant_display_name": "Tenant",
  "license_key": "MFX-XXXX-YYYY",
  "max_machines": 3,
  "expires_at": null,
  "revoked_at": null
}
```

**Errors**
- `400 invalid_body`
- `404 tenant_not_found_or_inactive`

### `GET /api/v1/tenants/:tenantId/licenses`
Returns array with:
- `id`
- `tenant_id`
- `license_key`
- `max_machines`
- `expires_at`
- `revoked_at`
- `revoked_reason`
- `created_at`

### `POST /api/v1/licenses/:licenseId/revoke`
**Request**
```json
{ "reason": "optional reason" }
```

**Response 200**
```json
{ "ok": true, "revoked_at": "ISO timestamp" }
```

**Errors**
- `400 invalid_body`
- `404 license_not_found`

---

## 5) Machine Activation Contract (Desktop + API)

### `POST /api/v1/activation/claim` (no bearer)
**Request**
```json
{
  "license_key": "MFX-XXXX-YYYY",
  "machine_fingerprint": "stable-machine-fingerprint",
  "machine_label": "optional",
  "app_version": "optional semver"
}
```

**Response 200**
```json
{
  "tenant_id": "tenant_xxx",
  "license_id": "lic_xxx",
  "machine_fingerprint": "stable-machine-fingerprint",
  "activation_token": "jwt",
  "expires_at": null,
  "force_min_app_version": null,
  "force_update_required": false
}
```

**Errors**
- `400 invalid_body`
- `404 license_not_found`
- `403 license_revoked`
- `403 license_expired`
- `409 slot_limit_reached`

---

## 6) Desktop Integration Freeze (Local Command Boundary)

Desktop `ProductLicenseGate` -> `claimProductActivation()` -> Tauri command:
- Tauri command: `submit_product_license_key(key, claim_json?)`
- Persisted fields in app settings include:
  - `key_sha256`
  - `submitted_at`
  - `activation_claim` (optional)
  - `pending_online_validation` (boolean)

This local command signature is considered **v1 frozen** for activation onboarding.

---

## 7) Deferred v1 Domains (Reserved In Phase 4 Plan)

The following domains were originally reserved in v1 scope and are now live in Phase 4 runtime:
- sync monitoring and repair queue APIs (`/api/v1/ops/sync-overview`)
- rollout policy/cohort control APIs (`/api/v1/ops/rollout-overview`, `/api/v1/cohorts/...`)
- platform health/pressure APIs (`/api/v1/ops/platform-overview`, `/api/v1/ops/observability/slo`)
- immutable audit/support workflow APIs (`/api/v1/audit/*`, `/api/v1/support/*`)

These remain frozen under additive evolution rules only (no breaking field renames/removals in `v1`).

---

## 8) Change Control

- Contract owner updates this file before any endpoint behavior change.
- VPS Agent validates runtime conformance after deployment:
  - endpoint availability
  - auth behavior
  - response schema compatibility
  - error code consistency

---

## 9) Runtime Conformance Snapshot (2026-04-17)

- Sprint 10 rerun passed (`15/15`) with final RC decision `GO`.
- Route verification evidence confirms full control-plane route set availability (HTTP `200/201` as expected).
- Immutable deployment identifiers:
  - API image digest: `sha256:49d57a8b35d146c3237f548fc1918d988edb8fced1845f4b0c57dd0b53fc3ce0`
  - Edge image digest: `sha256:65645c7bb6a0661892a8b03b89d0743208a18dd2f3f17a54ef4b76fb8e2f2a10`
  - API source SHA (`api/src/index.ts`): `a08fe6a9f5174fa37c35624f30bbe739513efd62`

