# Executive Dashboards Tab (M1)

**PRD:** §6.11

## Tasks

- **Projection** — `analytics::projection` + `get_analytics_executive_dashboard` (horizon + org scope).
- **Layout** — `user_analytics_dashboard_layouts` (device-local, separate from Home).
- **UI** — Executive Dashboards widgets, filters, customize dialog, D3 workload, drill-through.

## Completion

- **Date:** 2026-08-25
- **Verifier:** `cargo check` + `pnpm typecheck`
- **Notes:** PM adherence shown as due-queue buckets (KPI-036 Partial). Maintenance cost is horizon SUM of closed WO totals (KPI-037 Partial — no Pareto yet). Equipment class/family filter deferred.
