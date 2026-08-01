# Sprint 3 — Runtime Exposure Denominators And Reliability KPI Snapshots

**PRD:** §6.10.2

**Objective:** Compute **MTBF**, **MTTR**, **availability**, **failure rate**, **repeat failure rate** into `reliability_kpi_snapshots` using **governed** `failure_events` and **`runtime_exposure_logs`** denominators. **Prerequisite:** Gaps `03-01` + `03-02` complete.

---

## Standards & authoritative references

| Metric | PRD §6.10.2 | Standard citation in PRD |
|--------|-------------|---------------------------|
| MTBF | Total governed runtime / count of **unplanned** failure events | ISO 14224-aligned |
| MTTR | Total **active repair** hours / count of **repairable** events | ISO 14224-aligned |
| Availability | Uptime / (Uptime + governed downtime) **or** MTBF/(MTBF+MTTR) | IEC 60050-191 (PRD) |
| Failure rate | Failures / exposure | Reliability rule set |
| Repeat failure rate | Repeat asset+**failure mode** / total failures | Maintafox |

**ISO 14224** ([standard](https://www.iso.org/standard/64076.html)): equipment + failure + maintenance data categories; **failure mechanisms** for analysis (Clause 9 intro).

---

## Schema definition

### `runtime_exposure_logs`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `entity_sync_id` | TEXT | |
| `equipment_id` | INTEGER | |
| `exposure_type` | TEXT | `hours` / `cycles` / `output_distance` / `production_output` |
| `value` | REAL | Additive increment |
| `recorded_at` | TEXT | End of interval or reading time |
| `source_type` | TEXT | `meter_reading` / `iot_counter` / `manual` / `calendar_operating_schedule` |
| `row_version` | INTEGER | |

**Denominator for calendar period [T0,T1]:**

\[
T_{\text{exp}} = \sum_{i} \Delta t_i \cdot \mathbb{1}_{\text{equipment}=e}
\]

where \(\Delta t_i\) comes from **sum of `value`** for `exposure_type=hours` **or** derived from meter delta **or** **scheduled operating hours** minus **governed downtime** (tenant policy flag).

**Operational definition (Maintafox default):**

\[
T_{\text{exp}} = \max\left(0,\ \sum_{k} \text{runtime\_exposure\_logs.value in } [T0,T1]\ \text{for } exposure\_type=\texttt{hours}\right)
\]

**Alternative (calendar):** `T_exp = (T1-T0) * scheduled_utilization_ratio - sum(downtime_hours)` — must be **one** policy per tenant, stored in `reliability_policy_json`.

### `reliability_kpi_snapshots`

| Column | Type |
|--------|------|
| `id` | INTEGER PK |
| `entity_sync_id` | TEXT |
| `equipment_id` / `asset_group_id` | INTEGER NULL |
| `period_start`, `period_end` | TEXT |
| `mtbf` | REAL NULL |
| `mttr` | REAL NULL |
| `availability` | REAL NULL |
| `failure_rate` | REAL NULL |
| `repeat_failure_rate` | REAL NULL |
| `event_count` | INTEGER |
| `data_quality_score` | REAL 0..1 |
| `inspection_signal_json` | TEXT NULL | Rollup from Gaps `03` (warning/fail rates) |
| `row_version` | INTEGER |

---

## Business logic & validation rules

1. **Minimum sample:** If `event_count < N_min` (tenant default e.g. 5), set `data_quality_score < 0.5` and surface **warning** (per `MODULE_6_10` §8).

2. **MTBF:** Only events with `eligible_unplanned_mtbf=1` in `eligible_flags_json`.

3. **Repeat failure:** Same `(equipment_id, failure_mode_id)` within **lookback window** (e.g. 30d) counts as repeat — numerator `repeat_pairs`, denominator `total_events`.

---

## Mathematical triggers (exact formulas)

Let:

- \(F\) = count of governed **unplanned** failure events in period.
- \(T_{\text{exp}}\) = exposure hours (from `runtime_exposure_logs` policy).
- \(D_{\text{down}}\) = sum `downtime_duration_hours` for those events.
- \(R_{\text{active}}\) = sum `active_repair_hours` for **repairable** events.

Then:

\[
\text{MTBF} = \frac{T_{\text{exp}}}{F} \quad (F>0)
\]

\[
\text{MTTR} = \frac{R_{\text{active}}}{F_{\text{repair}}} \quad (F_{\text{repair}}>0)
\]

\[
\text{Failure rate} = \frac{F}{T_{\text{exp}}} \quad (\text{per hour})
\]

\[
\text{Availability}_A = \frac{T_{\text{exp}} - D_{\text{down}}}{T_{\text{exp}}} \quad \text{(simplified; align with IEC 60050-191 if using uptime definition)}
\]

**Alternate (PRD):** \(\text{Availability}_B = \frac{\text{MTBF}}{\text{MTBF}+\text{MTTR}}\) when both defined.

**Inspection integration (Gaps `03`):** Store rolling **warning rate** \(W_r = \frac{\text{warnings}}{\text{checkpoints}}\) in `inspection_signal_json` for **correlation**, not as divisor for MTBF.

---

## Sync transport specification

| `entity_type` | Payload |
|---------------|---------|
| `runtime_exposure_logs` | Full row |
| `reliability_kpi_snapshots` | Full row + `inspection_signal_json` |

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.
- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 05-reliability-data-foundation-iso-14224 — Sprint 03 — Runtime Exposure Denominators And Reliability KPI Snapshots

**Read Only:**
- @docs/PRD.md §6.10.1
- @docs/research/MODULE_6_10_RELIABILITY_ENGINE.md
- @src-tauri/src/migrations/
- @src-tauri/src/sync/domain.rs

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).
2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.
3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).

**Sync JSON:** Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*
