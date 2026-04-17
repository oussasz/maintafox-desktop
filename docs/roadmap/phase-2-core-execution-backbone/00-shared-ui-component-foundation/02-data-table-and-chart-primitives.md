# Phase 2 · Sub-phase 00 · File 02
# Data Table Engine and Chart Visualization Primitives

## Context and Purpose

Phase 2 modules — Organization, Equipment, DI, Work Orders, Users, Notifications —
all present tabular data as their primary interface pattern. The PRD (§4.1) specifies
**TanStack Table 8.x** as the headless table engine and **D3.js 7.x** for chart
visualization. Neither library has been installed or configured in the project.

This file delivers:
1. A project-standard `DataTable` wrapper around TanStack Table that integrates with
   the Shadcn/ui `Table` component, provides sorting/filtering/pagination out of the
   box, and is bilingual (column headers and empty-state text from i18n).
2. Base D3 chart primitives (BarChart, LineChart, PieChart as React wrappers) that
   Phase 5 analytics and the Phase 2 dashboard KPI widgets will use.

**Gap addressed:** Category B items 2 and 4 from the Phase 1 gap analysis.

## Architecture Rules Applied

- **Headless core, styled shell.** TanStack Table provides the logic (sorting,
  filtering, pagination state); the Shadcn `Table` component provides the visual
  shell. Module code only defines `ColumnDef[]` and data — it never touches DOM.
- **Server-side ready.** The DataTable supports both client-side and server-side
  pagination/sorting. Phase 2 modules use client-side (SQLite local data); Phase 4
  VPS modules may use server-side.
- **D3 charts are React-managed SVG.** Each chart component owns an SVG ref and
  uses D3 for scales and axes only — React manages the DOM. This avoids the
  React-D3 impedance mismatch.
- **Bilingual by default.** Empty states, pagination labels ("Page 1 of 5",
  "Aucun résultat"), and column header sort indicators use the `common` i18n
  namespace.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `pnpm add @tanstack/react-table` | TanStack Table 8.x dependency |
| `pnpm dlx shadcn@latest add table` | Shadcn Table component |
| `src/components/data/DataTable.tsx` | Generic data table with sorting, filtering, pagination |
| `src/components/data/DataTablePagination.tsx` | Pagination controls for DataTable |
| `pnpm add d3` + `pnpm add -D @types/d3` | D3.js 7.x dependency |
| `src/components/charts/BarChart.tsx` | Reusable bar chart wrapper |
| `src/components/charts/LineChart.tsx` | Reusable line chart wrapper |
| `src/components/charts/chart-utils.ts` | Shared scale/axis helpers |

## Prerequisites

- SP00-F01 complete: Shadcn/ui components installed, barrel export working
- SP02-F03 complete: Tailwind token system, cn() utility

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | TanStack Table Installation and DataTable Component | Dependencies, `DataTable.tsx`, `DataTablePagination.tsx` |
| S2 | D3 Chart Primitives | Dependencies, `BarChart.tsx`, `LineChart.tsx`, `chart-utils.ts` |
| S3 | Table and Chart Integration Tests | Vitest rendering tests, sample data validation |

---

## Sprint S1 — TanStack Table Installation and DataTable Component

### AI Agent Prompt

```
You are a TypeScript and React engineer. Install TanStack Table 8 and create a reusable
DataTable component that wraps TanStack's headless table with the Shadcn Table component.

────────────────────────────────────────────────────────────────────
STEP 1 — Install dependencies
────────────────────────────────────────────────────────────────────

```bash
pnpm add @tanstack/react-table
pnpm dlx shadcn@latest add table
```

────────────────────────────────────────────────────────────────────
STEP 2 — CREATE src/components/data/DataTable.tsx
────────────────────────────────────────────────────────────────────

Create a generic DataTable<TData, TValue> component that:
- Takes `columns: ColumnDef<TData, TValue>[]` and `data: TData[]`
- Supports optional client-side sorting (clickable column headers)
- Supports optional client-side filtering (global search input)
- Supports pagination with configurable page size
- Uses the Shadcn `Table, TableBody, TableCell, TableHead, TableHeader, TableRow`
  components for the DOM shell
- Displays an empty state message from `useTranslation("common")`
- Is fully typed with generics — no `any` types

The DataTable should accept an optional `onRowClick` callback and
optional `isLoading` state to show a skeleton.

────────────────────────────────────────────────────────────────────
STEP 3 — CREATE src/components/data/DataTablePagination.tsx
────────────────────────────────────────────────────────────────────

Create a pagination control component that:
- Shows "Page X of Y" with bilingual support
- Has Previous/Next buttons
- Has page size selector (10, 20, 50)
- Uses Shadcn Button and Select components
- Takes the TanStack Table instance as a prop

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes
- DataTable renders a list of 50 items with pagination (5 pages of 10)
- Clicking a sortable column header toggles sort direction
- Empty data shows the i18n empty-state message
```

---

### Supervisor Verification — Sprint S1

**V1 — TanStack Table is installed.**
Run `pnpm list @tanstack/react-table`. Version 8.x must be present.

**V2 — DataTable renders.**
Create a test page that renders `<DataTable columns={[...]} data={[...]} />`.
Confirm the table shows rows, pagination, and column headers.

---

## Sprint S2 — D3 Chart Primitives

### AI Agent Prompt

```
You are a TypeScript and React engineer. Install D3.js and create base chart
components that Phase 5 analytics and Phase 2 dashboard KPI widgets will use.

────────────────────────────────────────────────────────────────────
STEP 1 — Install D3
────────────────────────────────────────────────────────────────────

```bash
pnpm add d3
pnpm add -D @types/d3
```

────────────────────────────────────────────────────────────────────
STEP 2 — Create chart utilities and base components
────────────────────────────────────────────────────────────────────

Create:
- `src/components/charts/chart-utils.ts` — shared helpers for margins, scales,
  responsive container sizing, and Tailwind token color mapping
- `src/components/charts/BarChart.tsx` — React-managed SVG bar chart. Props:
  `data: { label: string; value: number }[]`, `width`, `height`, `color?`
- `src/components/charts/LineChart.tsx` — React-managed SVG line chart. Props:
  `data: { x: number | Date; y: number }[]`, `width`, `height`, `color?`

All charts use D3 for scales and axes only — React renders the SVG elements.
Charts must be responsive (use a ResizeObserver or container query).

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes
- BarChart renders visible bars with correct proportions
- LineChart renders a visible path through the data points
- Charts use the design token colors (primary, status-success, etc.)
```

---

### Supervisor Verification — Sprint S2

**V1 — D3 is installed.**
Run `pnpm list d3`. Version 7.x must be present.

**V2 — Chart components compile.**
Run `pnpm run typecheck`. Zero errors.

---

## Sprint S3 — Table and Chart Integration Tests

### AI Agent Prompt

```
Write Vitest rendering tests for the DataTable and chart components.

────────────────────────────────────────────────────────────────────
STEP 1 — DataTable tests
────────────────────────────────────────────────────────────────────

Test:
- Renders correct number of rows for given data
- Shows empty state message when data is empty
- Pagination controls appear when data exceeds page size
- Sorting changes row order

────────────────────────────────────────────────────────────────────
STEP 2 — Chart tests
────────────────────────────────────────────────────────────────────

Test:
- BarChart renders an SVG with correct number of rect elements
- LineChart renders an SVG with a path element

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm test` passes — all Phase 1 + Phase 2 SP00 tests pass
- At least 5 new tests added
```

---

### Supervisor Verification — Sprint S3

**V1 — All tests pass.**
Run `pnpm test`. Zero failures.

**V2 — No regressions.**
Run `pnpm run typecheck && pnpm run i18n:check`. Both exit 0.

---

## Sprint S4 — Web-Parity Gap Closure (Dashboard KPI Shell)

> **Scope** — The current DashboardPage shows only app version and online status.
> Full analytics is Phase 5, but the web reference has a functional KPI card + chart
> layout. Sprint S4 adds a reusable KPI card shell and a basic dashboard layout
> that Phase 2 modules can plug into as they ship.

### S4‑1 — Dashboard KPI Shell — GAP DSH‑01

```
LOCATION   src/pages/DashboardPage.tsx (patch — replace minimal content)
COMPONENT  src/components/dashboard/KpiCard.tsx
COMPONENT  src/components/dashboard/DashboardWorkloadChart.tsx
STORE      No new store — each KPI card calls its own IPC command
SERVICE    src/services/dashboard-service.ts (new — aggregation IPC wrappers)
COMMANDS   get_dashboard_kpis (Rust — queries counts from DI, WO, Asset, PM tables)

DESCRIPTION
Replaces the minimal DashboardPage with a KPI grid + workload chart:

  ┌────────────────────────────────────────────────────────────┐
  │  Dashboard                          Welcome, {display_name}│
  │                                                            │
  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐     │
  │  │ Open DIs │ │ Open WOs │ │ Assets   │ │ Overdue  │     │
  │  │    12    │ │     8    │ │   247    │ │    3     │     │
  │  │  ↑ 2     │ │  ↓ 1    │ │  — 0    │ │  ↑ 1    │     │
  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘     │
  │                                                            │
  │  ┌──────────────────────────────────────────────────────┐  │
  │  │  Workload — Last 7 Days                    [7d|30d]  │  │
  │  │                                                      │  │
  │  │  ██████                                              │  │
  │  │  ██████  ████                                        │  │
  │  │  ██████  ████  ██████  ████  ██████  ████  ██████    │  │
  │  │  Mon     Tue    Wed    Thu    Fri    Sat    Sun      │  │
  │  │                                                      │  │
  │  │  ■ DI Created  ■ WO Completed  ■ PM Due             │  │
  │  └──────────────────────────────────────────────────────┘  │
  │                                                            │
  │  Quick Actions                                             │
  │  [ + New DI ]  [ + New WO ]  [ + New Asset ]               │
  └────────────────────────────────────────────────────────────┘

KpiCard.tsx — Reusable component:
  Props: title, value (number), trend (number, positive=up), icon, color
  Renders: shadcn Card with icon, large value, trend arrow with delta
  Trend: ↑ green if positive change is good (assets), ↑ red if positive change
  is bad (overdue). Configurable via trendDirection: 'up-good' | 'up-bad'.

DashboardWorkloadChart.tsx:
  Reuses BarChart.tsx (from Sprint S2) with stacked bars
  Period selector: 7d / 30d toggle (segmented control)
  Data: get_dashboard_workload_chart IPC → {date, di_created, wo_completed, pm_due}[]
  Empty state: "Not enough data yet" with illustration

Quick Actions: permission-gated buttons linking to create flows:
  - "+ New DI" (di.create) → /requests?action=create
  - "+ New WO" (ot.create) → /work-orders?action=create (placeholder until WO ships)
  - "+ New Asset" (eq.manage) → /equipment?action=create

KPI data refreshes on page mount + every 5 minutes (setInterval).

ACCEPTANCE CRITERIA
- 4 KPI cards render with real counts from database
- trend arrows show change vs previous period
- bar chart renders with real workload data
- period toggle switches between 7d and 30d
- quick action buttons are permission-gated
- empty state renders gracefully when no data exists
```

### Supervisor Verification — Sprint S4

**V1 — KPI cards.**
With 12 open DIs and 8 open WOs in database, verify cards show correct counts. Verify
trend arrows show delta vs previous period.

**V2 — Workload chart.**
Verify stacked bar chart renders for last 7 days. Toggle to 30d. Verify chart updates.

**V3 — Quick actions.**
Login as user with di.create but not ot.create. Verify "+ New DI" visible, "+ New WO"
hidden.

---

*End of Phase 2 · Sub-phase 00 · File 02*
