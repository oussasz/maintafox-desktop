# Phase 5 — Gaps Integration Program (Index)

**Purpose:** Serialized execution tracks for **deferred PRD modules** and **cross-cutting foundations** that block RAMS credibility. This folder complements the numbered Phase 5 tracks (`01`–`07` in the parent directory).

**Authority:** `docs/PRD.md`, `docs/research/`, and the existing `maintafox-desktop/` tree (Tauri v2, `pnpm`, SQLite/SeaORM migrations, typed IPC).

**Format note:** Each sprint ends with **`## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical`** using **dual-agent dispatch** (two channels in one block):

| Channel | Audience | What to paste |
|--------|----------|----------------|
| 🌐 **Stage 1** | **VPS / infra agent** | From `🌐 [STAGE 1]: …` through the Stage 1 bullets only. **Do not** paste into the desktop/coding agent. |
| 🖥️ **Stage 2** | **Codebase agent (Cursor, etc.)** | From `🖥️ [STAGE 2]: …` through **Done:** — `[STATUS]`, **Read Only**, **Agent Rules**, **Actions**, **Sync JSON**. No Postgres/Nginx/server shell in this channel. |

Legacy `**[ATTENTION: REQUIRES VPS AGENT INTERVENTION]**` lines **above** the Ready-to-Execute block are blueprint reminders only; the **authoritative VPS list** is **Stage 1** in that block.

**Regenerate all `gaps/` prompts:** `node scripts/rebuild-surgical-prompts.mjs` (repo root). Parent Phase 5 tracks use `scripts/append-phase5-core-surgical.mjs` instead. See [Roadmap execution](../../../README.md#roadmap-execution).

**Detailed blueprint (thesis-grade):** Each sprint `.md` (except this index) includes **Standards & authoritative references** (ISO/IEC with official links where cited), **schema definition** (table/column level), **business logic & validation rules**, **mathematical triggers** (RAMS formulas with symbols), **sync transport specification** (`entity_type` + canonical `payload_json` keys aligned with `src-tauri/src/sync/domain.rs`), and the **Oussama Protocol** executor prompt. **ISO 14224:2016** failure taxonomy (**failure mode** §3.30, **failure cause** §3.24, **failure mechanism** §3.29 / Table B.2) is explicit wherever **failure events** or reliability coding appears.

---

## Cross-cutting requirement: Universal sync integration

**Rule:** Every Oussama sprint prompt that introduces or materially changes **persisted entity types** must extend the **Phase 4 sync transport layer** (outbox staging, entity type identifiers, payload envelopes, `row_version` semantics, checkpoint replay, `POST /api/v1/sync/exchange`) so **tenant data replicates to the VPS mirror**—not a local-only silo.

- Desktop: register new `entity_type` strings, serialize payloads, bump versions, stage outbox rows on authoritative writes.
- VPS: follow **Stage 1** in each sprint prompt (separate from codebase agent).

Desktop + mirror alignment: **Stage 2** stages outbox payloads; **Stage 1** applies the mirror side—same release train when entities change.

---

## Dependency: Inspections as primary condition-data feeder (RAMS)

**Inspection Rounds (§6.25) are not secondary.** Condition-based results, evidence, and anomaly signals are **primary inputs** to the Reliability Engine alongside WO-derived failure events (see `docs/research/MODULE_6_10_RELIABILITY_ENGINE.md` and PRD §6.10).

**Gate:** Sprints `03-inspection-rounds-and-checklists/01` and `02` (templates + field execution/results/evidence schema) **must be complete and stable** before **finalizing** RAMS KPI/computation logic in `05-reliability-data-foundation-iso-14224/03` (runtime exposure, denominators, and snapshot formulas that consume inspection-linked signals). Sprint `05-03` prompts state this prerequisite explicitly.

Parent track `01-reliability-engine-core-and-reproducible-snapshots/` (hashing, jobs, plot snapshots) may proceed in parallel **but** deterministic RAMS numerical contracts must not be **frozen** until inspection condition data structures exist—coordinate owners.

---

## Dependency: Scientific output validation harness

After RAMS KPI/computation outputs exist (`05-03` minimum), run **`07-scientific-output-validation-and-research-benchmarks/01`** to lock **golden benchmarks** derived from `docs/research/` and PRD metric definitions into an automated regression harness (see also parent `01-reliability-engine-core…/04-reliability-core-validation-and-performance-benchmarks.md`).

---

## Order of execution (finalized for approval)

Execute in this **serialized dependency order** unless noted *parallel OK*.

| Step | Track / sprint | Rationale |
|------|------------------|-----------|
| **1** | `06-work-order-closeout-and-data-integrity/` — `01` → `02` → `03` | Trustworthy WO close-out evidence for failure events and integrity contract freeze. |
| **2** | `03-inspection-rounds-and-checklists/` — `01` Templates, rounds, checkpoints | **Primary condition-data structures** for RAMS inputs. |
| **3** | `03-inspection-rounds-and-checklists/` — `02` Field execution, results, evidence, offline queue | Stable inspection result/evidence model before RAMS math finalization. |
| **4** | `05-reliability-data-foundation-iso-14224/` — `01` Failure taxonomy | *Parallel OK after step 1 starts* — independent of inspection schema. |
| **5** | `05-reliability-data-foundation-iso-14224/` — `02` Governed failure events (WO) | WO path to `failure_events`; can overlap with steps 2–3. |
| **6** | `05-reliability-data-foundation-iso-14224/` — `03` Exposure + **KPI snapshots** | **Hard gate:** run only after steps **2** and **3** complete — formulas must incorporate inspection condition signals per PRD. |
| **7** | `05-reliability-data-foundation-iso-14224/` — `04` Data-quality workspace | RAMS UI and badges after snapshots exist. |
| **8** | `03-inspection-rounds-and-checklists/` — `03` → `04` Anomaly routing and trends / reliability handoff | Completes inspection module; `04` feeds reporting and RAMS narrative. |
| **9** | `01-loto-work-permit-system/` — `01` → `04` | Safety / PTW gap module. |
| **10** | `02-training-certification-habilitation/` — `01` → `04` | Training / habilitation gap module. |
| **11** | `04-budget-cost-center-closure/` — `01` → `03` | ERP/sync hardening for budget domain. |
| **12** | `07-scientific-output-validation-and-research-benchmarks/` — `01` | **Validation harness** — after step **7** (and parent engine core as applicable). |

**Note:** Steps 9–11 may be *parallelized by team capacity* after step **7** if safety and resourcing allow—they do not unblock step **12** except that production confidence benefits from LOTO/training/budget stability.

---

## Folder map

| Folder | PRD anchor | Repo reality (audit) |
|--------|------------|----------------------|
| `01-loto-work-permit-system/` | §6.23 | `PermitsPage` placeholder |
| `02-training-certification-habilitation/` | §6.20 | `TrainingPage` placeholder |
| `03-inspection-rounds-and-checklists/` | §6.25 | `InspectionsPage` placeholder — **primary RAMS feeder** |
| `04-budget-cost-center-closure/` | §6.24 | `BudgetPage` implemented — closure/ERP/sync hardening |
| `05-reliability-data-foundation-iso-14224/` | §6.10.1, ISO 14224 alignment | Foundation layer |
| `06-work-order-closeout-and-data-integrity/` | §6.5 | WO evidence + integrity |
| `07-scientific-output-validation-and-research-benchmarks/` | §6.10.2, §9, research benchmarks | Golden tests / regression harness |

---

*Add completion footers to each sprint file when done (verifier, date, `cargo check` / `pnpm typecheck`).*
