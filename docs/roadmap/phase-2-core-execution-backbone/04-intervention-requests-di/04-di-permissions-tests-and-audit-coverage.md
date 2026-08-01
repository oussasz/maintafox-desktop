# Phase 2 - Sub-phase 04 - File 04
# DI Permissions, Tests, and Audit Coverage

## Context and Purpose

Files 01 through 03 delivered the complete DI operational machinery: schema, state machine,
triage workflow, SLA engine, attachments, and WO conversion. File 04 closes sub-phase 04 with
three hardening layers:

1. **Permission domain** — the full `di.*` permission matrix with scoping rules, dangerous
   action designations, and step-up requirements modeled consistently with the RBAC framework
   established in Phase 1.
2. **Audit trail** — migration 020 with `di_change_events` (append-only), the audit writer
   (`di/audit.rs`), and the change-event frontend timeline that surfaces every DI lifecycle
   action to supervisors and auditors.
3. **Test coverage specifications** — agent prompts for unit tests (state machine, SLA engine,
   concurrency guards) and integration tests (full submit → screen → approve → convert flow),
   plus a supervisor verification checklist that must pass before SP04 is considered complete.

Together these three areas ensure the DI module is not just functional but verifiable,
traceable, and governable — prerequisites for the analytics and reliability work in later
subphases.

---

## PRD Alignment Checklist

This file addresses PRD §6.4 requirements for:

- [x] `di.*` permission domain (PRD §6.7 domain table entry: "di.* — Intervention Requests")
- [x] Data quality: controlled classifications used where analytics require structured evidence
      (enforced via permission-guarded transitions, not free-text overrides)
- [x] Immutable origin record once converted (enforced by is_immutable_after_conversion guard +
      audit event for any attempt to bypass)
- [x] SLA and backlog analysis: all timestamps (submitted_at, screened_at, approved_at,
      converted_at) stored and computable; SLA breach fired as a `di_change_events` row

---

## Architecture Rules Applied

- **`di_change_events` is append-only.** No update or delete commands exist. The table is the
  permanent audit ledger for every DI lifecycle action, including blocked dangerous actions.
- **Dangerous actions write a `di_change_events` row even when blocked.** A failed step-up
  on approval is recorded with `apply_result = 'blocked'`. This makes attempts visible to
  audit even when they do not succeed.
- **Permission scope hierarchy:** `di.view` → `di.create` → `di.review` → `di.approve` →
  `di.convert` → `di.admin`. Higher scopes do not automatically grant lower scopes. The
  RBAC engine evaluates each permission independently. A reviewer role needs explicit `di.view`
  granted alongside `di.review`.
- **Scoped permissions follow the org_node binding from SP01.** A user with `di.review`
  scoped to entity X only sees DIs whose `org_node_id` belongs to entity X. Global scope
  sees all. This mirrors the `eq.*` and `org.*` scoping patterns.
- **Tests are written before the corresponding feature is tagged complete.** Sprint S2 of
  this file delivers all automated test coverage for the DI module as a whole.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000020_di_change_events.rs` | Immutable DI audit ledger |
| `src-tauri/src/di/audit.rs` | Audit event writer and reader functions |
| `src-tauri/src/di/permissions.rs` | Permission seed definitions for `di.*` domain |
| `src-tauri/src/commands/di.rs` (patch) | list_di_change_events command; audit writer integrated into all transition commands |
| `src-tauri/src/di/tests.rs` | Unit tests: state machine, SLA, concurrency, conversion |
| `src/services/di-audit-service.ts` | Frontend wrapper for change event timeline |
| `src/components/di/DiAuditTimeline.tsx` | Change event timeline component for DI detail dialog |

---

## Prerequisites

- Files 01, 02, and 03 complete (all migrations 017–019 applied)
- Phase 1 RBAC seeder pattern in place; `permissions` table with `domain.action` rows
- `require_permission!` and `require_step_up!` macros work for any domain string
- `di_review_events` and `di_state_transition_log` complete (used as audit sources in queries)

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Audit Trail and Permission Seed | migration 020 + `audit.rs` + `permissions.rs` |
| S2 | Unit and Integration Tests | `di/tests.rs` full test suite |
| S3 | Audit Timeline UI | `di-audit-service.ts` + `DiAuditTimeline.tsx` |

---

## Sprint S1 - Audit Trail and Permission Seed

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the DI audit trail and permission domain seed.

STEP 1 - CREATE src-tauri/migrations/m20260401_000020_di_change_events.rs

use sea_orm_migration::prelude::*;

CREATE TABLE di_change_events (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  di_id           INTEGER NULL REFERENCES intervention_requests(id),
  action          TEXT    NOT NULL,
    -- submitted / screened / returned / rejected / approved / deferred /
    --  reactivated / converted / closed_non_executable / archived /
    --  attachment_added / sla_breached / field_updated / admin_override
  actor_id        INTEGER NULL REFERENCES user_accounts(id),
  acted_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  summary         TEXT    NULL,    -- human-readable description
  details_json    TEXT    NULL,    -- structured JSON payload (field diffs, etc.)
  requires_step_up INTEGER NOT NULL DEFAULT 0,
  apply_result    TEXT    NOT NULL DEFAULT 'applied'
    -- applied / blocked / partial
);

CREATE INDEX idx_dce_di_id    ON di_change_events(di_id);
CREATE INDEX idx_dce_action   ON di_change_events(action);
CREATE INDEX idx_dce_actor    ON di_change_events(actor_id);

-- Seed DI permission domain --
INSERT OR IGNORE INTO permissions (name, description, category, is_dangerous, requires_step_up)
VALUES
  ('di.view',        'View intervention request list and details', 'di', 0, 0),
  ('di.create',      'Submit new intervention requests (all assets)', 'di', 0, 0),
  ('di.create.own',  'Submit intervention requests (own entity only)', 'di', 0, 0),
  ('di.review',      'Screen, return, and reject intervention requests', 'di', 0, 0),
  ('di.approve',     'Approve, defer, or reactivate intervention requests', 'di', 1, 1),
  ('di.convert',     'Convert approved DI to work order', 'di', 1, 1),
  ('di.admin',       'Override, archive, reopen, manage SLA rules', 'di', 1, 0);


STEP 2 - PATCH src-tauri/src/di/mod.rs
  Add: pub mod audit;
       pub mod permissions;

STEP 3 - CREATE src-tauri/src/di/audit.rs

Types:
- `DiChangeEvent` struct matching di_change_events DDL (Serialize/Deserialize)
- `DiAuditInput` struct:
    di_id: Option<i64>,
    action: String,
    actor_id: Option<i64>,
    summary: Option<String>,
    details_json: Option<String>,
    requires_step_up: bool,
    apply_result: String    // "applied" | "blocked" | "partial"

Functions:

A) `record_di_change_event(pool: &SqlitePool, input: DiAuditInput) -> Result<()>`
   INSERT into di_change_events.
   This is a fire-and-log function. It does not return the inserted row.
   Called from all transition commands after a successful (or blocked) action.

B) `list_di_change_events(pool: &SqlitePool, di_id: i64, limit: i64) -> Result<Vec<DiChangeEvent>>`
   SELECT from di_change_events WHERE di_id = ? ORDER BY acted_at ASC LIMIT ?.

C) `list_all_change_events(pool: &SqlitePool, filter: DiAuditFilter) -> Result<Vec<DiChangeEvent>>`
   DiAuditFilter: action: Option<String>, actor_id: Option<i64>,
   date_from: Option<String>, date_to: Option<String>, limit: i64, offset: i64.
   For audit log views used by di.admin users.

STEP 4 - CREATE src-tauri/src/di/permissions.rs

Provide a single function:
`di_permission_domain() -> Vec<(&'static str, &'static str, bool, bool)>`
Returns tuples of (name, description, is_dangerous, requires_step_up) for all 7 di.* permissions.
Used by the system seeder to idempotently insert permissions on app startup.

STEP 5 - PATCH all transition commands in src-tauri/src/commands/di.rs

After each successful transition (screen, return, reject, approve, defer, reactivate, convert):
  Call record_di_change_event(pool, DiAuditInput { di_id: Some(di.id), action, actor_id, ... }).

After each BLOCKED dangerous action (failed step-up on approve or convert):
  Call record_di_change_event with apply_result = "blocked".

STEP 6 - PATCH src-tauri/src/commands/di.rs

Add command:
`list_di_change_events`
  Permission: di.view
  Input: di_id: i64, limit: Option<i64> (default 50)
  Delegates to: audit::list_di_change_events

`list_all_di_change_events`
  Permission: di.admin
  Input: DiAuditFilter
  Delegates to: audit::list_all_change_events

ACCEPTANCE CRITERIA
- migration 020 applies; 7 di.* permission rows inserted
- record_di_change_event does not return error to caller even if insert fails (log,
  do not propagate, to avoid blocking the primary workflow)
- blocked approve attempt writes di_change_events row with apply_result = 'blocked'
- list_di_change_events returns events in chronological order
- cargo check passes
```

### Supervisor Verification - Sprint S1

**V1 - Permission seed.**
After migration 020 applies, `SELECT name FROM permissions WHERE name LIKE 'di.%'` returns 7 rows.

**V2 - Audit on approval.**
Approve a DI; verify 1 row in di_change_events with action = 'approved' and
apply_result = 'applied'.

**V3 - Audit on blocked step-up.**
Call approve_di with invalid step_up_token; verify row in di_change_events with
apply_result = 'blocked'.

**V4 - Audit on conversion.**
Convert a DI; verify row in di_change_events with action = 'converted' and
requires_step_up = 1.

**V5 - Audit immutability.**
Confirm no UPDATE or DELETE command exists for di_change_events in any .rs file.

---

## Sprint S2 - Unit and Integration Tests

### AI Agent Prompt

```text
You are a senior Rust test engineer. Write the full DI test suite.

STEP 1 - CREATE src-tauri/src/di/tests.rs (or tests/ directory)

Use #[cfg(test)] and tokio::test with an in-memory SQLite database seeded by running
all migrations (017–020).

--- UNIT TESTS: State Machine ---

test_01_all_valid_transitions
  For every valid (from, to) pair in the PRD §6.4 state machine, call guard_transition;
  assert Ok(()).

test_02_invalid_transitions
  Test at least the following invalid pairs; assert Err:
  - (submitted, approved_for_planning)
  - (submitted, archived)
  - (awaiting_approval, submitted)
  - (converted_to_work_order, pending_review)
  - (archived, submitted)

test_03_immutable_states
  is_immutable_after_conversion returns true for:
  converted_to_work_order, closed_as_non_executable, rejected, archived.
  Returns false for:
  submitted, pending_review, returned_for_clarification, screened,
  awaiting_approval, approved_for_planning, deferred.

test_04_di_code_generation
  Create 3 DIs sequentially; codes must be DI-0001, DI-0002, DI-0003.
  No gaps or duplicates.

--- UNIT TESTS: SLA Engine ---

test_05_sla_rule_priority
  Insert 2 rules: high+iot (response=2h) and high+NULL (response=8h).
  resolve_sla_rule(urgency=high, origin=iot) must return 2h rule.
  resolve_sla_rule(urgency=high, origin=operator) must return 8h rule.

test_06_sla_breach_detection
  Create DI with submitted_at = 10 hours ago, urgency = critical (target 1h),
  screened_at = NULL.
  compute_sla_status: is_response_breached = true.

test_07_sla_no_breach_when_screened
  Same DI but screened_at = 30 minutes ago.
  compute_sla_status: is_response_breached = false.

--- UNIT TESTS: Concurrency ---

test_08_optimistic_lock_on_draft_update
  Create DI (row_version = 1). Call update_di_draft with expected_row_version = 0.
  Must return error. DI unmodified.

test_09_optimistic_lock_on_screen
  Create and submit DI. Call screen_di with expected_row_version = 999.
  Must return error.

--- INTEGRATION TEST: Full Lifecycle ---

test_10_full_di_lifecycle
  Phase A - Submission:
    Create DI via create_intervention_request. Assert status = submitted.
    Assert 1 row in di_state_transition_log (action = submit).
    Assert 1 row in di_change_events (action = submitted).

  Phase B - Review:
    Call screen_di with valid inputs. Assert status = screened.
    Assert screened_at is set. Assert reviewer_id set.

  Phase C - Approval:
    Use a mock step_up_token or set SKIP_STEP_UP=true in test config.
    Call approve_di_for_planning. Assert status = approved_for_planning.
    Assert approved_at is set.

  Phase D - Conversion:
    Call convert_di_to_work_order (mock step-up).
    Assert status = converted_to_work_order.
    Assert converted_to_wo_id is NOT NULL.
    Assert converted_at set.
    Assert row in work_order_stubs with source_di_id = DI.id.

  Phase E - Immutability:
    Call update_di_draft on converted DI. Must return error.
    Assert DI fields unchanged.

  Phase F - Audit completeness:
    Call list_di_change_events(di.id, 100).
    Assert events include: submitted, screened, approved, converted (4 rows min).

--- INTEGRATION TEST: Return and Resubmission ---

test_11_return_and_resubmit
  Submit DI.
  Screen → return_for_clarification (with note). Assert status = returned_for_clarification.
  update_di_draft with new description (allowed in returned state).
  Screen again from pending_review. Assert screened_at updated.

--- INTEGRATION TEST: Rejection ---

test_12_rejection_path
  Submit → screen (pending_review, skip to awaiting_approval for speed using direct
  guard bypass in test only) → reject.
  Assert status = rejected, declined_at set.
  Attempt guard_transition(rejected, pending_review). Must fail.
  Call archive: guard_transition(rejected, archived). Must succeed.

ACCEPTANCE CRITERIA
- All 12 tests pass: cargo test di::tests
- Zero warnings about unused variables in test module
- test_10 runs end-to-end with all 4 audit events confirmed
- No test uses unwrap() without a comment explaining why it is safe in test context
```

### Supervisor Verification - Sprint S2

**V1 - Test count.**
`cargo test di::tests -- --list` shows at least 12 tests.

**V2 - All pass.**
`cargo test di::tests 2>&1` shows 0 failures, 0 errors.

**V3 - Invalid transition coverage.**
test_02 must include at least 5 distinct invalid pairs.

**V4 - Full lifecycle confirmation.**
test_10 must assert that `converted_to_wo_id` is populated after conversion.

---

## Sprint S3 - Audit Timeline UI

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the DI audit timeline service and component.

CREATE src/services/di-audit-service.ts

Types:
- DiChangeEvent (matches di_change_events DDL; Zod schema for validation)
- DiAuditFilter: {
    action?: string;
    actorId?: number;
    dateFrom?: string;
    dateTo?: string;
    limit?: number;
    offset?: number;
  }

Functions:
- listDiChangeEvents(diId: number, limit?: number): Promise<DiChangeEvent[]>
- listAllDiChangeEvents(filter: DiAuditFilter): Promise<DiChangeEvent[]>

Both functions validate response with Zod.

CREATE src/components/di/DiAuditTimeline.tsx

Props: diId: number

Behavior:

On mount, load change events via listDiChangeEvents(diId, 50).

Render a vertical timeline. For each event:
  - Icon based on action (submit=arrow-up, screened=eye, approved=check, rejected=x-circle,
    converted=bolt, blocked=shield-x, deferred=clock, returned=arrow-back, other=dot)
  - Actor name (if actor_id present, show "User #{actor_id}" until personnel joins are added)
  - acted_at formatted as locale datetime
  - summary field content
  - apply_result badge: "Applied" (green) / "Blocked" (red) / "Partial" (yellow)
  - requires_step_up badge: "Step-up" (blue) when requires_step_up = 1

Loading state: skeleton rows while fetching.
Empty state: "No audit events recorded for this request."
Error state: "Could not load audit trail."

The component does NOT expose delete or edit controls. It is read-only.

ALSO PATCH src/components/di/DiDetailPanel.tsx (or equivalent DI detail view):
Add a "Audit Trail" tab that renders <DiAuditTimeline diId={di.id} />.
This tab must be visible to any user with di.view permission.

> **UX NOTE (S4):** The DI detail view is rendered inside a floating dialog
> (`DiDetailDialog.tsx`), not a side panel. See `docs/UX_DETAIL_DIALOG_PATTERN.md`
> (pattern UX-DW-001). DiDetailPanel is a child of DiDetailDialog.

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- DiAuditTimeline renders at least 4 event rows in the test_10 full lifecycle scenario
- apply_result = 'blocked' renders a red badge
- The component is read-only: no mutating actions in DiAuditTimeline
- DiDetailPanel has an Audit Trail tab with the timeline
```

### Supervisor Verification - Sprint S3

**V1 - Timeline renders blocked event.**
Complete a conversion with a bad step-up token first; open audit timeline; blocked event must
show red "Blocked" badge.

**V2 - Read-only enforcement.**
Inspect DiAuditTimeline JSX; confirm no onClick handlers that call any mutating service.

**V3 - typecheck clean.**
`pnpm typecheck` returns 0 errors.

**V4 - Timeline tab visibility.**
Open DI detail as a user with only di.view; Audit Trail tab must be visible and populated.

---

## DI Module Completion Checklist

Before marking sub-phase 04 complete, verify all of the following:

**Schema:**
- [ ] Migrations 017 through 020 all apply cleanly via `cargo test` migration runner
- [ ] `intervention_requests` has all 11 status values enforced by the Rust state machine
- [ ] `di_state_transition_log` has a row for every status transition tested
- [ ] `di_review_events` has a row for every review decision tested
- [ ] `di_change_events` has rows including at least one blocked dangerous action
- [ ] `di_sla_rules` has 4 default seeded rows
- [ ] `work_order_stubs` has a row after conversion (or `work_orders` if SP05 ran first)

**Rust:**
- [ ] `cargo check` passes with zero errors
- [ ] All DI commands registered in the Tauri invoke_handler
- [ ] `di.view`, `di.create`, `di.create.own`, `di.review`, `di.approve`, `di.convert`,
      `di.admin` exist in the `permissions` table
- [ ] Guard transition rejects all non-listed transitions
- [ ] Step-up required for approve and convert commands
- [ ] Audit events written for both successful and blocked dangerous actions

**TypeScript:**
- [ ] `pnpm typecheck` passes with zero errors
- [ ] All 7 Rust commands have matching TypeScript invoke wrappers
- [ ] Zod validation on all response types

**Tests:**
- [ ] All 12 unit/integration tests pass: `cargo test di::tests`
- [ ] test_10 full lifecycle passes end-to-end

**Cross-module contracts established for SP05:**
- [ ] `converted_to_wo_id` on `intervention_requests` — nullable FK to `work_orders`
- [ ] `source_di_id` on `work_order_stubs` — consumed by SP05 to link WOs back to origin DI
- [ ] `di.convert` permission — consumed by SP05 conversion workflow
- [ ] `di-conversion-service.ts` `WoConversionResult.woId` — SP05 uses this for navigation

---

## Sprint S4 — Dashboard, Calendar View, Archive View, Context Menu, and Stats Endpoint

> **Gap addressed:** The web app has 4 views (list, kanban, calendar, dashboard)
> plus an archive section, context menus, and a /stats analytics endpoint. S1–S3
> delivered list + kanban + detail dialog but no dashboard, calendar, archive, or
> context menu. The backend also lacks a stats aggregation command. This sprint
> closes all remaining UI and analytics gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/di/stats.rs` | DI statistics aggregation queries |
| `src-tauri/src/commands/di.rs` (patch) | `get_di_stats` IPC command |
| `src/services/di-stats-service.ts` | Frontend wrapper for stats endpoint |
| `src/stores/di-stats-store.ts` | Zustand store for dashboard data |
| `src/components/di/DiDashboardView.tsx` | KPI cards + charts dashboard |
| `src/components/di/DiCalendarView.tsx` | Month / week / day calendar view |
| `src/components/di/DiArchivePanel.tsx` | Archive table for rejected + archived DIs |
| `src/components/di/DiContextMenu.tsx` | Right-click context menu for DI rows / cards |
| `src/pages/RequestsPage.tsx` (patch) | Add Dashboard + Calendar view tabs; archive section; context menu integration |
| `src/i18n/locale-data/{fr,en}/di.json` (patch) | Dashboard, calendar, archive labels |

### get_di_stats — Backend Stats Command

`get_di_stats` — requires `di.view`

**Input:** `DiStatsFilter`

```rust
struct DiStatsFilter {
    date_from: Option<String>,   // ISO date
    date_to: Option<String>,     // ISO date
    entity_id: Option<i64>,      // Org node scope
}
```

**Output:** `DiStatsPayload`

```rust
struct DiStatsPayload {
    // KPI card values
    total: i64,
    pending: i64,
    in_progress: i64,        // submitted + pending_review + screened + awaiting_approval
    closed: i64,             // closed_as_non_executable + archived
    closed_this_month: i64,
    overdue: i64,            // SLA breached
    sla_met_count: i64,      // DIs resolved within SLA
    sla_total: i64,          // DIs with SLA rules applied
    safety_issues: i64,      // safety_flag = 1

    // Distributions
    status_distribution: Vec<StatusCount>,      // { status, count }
    priority_distribution: Vec<PriorityCount>,  // { priority, count }
    type_distribution: Vec<TypeCount>,          // { origin_type, count }

    // Trend (adaptive granularity: daily if ≤93 days, monthly otherwise)
    monthly_trend: Vec<TrendPoint>,   // { period, created, closed }
    available_years: Vec<i32>,

    // Age stats (for open DIs)
    avg_age_days: f64,
    max_age_days: f64,

    // Tables
    top_equipment: Vec<EquipmentCount>,  // { asset_id, asset_label, count, percentage }
    overdue_dis: Vec<OverdueDi>,         // { id, code, title, priority, days_overdue }
}
```

### DiDashboardView.tsx — Dashboard Component

**Layout:**

**Row 1 — KPI Cards** (8 cards, responsive grid):
| Card | Value | Colour |
|------|-------|--------|
| Total | `total` | neutral |
| En attente | `pending` | amber |
| En cours | `in_progress` | blue |
| SLA respecté | `sla_met_count / sla_total` (%) | green if ≥80%, red otherwise |
| Clôturées (mois) | `closed_this_month` | teal |
| En retard | `overdue` | red |
| Enjeux sécurité | `safety_issues` | destructive |
| Âge moyen | `avg_age_days` days | muted |

**Row 2 — Charts** (responsive 2-col grid):
- **Status doughnut** — status_distribution (Recharts PieChart)
- **Priority bar** — priority_distribution (horizontal BarChart)

**Row 3 — Trend** (full width):
- Monthly trend (ComposedChart: bars = "created", line = "closed")
- Period pills: 3M / 6M / 12M / 24M / All
- Year picker + month picker for drill-down

**Row 4 — Tables** (2-col):
- **Top 10 Equipment** — asset_label, count, percentage bar
- **Overdue DIs** — code, title, priority badge, days overdue (red)

### DiCalendarView.tsx — Calendar View

**Props:** Uses `items` from di-store.

**Modes (toggle buttons):**
- **Month** — 7-day grid; DIs indexed by `submitted_at`; up to 3 chips per day cell, "+N" overflow
- **Week** — 7-day × 24-hour grid; all-day row above; DIs positioned by `submitted_at`
- **Day** — Full 24-hour timeline for single day

**Chip rendering:** Each DI chip shows code + priority color bar.
- Click chip → opens DiDetailDialog
- Right-click → DiContextMenu

**Navigation:** Prev / Next buttons, "Today" button, month/week/day toggle.

### DiArchivePanel.tsx — Archive View

Collapsible section at the bottom of RequestsPage.

**Content:**
- Header: "Archive" with badge count
- DataTable: code, title, equipment, status (Rejected / Archived), requester, date, reason
- Filters: search, status (rejected / archived)
- No edit actions — read-only

**Data source:** `loadDis({ status: ['rejected', 'archived'] })`.

### DiContextMenu.tsx — Right-Click Menu

Uses Radix ContextMenu or custom dropdown.

**Menu items (conditional on permissions + DI status):**

| Item | Condition | Action |
|------|-----------|--------|
| Voir détail | always | opens DiDetailDialog |
| Modifier | `di.create.own` + status=submitted/pending draft | opens DiFormDialog in edit mode |
| Supprimer | `di.admin` + status=submitted | delete confirmation dialog |
| Approuver | `di.approve` + status=awaiting_approval | opens DiApprovalDialog |
| Rejeter | `di.review` + status in review states | opens DiRejectionDialog |
| Renvoyer | `di.review` + status=pending_review | opens DiReturnDialog |
| Assigner | `ot.assign` (future) | opens assignment picker |

Integrate into:
- DiKanbanBoard (right-click on card)
- DataTable list rows (right-click on row)
- DiCalendarView (right-click on chip)

### RequestsPage.tsx Patch — View Tabs

Update the view toggle to include 4 modes:

```tsx
type ViewMode = "list" | "kanban" | "calendar" | "dashboard";
```

Add icons: List, Columns3, CalendarDays, BarChart3.

Below the main view area, render `<DiArchivePanel />` as a collapsible section.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors (Rust + TypeScript)
- get_di_stats returns correct KPI counts matching database state
- DiDashboardView renders 8 KPI cards and 4 chart/table sections
- DiCalendarView renders month/week/day modes with DI chips on correct dates
- DiArchivePanel shows only rejected + archived DIs
- DiContextMenu respects permission gates (items hidden for unauthorized users)
- Trend granularity switches: daily when range ≤ 93 days, monthly otherwise
- SLA % card shows green when ≥ 80%, red otherwise
- RequestsPage has 4 view tabs: List, Kanban, Calendar, Dashboard
```

### Supervisor Verification — Sprint S4

**V1 — Dashboard KPIs.**
Create 5 DIs with mixed statuses; verify KPI cards show correct counts.

**V2 — Calendar chip click.**
Click a DI chip in month view; DiDetailDialog must open.

**V3 — Context menu permission.**
Right-click a DI as a user without `di.approve`; "Approuver" must not appear.

**V4 — Archive isolation.**
DiArchivePanel must not show submitted/pending DIs; only rejected and archived.

**V5 — Stats trend granularity.**
Request stats with a 30-day range; verify daily data points. Request with 180-day range;
verify monthly aggregation.

---

*End of Phase 2 - Sub-phase 04 - File 04*
