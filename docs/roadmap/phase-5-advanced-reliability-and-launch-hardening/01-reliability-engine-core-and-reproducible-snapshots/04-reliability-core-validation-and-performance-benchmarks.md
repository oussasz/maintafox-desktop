# Reliability Core Validation And Performance Benchmarks

**PRD:** §9

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Benchmarks** — Criterion or timed tests for core metrics.
- **Validation** — Golden results for known inputs.
- **Perf budget** — Document max latency.

## Performance budget (desktop dev reference)

| Workload | Budget (wall) | Where |
|----------|----------------|--------|
| `compute_reliability_kpis` — 10k eligible events, 20 iterations | **&lt; 400 ms** | `src-tauri/tests/reliability_perf_budget.rs` |
| Single `compute_reliability_kpis` call | Use Criterion `cargo bench -p maintafox --bench reliability_core` for p50/p95 on this machine | `src-tauri/benches/reliability_core.rs` |

Hashes: `dataset_hash_sha256` empty-parts v1 fixture must stay stable; see `analysis_input::tests::dataset_hash_golden_empty_parts_v1`.






## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- None — no VPS dispatch required for this sprint document (infra-only work is out of scope here).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Phase 5 01-reliability-engine-core-and-reproducible-snapshots — Sprint 04 — Reliability Core Validation And Performance Benchmarks

**Read Only:**
- @docs/PRD.md
- @src-tauri/src/sync/domain.rs
- @src-tauri/src/migrations/
- @src-tauri/src/commands/

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Do not** reference or configure PostgreSQL, Nginx, or non-desktop hosts from this stage.

**Actions:**
1. Execute **Tasks** in this file using SQLite/Tauri/React only (local app + migrations).
2. **Benchmarks** — Criterion or timed tests for core metrics.
3. **Validation** — Golden results for known inputs.
4. **Perf budget** — Document max latency.

**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A.

**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).

---

*Completion: 2026-04-18 — `cargo check`, `cargo test -p maintafox`, `cargo bench --bench reliability_core` (optional), `pnpm typecheck`.*
