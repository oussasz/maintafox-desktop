# Phase 4 — Control Plane And Integrations

This folder is the **authoritative task map** for Phase 4. It is listed in [docs/README.md](../../README.md) under **Phase 4 - Control Plane And Integrations**.

## Subphases and what they cover

| Subfolder | Focus | Typical code / surface |
|-----------|--------|-------------------------|
| **01-sync-layer-checkpointing-and-conflict-handling** | Local sync contracts, conflicts, orchestrator, repair, observability | `src-tauri/src/sync/`, `src/services/sync-*`, `shared/ipc-types.ts` |
| **02-licensing-entitlements-and-machine-activation** | Entitlements, activation, license enforcement, licensing security | `src-tauri/src/entitlements/`, `activation/`, `license/`, Settings UI |
| **03-vps-backend-and-tenant-mirror-architecture** | VPS API boundaries, PostgreSQL mirror + workers, object storage backups, **deployment & ops** | Shared contracts (`shared/ipc-types.ts`, `src-tauri/src/vps/`), plus **live VPS** for Postgres, object storage, TLS, workers |
| **04-vendor-admin-console** | Vendor-only admin UI: auth, customers, licenses, sync/rollout ops | Web app + admin APIs (may be separate deployable from desktop) |
| **05-signed-updates-rollout-and-release-orchestration** | Signed updates, channels, client install safety | Updater plugin, manifest pipeline |
| **06-iot-gateway** | IoT ingestion and routing | Gateway services |
| **07-erp-and-external-system-connectors** | ERP mapping, jobs, reconciliation | Connectors |

## Implementation vs deployment (important)

- **Roadmap execution** defaults to work inside `maintafox-desktop/` (see [Roadmap execution](../../README.md#roadmap-execution)).
- **In-repo today:** contracts, guards, desktop integration, and tests (e.g. `src-tauri/src/vps/` mirror/worker logic, shared TypeScript types).
- **On the VPS (not automatic from repo):** PostgreSQL, Redis/queues, object storage, reverse proxy, TLS, DNS, secrets, and running API/worker/admin containers. Those are planned in **`03/04-vps-deployment-observability-and-recovery-validation.md`** and **`03/03-object-storage-backups-and-operations-baseline.md`**.
- **Vendor console URL:** production hostname for the admin UI (e.g. `console.maintafox.systems`) is specified in **`04-vendor-admin-console/01-admin-auth-access-control-and-console-shell.md`** and wired operationally in **`03/04-vps-deployment-observability-and-recovery-validation.md`** (DNS + TLS + routing).

## Suggested order when building

1. **01** + **02** — desktop authority and sync/licensing behavior (foundation).
2. **03** — VPS contracts, mirror model, backups baseline, then **deployment** slice when you are ready to point DNS at a real server.
3. **04** — vendor console **after** admin API boundaries and auth model are clear (often parallel to 03 deployment).
4. **05** — updates tied to rollout and signing.
5. **06** / **07** — as product scope requires.

## Missing pieces (if any)

If something is not in the numbered `.md` files under this folder, it is **not** part of the signed-off Phase 4 roadmap. Add a bullet under the relevant file’s **Delivery Slices** or open a PR to extend that file.

## Gap-closure companion artifacts

- `90-gap-closure-sprints-and-placeholder-burndown.md`
- `91-control-plane-v1-contract-freeze.md`
- `92-placeholders-and-hardcodes-inventory.md`
- `93-observability-slo-and-runbooks-pack.md`
- `94-uat-compliance-and-rc-signoff-pack.md`
- `95-incident-drill-evidence-and-postmortem-template.md`
- `96-phase4-gap-closure-report.md`
