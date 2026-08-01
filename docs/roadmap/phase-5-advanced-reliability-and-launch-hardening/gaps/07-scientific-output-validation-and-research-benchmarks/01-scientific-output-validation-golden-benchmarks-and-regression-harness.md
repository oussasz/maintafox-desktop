# Sprint 1 — Scientific Output Validation: Golden Benchmarks And Regression Harness

**PRD:** §6.10.2, §9

**Objective:** Automated **regression harness** proving Rust RAMS outputs match **closed-form benchmarks** derived from PRD formulas and `docs/research/MODULE_6_10_RELIABILITY_ENGINE.md` §7–8.

---

## Standards & authoritative references

| Source | Role |
|--------|------|
| **ISO 14224:2016** | Defines data categories feeding MTBF/MTTR; validation proves **computations** on synthetic ISO-shaped datasets. |
| **IEC 60050-191** | Vocabulary for **availability** — if implementing Availability\(_A\) vs \(_B\), document which definition tests assert. |
| **PRD §6.10.2** | Authoritative formula table for MTBF, MTTR, failure rate, repeat failure rate. |
| **Internal** | `MODULE_6_10_RELIABILITY_ENGINE.md` calculation governance; parent `01-reliability-engine-core…/04-reliability-core-validation-and-performance-benchmarks.md`. |

---

## Schema definition (test fixtures, not production)

### `fixtures/rams_golden/v1/*.json`

Each file:

```json
{
  "name": "mtbf_simple_two_failures",
  "period": { "start": "2026-01-01T00:00:00Z", "end": "2026-02-01T00:00:00Z" },
  "equipment_id": 1,
  "runtime_exposure_logs": [{ "value": 720.0, "exposure_type": "hours" }],
  "failure_events": [
    { "failed_at": "...", "is_planned": false, "failure_mode_id": 10, "eligible_flags_json": "{\"eligible_unplanned_mtbf\":true}" },
    { "failed_at": "...", "is_planned": false, "failure_mode_id": 10, "eligible_flags_json": "{\"eligible_unplanned_mtbf\":true}" }
  ],
  "expected": {
    "mtbf": 360.0,
    "failure_rate": 0.0027777778,
    "event_count": 2
  },
  "tolerance": { "rel": 1e-9 }
}
```

No DB migration required if tests load JSON from `src-tauri/tests/fixtures/`.

---

## Business logic & validation rules

1. **CI:** `cargo test rams_golden` must pass on every PR touching `src-tauri/**/reliability/**` or `**/analytics/**`.

2. **Citation:** Each golden file includes `prd_section` and `research_section` string fields.

---

## Mathematical triggers (benchmarks to implement)

| Test ID | Input | Expected |
|---------|-------|----------|
| `G-MTBF-01` | \(T_{\text{exp}}=1000\) h, \(F=2\) eligible | MTBF \(=500\) h |
| `G-FR-01` | Same | \(\lambda = 2/1000 = 0.002\) /h |
| `G-MTTR-01` | \(R_{\text{active}}=10\) h, \(F_{\text{rep}}=2\) | MTTR \(=5\) h |
| `G-RPT-01` | Two events same `(equipment_id, failure_mode_id)` within 30d | repeat failure rate \(= 1/2 = 0.5\) |

**Repeat failure (Maintafox):** \(\text{RFR} = \frac{\text{count of events flagged as repeat}}{\text{total eligible events}}\) per PRD widget definition — freeze in test comment.

---

## Sync transport specification

**N/A** (test assets). If golden data moved to SQL seed for integration tests, **do not** sync test DB to production VPS.

---

## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- None — golden/regression harness is desktop/CI; do not replicate test DB to tenant mirror.
- Optional (ops): if KPI recompute runs off-device, parity-check using same fixture JSON as `rams_golden` (separate ops ticket).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Gaps 07-scientific-output-validation-and-research-benchmarks — Sprint 01 — Scientific Output Validation: Golden Benchmarks And Regression Harness

**Read Only:**
- @docs/research/MODULE_6_10_RELIABILITY_ENGINE.md
- @src-tauri/
- gaps/05-reliability-data-foundation-iso-14224/03-runtime-exposure-denominators-and-reliability-kpi-snapshots.md

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.

**Actions:**
1. Add `rams_golden` (or equivalent) + JSON fixtures; call **only** public Rust calculator APIs used in production.
2. Assert tolerances from Business rules; forbid duplicated math in tests.
3. Wire `cargo test` / CI gate; keep fixtures in-repo.

**Sync JSON:** N/A — golden fixtures / local CI only; no exchange payloads (see Sync section).

**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).

---

*Completion: date, verifier, `cargo test rams_golden` / CI.*
