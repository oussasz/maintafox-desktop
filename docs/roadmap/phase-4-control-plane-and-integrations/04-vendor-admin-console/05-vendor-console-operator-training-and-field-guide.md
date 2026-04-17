# Vendor Console Operator Training And Field Guide

**Audience:** Vendor support operators, licensing operators, rollout operators, platform operators, and team leads.  
**Scope:** How to use `console.maintafox.systems` end-to-end, what each field/button does, and how to generate license keys for a company.

---

## 1) Access prerequisites

- You must have a valid operator account (email/password login).
- You must be able to reach:
  - `https://console.maintafox.systems`
  - `https://api.maintafox.systems`
- Your role must include the needed permissions:
  - `console.view` (open console)
  - `customer.manage` (customer/tenant management)
  - `entitlement.manage` (license and machine operations)
  - `sync.operate` (sync operations)
  - `rollout.manage` (rollout controls)
  - `platform.observe` (platform health)
  - `audit.view` (audit/support/hardening tabs)

If a section is missing from the left navigation, your role does not include its permission.

---

## 2) Login and session

## `/login`

- **Email**: operator account email.
- **Password**: operator account password.
- **Sign in**: calls control-plane login and stores bearer token.
- **Error message area**: shows network/auth/config issues.

Notes:
- Mock/developer auth bypass is removed. Real authentication is always required.
- If login succeeds but a section is inaccessible, you will be redirected to unauthorized/permission handling for that route.

---

## 3) Global shell elements

These are visible on every authenticated vendor-console page.

### Top bar

- **Home** (left arrow): returns to `/vendor-console`.
- **Product label + env badge**:
  - `prod` in production mode.
  - `dev` in development mode.
- **Operational search context** (`Tenant / machine / correlation id…`):
  - Stores a context string used by some drill links and workflows.
- **Host hint / degraded banner**:
  - Normal: shows host hint.
  - Degraded: shows queue/storage warning banner from telemetry context.

### Left navigation

Each item appears only if your role has permission:

- **Overview**
- **Customers**
- **Entitlements**
- **Machines**
- **Sync**
- **Rollouts**
- **Health**
- **Audit**

### Step-up dialog (sensitive-action UX)

- **Confirmation code**: operator-entered re-auth code for sensitive workflows.
- **Cancel**: closes dialog without validation.
- **Confirm**: marks step-up session as fresh client-side.

Important:
- Real enforcement is server-side on privileged endpoints.  
- UI step-up freshness does not replace API-side policy checks.

---

## 4) Overview page

## `/vendor-console`

Purpose: fast access to key operational areas and evidence drill-through.

### Main cards and buttons

- **Operations hub**
  - Shows open alerts, worst severity, aggregate DLQ.
  - **Refresh**: reloads sync + platform telemetry.
  - Drill buttons (permission-gated):
    - **Platform health & alerts**
    - **Sync diagnostics**
    - **Rollout control**

- **License & machines**
  - **Open customers**
  - **Evidence: last tenant**
  - **Machines & heartbeats**

- **Sync & rollouts**
  - **Queue evidence**
  - **Rollout manifest**
  - **Audit & support**

- **Step-up (operator lab)**
  - **Trigger step-up**: opens step-up dialog for training/test workflow.
  - Step-up freshness indicator text shows active/inactive window.

---

## 5) Customers page (tenant administration)

## `/vendor-console/customers`

Purpose: manage companies (tenants), view metadata, and set force-update policies.

### A) Workspace metadata section

- **Tenant slug** (select): selects active tenant.
- **Rollout cohort** (read-only)
- **Tenant display name** (read-only)
- **Tenant update channel** (read-only)

Live data controls:
- **tenant-slug** (input): new tenant slug (for creation).
- **Display name** (input): new tenant display name.
- **Create tenant**:
  - Creates tenant with selected rollout channel/cohort defaults.
  - Requires non-empty slug + display name.
- **Refresh**: reloads tenant list.

Tenant table fields:
- `id`, `slug`, `name`, `cohort`.
- Clicking a row selects active tenant context.

### B) Force-update governance section

Purpose: enforce minimum app version and force modes by tenant/cohort.

Shared security headers fields:
- **Audit reason code (header)**: sent as `X-Operator-Reason`.
- **Step-up token (header)**: sent as `X-Step-Up-Token`.

Tenant override card:
- **Minimum app version**
- **Mode**: `inherit`, `off`, `required`, `emergency`
- **Operator reason**
- **Apply tenant policy**: updates tenant-level update policy.
- **Rollback tenant policy**: rolls back tenant policy to previous snapshot.

Cohort baseline card:
- **Minimum app version**
- **Mode**: `off`, `required`, `emergency`
- **Operator reason**
- **Apply cohort policy**
- **Rollback cohort policy**

### C) Bulk channel reassignment (dry-run)

- **Tenant IDs (comma-separated)**
- **License / entitlement channel**: `stable`, `pilot`, `internal`
- **Rollout channel (must match)**: `stable`, `pilot`, `internal`
- **Dry run** checkbox
- **Build request payload**: generates preview JSON (no server mutation).

Safety behavior:
- If license channel and rollout channel do not match, apply preview is blocked by guardrail warning.

---

## 6) Entitlements page (license lifecycle)

## `/vendor-console/entitlements`

Purpose: issue/revoke licenses, validate lifecycle actions, preview claim payloads.

### Field-by-field

- **Tenant**: choose target tenant/company.
- **Max machines (issue)**: machine slot cap used when issuing a license.
- **Current state**: lifecycle source state (`active`, `grace`, `expired`, `suspended`, `revoked`).
- **Lifecycle action**:
  - `issue`
  - `renew`
  - `suspend`
  - `revoke`
  - `emergency_lock`
  - `resume_from_suspension`

Live licenses panel:
- **Refresh**: reload licenses for selected tenant.
- Table columns: `id`, `key`, `max`, `revoked`.
- Click row to select a specific license.

Policy matrix banner:
- Shows whether current state/action combination is allowed.

Signed claim preview:
- Displays computed entitlement envelope preview and payload hash.

Destructive approval fields (shown for destructive actions):
- **Reason code**
- **Second actor ID**
- **Rationale**

Primary buttons:
- **Preview audit payload**: builds and displays JSON preview of intended audit payload.
- **Submit (VPS)**:
  - For `issue`/`renew`: issues license.
  - For destructive actions: revokes selected license with rationale/reason context.

---

## 7) Machines page (activation + offline policy)

## `/vendor-console/machines`

Purpose: inspect machine activation posture and test activation claim workflow.

### A) Offline policy (read model)

- **Tenant** selector.
- Displayed fields:
  - **Grace hours**
  - **Trust revoke disconnects**
  - **Reconnect needs fresh heartbeat**

Buttons:
- **Extend grace +12h**: updates offline grace.
- **Toggle trust-revoke rule**: toggles trust revocation behavior.

### B) Activation claim (live API)

- **License key** (input)
- **Machine fingerprint** (input)
- **App version** (input)
- **Claim activation**:
  - Sends claim to API.
  - Displays JSON response.
  - Refreshes machine rows/policy for returned tenant.

### C) Machine activation table

Columns:
- **Machine**
- **Heartbeat**
- **App**
- **Trusted**
- **Freshness**
- **Actions**

Action buttons on each machine row:
- **Slot release**
- **Rebind**
- **Soft suspend**
- **Policy refresh**

Current behavior:
- These actions open step-up workflows; final privileged enforcement remains server-side.

---

## 8) Sync page

## `/vendor-console/sync`

Purpose: monitor tenant synchronization health and repair queues.

### Key sections

- KPI cards:
  - **Max tenant lag**
  - **Checkpoint age p95**
  - **Retry pressure**
  - **Dead-letter items**

- **Tenant sync posture**
  - **Refresh**
  - **Open drill-down** per tenant

- **Repair queue**
  - Displays allowed actions per queue item (`replay`, `requeue`, `acknowledge`, `escalate`) as read-only capability text.

- **Heartbeat policy anomalies affecting sync**
  - Tenant/machine anomaly list.

Drill-down sheet:
- Shows batch/failure/idempotency details for selected tenant.

---

## 9) Rollouts page

## `/vendor-console/rollouts`

Purpose: cohort-level rollout operations and diagnostics.

### Impact preview

- Release ID
- Affected tenants
- Affected machines
- Entitlement channel consistency
- Known blockers

### Channels & cohorts

- **Refresh**
- Per cohort card:
  - channel, label, counts, governance, paused timestamp
  - **Pause stage** (step-up workflow)
  - **Recall** (step-up workflow)

### Release diagnostics

- Buckets like download/signature/migration/post-deploy heartbeat
- 24h count, last event, sample correlations

---

## 10) Health page

## `/vendor-console/health`

Purpose: service-level health telemetry + pressure indicators + alert ownership.

### Service status

- **Refresh**
- Card per service (API, workers, PostgreSQL, Redis, object storage, admin UI)
- Severity badge and detail text

### Infrastructure pressure table

Columns:
- Metric
- Value
- Threshold hint
- Trend

### Alerts & ownership panel

- Read-only alert view with:
  - alert title/id
  - severity/state
  - drill references
  - owner actor id
  - notes timeline (if present)

### Related operational views

- **Open sync health**
- **Open rollouts**

---

## 11) Audit page

## `/vendor-console/audit`

Contains three tabs: **Audit ledger**, **Support**, **Hardening & drills**.

### A) Audit ledger tab

Fields:
- **Search**
- **Category** filter (`all`, `auth_session`, `entitlement`, `machine`, `sync_repair`, `rollout_intervention`, `platform_override`, `support_intervention`)

Buttons:
- **Verify sample chain**
- **Export CSV**
- **Refresh**
- **Open** (record detail sheet)

Detail sheet shows:
- correlation, reason/approval, before/after hashes, chain hash, linked entity refs.

### B) Support tab

Tickets section:
- **Export CSV**
- **Refresh**
- Per ticket:
  - intervention note input
  - **Log** button to create support intervention

Diagnostic bundle:
- Shows latest bundle id/profile/artifacts.
- **Request signed export (VPS)**: downloads local manifest JSON of the loaded bundle.

Offline-origin reconciliation:
- Maps desktop queue IDs to vendor ticket IDs and sync state.

Compliance export kinds:
- Entitlement history
- Machine state timeline
- Rollout actions
- Support resolution chronology

### C) Hardening & drills tab

Buttons:
- **Step-up: compliance export**
- **Step-up: rollout recall**
- **Refresh**

Read models:
- Incident runbook cards
- Drill-based readiness matrix with pass/gap status

---

## 12) Full training flow: generate a license key for a company

This is the exact operator flow to generate a key for a new company.

### Step 1 - Create company (tenant)

1. Go to `/vendor-console/customers`.
2. In live data controls:
   - Fill **tenant-slug** (example: `acme-industrial`).
   - Fill **Display name** (example: `ACME Industrial`).
3. Click **Create tenant**.
4. Confirm new tenant appears in tenant table and is selectable.

### Step 2 - Issue license key

1. Go to `/vendor-console/entitlements`.
2. Select the new tenant in **Tenant**.
3. Set **Max machines (issue)** (example: `10`).
4. Set:
   - **Current state** = `active`
   - **Lifecycle action** = `issue`
5. Click **Submit (VPS)**.
6. In **Live licenses**, click **Refresh**.
7. Confirm a new row exists and copy the value in the **key** column.

Result:
- The value in `key` is the company license key to deliver to the customer.

### Step 3 - Validate key quickly (optional but recommended)

1. Go to `/vendor-console/machines`.
2. In **Activation claim (live API)**:
   - Paste license key
   - Enter machine fingerprint
   - Enter app version
3. Click **Claim activation**.
4. Confirm a successful JSON response and check machine list updates.

### Step 4 - Set update governance (optional)

If the company needs minimum version policy:
1. Return to `/vendor-console/customers`.
2. Fill **Audit reason code** and **Step-up token**.
3. Configure tenant/cohort policy fields.
4. Click **Apply tenant policy** or **Apply cohort policy**.

---

## 13) Common errors and what they mean

- **Not authenticated / redirect to login**
  - Session token missing/expired. Sign in again.
- **Access denied / section missing**
  - Role lacks required permission.
- **`VITE_ADMIN_API_BASE_URL is not configured`**
  - Deployment/runtime env misconfiguration.
- **`... failed (401/403)`**
  - Auth token invalid or permission denied for that endpoint.
- **Policy apply/rollback rejected**
  - Missing/invalid `X-Operator-Reason` or `X-Step-Up-Token`, or server-side policy guard.

---

## 14) Operator best practices

- Always create tenant first, then issue license key.
- Capture ticket/incident IDs in reason fields when changing policy.
- Use preview actions (`Preview audit payload`, bulk payload preview) before destructive submissions.
- Keep cohort and license channels aligned unless exception is intentional and approved.
- Export CSV/manifests during incident handling for forensic evidence packs.

