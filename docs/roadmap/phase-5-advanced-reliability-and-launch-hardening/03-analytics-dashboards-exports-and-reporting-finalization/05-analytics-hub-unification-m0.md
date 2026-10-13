# Analytics Hub Unification And Durable Report Archive (M0)

**PRD:** §6.10 / §6.11

## Tasks

- **Hub shell** — Tabbed `/analytics` layout (Dashboards | Deep RAMS | Report Builder | Schedules | History).
- **Deep RAMS nest** — Relocate `/reliability/*` to `/analytics/rams/*` with permanent redirects; single sidebar entry.
- **Durable archive** — `report_artifacts` + app-data storage; History download.
- **KPI catalog** — KPI-033..037 for MTBF, MTTR, Availability, PM Adherence, Maintenance Cost.

## Completion

- **Date:** 2026-08-25
- **Verifier:** M0 implementation in `maintafox-desktop`
- **Notes:** Executive Dashboards tab is an honest coming-soon scaffold (M1). Templates/Schedules/History preserve prior behavior with durable archive. Email/SMTP still out of scope.
