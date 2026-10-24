# Maintafox Desktop — Completion Gap Audit (2026-10-09)

Read-only audit of `develop` against `docs/PRD.md` v3.0 and `docs/roadmap/**`.
Every finding cites code evidence (`path:line`). Critical items were re-verified by hand.

> **Note:** `docs/roadmap/phase-5-.../gaps/README.md` says Permits, Training and Inspections are
> placeholder pages. That is **out of date**: all three have real backends, and every command is
> registered. The real gaps are listed below.

---

## 1. Module status at a glance

| Module | PRD | Backend | UI | Sync | Tests | Verdict |
|---|---|---|---|---|---|---|
| Org & Sites | §6.2 | Done | Done | Partial | Yes | Solid |
| Asset Registry | §6.3 | Done | Done | Partial | Yes (some) | Solid |
| Intervention Requests (DI) | §6.4 | Done | Done | Done | Yes | Solid |
| Work Orders (OT) | §6.5 | Done (8 of 12 states) | Done | Done | Yes | Near-complete |
| Reference / Lookups | §6.13 | Done | Done | Partial | Yes | Near-complete |
| Personnel | §6.6 | Done | Done | Done | Yes | Solid |
| Inventory & Procurement | §6.8 | Done | Done | Partial (no PO/requisition sync) | Yes | Near-complete |
| Preventive Maintenance | §6.9 | Done (all strategies, scheduler) | Done | Done | Yes | Solid |
| Planning & Scheduling | §6.16 | Done | Done | **Missing** | Yes | Sync gap |
| Work Permit / LOTO | §6.23 | Done | **Read-only** | Done | **None** | **Blocking** |
| Training / Habilitation | §6.20 | Done | Done | Done | **None** | Needs tests |
| Inspection Rounds | §6.25 | Done | Developer-grade | Done | **None** | UI + tests |
| Budget & Cost Centers | §6.24 | Done | Done | Partial | Yes | Sync gap |
| Reliability (RAMS) | §6.10, §9 | Done with **math defects** | Done | Partial | Partial | **Correctness** |
| Analytics & Dashboards | §6.11 | Done with **wrong fleet KPIs** | Done | — | None | **Correctness** |
| Sync layer | §7–8 | Local engine done | — | ~20 of 63 types applied inbound | Yes | **Major gap** |
| Licensing / Entitlements | §10 | **Insecure** | Done | — | Yes | **Blocking** |
| Auto-update | §11 | **Placeholder** | — | — | — | **Blocking** |
| Backup / Restore | §6.18 | Partial (plaintext, no restore) | Partial | — | Some | Gap |
| Archive | §6.12 | Done | Done | — | Yes | Solid |
| Notifications | §6.14 | In-app only | Done | — | Some | Partial |
| Activity / Audit | §6.17 | Done (not hash-chained) | Done | — | Some | Solid |
| Settings / Profile | §6.18–19 | Done | Done | — | Yes | Solid |
| Documentation / Support | §6.15 | Document library only | Partial | — | — | Partial |
| IoT Gateway | §6.21 | **Missing** | Route redirects | — | — | Missing |
| ERP Connectors | §6.22 | Manual finance export only | Route redirects | — | — | Missing |
| Configuration Engine | §6.26 | **Missing** | Route redirects | — | — | Missing |

---

## 2. Release blockers (P0) — must fix before any customer install

| # | Gap | Evidence | Fix |
|---|---|---|---|
| P0-1 | **No database encryption at rest.** This contradicts ADR-002 and PRD §12.1. | `src-tauri/src/db/mod.rs:26` opens plain `sqlite://`; `Cargo.toml` has no SQLCipher | Move to SQLCipher (`libsqlite3-sys` bundled-sqlcipher), take the key from the OS keyring, add a one-time migration that encrypts existing DBs |
| P0-2 | **Licence signatures can be forged.** The secrets are literal strings in the source, the check uses symmetric SHA-256, and soft-accept is on. | `src-tauri/src/entitlements/queries.rs:17-27, 67-75, 401-429` | Switch to Ed25519 verification against an embedded public key, set `SOFT_ACCEPT_CONTROL_PLANE_ENVELOPES=false`, delete the debug-log writes to `c:\Dev\...\debug-a6aab3.log` (`entitlements/queries.rs:405-422`, `commands/product_license.rs:342,371`) |
| P0-3 | **Auto-update does not work.** | `tauri.conf.json` has `pubkey: PLACEHOLDER_...` and endpoint `updates.maintafox.local/stub`; `commands/updater.rs:8` always returns "unavailable" | Generate an updater keypair, add the real endpoint, set `createUpdaterArtifacts`, implement check, install and rollback |
| P0-4 | **No code signing or installer pipeline.** | No `certificateThumbprint`/`signCommand`; `ci.yml:215` uses an empty signing key; no release workflow | Add a `release.yml` that produces a signed NSIS/MSI and the updater manifest |
| P0-5 | **Permission scope is too broad.** | `capabilities/default.json:12,18,28-29,36`: `fs:allow-remove`, home-write-recursive, `$HOME/**`, `shell:allow-open`; `tauri.conf.json` assetProtocol `$HOME/**` | Limit scope to `$APPDATA`/`$APPLOCALDATA` plus dialog-picked paths; route file I/O through Rust commands |
| P0-6 | **IPC commands with no auth check.** | `commands/app.rs:64` `shutdown_app`; `commands/diagnostics.rs:13` `run_integrity_check`; `commands/product_license.rs:1435` `get_product_activation_token` (done in 0.9.2: removed, policy refresh in Rust; claim, tenant status and initialisation in the webview remain a follow-up), `:936` `submit_product_license_key`, `:1059` `apply_product_license_reconciliation` (user falls back to `unwrap_or(0)`) | Add `require_session!` / `require_permission!` and review `bootstrap_initial_tenant_admin` |
| P0-7 | **Permit/LOTO workflow cannot be used.** The backend write commands have no UI, yet the WO gate blocks WOs that need a permit. | 8 write commands with no frontend caller (`create_work_permit`, `set_work_permit_status`, `upsert_permit_isolation`, ...); gate at `wo/execution.rs:263` → `permit/wo_gate.rs:10` | Build the permit create, activate, suspend, handover and close UI in `services/permit-service.ts` and `PermitsPage` |
| P0-8 | **LOTO safety rules are missing.** | `permit/queries.rs:1167-1226` has no applier ≠ verifier check; `removal_verified_at` is never used as a close gate | Enforce the two-person check and require verified isolation removal before close; add tests |

## 3. Correctness defects (P1) — wrong numbers shown to users

| # | Gap | Evidence | Fix |
|---|---|---|---|
| P1-1 | **The Weibull censored tail is treated as a failure.** | `reliability/advanced_rams/queries.rs:87-98`; `weibull_fit.rs:19-24` has a complete-data likelihood only | Censored MLE: add a −Σ(cᵢ/η)^β term and store censoring flags |
| P1-2 | **The Weibull dataset is inconsistent.** It uses calendar-time gaps, ignores eligibility, and pools all failure modes; RUL is then evaluated on exposure hours. | `advanced_rams/queries.rs:41-64, 1534` | Fit on exposure time, filter `eligible_unplanned_mtbf`, allow a failure-mode filter |
| P1-3 | **Fleet MTBF/MTTR/Availability are averages of ratios** and double-count overlapping snapshots. | `analytics/overview_kpis.rs:310-360` | Compute ΣT_exp/ΣF from the latest non-overlapping snapshot per asset, and filter by DQ |
| P1-4 | **The availability formula doesn't match its label.** It is labelled "Operational Availability", and downtime may be subtracted twice. | `reliability/compute.rs:102-106`; KPI-035 | Separate inherent availability (A_i) from operational availability (A_o, which includes waiting/logistics time), per PRD |
| P1-5 | **The readiness gate is not used inside the app.** The RUL path forces `min_sample_n=1`. | `reliability::readiness` is called only from `bin/rams_readiness_eval.rs`; `advanced_rams/queries.rs:1497,1572` | Expose readiness as an IPC command, block Weibull/RUL on DENY, and show the blocking reasons in the UI |
| P1-6 | **Closed-WO → failure-event ingest can fail silently.** | `wo/closeout.rs:1440-1441` only logs a warning; `queries.rs:975` sets `is_planned=false`; `queries.rs:926` counts open downtime as 0 | Make ingest transactional or queue it for retry, and surface failures |
| P1-7 | **The FTA top event is wrong when basic events are shared.** The cut-set test is also loose. | `fta_rbd_eta/fta_eval.rs:43-63, 84-98` | Compute the top event from minimal cut sets (MOCUS + inclusion–exclusion or BDD) |
| P1-8 | **The WO analytics feed is hard-coded.** | `wo/analytics.rs:325-327` returns `pm_occurrence_id: None` and `permit_ids: vec![]` | Join the real PM occurrence and permit links |

## 4. Data integrity / sync (P1) — data that silently never reaches other devices

| # | Gap | Evidence |
|---|---|---|
| S-1 | About 43 registered entity types are **skipped on inbound apply**, including permits, inspections, budget, failure events, qualification and data integrity | `sync/materialize.rs:233` (`_ => Ok(ApplyOutcome::Skipped)`) |
| S-2 | 40+ business tables have **no sync entity**: purchase orders, requisitions, goods receipts, suppliers, external companies, FMECA/RCM/FTA/RBD/Markov models, WO parts/tools/verifications/delay/downtime segments, PM executions/findings, stock balances, inventory counts, personnel skills/rate cards, schedule_*, planning windows, report templates, notification rules, retention policies, cost centers, budget actuals/commitments/forecasts | migrations vs `sync/domain.rs:342+` |
| S-3 | Planning writes nothing to the outbox | no stage call in `planning/` |
| S-4 | Asset lifecycle, decommission, hierarchy and move changes are not staged; org models, node types and equipment meters have constants that nothing uses | `assets/identity.rs:872,1131` only; `sync/domain.rs:373-374` |
| S-5 | Reference deactivate, merge and publish are not staged | `reference/values.rs:551,622` only |
| S-6 | Transport runs only in the webview (`fetch`). Sync stops when the UI isn't running; the tenant_config, device_class and media manifests are withheld; the CSP blocks non-production base URLs | `src/services/sync-vps-transport-service.ts:34-38,163` |
| S-7 | Staging failures only log a warning (`warn_stage`) and lose the change | `sync/foundation_stage.rs:204`, `wo` `warn_stage_wo` |

**Fix approach:** one sync-coverage sprint. Build a single entity registry table (entity → stager → materializer) with a CI test that fails if a migration adds a business table with no registry entry. Then fill in the stagers and materializers, and move transport into a Rust background worker.

## 5. Functional gaps (P2) — PRD features missing or incomplete

**Execution**
- WO state model has 8 macro states instead of the 12 in PRD §6.5 (`wo/workflow/state_machine.rs:43-45`). Closed WOs cannot be reopened, there is no configurable reopen window, and reopen clears `technically_verified_at` (`wo/closeout.rs:1514,1531`).
- Backlog heatmap is missing; the WO print sheet has no QR code.
- Reference publish impact shows "unavailable" for WOs, inventory and reliability (`reference/publish.rs:396,408-410`); there is no `unit_conversions` entity.

**Reliability / RCM**
- MTTF metric is missing.
- RCM has no decision logic (the tactic is picked by hand and consequence is free text), and there is **no feedback loop from RCM/FMECA into PM**. FMECA actions don't create WOs or PM revisions.
- FMECA uses RPN only, with no criticality number (Cm/Cr).
- Markov is discrete-time only (no CTMC with λ/μ); Monte Carlo is a single-distribution sampler, not a system-availability simulation; RBD has no k-out-of-n.
- Snapshots and Weibull runs store no algorithm version; Weibull runs store no dataset hash.
- Bad-actor view is top 5 by count only: no Pareto by failure mode or cost, no cumulative %.
- No reliability, FMECA or Weibull export. The PDF writer is a single-page Helvetica text dump that breaks on long content and probably on accents.

**Field usability**
- Inspection UI makes users type raw IDs and UUIDs (`InspectionsPage.tsx:531,708,757`); it needs pickers and a field-friendly checklist runner.

**Platform**
- Backup is plaintext, with no restore command and no scheduled backups; `factory_reset_stub` always fails (`backup/mod.rs:14,138`, `commands/backup.rs:92-121`).
- Notifications are in-app only (`notifications/delivery.rs:6-24`): no OS toast or email.
- Audit log is not hash-chained (no tamper evidence).
- Documentation center is a file library only: no contextual help and no support-ticket flow.

**Not started**
- IoT gateway (§6.21): permissions only; `/iot` redirects.
- ERP connectors (§6.22): manual finance CSV only; `/erp` redirects.
- Configuration engine / tenant customization (§6.26): no custom fields or workflow designer; `/configuration` redirects.

## 6. Quality debt (P2)

- **RBAC:** 10 permissions are declared but never enforced: `fin.approve`, `per.sensitiveview`, `log.admin`, `iot.*`, `erp.*`, `cfg.*`. `/settings` has no `PermissionRoute`. Lookup commands check only that a session exists, not `ref.view` (`commands/lookup.rs:20,31,41`).
- **Tests:** these Rust modules have **zero** tests: `permit`, `qualification`, `inspection`, `analytics`, `analytics_contract`, `reports`, `data_integrity`, `jobs`, `background`, `security`. There is no e2e suite (no Playwright or WebDriver). Frontend component tests are thin for WO, DI, assets and reference.
- **i18n:** FR/EN key sets match (5,463 each), but there are **about 207 hardcoded French strings** in pages and components (`ReliabilityPage.tsx` 56, `InspectionsPage.tsx` 19, `ReliabilityFoundationPage.tsx` 12, `PermitsPage.tsx` 8, ...). Three locale JSON files have a UTF-8 BOM.
- **CSP** still allows `localhost:1420` dev origins and `style-src 'unsafe-inline'`.
- **Dead code and clutter:**
  - `personnel/_tmp_test_file.rs`
  - `org-service.ts:46` `publishOrgStructureModel` (the backend command was removed)
  - unused `list_schedule_candidates` and `list_schedule_classes`
  - outdated "not_implemented" comments (`assets/bindings.rs:12`, `AssetBindingSummary.tsx:5`)
  - legacy redirect pages (`AnalyticsPage`, `ActivityPage`)
  - `debug-a6aab3.log`, `dev-data/`, `target-*` folders in the repo root
- **Swallowed errors:** `commands/archive.rs:994`, `commands/reports.rs:163`.
- **Down-migrations drop tables** (230 drops), so rollback is not a safe production path. Document forward-only plus backup/restore as the recovery strategy.

---

## 7. Recommended execution order

Work packages are sized to fit one session or PR each. Packages in the same wave can run in parallel when they touch different files.

| Wave | Work package | Covers | Size |
|---|---|---|---|
| **1 — Security & trust** | WP-01 SQLCipher encryption at rest + key in keyring + migration of existing DBs | P0-1 | L |
| | WP-02 Ed25519 entitlement verification, remove soft-accept and debug logging | P0-2 | M |
| | WP-03 Narrow capabilities, harden CSP, guard unauthenticated commands, enforce the 10 orphan permissions, guard `/settings` | P0-5, P0-6, RBAC | M |
| **2 — Blocked workflows** | WP-04 Permit/LOTO write UI + two-person verification + removal gate + Rust tests | P0-7, P0-8 | L |
| | WP-05 Inspection field UI (pickers, checklist runner) + Rust tests; qualification tests | Field usability, tests | M |
| **3 — Correct numbers** | WP-06 Censored Weibull MLE on exposure time, eligibility and failure-mode filtering, algorithm-version and hash columns | P1-1, P1-2, snapshots | M |
| | WP-07 Fleet KPI aggregation fix, A_i vs A_o split, MTTF, readiness gate as IPC + UI, transactional failure-event ingest | P1-3 to P1-6 | M |
| | WP-08 FTA via minimal cut sets, WO analytics links | P1-7, P1-8 | S |
| **4 — Sync completeness** | WP-09 Sync entity registry + CI coverage test + stagers and materializers for every business table | S-1 to S-5, S-7 | XL (split by domain) |
| | WP-10 Rust-side sync worker (transport off the webview), send manifests | S-6 | M |
| **5 — Ship pipeline** | WP-11 Updater keys, endpoint, manifest, rollback + signed NSIS/MSI release workflow | P0-3, P0-4 | M |
| | WP-12 Encrypted backups, restore command, scheduled backups, factory reset | Backup | M |
| **6 — Feature completion** | WP-13 WO 12-state model, reopen window, backlog heatmap, QR on print sheet | Execution | M |
| | WP-14 RCM decision logic, FMECA/RCM → PM/WO feedback loop, criticality number | RCM | L |
| | WP-15 Pareto/bad-actor analytics + reliability, FMECA and Weibull export with a proper PDF engine | Analytics | M |
| | WP-16 Notification channels (OS toast, email), hash-chained audit log | Platform | M |
| | WP-17 i18n sweep (~207 strings) + e2e smoke suite (Playwright/WebDriver) + dead-code cleanup | Quality | M |
| **7 — Enterprise scope** | WP-18 Configuration engine (custom fields, workflow config) | §6.26 | XL |
| | WP-19 ERP connectors (WO/inventory/finance mapping + transport) | §6.22 | XL |
| | WP-20 IoT gateway (MQTT/OPC-UA ingest → meters, condition alerts) | §6.21 | XL |

**Release line:** Waves 1–5 make a shippable, trustworthy v1.0. Waves 6–7 complete the PRD.
