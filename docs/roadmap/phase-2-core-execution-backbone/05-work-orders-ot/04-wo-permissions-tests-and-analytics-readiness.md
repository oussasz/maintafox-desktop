# Phase 2 - Sub-phase 05 - File 04
# WO Permissions, Tests, and Analytics Readiness

## Context and Purpose

Files 01 through 03 delivered the complete WO operational machinery: core schema, state machine,
planning/execution sub-entities, close-out evidence, verification, cost roll-up, and attachments.
File 04 closes sub-phase 05 with three hardening layers:

1. **Permission domain** — the full `ot.*` matrix with scoping rules, dangerous action
   designations, step-up requirements, and integration with the RBAC framework from Phase 1.
2. **Audit trail** — migration 024 with `wo_change_events` (append-only), the audit writer
   (`wo/audit.rs`), and the change-event frontend timeline for supervisors. Every close-out,
   verification, reopen, and cost-posting action is permanently recorded.
3. **Test coverage and analytics readiness** — a 14-test suite covering the state machine,
   execution gates, quality gates, cost computation, concurrency, and the full lifecycle from
   creation to closure. An analytics readiness checklist ensures the WO module produces the
   data that later subphases (RAMS 6.10, Analytics 6.11, Budget 6.24) need.

---

## PRD Alignment Checklist

This file addresses PRD §6.5 requirements for:

- [x] `ot.*` permission domain (PRD §6.7 domain table: "ot.* — Work Orders")
      • `ot.view` / `ot.create` / `ot.edit` / `ot.delete` (PRD explicit)
      • `ot.approve`, `ot.close`, `ot.reopen`, `ot.admin` (operational requirements)
- [x] View modes served by existing list/filter API: Table / Kanban (status groups) /
      Calendar (planned_start / planned_end date range queries)
- [x] Backlog heatmap data: list_wo with date range + open-status filter feeds D3 chart
- [x] Closed WO analytics feed: `get_wo_analytics_snapshot` confirmed as analytics contract
- [x] Reopen logic: implemented in closeout.rs with recurrence window; evidence preserved
- [x] Stage-gated data quality: each transition command enforces its minimum field set

---

## Architecture Rules Applied

- **`wo_change_events` is append-only.** No update or delete command targets this table.
  Blocked dangerous actions (failed step-up on close, failed quality gate on close) still
  write audit rows with `apply_result = 'blocked'` so that closure attempts are always
  visible to supervisors.
- **Permission scope hierarchy:** `ot.view` → `ot.create` → `ot.edit` → `ot.approve` →
  `ot.close` → `ot.reopen` → `ot.admin`. Each is evaluated independently in RBAC — higher
  scopes do not imply lower.
- **`ot.edit` scoped to entity_id.** A technician holding `ot.edit` scoped to Entity A
  can only update WOs whose `entity_id = Entity A org_node_id`. Global scope = all.
- **Analytics readiness is a structural guarantee, not a runtime feature.** The tests in S2
  verify that after a full WO lifecycle, the `WoAnalyticsSnapshot` contains all fields
  required by RAMS (failure codes), Cost Center (cost components), and Schedule Compliance
  (plan vs actual timestamps). These are contracts, not aspirations.
- **Three view modes are query-layer concerns.** Kanban groups WOs by `work_order_statuses.
  macro_state`; Calendar uses `planned_start`/`planned_end` date filtering; Table uses the
  standard paginated `list_wo` filter. No additional commands are needed — view mode is a
  frontend presentation choice over the same data queries.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000024_wo_change_events.rs` | Immutable WO audit ledger + `ot.*` permission seed |
| `src-tauri/src/wo/audit.rs` | Audit event writer and reader; fire-and-log pattern |
| `src-tauri/src/wo/permissions.rs` | `ot.*` permission domain seed definitions |
| `src-tauri/src/commands/wo.rs` (patch) | `list_wo_change_events` + `list_all_wo_change_events` commands; audit writer integrated into all transition commands |
| `src-tauri/src/wo/tests.rs` | 14-test suite: state machine, execution gates, quality gates, costs, concurrency, full lifecycle |
| `src/services/wo-audit-service.ts` | Frontend wrapper for WO change event timeline |
| `src/components/wo/WoAuditTimeline.tsx` | Change event timeline component for WO detail dialog |

---

## Prerequisites

- Files 01 through 03 complete (migrations 021–023 applied)
- Phase 1 RBAC seeder pattern; `permissions` table exists
- `require_permission!` and `require_step_up!` macros work for any domain

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Audit Trail and Permission Seed | migration 024 + `audit.rs` + `permissions.rs` |
| S2 | Unit and Integration Tests | `wo/tests.rs` full test suite |
| S3 | Audit Timeline UI | `wo-audit-service.ts` + `WoAuditTimeline.tsx` |

---

## Sprint S1 - Audit Trail and Permission Seed

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the WO audit trail and permission domain seed.

STEP 1 - CREATE src-tauri/migrations/m20260401_000024_wo_change_events.rs

CREATE TABLE wo_change_events (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  wo_id           INTEGER NULL REFERENCES work_orders(id),
  action          TEXT    NOT NULL,
    -- created / planned / assigned / started / paused / resumed / held /
    --  mech_completed / verified / closed / cancelled / reopened /
    --  labor_added / part_added / task_completed / attachment_added /
    --  failure_detail_saved / cost_updated / admin_override
  actor_id        INTEGER NULL REFERENCES user_accounts(id),
  acted_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  summary         TEXT    NULL,
  details_json    TEXT    NULL,
  requires_step_up INTEGER NOT NULL DEFAULT 0,
  apply_result    TEXT    NOT NULL DEFAULT 'applied'
    -- applied / blocked / partial
);

CREATE INDEX idx_wce_wo_id  ON wo_change_events(wo_id);
CREATE INDEX idx_wce_action ON wo_change_events(action);
CREATE INDEX idx_wce_actor  ON wo_change_events(actor_id);

-- Seed ot.* permission domain --
INSERT OR IGNORE INTO permissions (name, description, category, is_dangerous, requires_step_up)
VALUES
  ('ot.view',    'View work orders and details',                   'ot', 0, 0),
  ('ot.create',  'Create new work orders',                        'ot', 0, 0),
  ('ot.edit',    'Edit, plan, assign, and execute work orders',   'ot', 0, 0),
  ('ot.approve', 'Approve work orders from draft',                'ot', 0, 0),
  ('ot.close',   'Close technically verified work orders',        'ot', 1, 1),
  ('ot.reopen',  'Reopen recently closed work orders',            'ot', 1, 1),
  ('ot.admin',   'Override, archive, manage WO settings',         'ot', 1, 0),
  ('ot.delete',  'Delete draft work orders',                      'ot', 1, 0);


STEP 2 - PATCH src-tauri/src/wo/mod.rs
  Add: pub mod audit;
       pub mod permissions;


STEP 3 - CREATE src-tauri/src/wo/audit.rs

Types:
- WoChangeEvent (matches DDL, Serialize/Deserialize)
- WoAuditInput: wo_id: Option<i64>, action: String, actor_id: Option<i64>,
  summary: Option<String>, details_json: Option<String>,
  requires_step_up: bool, apply_result: String

Functions:

A) `record_wo_change_event(pool: &SqlitePool, input: WoAuditInput) -> Result<()>`
   INSERT into wo_change_events.
   Fire-and-log: if insert fails, log the error but do NOT return it to caller.
   Never block the primary workflow due to audit failure.

B) `list_wo_change_events(pool, wo_id: i64, limit: i64) -> Result<Vec<WoChangeEvent>>`
   SELECT WHERE wo_id = ? ORDER BY acted_at ASC LIMIT ?.

C) `list_all_wo_change_events(pool, filter: WoAuditFilter) -> Result<Vec<WoChangeEvent>>`
   WoAuditFilter: action?, actor_id?, date_from?, date_to?, wo_id?, limit, offset.


STEP 4 - CREATE src-tauri/src/wo/permissions.rs

Function:
`wo_permission_domain() -> Vec<(&'static str, &'static str, bool, bool)>`
Returns tuples of (name, description, is_dangerous, requires_step_up) for all 8 ot.* permissions.


STEP 5 - PATCH all transition commands in src-tauri/src/commands/wo.rs

After each successful transition: record_wo_change_event with apply_result='applied'.
After each BLOCKED dangerous action (failed step-up on close, failed quality gate on close):
  record_wo_change_event with apply_result='blocked' and details_json including error list.

STEP 6 - ADD commands to src-tauri/src/commands/wo.rs

`list_wo_change_events`
  Permission: ot.view; Input: wo_id, limit?; Delegates to audit::list_wo_change_events

`list_all_wo_change_events`
  Permission: ot.admin; Input: WoAuditFilter; Delegates to audit::list_all_wo_change_events

ACCEPTANCE CRITERIA
- migration 024 applies; 8 ot.* permission rows inserted
- fire-and-log: simulate audit table failure; primary workflow still succeeds
- blocked close_wo attempt writes change event with apply_result='blocked'
  AND details_json contains the quality gate error list
- cargo check passes
```

### Supervisor Verification - Sprint S1

**V1 - Permission seed count.**
`SELECT COUNT(*) FROM permissions WHERE name LIKE 'ot.%'` returns 8 after migration 024.

**V2 - Audit on close.**
Close WO successfully; verify 1 wo_change_events row with action='closed', apply_result='applied',
requires_step_up=1.

**V3 - Blocked close audit.**
Call close_wo with quality gate failure; verify wo_change_events row with apply_result='blocked'
and details_json containing error text.

**V4 - Fire-and-log.**
Drop wo_change_events table from a test DB; call close_wo; primary workflow must succeed despite
audit failure.

---

## Sprint S2 - Unit and Integration Tests

### AI Agent Prompt

```text
You are a senior Rust test engineer. Write the full WO test suite.

STEP 1 - CREATE src-tauri/src/wo/tests.rs

Use #[cfg(test)] and tokio::test with in-memory SQLite seeded by all migrations (021–024).

--- UNIT TESTS: State Machine ---

test_wo_01_all_valid_transitions
  For every valid (from, to) pair in allowed_transitions, call guard_wo_transition;
  assert Ok(()).

test_wo_02_invalid_transitions
  At least 6 invalid pairs asserted Err:
  - (draft, in_progress)
  - (draft, closed)
  - (planned, in_progress)
  - (closed, draft)
  - (cancelled, draft)
  - (technically_verified, in_progress)

test_wo_03_terminal_states
  is_terminal returns true for closed, cancelled; false for all others.

test_wo_04_cancelled_reachability
  Cancelled is in allowed_transitions for draft, awaiting_approval, planned,
  ready_to_schedule, assigned, waiting_for_prerequisite, in_progress, paused,
  mechanically_complete. Assert true for all 9.

test_wo_05_wo_code_generation
  Create 3 WOs; codes must be WOR-0001, WOR-0002, WOR-0003 with no gaps.

--- UNIT TESTS: Execution Gates ---

test_wo_06_plan_requires_dates
  Call plan_wo with planned_end < planned_start; must return validation error.

test_wo_07_assign_requires_assignee
  Call assign_wo with both assigned_group_id=None and primary_responsible_id=None;
  must return "At least one assignee required" error.

test_wo_08_pause_requires_delay_reason
  Call pause_wo with invalid delay_reason_id; must fail.

--- UNIT TESTS: Close-Out Quality Gate ---

test_wo_09_close_all_gates
  Create and fully close a corrective WO with: labor actuals, parts actuals, failure detail
  with failure_mode_id set, root_cause_summary, verification pass.
  close_wo must succeed.

test_wo_10_close_missing_failure_coding
  Create corrective WO, skip failure detail; call close_wo.
  Must return error specifically mentioning "Failure coding required".

test_wo_11_close_missing_verification
  Create corrective WO with all fields but no verification; call close_wo.
  Must return error mentioning "Technical verification required".

--- UNIT TESTS: Cost Computation ---

test_wo_12_cost_roll_up
  Add labor entries: (3h × 60.0 = 180), (2h × 50.0 = 100).
  Add part: (2 units × 45.0 = 90).
  service_cost_input = 30.
  Close WO; assert:
    labor_cost ≈ 280.0 (allow 0.01 float tolerance)
    parts_cost ≈ 90.0
    service_cost ≈ 30.0
    total_cost ≈ 400.0

--- UNIT TESTS: Concurrency ---

test_wo_13_optimistic_lock_on_plan
  Create WO; call plan_wo with expected_row_version=0; must return error.

--- INTEGRATION TEST: Full Lifecycle ---

test_wo_14_full_wo_lifecycle

  Phase A — Create WO from DI:
    Create a DI (mock); create WO with source_di_id set.
    Assert status = draft, code = WOR-0001, source_di_id = DI.id.

  Phase B — Plan:
    call plan_wo. Assert status = planned, planned_start set.

  Phase C — Assign:
    Transition to ready_to_schedule, then assign_wo.
    Assert status = assigned, primary_responsible_id set.

  Phase D — Execute:
    start_wo. add_labor(started_at=now). add_task(mandatory=true). add_part(qty=2).
    complete_task. open_downtime. close_downtime.
    pause_wo(reason=delay_reason). resume_wo.
    Assert total_waiting_hours > 0 after resume.

  Phase E — Mechanical Completion:
    record_part_usage(qty=2). close_labor(ended_at=now+2h).
    complete_wo_mechanically. Assert mechanically_completed_at set.

  Phase F — Verification and Close:
    save_failure_detail(failure_mode_id=someId, is_permanent_repair=1).
    save_verification(result='pass', return_to_service_confirmed=1).
    Assert status = technically_verified.
    Update root_cause_summary and corrective_action_summary on WO.
    close_wo. Assert status = closed.

  Phase G — Analytics Snapshot:
    get_wo_analytics_snapshot.
    Assert: failure_details.len() == 1.
    Assert: verifications.len() == 1.
    Assert: total_cost > 0.
    Assert: was_planned == true (planned_start was set).
    Assert: reopen_count == 0.

  Phase H — Reopen:
    reopen_wo (within default 7-day window). Assert status = technically_verified.
    Assert reopen_count == 1.
    re-close (save_verification again + close_wo). Assert status = closed, reopen_count = 1.

  Phase I — Audit Trail:
    list_wo_change_events(wo.id, 100).
    Assert events include: created, planned, assigned, started, paused, resumed,
    mech_completed, verified, closed, reopened, closed (11+ rows).

ACCEPTANCE CRITERIA
- All 14 tests pass: cargo test wo::tests
- Zero warnings about unused variables in test module
- test_wo_14 Phase G asserts confirm analytics snapshot is complete
- Costs within 0.01 float tolerance
```

### Supervisor Verification - Sprint S2

**V1 - Test count.**
`cargo test wo::tests -- --list` shows at least 14 tests.

**V2 - All pass.**
`cargo test wo::tests 2>&1` shows 0 failures, 0 errors.

**V3 - Cost precision.**
test_wo_12 asserts approximate equality; no `==` comparison on floats.

**V4 - Full lifecycle phase G.**
Assert in test_wo_14 Phase G that was_planned == true and total_cost > 0;
these confirm two PRD requirements: planning was used AND costs are populated.

---

## Sprint S3 - Audit Timeline UI

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the WO audit timeline service and component.

CREATE src/services/wo-audit-service.ts

Types:
- WoChangeEvent (Zod-validated, matching DDL)
- WoAuditFilter

Functions:
- listWoChangeEvents(woId: number, limit?: number): Promise<WoChangeEvent[]>
- listAllWoChangeEvents(filter: WoAuditFilter): Promise<WoChangeEvent[]>

CREATE src/components/wo/WoAuditTimeline.tsx

Props: woId: number

Same rendering pattern as DiAuditTimeline from SP04:
- Icon by action (plan=calendar, assign=user, start=play, pause=pause, resume=play-circle,
  mech_completed=tool, verified=check-badge, closed=lock, blocked=shield-x, other=dot)
- Actor, acted_at, summary, apply_result badge, requires_step_up badge
- Read-only; no mutating actions
- Loading / empty / error states

PATCH the WO detail view (WoDetailPanel.tsx or equivalent):
  Add "Audit Trail" tab rendering <WoAuditTimeline woId={wo.id} />.
  Tab visible to any user with ot.view permission.

> **UX NOTE (S4):** The WO detail view MUST be rendered inside a floating dialog
> (`WoDetailDialog.tsx`), not a side panel. Follow the same pattern as DI:
> see `docs/UX_DETAIL_DIALOG_PATTERN.md` (pattern UX-DW-001).
> The WO store needs a `closeWo()` action (`set({ activeWo: null })`).

CREATE src/components/wo/WoKanbanView.tsx

A Kanban board grouped by macro_state (from work_order_statuses.macro_state):
  Columns: Open | Executing | Completed | Closed | Cancelled
  Each WO card shows: code, title, asset label, urgency color bar, assignee name, planned_end.
  Cards draggable (visual only; drop triggers assignWo or appropriate transition command
  based on the target column, validated against allowed_transitions before any invoke call).
  Pagination: each column shows up to 20 cards; "Load more" for the rest.

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- WoAuditTimeline is read-only (no mutation handlers)
- WoKanbanView grouping matches work_order_statuses.macro_state enumeration
- Kanban drag validates transition before invoke; shows error toast if transition invalid
- Audit Trail tab visible with ot.view permission
```

### Supervisor Verification - Sprint S3

**V1 - Timeline shows blocked events.**
Close WO with missing fields (blocked); open audit timeline; blocked entry renders with red badge.

**V2 - Kanban grouping.**
With WOs in planned, assigned, in_progress, closed: Kanban shows cards in Open, Executing, Closed
columns correctly.

**V3 - Kanban drag validation.**
Drag a closed WO to Executing column; must show error toast (transition invalid), not call any
invoke command.

**V4 - typecheck.**
`pnpm typecheck` — 0 errors.

---

## WO Module Completion Checklist

Before marking sub-phase 05 complete, verify all of the following:

**Schema:**
- [ ] Migrations 021–024 all apply cleanly
- [ ] `work_order_stubs` table absent after migration 021 (migrated to `work_orders`)
- [ ] All 7 type rows, 12 status rows, 5 urgency rows, 10 delay reason rows seeded
- [ ] `work_order_interveners`, `work_order_parts`, `work_order_tasks`,
      `work_order_delay_segments`, `work_order_downtime_segments` present
- [ ] `work_order_failure_details`, `work_order_verifications`, `work_order_attachments` present
- [ ] `wo_change_events` with `apply_result` and `requires_step_up` columns

**Rust:**
- [ ] `cargo check` passes with zero errors
- [ ] All WO commands registered in the Tauri invoke_handler
- [ ] All 8 `ot.*` permissions seeded
- [ ] Quality gate on close_wo returns ALL failures, not just the first
- [ ] Step-up required for close_wo and reopen_wo
- [ ] Audit events written for blocked close attempts

**TypeScript:**
- [ ] `pnpm typecheck` passes with zero errors
- [ ] All Rust commands have matching TypeScript invoke wrappers
- [ ] Zod validation on all response types

**Tests:**
- [ ] All 14 tests pass: `cargo test wo::tests`
- [ ] test_wo_14 full lifecycle passes end-to-end (all 9 phases)

**Cross-module contracts established for future subphases:**

| Contract | Consuming Module |
|----------|-----------------|
| `source_di_id` on `work_orders` | SP04 DI traceability |
| `get_wo_analytics_snapshot` — failure codes, costs, timings | SP RAMS 6.10, Analytics 6.11 |
| `get_cost_posting_hook` — cost payload by entity/asset/type | SP Budget 6.24 |
| `work_order_parts.article_id` (nullable) | SP Inventory 6.8 |
| `work_order_interveners.skill_id` (nullable) | SP Personnel 6.6 |
| `wo_id` on future `permit_work_orders` join table | SP Work Permit 6.23 |
| `pm_occurrence_id` placeholder in analytics snapshot | SP PM Planning 6.9 |
| `work_order_statuses` configurable via `is_system=0` rows | SP Config Engine 6.26 |

---

## Sprint S4 — Calendar View, Dashboard / Stats, Context Menu, and Archive

> **Gap addressed:** The web app provides three presentation modes (List / Kanban /
> Calendar) plus a dashboard section with KPIs, and a right-click context menu
> for quick actions. S1–S3 specified `get_wo_analytics_snapshot` but no calendar
> component, no dashboard view, no context menu, and no archive panel. This sprint
> closes all four gaps and wires the WorkOrdersPage view selector created in
> File 01 — Sprint S4.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/wo/WoCalendarView.tsx` | Month/week calendar with urgency-coloured chips |
| `src/components/wo/WoDashboardView.tsx` | KPI cards + charts (status, urgency, trend, backlog) |
| `src/components/wo/WoContextMenu.tsx` | Right-click quick-action menu on WO rows/cards |
| `src/components/wo/WoArchivePanel.tsx` | Archived (Fermé/Annulé) WO browser |
| Backend: `get_wo_stats` IPC command (optional extension) | Aggregated KPI data for dashboard |
| `src/i18n/locale-data/{fr,en}/ot.json` (patch) | Calendar, dashboard, and context menu labels |

### WoCalendarView.tsx — Calendar Presentation

**Props:** `{ workOrders: WorkOrder[], onSelect: (wo) => void }`

**Behaviour:**
- Default view: **Month**. Toggle to **Week** via top-right button.
- Each WO renders as a coloured chip on its `planned_start` date cell.
- **Chip colour** follows urgency:
  - Basse → `bg-emerald-100 text-emerald-800`
  - Normale → `bg-blue-100 text-blue-800`
  - Haute → `bg-amber-100 text-amber-800`
  - Critique → `bg-red-100 text-red-800`
- Click chip → calls `onSelect(wo)` (opens WoDetailDialog via wo-store).
- Month navigation via ← / → arrows.
- "Today" button resets to current month/week.

**Layout (Month):**
```
┌──────────────────────────────────────────────────┐
│  ← April 2026 →                [Week] [Month]   │
├──────┬──────┬──────┬──────┬──────┬──────┬──────┤
│ Lun  │ Mar  │ Mer  │ Jeu  │ Ven  │ Sam  │ Dim  │
├──────┼──────┼──────┼──────┼──────┼──────┼──────┤
│      │  1   │  2   │  3   │  4   │  5   │  6   │
│      │      │ [WO] │      │ [WO] │      │      │
│      │      │ [WO] │      │      │      │      │
├──────┼──────┼──────┼──────┼──────┼──────┼──────┤
│  7   │  8   │  9   │ 10   │ 11   │ 12   │ 13   │
│      │ [WO] │      │      │ [WO] │      │      │
│      │      │      │      │ [WO] │      │      │
│      │      │      │      │ [WO] │      │      │
└──────┴──────┴──────┴──────┴──────┴──────┴──────┘
```

### WoDashboardView.tsx — KPI Cards and Charts

**Props:** `{ period?: { from: Date, to: Date } }`

Renders inside WorkOrdersPage when view selector = "Dashboard".

**KPI Cards (top row, 4 cards):**

| Card | Value | Icon | Colour |
|------|-------|------|--------|
| Total OT | `stats.total` | `ClipboardList` | blue |
| En cours | `stats.in_progress` | `PlayCircle` | amber |
| Terminés | `stats.completed` | `CheckCircle2` | emerald |
| En retard | `stats.overdue` | `AlertTriangle` | red |

**Charts (2 × 2 grid, using Recharts):**

| Position | Chart | Type |
|----------|-------|------|
| Top-left | Status Distribution | Donut (PieChart) |
| Top-right | Urgency Breakdown | Horizontal bar (BarChart) |
| Bottom-left | Completion Trend (30 days) | Area chart (AreaChart) |
| Bottom-right | Backlog Heatmap by Entity | Treemap or bar |

**Data source:** Calls `get_wo_analytics_snapshot` (already spec'd in S1). If additional
aggregations are needed, extend with a `get_wo_stats` command returning
`WoStatsPayload { total, in_progress, completed, overdue, by_status, by_urgency,
daily_completed: Vec<(String, i32)>, by_entity: Vec<(String, i32)> }`.

### WoContextMenu.tsx — Right-Click Quick Actions

**Props:** `{ wo: WorkOrder, position: { x, y }, onClose: () => void }`

Triggered on right-click from WoKanbanCard and WO list rows.

**Menu Items (conditional on status and permissions):**

| Label | Icon | Condition | Action |
|-------|------|-----------|--------|
| Voir détail / View detail | `Eye` | always | `openWo(wo.id)` |
| Modifier / Edit | `Pencil` | status ∈ {Brouillon, Planifié} | open WoFormDialog in edit mode |
| Démarrer / Start | `Play` | status = Planifié | `start_wo` |
| Compléter / Complete | `CheckCircle` | status = En_cours | open WoCompletionDialog |
| Imprimer / Print | `Printer` | always | open WoPrintFiche |
| Dupliquer / Duplicate | `Copy` | always | pre-fill WoFormDialog with WO data |
| Annuler / Cancel | `XCircle` | status ∉ {Fermé, Annulé} | confirm dialog → `cancel_wo` |

**Implementation:** Use Radix `ContextMenu` primitive (`@radix-ui/react-context-menu`).

### WoArchivePanel.tsx — Archived WO Browser

Accessible via an "Archive" toggle at the bottom of WorkOrdersPage or as a 5th tab
alongside List/Kanban/Calendar/Dashboard.

**Behaviour:**
- Lists WOs with `status ∈ {Fermé, Annulé}`.
- Sortable columns: code, type, closed_at, urgency, entity.
- Click row → opens WoDetailDialog (read-only).
- "Restaurer" button available for role `ADMIN` to reopen a WO.

**Backend:** Reuses `list_work_orders` with filter `{ archived: true }` or status
filter. No new IPC command required, but extended filter support already exists in
the list command specification.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- WoCalendarView renders WOs on correct dates with urgency colours
- Month ← / → navigation works; "Today" button resets view
- Click chip opens WoDetailDialog
- WoDashboardView shows 4 KPI cards with correct counts
- 4 charts render: status donut, urgency bar, trend area, backlog
- WoContextMenu appears on right-click with correct items per status
- Context menu actions trigger correct store actions/dialogs
- WoArchivePanel lists only Fermé/Annulé WOs
- Archive WOs open in read-only detail view
```

### Supervisor Verification — Sprint S4

**V1 — Calendar view.**
Switch WorkOrdersPage to Calendar; verify WO chips appear on planned_start dates;
verify urgency colour mapping; click a chip and confirm WoDetailDialog opens.

**V2 — Dashboard KPIs.**
Create 3 WOs (1 En_cours, 1 Terminé, 1 En_retard); open Dashboard; verify card
counts = 3 total, 1 en cours, 1 terminé, 1 en retard.

**V3 — Dashboard charts.**
Verify status donut shows 3 segments; urgency bar shows correct distribution;
trend area chart renders last 30 days correctly.

**V4 — Context menu.**
Right-click a Planifié WO; verify "Démarrer" option present; right-click a Fermé WO;
verify "Démarrer" option absent; click "Imprimer" and verify print fiche opens.

**V5 — Archive panel.**
Fermer 2 WOs and Cancel 1; switch to Archive tab; verify 3 WOs listed; click to
open in read-only mode; verify "Restaurer" button visible for admin role only.

---

## Sprint S5 — Frontend Quality Remediation (Post-Audit)

### Context

A professional gap audit conducted on 2026-04-11 reviewed all 17 WO component files,
3 service files, 1 store, 2 i18n locale files, and 1 page file. 41 gaps were identified
(GA-016 through GA-061). This sprint organizes the fixes by priority tier.

### Gap Inventory Summary

| Severity | Count | Focus |
|----------|-------|-------|
| Critical | 3 | i18n (WoAttachmentPanel, WoCloseOutPanel), type-safety (WoDetailDialog) |
| High | 12 | i18n, error-handling, spec-gaps, dead-code |
| Medium | 14 | i18n, UX, accessibility, spec-gaps |
| Low | 12 | i18n locale hardcoding, dead-code, minor UX |

---

### Tier 1 — Critical & High Priority (must fix)

#### GA-016 — WoAttachmentPanel: No i18n (entire component hardcoded French)

**File:** `src/components/wo/WoAttachmentPanel.tsx`
**Category:** i18n — Critical

**Problem:** The component never calls `useTranslation()`. All user-visible strings are
hardcoded French: `"Pièces jointes"`, `"Téléversement en cours…"`, `"Glisser-déposer ou
cliquer pour ajouter un fichier"`, `"Max … Mo"`, `"Aucune pièce jointe."`, `"Confirmer"`,
`"Annuler"`, `"Supprimer la pièce jointe"`, file size suffixes (`"o"`, `"Ko"`, `"Mo"`).

**Fix:** Import `useTranslation("ot")`, add keys under `attachment.*` namespace in both
locale files, replace all hardcoded strings.

---

#### GA-017 — WoCloseOutPanel: No i18n (entire component hardcoded French)

**File:** `src/components/wo/WoCloseOutPanel.tsx`
**Category:** i18n — Critical

**Problem:** ~40+ hardcoded French strings across 4 sections: section headers (`"Symptôme &
Narrative"`, `"Analyse de défaillance"`, `"Action réalisée"`, `"Retour en service"`),
form labels, placeholders, repair type labels (`"Temporaire"`, `"Permanente"`, `"S/O"`),
verification results (`"Approuvé"`, `"Refusé"`, `"Surveiller"`), guidance text, select
items, success messages, button labels.

**Fix:** Import `useTranslation("ot")`, add ~40 keys under `closeout.*` namespace, replace
all strings. Add corresponding EN and FR translations.

---

#### GA-027 — WoDetailDialog: `closeWorkOrder` called with wrong input shape

**File:** `src/components/wo/WoDetailDialog.tsx`
**Category:** type-safety — Critical

**Problem:** `closeWorkOrder` is called with `{ id: wo.id, expected_row_version:
wo.row_version }`, but the store's `closeWorkOrder` expects `WoCloseInput` which requires
`{ wo_id, actor_id, expected_row_version }`. Field is named `id` instead of `wo_id`, and
`actor_id` is entirely missing. This would fail at runtime.

**Fix:** Pass `{ wo_id: wo.id, actor_id: currentUserId, expected_row_version:
wo.row_version }`. Requires access to session/auth context in `FooterActions`.

---

#### GA-019 — WoExecutionControls: 13+ hardcoded English error strings

**File:** `src/components/wo/WoExecutionControls.tsx`
**Category:** i18n — High

**Problem:** Error/fallback messages are hardcoded English: `"Unable to start work order."`,
`"Intervener ID is required."`, `"Quantity used must be a valid non-negative number."`,
labor row labels `"Start:"`, `"End:"`, `"Hours:"`, and 10 more.

**Fix:** Replace with `t("execution.error.*")` keys, add translations in both locales.

---

#### GA-020 — WoPlanningPanel: Hardcoded English urgency labels + error messages

**File:** `src/components/wo/WoPlanningPanel.tsx`
**Category:** i18n — High

**Problem:** Urgency levels are hardcoded English: `"Very Low"`, `"Low"`, `"Medium"`,
`"High"`, `"Critical"`. Error messages like `"Unable to plan work order."` are also hardcoded.

**Fix:** Use `t("urgency.veryLow")` etc. Replace error strings with i18n calls.

---

#### GA-018 — WoKanbanBoard: Hardcoded French column labels

**File:** `src/components/wo/WoKanbanBoard.tsx`
**Category:** i18n — High

**Problem:** Kanban column labels are hardcoded French: `"Brouillon"`, `"Planifié"`,
`"En cours"`, `"Clôture"`, `"Terminés"`.

**Fix:** Use `t("kanban.column.*")` keys.

---

#### GA-045 — Missing i18n keys for 6+ WO statuses

**Files:** `src/i18n/locale-data/{en,fr}/ot.json`
**Category:** i18n — High

**Problem:** Both i18n files only have 9 status keys. Missing: `readyToSchedule`, `assigned`,
`waitingForPrerequisite`, `paused`, `mechanicallyComplete`, `technicallyVerified`.
Components fall through to `"Draft"` via `statusToI18nKey` mapping.

**Fix:** Add the missing status keys in both JSON files.

---

#### GA-058/GA-059 — Missing i18n namespaces: `attachment.*` and `closeout.*`

**Files:** `src/i18n/locale-data/{en,fr}/ot.json`
**Category:** i18n — High

**Problem:** Both locale files lack `attachment.*` (10+ keys) and extended `closeout.*`
(40+ keys) namespaces. Components GA-016 and GA-017 have no translation keys to use.

**Fix:** Add comprehensive key blocks for both namespaces.

---

#### GA-022 — WoArchivePanel: Silent error swallowing — no user feedback

**File:** `src/components/wo/WoArchivePanel.tsx`
**Category:** error-handling — High

**Problem:** `load()` uses `try/finally` with no `catch` — errors are silently swallowed.

**Fix:** Add `catch` block with error state and display an error banner.

---

#### GA-025 — Duplicate function declarations across service files

**Files:** `wo-service.ts`, `wo-execution-service.ts`, `wo-closeout-service.ts`
**Category:** dead-code — High

**Problem:** `listLabor`, `closeLabor`, `listTasks`, `listParts`, `getCostSummary` are
exported from both wo-service.ts AND wo-execution-service.ts with different signatures
and different IPC command names.

**Fix:** Each function should exist in exactly one service file. Remove duplicates and
unify callers.

---

#### GA-049 — WoCloseOutPanel: Form state not pre-populated from existing data

**File:** `src/components/wo/WoCloseOutPanel.tsx`
**Category:** spec-gap — High

**Problem:** When reopening the closeout panel for a WO with saved failure details, the
form starts empty. No `useEffect` loads existing data.

**Fix:** Fetch existing failure detail on mount and pre-populate form fields.

---

#### GA-054 — WoDetailDialog: FooterActions verify button calls `onClose`

**File:** `src/components/wo/WoDetailDialog.tsx`
**Category:** spec-gap — High

**Problem:** For `mechanically_complete` status, the "Verify" button calls `onClose`
(dismisses dialog) instead of switching to the closeout/verification tab.

**Fix:** Verify button should call `setActiveTab("closeout")`.

---

### Tier 2 — Medium Priority (should fix)

#### GA-021 — WoPrintFiche: Entirely hardcoded French HTML

**File:** `src/components/wo/WoPrintFiche.tsx`
**Category:** i18n — Medium

Accept a translations object parameter to `buildHtml()` for language-appropriate print output.

---

#### GA-028/GA-029 — Three duplicated `statusToI18nKey` functions

**Files:** `WoDetailDialog.tsx`, `WorkOrdersPage.tsx`, `WoCalendarView.tsx`
**Category:** dead-code — Medium

Extract into shared utility `src/utils/wo-status.ts`.

---

#### GA-030 — Three duplicated `formatDate` helpers

**Files:** `WoArchivePanel.tsx`, `WoDetailDialog.tsx`, `WorkOrdersPage.tsx`
**Category:** dead-code — Medium

Consolidate into shared utility.

---

#### GA-032 — WoKanbanBoard: `on_hold` status not routed to any column

**File:** `src/components/wo/WoKanbanBoard.tsx`
**Category:** spec-gap — Medium

Add `on_hold` to "En cours" column. Add `completed`, `verified` to "Terminés".

---

#### GA-036 — WoExecutionControls: `Reason #${id}` fallback text hardcoded English

**File:** `src/components/wo/WoExecutionControls.tsx`
**Category:** i18n — Medium

Use `t("execution.fallbackReason", { id })`.

---

#### GA-046 — WoPrintFiche: Missing closeout/failure detail section

**File:** `src/components/wo/WoPrintFiche.tsx`
**Category:** spec-gap — Medium

Add failure analysis, verification result, cost summary, and shift value to print output.

---

#### GA-050 — WoDetailDialog: Missing WoCostSummaryCard in info grid

**File:** `src/components/wo/WoDetailDialog.tsx`
**Category:** spec-gap — Medium

Add cost summary card visible when execution tab is available.

---

#### GA-051 — Accessibility: Calendar/Kanban chips missing accessible labels

**Files:** `WoCalendarView.tsx`, `WoKanbanBoard.tsx`
**Category:** accessibility — Medium

Add `aria-label` on chip buttons, `tabIndex={0}` on Kanban cards.

---

#### GA-053 — WoDiManagementPanel: "Schedule" button is non-functional

**File:** `src/components/wo/WoDiManagementPanel.tsx`
**Category:** spec-gap — Medium

Wire `onClick` to open WoDetailDialog on planning tab.

---

#### GA-060 — WoExecutionControls: Downtime management UI missing

**File:** `src/components/wo/WoExecutionControls.tsx`
**Category:** spec-gap — Medium

Add downtime tracking section with open/close buttons and segment list.

---

#### GA-061 — WorkOrdersPage: No status/type/priority filter dropdowns

**File:** `src/pages/WorkOrdersPage.tsx`
**Category:** spec-gap — Medium

Add dropdown filters using existing i18n keys `list.filters.*`.

---

### Tier 3 — Low Priority (nice to fix)

| ID | File | Issue |
|----|------|-------|
| GA-023/GA-039 | WoArchivePanel | `formatDate` hardcodes `"fr-FR"` locale |
| GA-031 | WoCostSummaryCard | Silent error on cost load failure |
| GA-033 | WoCalendarView | Unused local `statusToI18nKey` function |
| GA-037 | WoDetailDialog | `"DI"` label hardcoded |
| GA-038 | WorkOrdersPage | Search placeholder uses equipment key |
| GA-040 | WoCloseOutPanel | `stepUpPin` state unused |
| GA-042 | WoCalendarView | Day name arrays hardcoded vs `Intl.DateTimeFormat` |
| GA-043 | WoCostSummaryCard | `fmt()` hardcodes `"fr-FR"` locale |
| GA-044 | WoKanbanBoard | `formatShortDate` hardcodes `"fr-FR"` locale |
| GA-047 | WoPrintFiche | Shift field always shows `"—"` |
| GA-048 | WoDetailDialog | Phantom `"completed"` / `"verified"` statuses in visibility sets |
| GA-052 | WoArchivePanel | Toggle button missing `aria-expanded` |
| GA-055 | WoCloseOutPanel | `info.user_id!` non-null assertion fragile |
| GA-056 | WoExecutionControls | `downtimeRows` loaded but discarded |
| GA-057 | WoExecutionControls | `setLoading` state defined but never read |

---

### Acceptance Criteria for Sprint S5

```
- pnpm typecheck passes with zero errors after all fixes
- No hardcoded French or English user-visible strings in any WO component
- All i18n locale files have parity (identical key structure)
- WoCloseOutPanel pre-populates existing failure detail data
- closeWorkOrder receives correct WoCloseInput shape with actor_id
- Verify button switches to closeout tab, not closes dialog
- Duplicate service functions consolidated
- Shared utilities extracted: formatDate, statusToI18nKey
- Kanban board includes on_hold and completed statuses
- WorkOrdersPage has status/type/priority filter dropdowns
```

### Supervisor Verification — Sprint S5

**V1 — i18n completeness.**
Switch locale to EN; open each WO view (list, kanban, calendar, dashboard, archive); open a WO
detail (all tabs: plan, execution, closeout, audit, attachments). Verify zero French strings
visible. Switch to FR; verify all labels in French.

**V2 — closeWorkOrder shape.**
Open a WO in `technically_verified` state; click Close; verify the IPC call includes `wo_id`,
`actor_id`, and `expected_row_version` (check Tauri dev console).

**V3 — Verify button behaviour.**
Open a `mechanically_complete` WO; click Verify; confirm dialog switches to Closeout tab
(does not close the dialog).

**V4 — Closeout pre-population.**
Save failure details on a WO; close and reopen the detail dialog; open Closeout tab; verify
previously saved symptom, failure mode, and notes are pre-populated.

**V5 — Filter dropdowns.**
Open WorkOrdersPage; verify Status, Type, and Priority filter dropdowns are present and
functional.

---

*End of Phase 2 - Sub-phase 05 - File 04*
