# Maintafox — Master Completion Plan (Desktop + Sync + VPS + Vendor Console)

**Date:** 2026-10-09 · **Basis:** 7 read-only audits of `maintafox-desktop` and `maintafox-vendor-console` (`api/` + `src/`); the critical findings were re-verified by hand.
**Detailed desktop evidence:** [`COMPLETION_GAP_AUDIT_2026-10-09.md`](COMPLETION_GAP_AUDIT_2026-10-09.md).

**Engineering rules for every phase:**
- No hardcoded secrets, no silent fallbacks, no hidden defaults.
- No mocked success. Every user-visible action hits the real backend and reports real errors.
- Reuse the existing patterns: `require_session!`/`require_permission!`, the sync outbox, the UI primitives, the i18n namespaces, and the migration framework.
- Every phase keeps existing data and flows backward compatible. Any break must come with an explicit migration.

---

## 0. Current state

| Area | State |
|---|---|
| **Desktop business modules** | **Mostly done.** All 774 IPC commands are registered and nearly all enforce permissions; no page uses mock data. The work-order, DI, asset, org, reference, PM, inventory, personnel, planning, training and budget modules work end to end on one machine. |
| **Desktop safety and correctness** | **Not acceptable for production:** the DB is not encrypted, permits cannot be created (which deadlocks WOs that require a permit), the LOTO two-person rule is missing, the Weibull fit treats censored data as failures, and fleet KPIs average ratios. |
| **Sync (desktop side)** | **Partially works.** The outbox, inbox and conflicts engine is solid locally, but about 43 of the 63 types are silently *skipped* on inbound apply and 40+ business tables are never staged. Transport runs in the webview only. |
| **Sync (server side)** | **Prototype.** It is an append-only JSON log with per-device cursors, with no conflict detection (`row_version` is ignored), no current-state view, no transaction around an exchange, and the client checkpoint is ignored. A new device has to replay the whole history. It is not safe for multi-device production. |
| **Control API security** | **Critical hole:** `authMiddleware` (`api/src/index.ts:1298`) does not check the token kind, so **a desktop activation token passes every vendor-admin route across all tenants.** Licence signing is symmetric, and its default secret `"MAINTAFOX_TRUSTED_ISSUER_KEY_V1"` is shipped inside the desktop binary, so anyone can forge licences. There is no rate limiting. |
| **Control API platform** | **Persistence is split:** SQLite holds tenants, licences and users; Postgres holds only the sync log. There is no migration framework (`CREATE IF NOT EXISTS` at boot), no CI, and no backups. A 4,639-line `index.ts`. Default DB and MinIO passwords are used as fallbacks. |
| **Vendor console** | **Real API calls, but unsafe.** The step-up/MFA dialog accepts any non-empty code on the client only. Machine release/rebind/suspend and rollout pause/recall **do nothing** when clicked. The JWT is kept in localStorage, and there is no logout and no TOTP. Ops metrics are fabricated (`lag = staleCount*45`). There is no release catalog, billing view or vendor user admin. |
| **Updates / release** | **Not functional anywhere.** The desktop updater has a placeholder key and a stub URL, the server has no manifest or artifact endpoint, and there is no code signing or release workflow. |
| **Not started** | IoT gateway, ERP connectors, configuration engine. |

**Conclusion:** No code is mocked, but the platform cannot go to customers. The work is mainly hardening, sync v2, the release chain, and finishing the modules that are half-built. It is not a rewrite.

---

## 1. Decisions

> **Decided 2026-10-10 (product owner):**
> - **Nothing is in production yet.** There are no live tenants and no desktops in the field, so D6 dual mode is **not** needed: v1 sync and licence formats are replaced directly. D1 needs no live data migration.
> - **D2 = generic current-state mirror** (recommended option). PRD §7.4 is to be updated accordingly.
> - Execution starts with **Phase 0** in the working session.

| # | Decision | Recommendation | Why |
|---|---|---|---|
| D1 | **Server persistence:** keep SQLite + Postgres split, or consolidate? | **Consolidate everything in Postgres** with a real migration tool. Migrate the SQLite control data once. | One source of truth, transactions across licences, activations and sync, backups, high availability. |
| D2 | **Server mirror model:** PRD §7.4 says *typed per-entity tables*; the code has a generic JSON log | **A generic *current-state* table** `(tenant_id, entity_type, entity_id, row_version, payload jsonb, deleted_at, origin_device, updated_at)` plus the existing append log. Typed projections only where the server needs to query (console, analytics). | 100+ typed server tables would duplicate every desktop migration and drift. A current-state table gives conflict control, bootstrap snapshots and deletes. **This departs from the PRD, so you need to approve it.** |
| D3 | **Conflict policy** | Optimistic `row_version` per entity. The server rejects stale writes as a *conflict* and returns the current row. The desktop shows it in its existing `sync_conflicts` UI. Per-entity policies (e.g. append-only for logs, field-merge for none at first). | It reuses the conflict UI that already exists. Silent last-writer-wins would lose maintenance data. |
| D4 | **Licence signing algorithm** | **Ed25519.** The private key lives only on the server (file or secret store); the public keys are embedded in the desktop, keyed by `key_id` for rotation. | A symmetric secret in the binary can be forged by anyone who has the binary. |
| D5 | **Shared contracts** between desktop, API and console | One generated contracts package (sync envelope, entitlement envelope, activation, update manifest), generated from the Rust types, with a CI drift check. | The console's 6,327-line `shared/ipc-types.ts` is a hand copy that has already drifted. |
| D6 | **Transition for installed desktops** (if any are in the field) | Run v1 and v2 side by side for **one** release, with an explicit cutoff date in config. After the cutoff, v1 is rejected. | Backward compatibility without permanent insecure paths. If nothing is deployed yet, skip the dual mode. |

---

## 2. Phases

Each phase has **exit criteria**. A phase is not done until every criterion is met and verified in CI or a recorded test run.

### Phase 0 — Stop the bleeding (security emergency) · *both repos* · small, do first

| Item | Repo | Detail |
|---|---|---|
| 0.1 | API | `authMiddleware` must require `kind === "vendor_admin"` and a valid `sub` that exists in `users`. Use separate signing secrets (or audiences) for admin tokens and device tokens. |
| 0.2 | API | Remove every fallback secret and password (`entitlement-signer.ts:19`, Postgres/MinIO defaults in compose, default admin email). The process **refuses to start** if a required secret is missing. Add the missing variables to `.env.example`. |
| 0.3 | API | Rate limiting on `/auth/login`, activation and sync. A body-size limit sized for sync batches, plus a maximum `outbox_batch` length. Set `trust proxy`. |
| 0.4 | Desktop | Guard `shutdown_app`, `run_integrity_check`, `get_product_activation_token`, `submit_product_license_key` and `apply_product_license_reconciliation`. Narrow `capabilities/default.json` (no `fs:allow-remove`, no `$HOME/**`). Remove the debug-log writes to `c:\Dev\...`. |
| 0.5 | Console | Remove the fake step-up. Until Phase 4 brings TOTP, dangerous actions require the server-side reason plus re-entering the password, checked by the API. |

**Exit:** a device token gets 401/403 on every admin route (automated test); the API does not boot with missing secrets; the desktop capability scope is limited to app dirs and dialog-picked paths; no client-only security check remains.

### Phase 1 — Trust foundation · *desktop + API*

| Item | Detail |
|---|---|
| 1.1 | **Ed25519 entitlements end to end** (D4): the server signer, desktop verification against embedded public keys, `key_id` rotation, `SOFT_ACCEPT` removed, and test vectors shared by both repos. |
| 1.2 | **Desktop DB encryption:** SQLCipher with the key in the OS keyring, a one-time encrypt-in-place migration of existing DBs with a backup first, and a recovery path documented. Verify that the single-connection pool and the PRAGMA handling still work. |
| 1.3 | **Backups:** encrypted backups, a real `restore` command with integrity check and pre-restore snapshot, scheduled backups, and a real factory reset (no stub). |
| 1.4 | **Device token lifecycle (API):** refresh/rotation endpoint, a heartbeat endpoint that updates `last_seen_at`, and middleware that checks licence expiry, `revoked_at` and tenant `is_active`. Desktop calls the heartbeat from the sync orchestrator. |
| 1.5 | **Desktop RBAC completion:** enforce `fin.approve` and `per.sensitiveview`; `ref.view` on lookups; route guard on `/settings`. |

**Exit:**
- A forged envelope is rejected.
- A rotated key is accepted.
- The DB file is unreadable without the keyring key.
- Backup → wipe → restore round-trips in an automated test.
- A revoked or expired licence blocks sync within one heartbeat.

### Phase 2 — Sync protocol v2 · *the core of "with synchronisation working"* · largest phase

**Server (`maintafox-vendor-console/api`):**
1. Move to Postgres-only persistence with versioned migrations (D1).
2. Add a current-state mirror table (D2) next to the existing append log.
3. Make the **exchange atomic**: one transaction covers ingress, conflict check, state upsert, cursor and idempotency record. Idempotency replay must verify that the body hash matches.
4. **Conflict detection** (D3): an incoming `row_version` that does not equal stored + 1 (or the base version) produces a conflict response carrying the server row.
5. **Per-device checkpoints**, and the client's `checkpoint_token` is honored. Checkpoint tokens are opaque and signed.
6. **Bootstrap snapshot endpoint**: a new or reset device pulls the current state in pages, then switches to deltas.
7. **Tombstones** for deletes; retention and compaction of the append log.
8. **Typed entity registry** shared with the desktop (D5). Unknown types are rejected, not silently accepted.

**Desktop (`maintafox-desktop`):**
1. **A single sync entity registry** (entity → stager → materializer → conflict policy), with a **CI test that fails** when a migration adds a business table that has no registry entry or explicit "local-only" mark.
2. **Stagers and materializers for every business entity.** That means the ~43 currently skipped types plus the 40+ that were never staged (POs, requisitions, receipts, suppliers, planning, WO parts/tools/verifications/segments, PM executions, budget actuals/commitments/forecasts, FMECA/RCM/models, personnel skills, notification rules, …).
3. Asset lifecycle/hierarchy/move, org models/node types and reference merge/publish are staged too.
4. **Staging failure must fail the write, or be queued and visible.** Replace `warn_stage` everywhere.
5. Move transport from the webview `fetch` into a **Rust background worker**, using `reqwest` with rustls. Add retry/backoff, network-state awareness and pause/resume, and send the withheld manifests (`tenant_config`, `device_class`, `media_manifest`).
6. **Conflict UX:** the existing `sync_conflicts` UI handles server-returned conflicts.

**Test harness:** a docker-compose environment with the API and Postgres plus **2–3 headless desktop DB instances** that run scripted scenarios:
- concurrent edits
- offline for N days then reconnect
- delete vs edit
- new device bootstrap
- crash mid-exchange
- replay with a duplicate idempotency key

**Exit:**
- Every business entity round-trips between two devices (test generated from the registry).
- The conflict scenarios produce visible conflicts, never silent loss.
- A crash in the middle of an exchange loses nothing.
- A new device bootstraps from a snapshot.
- Sync continues while the UI window is closed.

#### Phase 2 execution plan (agreed 2026-10-10)

**Owner decisions**
- **Conflicts:** a person decides. A concurrent edit becomes a conflict shown in the Sync Center
  ("keep mine / take theirs" actually applies the choice). Last-write-wins is never applied silently.
- **Personal items** (notification read state, dashboard layouts, saved filters) sync per user,
  delivered only to that user's devices.
- **All** control-API data moves to Postgres in this phase, not only sync. Phase 3 item D1 is
  pulled forward.
- Steps 2.1 (server) and 2.2 (desktop) run in **parallel sessions**. Then come 2.3 → 2.5.

**Findings that drive the work** (research on 2026-10-10)
- **Server:**
  - The exchange is not atomic, and the device cursor advances before the response is saved, so
    inbound items can be lost.
  - A device whose last 300 rows are its own is starved forever (`fanout.ts:30`).
  - Idempotency replay ignores body changes and is scoped per tenant, not per device.
  - The checkpoint is tenant-global and unsigned.
  - There is no conflict check, no current-state table, no tombstones, no snapshot endpoint, no
    entity allowlist (`validate.ts:26`) and no retention.
  - On Postgres, tenant delete hits SQLite-only tables.
- **Desktop:**
  - Only 20 of 81 entity types are applied inbound. 54 are staged but Skipped *and marked
    applied*, so they are lost even after materializers exist. 7 constants have nothing behind
    them.
  - About 100 business tables are not synced.
  - WO, personnel, org and reference rows lack a stable `entity_sync_id`: the stager falls back to
    the code or a fresh random UUID per call.
  - Staging runs after commit: about 80 call sites only warn, and the legacy modules fail the
    command after commit.
  - Legacy payloads carry local integer FKs.
  - Equal `row_version` is Skipped, so concurrent edits go undetected.
  - Materialize conflicts are invisible, and conflict resolution does not touch domain data.
  - Transport is webview `fetch`, and media manifests are withheld.

**2.1 Server protocol v2 + Postgres** (vendor-console)
- **Versioned migrations; all API data on Postgres.**
  - Includes tenants, licences, activations, users, audit hash chain, support, payments,
    edition catalog and sync.
  - A one-shot SQLite → Postgres copy with row counts and hash-chain verification, backup first,
    and a rollback path.
- **Atomic exchange:** one transaction covering ingress, conflict check, state upsert, cursor and
  idempotency record.
  - Idempotency is per device and stores the request hash; the same key with a different body
    returns 409.
  - The cursor advances only with the saved response, and the starvation bug is fixed.
- **Current-state table** keyed by `(tenant, entity_type, entity_sync_id)`.
  - An incoming item carries `base_row_version`. A mismatch returns a `conflict` item with the
    server row and is never applied silently.
  - Deletes become tombstones.
- **Signed per-device checkpoints.**
- **Snapshot endpoint** for paged bootstrap, then a switch to deltas.
- **Typed entity registry** shared with the desktop; unknown types are rejected.
- **Per-user scope:** an item with `owner_user_sync_id` is delivered only to that user's devices.
- **Retention and compaction** of the append log.
- **Protocol `v2`** alongside `v1` until all desktops are updated.
- Tests cover concurrency, a crash mid-exchange, replay, starvation, conflicts, snapshot, and
  Postgres in CI.

**2.2 Desktop foundations** (desktop)
- **Stable `entity_sync_id`** on every business insert, plus a backfill migration; stagers never
  invent IDs.
- **Staging inside the business transaction**: `warn_stage` is removed, and a failed stage fails
  the write. The outbox gains attempt/in-flight/dead-letter state and `origin_machine_id`.
- **A single sync entity registry** (entity → stager → materializer → conflict policy → scope).
  - A CI test fails on any business table without an entry or an explicit local-only mark.
  - Unknown inbound types stay `deferred`, not `applied`.
- **Conflicts:**
  - Materialize conflicts land in `sync_conflicts`.
  - Accept-local re-stages; accept-remote applies the inbound payload.
  - Equal-version divergence is detected through `base_row_version`.

**2.3 Rust sync worker:**
- `reqwest` with rustls in the `BackgroundTaskSupervisor` (task id `sync`).
- Retry and backoff, network awareness, pause/resume.
- Sends the manifests and uploads media.
- Runs while the window is closed. The webview transport is removed.

**2.4 Coverage by domain:** legacy payloads move to sync-id references, plus stagers and
materializers for:
- inventory/procurement
- WO details
- PM/planning
- finance
- personnel/qualification
- inspections/permits
- RAMS models
- config/reference/notifications/reports
- per-user items

**2.5 Proof:**
- A docker-compose harness (API + Postgres + 3 headless desktop DBs) runs the scenarios in the
  Phase 2 test-harness list above.
- A registry-generated round-trip test covers every entity.

### Phase 3 — Control plane hardening · *API*

| Item | Detail |
|---|---|
| 3.1 | Split `index.ts` (4,639 lines) into route modules by family (auth, tenants, licences, activation, sync, ops, audit, support, billing, releases). This is a pure refactor with no behavior change, gated by the existing script tests. |
| 3.2 | Tenant isolation: Postgres **row-level security** on tenant-scoped tables in addition to the app-level `WHERE`. Sensitive payloads (e.g. `user_accounts`) are encrypted or redacted at rest. |
| 3.3 | **Real ops metrics** computed from the sync store: lag, checkpoint age, rejection rate, dead letters, per device. Delete the heuristic formulas (`index.ts:3227-3231`). Prometheus/OpenMetrics endpoint. |
| 3.4 | Support tickets, bundles and runbooks become real persisted entities instead of seed fixtures (`index.ts:781-891`). Diagnostic bundle upload goes from desktop to MinIO, and the console can download it. |
| 3.5 | Postgres backup with `pg_dump` plus WAL archiving to object storage, a restore drill script, and MinIO bucket versioning. |
| 3.6 | CI for the vendor-console repo: typecheck, lint, unit tests (add vitest/supertest for the API), the integration scripts against a docker Postgres, dependency audit, secret scan. |

**Exit:** a monthly restore drill passes; ops dashboards show measured numbers; an RLS test proves that one tenant cannot read another's data even when a query bug drops the `WHERE`; CI is green on every PR.

### Phase 4 — Vendor console completion · *console + API*

| Item | Detail |
|---|---|
| 4.1 | **Vendor auth:** short-lived access token in memory, refresh token in an httpOnly Secure SameSite cookie, logout everywhere, **mandatory TOTP enrollment**, and a server-issued **step-up token** with a short TTL bound to the action kind. Align the step-up action kinds between the console and the Rust `StepUpActionKind`. |
| 4.2 | Wire the dead buttons to real API endpoints (built in the same item if missing): slot release, rebind, soft suspend, policy refresh, rollout pause, rollout recall. Each one is audited in the ledger. |
| 4.3 | **Vendor users and roles admin** page (CRUD, role assignment, MFA reset with step-up). |
| 4.4 | **Billing and subscription view:** SlickPay payment history, licence term, renewal/extension, extra slots. The routes already exist on the API; add the UI and reconciliation. |
| 4.5 | **Sync operations:** a per-device timeline, plus replay and repair actions (step-up gated) that call the Phase 2 endpoints. |
| 4.6 | Shared contracts package (D5) replaces the copied `shared/ipc-types.ts`; CI drift check. |
| 4.7 | French selectable, no hardcoded strings; tests for the sync, rollout, health, machines, editions and audit panels; Playwright e2e for login + TOTP + one dangerous action. |

**Exit:** no client-only security check anywhere; every button produces a real server effect and an audit row; the e2e suite passes.

### Phase 5 — Release & update chain · *desktop + API + console + CI*

| Item | Detail |
|---|---|
| 5.1 | Tauri updater keypair: the private key in CI secrets, the public key in `tauri.conf.json` (no placeholder). `createUpdaterArtifacts` on. |
| 5.2 | Windows code signing (certificate in CI secrets), NSIS/MSI configuration, and a `release.yml` workflow that builds, signs, uploads artifacts to MinIO/S3 and registers the release in the API. |
| 5.3 | API **update manifest endpoint** that resolves `{target, arch, current_version}` against the tenant/cohort channel, a **staged % rollout**, pause, recall and a minimum-version floor. Replace the synthetic `release_id`. |
| 5.4 | Console **release catalog**: releases, notes, channel promotion, rollout %, recall (step-up). |
| 5.5 | Desktop: a real update check/download/install with progress, a pre-update backup, migration safety (forward-only migrations plus the backup restore from Phase 1), and rollback by reinstalling the previous signed artifact. |

**Exit:** a signed build moves internal → pilot → stable through the console; a recall stops further installs within one manifest poll; a failed migration restores the pre-update backup automatically.

### Phase 6 — Desktop module completion · *desktop* · can run in parallel with Phases 2–5 (different files)

| Item | Detail |
|---|---|
| 6.1 | **Permit/LOTO:** the full write UI, the two-person isolation verification, a removal-verified gate before close, and Rust tests. |
| 6.2 | **Inspections:** a field-grade checklist runner (pickers, no raw IDs or UUIDs, large touch targets, offline queue visible), plus Rust tests. Qualification Rust tests. |
| 6.3 | **Reliability math:** censored Weibull MLE on exposure time; eligibility and failure-mode filters; pooled fleet KPIs; separate A_i and A_o; MTTF; the readiness gate wired into the app (IPC + UI, no `min_sample_n=1` bypass); transactional failure-event ingest; algorithm version and dataset hash on snapshots; FTA via minimal cut sets. |
| 6.4 | **Work orders:** the 12-state model per PRD §6.5 (with migration of the existing 8-state rows), a reopen-within-window setting, keep `technically_verified_at` on reopen, backlog heatmap, QR on the print sheet, and real PM/permit links in the analytics feed. |
| 6.5 | **RCM/FMECA → PM loop:** RCM decision logic (consequence → tactic), FMECA actions that create WOs or PM plan revisions with traceability, and a criticality number. |
| 6.6 | **Analytics and exports:** Pareto by failure mode and cost; a proper PDF engine (multi-page, Unicode/accents, charts); reliability/FMECA/Weibull export. |
| 6.7 | **Notifications:** OS toast plus an email channel (sent through the API, no SMTP credentials on the desktop); hash-chained audit log. |
| 6.8 | **Reference data:** publish-impact analysis for WOs, inventory and reliability, and a `unit_conversions` entity. |
| 6.9 | **Quality:** i18n sweep (~207 hardcoded French strings), Rust tests for analytics/reports/data_integrity/jobs/security, a WebDriver e2e smoke suite on the Tauri build, and dead-code cleanup (`_tmp_test_file.rs`, the removed-command client, legacy redirect pages). |

**Exit:** every PRD §6 core module passes its acceptance scenario in the e2e suite in both FR and EN; the reliability numbers match the reference test values.

### Phase 7 — Enterprise scope · *desktop + API* · after v1.0

| Item | Detail |
|---|---|
| 7.1 | **Configuration engine (§6.26):** tenant custom fields, configurable form layouts, and workflow rules within guardrails. It syncs as tenant config through Phase 2. |
| 7.2 | **ERP connectors (§6.22):** a connector framework (mapping, scheduling, error queue) on the API side, with first connectors for finance actuals, inventory masters and WO cost posting. This builds on the existing finance export contracts. |
| 7.3 | **IoT gateway (§6.21):** an MQTT/OPC-UA edge ingest (API or on-site gateway) that writes meter readings and condition alerts, feeding the PM meter triggers, inspections and runtime exposure (this closes the exposure gap in the reliability engine). |

### Phase 8 — Pilot & go-live

- Performance budgets: a 100k-WO tenant, sync throughput, and startup time (the existing `startup-time-gate.yml`).
- A migration rehearsal on a copy of real customer data.
- Runbooks: the incident, restore and key rotation runbooks.
- A pilot site with telemetry, and go/no-go criteria per PRD §15.

---

## 3. Order and parallelism

```
Phase 0 ──► Phase 1 ──► Phase 2 (sync v2) ──► Phase 3 ──► Phase 4 ──► Phase 5 ──► Phase 8 (v1.0)
                 └────────── Phase 6 (desktop modules, parallel track) ──────────┘
                                                                     Phase 7 (after v1.0)
```

- Phases 0–1 come first: they close exploitable holes.
- Phase 2 is the critical path for the "synchronisation working" goal. Phase 3 items 3.1 and 3.6 can start alongside it.
- Phase 6 runs in parallel on separate files. Its items 6.1, 6.2 and 6.3 are independent of each other.
- **v1.0 = Phases 0–6 + 8.** Phase 7 completes the full PRD.

## 4. Beyond v1.0: competitive edge

These are optional and come only after v1.0 is stable. Commodity CMMS products do not offer them, and Maintafox already has most of the foundations:
1. **Readiness-gated reliability.** The app refuses to show MTBF or Weibull figures on data that cannot support them, and says why. The Phase 6.3 readiness gate already provides this.
2. **Closed-loop strategy.** RCM/FMECA decisions generate PM revisions automatically, and failure recurrence after the change is measured (6.5 + analytics).
3. **True offline-first multi-site** with visible conflict governance (Phase 2), which most cloud CMMS products lack.
4. **Planner optimization:** a capacity- and skill-aware auto-scheduling suggestion, built on the existing planning gates.
5. **Field companion (mobile/tablet)** for inspections and WO execution, reusing the sync v2 protocol. **This is not in the current PRD scope, so it needs a product decision.**

## 5. Phase 0 execution log (2026-10-10)

Work is on branch `phase0/security-hardening` in both repos. Each side has a baseline commit of the work that was previously uncommitted.

**Done (vendor-console `api/` + console):**
- Tokens are scoped by `aud`/`iss`/`kind` (`api/src/auth-tokens.ts`), so device and step-up tokens are rejected on admin routes and vice versa.
- Real step-up: `POST /api/v1/auth/step-up` re-checks the password and issues a 5-minute token bound to the operator. Privileged routes verify it and always require a reason.
- Rate limits on login, step-up, activation, public checkout and device routes.
- Body limit of 32 MB on sync only, plus nginx `client_max_body_size`; `outbox_batch` is capped at 1000.
- No fallback secrets; the API exits when a required variable is missing; compose requires the DB, MinIO and admin variables.
- The console dialog uses server step-up. Every privileged call sends a reason and a token. Buttons with no server endpoint are disabled with an explicit reason.
- Tests: new `test:auth-boundaries`. All API integration scripts and 24/24 console tests pass.

**Done (desktop):**
- `submit_product_license_key` is open before login only while the activation is unusable (onboarding or recovery). Otherwise it needs `adm.settings` plus step-up.
- `shutdown_app` requires a session.
- 10 debug blocks writing to a hardcoded developer path are removed.
- The fs capability is reduced to Save-dialog files plus temp print files; the asset scope covers temp print files only.
- Production CSP no longer lists the dev origins; a `devCsp` keeps HMR working.
- Fixed the mojibake in WO completion-blocker matching.

**Additional findings discovered during Phase 0 (added to later phases):**

| Finding | Where | Phase |
|---|---|---|
| Server step-up was a length check (any 8 chars); compose disabled the guard by default | API | fixed in 0 |
| nginx had no `client_max_body_size` (1 MB default), so large sync pushes already failed | deploy | fixed in 0 |
| Public SlickPay checkout creates tenants from unauthenticated input (rate-limited now) | API | 4.4: add bot protection and checkout validation |
| No server-side RBAC: every vendor operator has every permission | API | 4.3 |
| Licence key stored in plain text in `app_settings` (`license_key_plaintext`) | desktop | 1.2: move to the OS keyring |
| Unauthenticated licence actions write `changed_by_id = 0` (fake user) into settings audit | desktop | 1.5: system-actor support in `settings::set_setting` |
| `get_product_activation_token` / `run_integrity_check` are pre-auth by design | desktop | 2.3 done for sync: the sync worker reads the token in Rust and `get_control_plane_activation_bearer_token` is removed. Still open: the licence checks (tenant status, policy refresh) run in the webview with `get_product_activation_token`; moving them to Rust removes the last command that returns the token |
| "Show in folder" uses `shell.open` on a folder path; the default shell scope only allows URLs, so it is probably broken | desktop | 6.9: switch to the opener plugin's reveal |
| Double-encoded UTF-8 (mojibake) in at least 20 files on develop, some user-visible | desktop | 6.9: encoding sweep |
| `style-src 'unsafe-inline'` still in the CSP (needed by the current UI libraries) | desktop | 6.9: evaluate nonce/hash |

**Breaking (acceptable, nothing in production):** existing console sessions and desktop activation tokens become invalid, so operators log in again and desktops re-activate. Deployments must add `ENTITLEMENT_ISSUER_SECRET_V1`, `SEED_ADMIN_EMAIL`, `DATABASE_URL`, `POSTGRES_*` and `MINIO_*` to `.env`.

**Still open until Phase 1.1:** licence envelopes are still symmetric-signed, and the desktop still has `SOFT_ACCEPT_CONTROL_PLANE_ENVELOPES = true`, so envelope forgery is **not yet** prevented on the desktop side. Ed25519 is the next item.

### CI cleanup (2026-10-10/11)

**Finding: CI on `develop` and `main` had been red since at least 2026-08-01.** Clippy failed first, so the Rust unit tests never ran in CI; the Vitest job was red too. Fixed:

- **Lint policy.** `#![deny(clippy::pedantic)]`, plus `nursery` (which `-D warnings` makes fatal), produced about 1,985 errors. The policy is now `deny(clippy::all)`; pedantic and nursery are tracked as gradual debt. `too_many_arguments` (37 functions) and `type_complexity` (4 types) are allowed until they are refactored into input structs. About 20 real `clippy::all` findings were fixed.
- **MSRV.** A hidden `src-tauri/.clippy.toml` pinned `msrv = 1.78`, but the code needs 1.82. Both it and `Cargo.toml` `rust-version` are now 1.82.
- **RBAC parity:** `pm.approve` is enforced in the backend and the frontend, so the registry now says `usage: both`. The generated artifacts were regenerated.
- **Frontend tests (10 failing, now all 432 pass):** IPC call assertions were updated to the central wrapper, the i18n namespace list was refreshed, and the archive tests mock an authenticated session.
- **Rust tests (39 failing locally on first run).** Fixtures were updated for these changes:
  - time bombs: fixed dates against the 24 h anti-replay window
  - crews (`teams`) replacing org nodes as the budget team
  - the no-orphan-WO and installed-node rules
  - the org structure model being required
  - the 7-state DI model
  - snapshot counts and the golden hash after legitimate changes

**Real production bugs found by the tests that CI never ran:**

| Bug | Impact | Fix |
|---|---|---|
| `count_open_di` still queried `intervention_requests.asset_id` after migration 146 renamed it | **Asset decommission failed for every asset** (SQL error) | Query `equipment_id` |
| Successor budget version INSERT had 15 values for 14 columns (since commit `a5c01445`, April) | **Creating a successor budget version always failed** | Placeholder count fixed |
| `AssetBindingSummary` had no stale-response guard | Switching assets quickly could show **another asset's** bindings | Request-id guard |
| `normalizePreflightCode` compared mojibake literals | French completion blockers showed as a generic error | Real characters |
| Licence compromise response hardcoded `key-v1` | After Ed25519, the emergency action would target a key that does not exist | Uses the active envelope's `key_id` (new `envelope_key_id` in the status view) |

**Speeding up local Rust checks:** run clippy plus the tests of the touched modules with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` (no debug info, so compile and link are much faster). Open a draft PR into `develop` so GitHub runs the full suite.

### Phase 1.3 — Backups: restore, scheduled backups, factory reset (2026-10-10)

- **Restore** (`restore_from_backup`, `backup/restore.rs`): the backup must pass the integrity check
  with this installation's key and must not carry migrations unknown to this build. The current
  database is saved first (`trigger = pre_restore`), the backup is staged as
  `maintafox.db.restore-pending`, and the app restarts. At startup the file is swapped in before the
  database opens, with rollback if the rename fails; stale `-wal`/`-shm` files are removed and
  migrations then upgrade an older schema.
- **Scheduled backups** (`backup/scheduler.rs`): they follow the backup policy `schedule_cron` in
  local time, and 5-field cron expressions are normalised. A run missed while the app was closed
  runs at the next tick. Output goes to `<app_data_dir>/backups/maintafox-YYYYMMDD-HHMMSS.db`.
  Failures are recorded as `failed` runs and retried after 1 h. Retention is
  grandfather-father-son from `backup.retention_daily/weekly/monthly` and only deletes scheduled
  files inside the backups folder. Policy validation now rejects an unparseable cron.
- **Factory reset** (`factory_reset` replaces `factory_reset_stub`): it writes a final
  `pre_factory_reset` backup, then reuses the activation-reset wipe (`reset_to_first_run`, now
  shared with `reset_product_license_activation`) and restarts into first-run setup.
- `backup_runs.trigger` is now set by the caller (`manual`, `scheduled`, `pre_restore`,
  `pre_factory_reset`), and system runs record `initiated_by_id = NULL`.
- **Open point:** the backup policy's `retention_days` field is not used. Retention comes from the
  three `backup.retention_*` settings. Remove the field or map it in a later settings cleanup.

### Phase 1.4 — Licence lifecycle enforcement (2026-10-10)

**Server (vendor-console `9462503`)**
- Every device request (sync, policy refresh, tenant status) goes through
  `activationAuthMiddleware`. It checks that the token is the latest issued to the machine
  (`401 token_superseded`) and that the tenant, licence and machine are still valid
  (`403 tenant_inactive | license_revoked | license_expired | activation_revoked`). It also
  updates `last_seen_at`, at most once a minute.
- New operator routes `POST /tenants/:id/machines/:fp/revoke` and `/reinstate`: they need
  step-up and a reason, are audited and idempotent, and reinstate needs a free slot. The console
  Machines page has live Revoke and Reinstate buttons in place of the disabled "Soft suspend"
  placeholder.
- New column `machine_activations.revocation_kind` (`machine` or `license`). **Bug fixed:** a
  claim used to reset `revoked_at`, so a desktop re-entering its key silently undid a
  revocation. Preview and claim now refuse an operator-revoked machine.
- **Bug fixed:** the slot check skipped any machine that already had a row, including a revoked
  machine or one bound to another licence.
- Policy refresh replaces device tokens older than 7 days, and the old token stops working.
- New test `test:machine-revocation`; all existing API suites still pass.
- **Deployed to the VPS on 2026-10-10.**
  - Backup: `/opt/backups/20261010T124158Z/`.
  - Rollback folder: `/opt/maintafox-vendor-console-prev-3da1090` (swap the folders back, then
    `docker compose up -d --build`).
  - The new column was added on startup; the 4 existing machines are unchanged.

**Desktop (`96acb3aa`)**
- A licence check runs every 15 minutes (it was every 6 hours). A sync refused with 401/403
  starts a check immediately.
- The desktop re-claims with the cached licence key only after `missing_token`,
  `invalid_token` or `token_superseded`. A revocation or any other denial is recorded as a
  denial. `activation_revoked` and `tenant_inactive` map to `denied_revoked`.
- New status `denied_offline_grace_expired`: an active or degraded activation with no
  successful contact for the licence's `offline_grace_hours` (default 72 h, set per tenant in
  the console) is blocked. The status is computed when read, so it also applies while offline,
  and it clears at the next successful check.
- **Known limits:**
  - The block is enforced by the licence screen (UI gate) plus the server refusing sync. Local
    Rust commands do not check licence status yet.
  - The offline grace uses the device clock, so setting the clock back extends it.
  - Other per-machine console buttons (Slot release, Rebind, Policy refresh) are still disabled
    placeholders.

### VPS deployment of the Phase 0/1.1 control plane (2026-10-10)

- The server (188.165.123.199, SSH key login) now runs vendor-console `3da1090`; it was on
  `5badef9`. Every hand edit in the old server folder matched baseline `911028c` (83/83 files),
  so nothing was lost.
- Backup taken first: `/opt/backups/20261010T120725Z/` holds the app folder with `.env`, a
  `pg_dump` of the sync Postgres, and the control-API and MinIO volumes.
- `.env` gained `ENTITLEMENT_SIGNING_KEY_ID` and `ENTITLEMENT_SIGNING_PRIVATE_KEY`; all other
  values are unchanged and already met the new secret rules.
- Smoke-tested over HTTPS: health 200, console 200, and admin routes return 401 with no token or
  a forged token. Step-up returns 401 without a session, and policy-refresh without a token
  returns `missing_token`. Ten failed logins get 429 from the rate limit.
- **Rollback:** `cd /opt && mv maintafox-vendor-console maintafox-vendor-console-3da1090 &&
  mv maintafox-vendor-console-prev-5badef9 maintafox-vendor-console && cd maintafox-vendor-console &&
  docker compose up -d --build`. If needed, restore the data volumes from the backup folder.
- Not yet verified: an admin console login plus step-up with real credentials (the owner does this).
  The API runs Node 20; the AWS SDK warns that its updates after January 2027 require Node 22.

### Phase 1.5 — Permission enforcement (2026-10-10)

- **`fin.approve` (owner decision: Administrator + finance managers).**
  - A budget version moving to `approved`, or a variance review moving to `accepted`/`closed`,
    now needs `fin.approve` and a fresh step-up. Other transitions keep `fin.budget`.
  - BudgetPage: Approve, Accept and Close are enabled only with `P.FIN_APPROVE` and ask for the
    password again.
  - New editable role **Finance Manager**, created by the seeder with `role_defaults.finance_manager`
    (fin.view, budget, report, post, approve, plus read access to org, ref, eq, ot, inv, pm, plan,
    rep and doc). There is also a matching role template.
  - `fin.approve` is added to `supervisor_exclusions`. Migration 151 removes the existing
    Supervisor and Maintenance Supervisor grants and records `fin.approve → fin.view`.
- **`per.sensitiveview`, enforced in the backend.**
  - `list_personnel` and `get_personnel` withhold email, phone, HR id, contract number and dates,
    and notes (`sensitive_masked = true`, values `None`) unless the caller holds the permission or
    the record is their own.
  - `get_personnel` drops rate cards in the same case. `list_rate_cards` and
    `get_active_rate_card` refuse, and the planning commit dialog already handles a missing
    rate. Availability-block `reason_note` is withheld (`reason_masked`).
  - The detail screen shows "Restricted" instead of empty values. Planning and WO cost
    calculations read rates server-side and are unaffected.
  - Default holders: Administrator, Supervisor and Maintenance Supervisor. Operator, Planner and
    Readonly don't hold it.
- **Settings actor:** `set_setting` and `delete_setting` take `Option<i32>`. Licence, startup and
  bootstrap writes record NULL instead of a fake user 0. Diagnostics bootstrap records the real
  admin.
- **Not done, on purpose — lookups stay "any signed-in user", not `ref.view`.** The registry role
  templates (Supervisor, Technician, Planner, Observer) don't include `ref.view`, so gating would
  empty the dropdowns on every form for roles built from them. The values are non-sensitive
  vocabularies, and editing still needs `ref.manage`.
- **Known gaps:**
  - `create_personnel` and `update_personnel` return the full record to `per.manage` holders
    (Supervisors, who hold `per.sensitiveview` by default).
  - Workforce report exports were not reviewed for sensitive fields.
  - The variance review commentary sent by BudgetPage is a fixed string (pre-existing).
  - `fin.post` is marked `requires_step_up` but `require_fin_post` doesn't enforce step-up
    (pre-existing).

### Phase 2.2 — Desktop sync foundations (2026-10-10)

- **Registry:** `src-tauri/src/sync/registry.rs` lists 201 replicated tables (scope, stager,
  materializer, conflict policy, parents → apply order) and 46 local-only tables. Exported to
  `shared/sync/entity-registry.json` for the server allowlist. CI tests fail on a table in neither
  list, a parent cycle, an undeclared FK parent, a staged type missing from the server allowlist
  copy, and any raw INSERT into a synchronised table without `entity_sync_id`.
- **Identity:** every insert sets `entity_sync_id`; seeded system rows (roles) use a deterministic
  id; stagers never invent ids. Migration 160 backfills missing ids.
- **Transactional staging:** `warn_stage` is gone; every stager runs in the business transaction and
  a failed stage fails the write (≈90 call sites, incl. finance, permits, inspections,
  qualification, reliability).
- **Outbox/inbox model (migration 160):** base_row_version, owner, origin replica id, attempts,
  in_flight / dead_letter / superseded; inbox deferred state; per-entity server version.
- **Inbound:** unknown or not-yet-applied types stay `deferred` (rows wrongly marked applied before
  were re-flagged); children wait for parents; conflicts (concurrent edit, stale, equal-version
  divergence, materialize error) are written to `sync_conflicts` and counted in the summary.
- **Conflicts:** keep mine re-stages on the server version; take theirs applies the server payload.
  Sync Center shows the kind and versions (FR/EN).
- **Open for 2.3/2.4:** see `docs/engineering/SYNC_STRATEGY.md` and the session notes: seeded
  catalog rows created by old migrations still have per-database ids; delivery-failure hook not
  wired to the transport; deletes mostly not staged; legacy payloads keep local integer FKs.

### Phase 2.3 — Rust sync worker (2026-10-11)

- **Transport in Rust:** `src-tauri/src/sync/transport/` runs as background task `sync` in
  `BackgroundTaskSupervisor`, speaking protocol v2 only (exchange, paged snapshot bootstrap,
  `has_more` paging, `user_snapshot_required` user snapshots, media manifest + presigned upload).
  `reqwest` with rustls/ring, already in the dependency tree through the updater (no OpenSSL);
  its `system-proxy` feature (one new crate, `windows-registry`) keeps following the Windows proxy
  settings like the webview did. The webview transport (`sync-vps-transport-service.ts`), its scheduling core
  and the commands it used (`get_control_plane_activation_bearer_token`, `get_sync_push_payload`,
  `apply_sync_batch`, `materialize_sync_inbox`) are removed: the webview can no longer read the
  activation token for sync or inject sync batches.
- **Crash safety:** each request (idempotency key, exact body, outbox ids) is stored in
  `sync_transport_state` (migration 161) before it is sent and resent unchanged after a failure or a
  restart; the checkpoint is stored with the applied answer. Acks now store `server_seq` and
  `duplicate`.
- **Reliability:** offline / 429 (`Retry-After`) / 5xx / 401–403 / checkpoint refusal / 409 handled
  as listed in `SYNC_STRATEGY.md` §3; exponential backoff with jitter (2 s → 5 min), no retry budget
  to exhaust; transport failures go through `record_outbox_delivery_failure` (dead letter after 25);
  a refused activation stops sync until the token changes and makes the UI run a licence check.
- **Runs with the window closed:** closing the window already hid it in the tray; the worker lives in
  the Rust process, so sync continues until the app is quit from the tray.
- **Sync Center (FR/EN):** status card (state, reason, last success, pending, conflicts, next
  attempt, media, last error) with sync now / pause (15 min–24 h, persisted) / resume, in the Sync
  Center and in Settings; top bar and status bar labels translated. The simulated "entitlement /
  bandwidth" selectors of the old panel are gone.
- **API base URL:** one default in `shared/control-plane.json`, read by the webview and by
  `src-tauri/build.rs`; `VITE_ADMIN_API_BASE_URL` overrides both.
- **Fixed on the way:** "reset local tenant data" deleted the `sync_replica_identity` row, after
  which every business write failed at staging; the reset now re-creates it (and the transport row).
- **Tests:** `cargo test --lib -- sync::transport` (worker against an in-process HTTP stub, 21 tests).
- **Not verified here / open:** no end-to-end run against a local vendor-console API (Phase 2.5);
  media download from other devices (`presign-get`) not implemented (2.4); a database restored from a backup keeps its old
  checkpoint, so rows written after the backup are not re-sent by the server (2.5: force a snapshot
  after restore).

### Phase 2.4 — Coverage of every domain (2026-10-11)

- **One generic engine instead of hand-written stagers and materializers**
  (`src-tauri/src/sync/`: `capture.rs`, `row_spec.rs`, `row_layout.rs`, `row_codec.rs`,
  `apply_hooks.rs`, `seeded_identity.rs`). SQLite triggers capture every change inside the
  business transaction; a rule per table (`RowSpec`) says how each local id travels (as the
  target's sync id, as a catalog key, through JSON paths or URL parameters, or not at all);
  inbound rows are applied by one materializer that defers a row whose parent has not arrived.
  Migrations 162 (capture tables, `entity_sync_id` on 111 tables, seeded identities), 163 (PM meter
  due basis by sync id), 164 (last foreign keys on the legacy `users` stub moved to
  `user_accounts`; planning/PM writes with real user ids failed before).
- **Coverage:** every table in the registry syncs (test `registry_flags_match_stager_and_materializer_dispatch`):
  foundation, inventory/procurement (a received movement updates stock balances), WO and DI, PM and
  planning, finance, personnel and qualification, inspections and permits, RAMS, data integrity,
  analytics contract, settings, history logs, notifications, reports, and per-user items
  (notification preferences, saved filters, dashboard layouts, RAMS dismissals, notification read
  state) delivered only to the devices where their owner is signed in. Domain commits: 97a6ebf9,
  72af1028, 78a015d0, cdb82568, 0b6a500b, ada22034.
- **Data each device makes for itself** (PM occurrences, schedule candidates, data-integrity
  findings, automatic notifications, escalation) is generated with capture off and with ids built
  from sync ids, so a person's action on it (waive, read, acknowledge) reaches the same row on the
  other devices. Projections recomputed per device are local-only (scheduling conflicts,
  readiness snapshots, training expiry alerts, inspection signals, reliability KPI snapshots), as
  are archives, event links and library documents (no transfer path for their files yet).
- **Server allowlist:** 133 types appended to `maintafox-vendor-console/shared/sync-entity-registry.v2.json`
  (vendor commits 4a95b48, 19178ef, add7d5e, 38ac625); `test:sync-v2` passes. **To deploy** with the
  API. Server entries the desktop no longer sends: `user_role_assignments` (no desktop table),
  `reliability_kpi_snapshots`, `personnel_readiness_snapshots`, `training_expiry_alert_events`,
  `inspection_reliability_signals` (left in place: the registry file is append-only from this side).
- **Fixed on the way:** an update of device-local columns only (login time, SLA "notified" stamp)
  was sent, so every device's SLA poller pushed the same request (false conflicts); the baseline
  skipped 1 in 16 rows whose id had a prefix (`failure_event:<uuid>`); `PO_GR` movements pointed to a
  goods receipt or a PO line depending on the path (now always the receipt).
- **Known limits:** local ids remain inside some history texts (audit `target_id`, activity and
  change event summaries, notification event payload fields other than the routing ids); WO/DI
  code numbering is per device and can collide; media files received from another device are not
  downloaded; the same notification read on two devices before they sync is a conflict for the
  person (no silent last-write-wins); a user's report schedule runs on each of that user's devices.

### Phase 2.5 — Proof (2026-10-11)

- **Registry-wide round trip:** `every_registered_type_round_trips` (`sync/row_sync_tests.rs`)
  takes every type from the registry, fails if one is in no domain round trip, and sends all of them
  between two devices (create, edit, delete). Passes (19 s).
- **Multi-device harness:** `pnpm sync:harness` (`scripts/sync-harness.mjs`,
  `src-tauri/src/sync/harness_tests.rs`). The real control API from `../maintafox-vendor-console`
  on embedded PostgreSQL 16, three desktop databases syncing through the real Rust worker over
  HTTP, everything on 127.0.0.1. Nine scenarios pass: create; concurrent edit shown as a conflict
  and resolved keeping mine and taking theirs; offline then reconnect; per-user rows only where the
  owner is signed in; delete (tombstone); snapshot bootstrap of a new device; answer lost after the
  server committed, resent once and applied once; backup restore; revoked device refused.
- **Backup restore (2.3 gap closed):** after a restore is swapped in, startup drops the sync
  checkpoint and unanswered request (`transport::store::forget_server_position`), so the worker
  re-bootstraps from a snapshot and gets back the rows this device wrote after the backup.
- **Tests:** full `cargo test --lib`: 1013 passed, 0 failed, 1 ignored (the harness, which needs
  the API; run through `pnpm sync:harness`), 88 min. `cargo clippy --lib -- -D warnings` clean,
  `pnpm typecheck` clean, vendor `test:sync-v2` passes.
- **Not verified here:** the harness runs the API on this machine, not on the VPS; nothing was
  deployed. Media download, two people editing at once through the real UI, and very large
  tenants (snapshot size, time) are not covered by the harness.

### Phase 2 leftovers — files, numbers, history ids, large tenants (2026-10-11)

- **1. Files from other devices** (desktop `235304c8`, vendor `1ff8562`): received
  `media_attachments` rows queue a download (migration 165); the sync worker fetches the file
  through `presign-get`, checks size and SHA-256 while streaming, and only then moves it into
  place; retries with backoff, never blocks sync. WO/DI attachments, asset photos and library
  documents show "not downloaded yet / download failed, retry / not on this device" (FR/EN). The
  document library now syncs (row + file); archives stay device-local (no production writer).
  Fixed on the way: asset photo upload re-read the wrong row. Server: `library_documents` added to
  the registry (deploy).
- **2. Record numbers** (see SYNC_STRATEGY §6a): new WO/DI/personnel/permit numbers carry a
  4-character device code (`OT-7KQ2-0001`), so devices never hand out the same number; old numbers
  unchanged. A clash (old numbers, or two devices sharing a code) is a `CODE_CLASH` conflict with
  one action, "renumber the record on this device"; never renamed silently. Migration 166 re-types
  clashes already recorded as technical errors.
- **3. History ids** (SYNC_STRATEGY §6b): audit targets/details, activity summaries, admin
  scope/diff, request and schedule change details and notification payloads travel as sync ids;
  rows received before this version are marked and shown as "from another device" (migration 167).
  Fixed on the way: article stock state events could not be sent (unmapped type); notification
  de-duplication keys no longer travel (they would suppress unrelated events on another device).
- **4. Large tenant:** measured with `pnpm sync:harness --large N` (SYNC_STRATEGY §12). Before:
  a bootstrap of even 4 000 rows left almost every row unapplied (waiting rows blocked the queue),
  and a snapshot over ~60 000 rows could never finish (server 429 restarted it from page 1). After:
  100 008 work orders (333 574 rows) bootstrap in 15.5 min on the development PC (671 MB received,
  peak memory 202 MB, device file 1.47 GB, 0 conflicts); the server's snapshot page query takes
  ~6.5 ms; one change in steady state travels in < 50 ms. Fixes are desktop-only (apply order,
  chunked transactions, two indexes, resumable snapshot, 1 000-row pages); no server change.
  Follow-up: compact applied inbox rows (they double the device file during a bootstrap).
- **Tests:** full `cargo test --lib`: 1022 passed, 0 failed, 2 ignored (the two
  harness tests, run through `pnpm sync:harness`), 91 min; `pnpm sync:harness`: 11 scenarios
  pass (incl. 9 files through object storage, 10 number clash); `cargo clippy --lib -- -D
  warnings` clean; `pnpm typecheck` clean; vitest (sync, activity, admin, stores): 39 passed;
  vendor `test:sync-v2` passes. Commits: desktop 235304c8, 52c82684, 8201a98a; vendor 1ff8562
  (deploy: registry gains `library_documents`).
- **Not verified here:** uploads/downloads against real MinIO (the harness uses a local
  S3-compatible stand-in that checks the presigned query but not the signature); the new UI states
  in the running app (covered by component tests only); timings on a field PC.

### Phase 3 — Control plane hardening (2026-10-12)

Vendor console commits (branch phase0/security-hardening): f8bc541 3.5, 66309f5 3.6, 11eea48 3.1,
300ae9c 3.2, 72c6805 3.3, b6fbdd3 3.4, e6da4b1 `unit_conversions`, 43cdc6a notification email
(+ CI fixes e32d6da, 73d27fe). Desktop: 8cdc8630 (send support bundle). CI on 43cdc6a: all 7 jobs green.

- **3.5 Backups:** nightly `pg_dump` + `pg_basebackup`, continuous WAL archiving (15 min), local
  retention, MinIO bucket versioning, monthly automatic restore drill (row counts per table vs the
  dump, migrations, audit chain), every run in `backup_runs`. Off-site copy to any S3 bucket, age-
  encrypted to the owner's key, **disabled until the owner buys a bucket** (local copies only).
  Drill verified locally with real PostgreSQL 16 (backup, drill, corrupt/tampered refusals,
  point-in-time recovery both ways) and in CI. Runbook `deploy/BACKUP_AND_RESTORE.md`.
- **3.6 CI:** lint (new ESLint), typecheck, console tests, build, `test:all` on Postgres 16,
  backup drill, off-site + bundle upload against an S3 server, Docker images, `pnpm audit`
  (high), gitleaks full history. Fixed on the way: audit findings (vite, overrides), lint findings,
  a stale tenant-selection closure in the console, an embedded-Postgres orphan process on Windows.
- **3.1:** `index.ts` (4,500 lines) split into 11 route modules + `app/` helpers; route
  inventory identical; `test:all` before/after.
- **3.2:** Postgres row-level security on every tenant table (role `mfx_app`, per-transaction
  tenant/operator scope, operator access recorded); `user_accounts` payloads and stored exchange
  responses AES-256-GCM encrypted at rest, delivery byte-exact. Test proves no-scope sees nothing
  and tenant A cannot read/update/insert B.
- **3.3:** ops figures measured from the sync store (per-device lag and checkpoint age, rejection
  rate, dead letters, conflicts, retries); heuristics and invented services removed; Prometheus
  endpoint `/api/v1/ops/metrics`.
- **3.4:** support tickets (operator and device), diagnostic bundles uploaded from the desktop to
  object storage (presigned, verified by size + SHA-256) and downloadable from the console
  (audited), editable runbooks with real procedures, readiness computed; seed fixtures removed.
- **Queued:** registry `unit_conversions`; `POST /api/v1/notifications/email` (SMTP from env,
  capability `notifications.email` off by default).
- **Found:** `minio/minio` and `minio/mc` images can no longer be pulled (Docker Hub, quay.io); the
  VPS runs from its cache. Owner decision needed (`deploy/CI.md`).
- **Not deployed.** New env: `PG_WAL_ARCHIVE_TIMEOUT_SECONDS`, `BACKUP_*`,
  `MINIO_NONCURRENT_VERSION_RETENTION_DAYS`, `SYNC_PAYLOAD_ENCRYPTION_KEY`,
  `OPERATOR_ACCESS_LOG_RETENTION_DAYS`, `EMAIL_DELIVERY_MODE` (+ optional `METRICS_SCRAPE_TOKEN`,
  `SMTP_*`). Migrations 006–010.

## 6. Risks and open points

- **D2 departs from the PRD.** The server would use a generic current-state table instead of typed per-entity tables, so the PRD needs an update if you approve it.
- **Encrypting existing desktop DBs (1.2) is irreversible for users** if the keyring entry is lost. A recovery-key export flow is required before release.
- **Consolidating SQLite into Postgres (D1) needs a one-time data migration** of production tenants and licences, if the API is already live. Confirm whether `api.maintafox.systems` currently holds real customer data.
- **WO 8→12 state migration (6.4)** changes status analytics history. Map the old states carefully and keep the original value.
- **Installed desktops in the field:** if any exist, D6 dual-mode applies to Phases 1 and 2. Otherwise, skip it.
- The earlier session card "Security hardening: entitlements, capabilities, IPC guards" covered only the desktop side. Ed25519 must change on both sides together, so that card is replaced by Phase 0 + 1.1.
