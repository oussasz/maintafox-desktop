# Performance Targets

Source of truth: PRD §14.1 — Non-Functional Requirements: Performance

These targets are enforced by automated tests where possible. Any sprint that risks
violating a target must include a performance regression test.

## Startup Budget

| Metric | Target | Enforcement |
|--------|--------|-------------|
| Cold start (first launch, DB init) | < 4 000 ms | `validate_startup_duration` unit test + tracing warn log |
| Cold start (subsequent launches, DB open) | < 3 000 ms | tracing log review |
| Startup sequence individual phase | < 1 000 ms per phase | tracing span review |

## Navigation Budget

| Metric | Target | Enforcement |
|--------|--------|-------------|
| Module navigation (warm, lazy chunk already loaded) | P95 < 150 ms | Vitest render perf tests (Phase 2+) |
| Module navigation (cold, new lazy chunk) | P95 < 500 ms | Vite bundle analysis |

## Data Query Budget

| Metric | Target | Enforcement |
|--------|--------|-------------|
| List query (< 500 rows, indexed columns) | P95 < 300 ms | sea-orm query instrumentation (Phase 2+) |
| Detail panel load (single entity) | P95 < 200 ms | — |
| Report generation (< 10 000 rows aggregated) | P95 < 2 000 ms | — |

## Rendering Budget

| Metric | Target | Enforcement |
|--------|--------|-------------|
| D3 chart initial render | < 500 ms | — |
| D3 chart resize / re-render | < 200 ms | — |
| TanStack Table with 500 rows | < 50 ms render time | Vitest bench (Phase 5+) |

## Memory Budget

| Metric | Target | Notes |
|--------|--------|-------|
| Idle RSS after startup | < 150 MB | Tauri + SQLite overhead |
| Peak during D3 chart render | < 300 MB | — |
| Memory growth over 8-hour session | < 20 MB | Monitor for React listener leaks |

## Implementation Notes

- All tracing spans provide `elapsed_ms` field for log aggregation.
- Startup timing is validated by the `validate_startup_duration` unit test.
- Cold-start budget exceeded triggers `startup::COLD_START_BUDGET_EXCEEDED` warning log.
- Phase 2+ will add `pnpm run bench` command using Vitest bench.
- Post-Phase 3: add `cargo bench` for hot-path Rust query functions.
