# Phase 2 - Sub-phase 05 - File 01
# WO Domain Model and Execution States

## Context and Purpose

Sub-phase 05 delivers the Work Order (OT) module — the formal authorization and evidence record
for all maintenance execution in Maintafox Desktop. Every work order is both a planning contract
(what is intended) and a structured evidence object (what actually happened). Together these two
roles make the WO the single most important analytical record in the system.

This file establishes:

- the complete `work_orders` core schema (migration 021) — superseding the `work_order_stubs`
  placeholder created by SP04 conversion
- the `wo_state_transition_log` append-only table (migration 021)
- the exact 12-state machine from PRD §6.5 with transition guards
- base CRUD, list, and search IPC commands
- the Rust domain types and TypeScript service/store

Files 02, 03, and 04 add the planning/execution machinery, close-out/cost quality gates, and the
permission/audit/analytics readiness layer.

---

## PRD Alignment Checklist

This file addresses PRD §6.5 requirements for:

- [x] Core entity `work_orders` with all PRD-listed fields:
      id, code (WOR-XXXX), type_id, status_id, equipment_id, component_id (nullable),
      location_id (nullable), requester_id, source_di_id, entity_id, planner_id, approver_id,
      assigned_group_id, primary_responsible_id, urgency_id, title, description,
      planned_start, planned_end, scheduled_at, actual_start, actual_end,
      mechanically_completed_at, technically_verified_at, closed_at,
      expected_duration_hours, actual_duration_hours, active_labor_hours,
      total_waiting_hours, downtime_hours, labor_cost, parts_cost, service_cost, total_cost,
      recurrence_risk_level, production_impact_id, root_cause_summary,
      corrective_action_summary, verification_method, notes
- [x] Supporting entities: `work_order_types`, `work_order_statuses`, `urgency_levels`,
      `delay_reason_codes`
- [x] 12-state workflow:
      Draft → Awaiting Approval → Planned → Ready To Schedule → Assigned →
      Waiting For Prerequisite → In Progress → Paused → Mechanically Complete →
      Technically Verified → Closed; any pre-close state → Cancelled
- [x] `source_di_id` FK — permanent traceability back to the originating DI (SP04 contract)
- [x] `ot.*` permission domain established

---

## Architecture Rules Applied

- **`work_order_stubs` is replaced atomically.** Migration 021 renames `work_order_stubs`
  to `work_orders` and adds all missing columns in one ALTER sequence. Existing stub rows
  are preserved with NULL for new non-required columns. `source_di_id` is kept as-is.
- **State machine is enforced in Rust, never the frontend.** `WoStatus::allowed_transitions`
  drives all state movement. No raw status string write bypasses the guard.
- **`wo_state_transition_log` is append-only.** Every status change writes a row; no update
  or delete command targets this table.
- **WO code is unique, uppercase, non-recycled.** Format `WOR-NNNN`. Sequence never reused.
- **No hard delete for WOs beyond draft.** A WO in any status other than `draft` is cancelled
  or archived; delete is blocked and returns an error. Draft WOs may be deleted by `ot.admin`.
- **Optimistic concurrency via `row_version`.** All mutation commands check `expected_row_version`.
- **`work_order_types` and `work_order_statuses` are soft-configurable.** The system seeds the
  7 PRD types and 12 PRD statuses as `is_system = 1` rows. Tenants can add custom types/statuses
  but cannot delete or rename system rows. This follows the reference-data governance pattern from SP03.
- **Urgency is separate from the reference data system.** `urgency_levels` is a fixed 5-level
  scale (1=Faible → 5=Critique) seeded in migration 021. It is not in the `reference_values`
  table to enable reliable numeric sort and color-coding.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000021_wo_domain_core.rs` | Full `work_orders` schema; `work_order_types`; `work_order_statuses`; `urgency_levels`; `delay_reason_codes`; `wo_state_transition_log` |
| `src-tauri/src/wo/mod.rs` | Module root with sub-module declarations |
| `src-tauri/src/wo/domain.rs` | `WoStatus` 12-state enum, transition guard, code generator, supporting enums |
| `src-tauri/src/wo/queries.rs` | List, get, search, DI-linked lookup queries |
| `src-tauri/src/commands/wo.rs` | IPC commands: create, get, list, update draft fields |
| `src/services/wo-service.ts` | Typed Tauri invoke wrappers for all WO commands |
| `src/stores/wo-store.ts` | Zustand store: list, pagination, active WO, filters |

---

## Prerequisites

- SP04 complete: `work_order_stubs` table exists from migration 019; `source_di_id` present
- SP01 complete: `org_nodes` available for `entity_id` / `assigned_group_id` bindings
- SP02 complete: `asset_registry` for `equipment_id` lookups
- SP03 complete: `reference_values` for failure codes (`symptom_id`, `failure_mode_id`,
  `failure_cause_id`, `failure_effect_id`) consumed in File 03
- Phase 1 auth: `user_accounts`, `require_permission!`, `require_step_up!` available

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Core Schema, State Machine, and Seed Data | migration 021 + `domain.rs` |
| S2 | Queries and IPC Commands | `queries.rs` + `commands/wo.rs` |
| S3 | Frontend Service and Store | `wo-service.ts` + `wo-store.ts` |

---

## Sprint S1 - Core Schema, State Machine, and Seed Data

### AI Agent Prompt

```text
You are a senior Rust engineer working on Maintafox Desktop (Tauri 2 / SQLite / sqlx).
Implement the WO core migration and domain type layer.

STEP 1 - CREATE src-tauri/migrations/m20260401_000021_wo_domain_core.rs

use sea_orm_migration::prelude::*;

-- A: Rename work_order_stubs to work_orders and add all missing columns --
-- Use CREATE + INSERT + DROP pattern (SQLite does not support ALTER TABLE RENAME in older
-- versions). The pattern is:
--   1. CREATE TABLE work_orders (...all columns...)
--   2. INSERT INTO work_orders SELECT stub columns, NULL for new ones FROM work_order_stubs
--   3. DROP TABLE work_order_stubs

CREATE TABLE work_order_types (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  code        TEXT    NOT NULL UNIQUE,
  label       TEXT    NOT NULL,
  is_system   INTEGER NOT NULL DEFAULT 0,
  is_active   INTEGER NOT NULL DEFAULT 1
);
INSERT INTO work_order_types (code, label, is_system) VALUES
  ('corrective',      'Corrective',       1),
  ('preventive',      'Preventive',       1),
  ('improvement',     'Improvement',      1),
  ('inspection',      'Inspection',       1),
  ('emergency',       'Emergency',        1),
  ('overhaul',        'Overhaul',         1),
  ('condition_based', 'Condition-Based',  1);

CREATE TABLE work_order_statuses (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  code        TEXT    NOT NULL UNIQUE,
  label       TEXT    NOT NULL,
  color       TEXT    NOT NULL DEFAULT '#808080',
  macro_state TEXT    NOT NULL,
    -- open / executing / completed / closed / cancelled
  is_terminal INTEGER NOT NULL DEFAULT 0,
  is_system   INTEGER NOT NULL DEFAULT 0,
  sequence    INTEGER NOT NULL DEFAULT 0
);
INSERT INTO work_order_statuses (code, label, color, macro_state, is_terminal, is_system, sequence) VALUES
  ('draft',                   'Draft',                   '#94A3B8', 'open',      0, 1, 1),
  ('awaiting_approval',       'Awaiting Approval',       '#F59E0B', 'open',      0, 1, 2),
  ('planned',                 'Planned',                 '#3B82F6', 'open',      0, 1, 3),
  ('ready_to_schedule',       'Ready To Schedule',       '#6366F1', 'open',      0, 1, 4),
  ('assigned',                'Assigned',                '#8B5CF6', 'executing', 0, 1, 5),
  ('waiting_for_prerequisite','Waiting For Prerequisite','#F97316', 'executing', 0, 1, 6),
  ('in_progress',             'In Progress',             '#10B981', 'executing', 0, 1, 7),
  ('paused',                  'Paused',                  '#EF4444', 'executing', 0, 1, 8),
  ('mechanically_complete',   'Mechanically Complete',   '#06B6D4', 'completed', 0, 1, 9),
  ('technically_verified',    'Technically Verified',    '#22C55E', 'completed', 0, 1, 10),
  ('closed',                  'Closed',                  '#64748B', 'closed',    1, 1, 11),
  ('cancelled',               'Cancelled',               '#DC2626', 'cancelled', 1, 1, 12);

CREATE TABLE urgency_levels (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  level       INTEGER NOT NULL UNIQUE,  -- 1 (lowest) to 5 (highest)
  label       TEXT    NOT NULL,
  label_fr    TEXT    NOT NULL,
  hex_color   TEXT    NOT NULL
);
INSERT INTO urgency_levels (level, label, label_fr, hex_color) VALUES
  (1, 'Very Low',  'Très Faible', '#64748B'),
  (2, 'Low',       'Faible',      '#3B82F6'),
  (3, 'Medium',    'Moyenne',     '#F59E0B'),
  (4, 'High',      'Haute',       '#F97316'),
  (5, 'Critical',  'Critique',    '#DC2626');

CREATE TABLE delay_reason_codes (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  code      TEXT    NOT NULL UNIQUE,
  label     TEXT    NOT NULL,
  category  TEXT    NOT NULL,
    -- parts / permit / shutdown / vendor / labor / access / diagnosis / other
  is_active INTEGER NOT NULL DEFAULT 1
);
INSERT INTO delay_reason_codes (code, label, category) VALUES
  ('no_parts',         'Awaiting Spare Parts',        'parts'),
  ('backordered',      'Parts Backordered',            'parts'),
  ('no_permit',        'Permit Not Issued',            'permit'),
  ('permit_expired',   'Permit Expired / Revoked',     'permit'),
  ('no_shutdown',      'Shutdown Window Unavailable',  'shutdown'),
  ('vendor_delay',     'Vendor / Contractor Delay',    'vendor'),
  ('no_labor',         'Insufficient Labor',           'labor'),
  ('no_access',        'Access to Equipment Denied',   'access'),
  ('diagnosis',        'Awaiting Diagnosis Result',    'diagnosis'),
  ('other',            'Other (see notes)',            'other');

-- FULL work_orders TABLE --
CREATE TABLE work_orders (
  id                          INTEGER PRIMARY KEY AUTOINCREMENT,
  code                        TEXT    NOT NULL UNIQUE,     -- WOR-0001
  -- Classification
  type_id                     INTEGER NOT NULL REFERENCES work_order_types(id),
  status_id                   INTEGER NOT NULL REFERENCES work_order_statuses(id),
  -- Asset context
  equipment_id                INTEGER NULL REFERENCES asset_registry(id),
  component_id                INTEGER NULL,
  location_id                 INTEGER NULL REFERENCES org_nodes(id),
  -- People
  requester_id                INTEGER NULL REFERENCES user_accounts(id),
  source_di_id                INTEGER NULL REFERENCES intervention_requests(id),
  entity_id                   INTEGER NULL REFERENCES org_nodes(id),
  planner_id                  INTEGER NULL REFERENCES user_accounts(id),
  approver_id                 INTEGER NULL REFERENCES user_accounts(id),
  assigned_group_id           INTEGER NULL REFERENCES org_nodes(id),
  primary_responsible_id      INTEGER NULL REFERENCES user_accounts(id),
  -- Urgency
  urgency_id                  INTEGER NULL REFERENCES urgency_levels(id),
  -- Core description
  title                       TEXT    NOT NULL,
  description                 TEXT    NULL,
  -- Timing
  planned_start               TEXT    NULL,
  planned_end                 TEXT    NULL,
  scheduled_at                TEXT    NULL,
  actual_start                TEXT    NULL,
  actual_end                  TEXT    NULL,
  mechanically_completed_at   TEXT    NULL,
  technically_verified_at     TEXT    NULL,
  closed_at                   TEXT    NULL,
  cancelled_at                TEXT    NULL,
  -- Duration accumulators
  expected_duration_hours     REAL    NULL,
  actual_duration_hours       REAL    NULL,
  active_labor_hours          REAL    NULL DEFAULT 0,
  total_waiting_hours         REAL    NULL DEFAULT 0,
  downtime_hours              REAL    NULL DEFAULT 0,
  -- Cost accumulators
  labor_cost                  REAL    NULL DEFAULT 0,
  parts_cost                  REAL    NULL DEFAULT 0,
  service_cost                REAL    NULL DEFAULT 0,
  total_cost                  REAL    NULL DEFAULT 0,
  -- Close-out evidence
  recurrence_risk_level       TEXT    NULL,  -- none / low / medium / high
  production_impact_id        INTEGER NULL REFERENCES reference_values(id),
  root_cause_summary          TEXT    NULL,
  corrective_action_summary   TEXT    NULL,
  verification_method         TEXT    NULL,
  -- Metadata
  notes                       TEXT    NULL,
  cancel_reason               TEXT    NULL,
  row_version                 INTEGER NOT NULL DEFAULT 1,
  created_at                  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at                  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE INDEX idx_wo_status    ON work_orders(status_id);
CREATE INDEX idx_wo_equipment ON work_orders(equipment_id);
CREATE INDEX idx_wo_entity    ON work_orders(entity_id);
CREATE INDEX idx_wo_planner   ON work_orders(planner_id);
CREATE INDEX idx_wo_source_di ON work_orders(source_di_id);
CREATE INDEX idx_wo_urgency   ON work_orders(urgency_id);

-- Migrate stubs if table exists --
INSERT OR IGNORE INTO work_orders
  (code, source_di_id, equipment_id, location_id, title, urgency_id,
   status_id, type_id, created_at)
SELECT
  s.code,
  s.source_di_id,
  s.asset_id,
  s.org_node_id,
  s.title,
  (SELECT id FROM urgency_levels WHERE label = s.urgency LIMIT 1),
  (SELECT id FROM work_order_statuses WHERE code = 'draft' LIMIT 1),
  (SELECT id FROM work_order_types WHERE code = 'corrective' LIMIT 1),
  s.created_at
FROM work_order_stubs s
WHERE NOT EXISTS (SELECT 1 FROM work_orders w WHERE w.code = s.code);

DROP TABLE IF EXISTS work_order_stubs;

-- Append-only state transition log --
CREATE TABLE wo_state_transition_log (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  wo_id       INTEGER NOT NULL REFERENCES work_orders(id),
  from_status TEXT    NOT NULL,
  to_status   TEXT    NOT NULL,
  action      TEXT    NOT NULL,
  actor_id    INTEGER NULL REFERENCES user_accounts(id),
  reason_code TEXT    NULL,
  notes       TEXT    NULL,
  acted_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX idx_wostl_wo_id ON wo_state_transition_log(wo_id);


STEP 2 - CREATE src-tauri/src/wo/mod.rs

pub mod domain;
pub mod queries;


STEP 3 - CREATE src-tauri/src/wo/domain.rs

A) `WoStatus` enum — exact 12 PRD states:
   Draft, AwaitingApproval, Planned, ReadyToSchedule, Assigned,
   WaitingForPrerequisite, InProgress, Paused, MechanicallyComplete,
   TechnicallyVerified, Closed, Cancelled

B) `WoStatus::as_str()` returning snake_case DB values:
   draft / awaiting_approval / planned / ready_to_schedule / assigned /
   waiting_for_prerequisite / in_progress / paused / mechanically_complete /
   technically_verified / closed / cancelled

C) `WoStatus::try_from(s: &str)`

D) `WoStatus::allowed_transitions(&self) -> &'static [WoStatus]`
   EXACT PRD transition table:
   draft                    -> [awaiting_approval, planned]
   awaiting_approval        -> [planned, cancelled]
   planned                  -> [ready_to_schedule, cancelled]
   ready_to_schedule        -> [assigned, cancelled]
   assigned                 -> [waiting_for_prerequisite, in_progress, cancelled]
   waiting_for_prerequisite -> [assigned, in_progress, cancelled]
   in_progress              -> [paused, mechanically_complete, cancelled]
   paused                   -> [in_progress, cancelled]
   mechanically_complete    -> [technically_verified, in_progress, cancelled]
   technically_verified     -> [closed]
   closed                   -> []
   cancelled                -> []

   NOTE: cancelled is reachable from any pre-terminal state. Implement as:
   `draft | awaiting_approval | ... | mechanically_complete` -> includes Cancelled in list.

E) `WoStatus::is_terminal(&self) -> bool`
   Returns true for Closed, Cancelled.

F) `WoStatus::is_executing(&self) -> bool`
   Returns true for InProgress, Paused.

G) `WoStatus::requires_step_up_for_close(&self) -> bool`
   Returns true for TechnicallyVerified → Closed transition (closure quality gate).

H) `guard_wo_transition(from: &WoStatus, to: &WoStatus) -> Result<(), String>`
   Checks `to` is in `from.allowed_transitions()`; returns descriptive error if not.

I) `WoMacroState` enum: Open, Executing, Completed, Closed, Cancelled
   with as_str() / try_from().

J) `generate_wo_code(pool: &SqlitePool) -> Result<String, sqlx::Error>`
   SELECT COALESCE(MAX(CAST(SUBSTR(code,5) AS INT)),0)+1 FROM work_orders
   WHERE code LIKE 'WOR-%';
   Return "WOR-" + zero-padded 4 digits.

K) `WorkOrder` struct matching all DDL columns.

L) `WoCreateInput` struct:
   type_id, equipment_id?, location_id?, source_di_id?, entity_id?,
   planner_id?, urgency_id?, title, description?, notes?,
   creator_id

M) `WoDraftUpdateInput` struct:
   id, expected_row_version, type_id?, equipment_id?, location_id?,
   description?, planned_start?, planned_end?, expected_duration_hours?,
   notes?, urgency_id?

ACCEPTANCE CRITERIA
- All 12 WoStatus variants present
- Transition table matches PRD §6.5 exactly
- guard_wo_transition returns error for any non-listed pair
- Closed and Cancelled have empty allowed_transitions
- generate_wo_code returns WOR-0001 on first call in empty DB
- Migration migrates work_order_stubs rows before dropping the table
```

### Supervisor Verification - Sprint S1

**V1 - Migration applies cleanly.**
`cargo test` with migration runner. All tables created; stubs migrated; `work_order_stubs`
table absent after migration 021.

**V2 - Seed row counts.**
After migration: `work_order_types` has 7 rows, `work_order_statuses` has 12 rows,
`urgency_levels` has 5 rows, `delay_reason_codes` has 10 rows.

**V3 - State machine completeness.**
All valid forward transitions pass `guard_wo_transition`. At least 5 invalid pairs
(e.g., draft→in_progress, closed→draft) return Err.

**V4 - Cancelled reachability.**
Cancelled is in `allowed_transitions` for draft, awaiting_approval, planned,
ready_to_schedule, assigned, waiting_for_prerequisite, in_progress, paused,
mechanically_complete.

---

## Sprint S2 - Queries and IPC Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement WO queries and base IPC commands.

STEP 1 - CREATE src-tauri/src/wo/queries.rs

Functions (all async, &SqlitePool):

A) `list_work_orders(pool, filter: WoListFilter) -> Result<WoListPage>`
   WoListFilter: status_codes: Option<Vec<String>>, type_codes: Option<Vec<String>>,
   equipment_id: Option<i64>, entity_id: Option<i64>, planner_id: Option<i64>,
   primary_responsible_id: Option<i64>, urgency_level: Option<i64>,
   source_di_id: Option<i64>, search: Option<String>,
   date_from: Option<String>, date_to: Option<String>,
   limit: i64, offset: i64
   Returns WoListPage { items: Vec<WorkOrder>, total: i64 }
   Join: work_order_statuses (code, label, color), work_order_types (code, label),
         urgency_levels (level, label, hex_color), asset_registry (asset_code, asset_label),
         user_accounts planner (username), user_accounts responsible (username)

B) `get_work_order(pool, id: i64) -> Result<Option<WorkOrder>>`
   Full row with all joins.

C) `get_wo_transition_log(pool, wo_id: i64) -> Result<Vec<WoTransitionRow>>`
   SELECT all from wo_state_transition_log WHERE wo_id = ? ORDER BY acted_at ASC.
   WoTransitionRow: id, from_status, to_status, action, actor_id, reason_code, notes, acted_at.

D) `create_work_order(pool, input: WoCreateInput) -> Result<WorkOrder>`
   Generates WO code; sets status to 'draft' (status_id for code='draft');
   sets created_at = updated_at = now; row_version = 1.
   Writes wo_state_transition_log: from='none', to='draft', action='create'.
   If source_di_id is provided, verify the DI exists before insert.
   Returns created WorkOrder.

E) `update_wo_draft_fields(pool, input: WoDraftUpdateInput) -> Result<WorkOrder>`
   Only allowed if status = draft.
   UPDATE with row_version check (expected_row_version). rows_affected must = 1.
   Returns updated WorkOrder.

F) `cancel_work_order(pool, input: WoCancelInput) -> Result<WorkOrder>`
   WoCancelInput: id, expected_row_version, actor_id, cancel_reason: String (required)
   Calls guard_wo_transition from current status to Cancelled.
   cancel_reason must not be empty.
   UPDATE: status = cancelled, cancel_reason = ?, cancelled_at = now,
   row_version + 1, updated_at = now.
   Writes wo_state_transition_log: action = 'cancel'.
   Returns updated WorkOrder.

STEP 2 - CREATE src-tauri/src/commands/wo.rs

Register these Tauri commands. Pull user identity from session state.

A) `list_wo` — requires `ot.view`
   Input: WoListFilter
   Output: WoListPage

B) `get_wo` — requires `ot.view`
   Input: id: i64
   Output: WorkOrder + WoTransitionRow list

C) `create_wo` — requires `ot.create`
   Validates: title not empty, type_id resolves, source_di_id resolves if provided.
   Calls: queries::create_work_order

D) `update_wo_draft` — requires `ot.edit`
   Guard: reject if status != draft.
   Calls: queries::update_wo_draft_fields

E) `cancel_wo` — requires `ot.edit` + step-up
   Calls: queries::cancel_work_order
   Step-up required if WO is in_progress or later.

STEP 3 - PATCH src-tauri/src/lib.rs (or main.rs)
   Register wo module and all wo commands in invoke_handler.

ACCEPTANCE CRITERIA
- cargo check passes
- create_wo returns WOR-0001 on first call
- list_wo filters work for status, equipment, entity, search
- cancel_wo with empty cancel_reason returns validation error
- update_wo_draft on a non-draft WO returns error
```

### Supervisor Verification - Sprint S2

**V1 - Create WO from DI.**
Call `create_wo` with `source_di_id` = valid DI id; WO created with that FK. Verify stub row
migrated from SP04 is not duplicated.

**V2 - Cancel reason required.**
Call `cancel_wo` with empty cancel_reason; must return error.

**V3 - Draft guard.**
Call `update_wo_draft` on a WO with status `planned`; must return error.

**V4 - List filter.**
Create 3 WOs with different equipment_ids; filter by one equipment_id; must return only 1.

---

## Sprint S3 - Frontend Service and Store

### AI Agent Prompt

```text
You are a TypeScript engineer. Implement the WO frontend service and Zustand store.

CREATE src/services/wo-service.ts

Import invoke from @tauri-apps/api/core.

Types (Zod-validated):
- WoStatus (string union of all 12 snake_case codes)
- WoMacroState (string union)
- WorkOrder (all DDL fields + display join fields: statusLabel, statusColor, typeLabel,
  urgencyLevel, urgencyColor, assetCode, assetLabel, plannerUsername, responsibleUsername)
- WoListFilter
- WoListPage
- WoTransitionRow
- WoCreateInput
- WoDraftUpdateInput
- WoCancelInput
- WoDetailPayload: { wo: WorkOrder; transitions: WoTransitionRow[] }

Functions (all async, invoke-based):
- listWos(filter: WoListFilter): Promise<WoListPage>
- getWo(id: number): Promise<WoDetailPayload>
- createWo(input: WoCreateInput): Promise<WorkOrder>
- updateWoDraft(input: WoDraftUpdateInput): Promise<WorkOrder>
- cancelWo(input: WoCancelInput): Promise<WorkOrder>

All use Zod validation on response. Map Tauri error strings to typed Error.

CREATE src/stores/wo-store.ts

State:
- items: WorkOrder[]
- total: number
- activeWo: WoDetailPayload | null
- filter: WoListFilter
- loading: boolean
- saving: boolean
- error: string | null

Actions:
- setFilter(partial: Partial<WoListFilter>): void
- loadWos(): Promise<void>
- openWo(id: number): Promise<void>
- submitNewWo(input: WoCreateInput): Promise<WorkOrder>
- updateDraft(input: WoDraftUpdateInput): Promise<void>
- cancel(input: WoCancelInput): Promise<void>

Follow same loading/saving/error pattern as di-store.

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- WoStatus string union is exhaustive for all 12 codes
- Zod schema validates WorkOrder including urgencyLevel as number (1-5)
- Store filter merge works: setFilter({ status_codes: ['draft'] }) then
  setFilter({ entity_id: 3 }) preserves both fields
```

### Supervisor Verification - Sprint S3

**V1 - Union exhaustiveness.**
Assign `"invalid"` to `WoStatus`; TypeScript error must appear.

**V2 - Zod urgency.**
Mock response with urgencyLevel = "five" (string); Zod parse must throw.

**V3 - Store filter merge.**
Verify both fields survive two successive `setFilter` calls.

---

## Sprint S4 — WO Create / Edit Form, WorkOrdersPage, and DI Management Panel

> **Gap addressed:** S1–S3 defined the backend (`create_wo`, `update_wo_draft`),
> the service, and the store — but no page-level layout or creation form. The web
> app provides a "Créer un OT" button, a create modal with equipment/type/urgency/
> dates/description fields, a DI management panel (unscheduled DI-sourced WOs), and
> a WorkOrdersPage with list/kanban/calendar tabs. This sprint delivers the
> equivalent desktop components.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/wo/WoCreateForm.tsx` | Create / edit-draft form for a work order |
| `src/components/wo/WoFormDialog.tsx` | Dialog wrapper for WoCreateForm (UX-DW-001) |
| `src/components/wo/WoDiManagementPanel.tsx` | Banner showing unscheduled DI-sourced WOs with quick "Schedule" action |
| `src/pages/WorkOrdersPage.tsx` | Full WO workspace page with 4 view tabs, filters, DI panel |
| `src/stores/wo-store.ts` (patch) | `openCreateForm`, `closeCreateForm`, `closeWo` actions |
| `src/i18n/locale-data/fr/ot.json` (patch) | Page, form, action, DI panel labels |
| `src/i18n/locale-data/en/ot.json` (patch) | Page, form, action, DI panel labels |

### WoCreateForm.tsx — Create/Edit Form

**Props:**

```ts
interface WoCreateFormProps {
  initial: WorkOrder | null;   // null = create, non-null = edit draft
  onSubmitted: (wo: WorkOrder) => void;
  onCancel: () => void;
}
```

**Layout:** Single scrollable form, 2 sections.

**Section 1 — Work Order Info**

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| `type_id` | Select (7 WO types) | ✓ | Correctif, Préventif, Prédictif, Amélioratif, Inspection, Overhaul, Condition-Based |
| `equipment_id` | Asset combobox (search) | ✗ | Same pattern as DiCreateForm equipment picker |
| `location_id` | Org-node picker | ✗ | |
| `entity_id` | Org-node picker | ✗ | |
| `urgency_id` | Select (5 urgency levels) | ✗ | Color-coded: VeryLow → Critical |
| `title` | Input | ✓ | Max 200 chars |
| `description` | Textarea | ✗ | |

**Section 2 — Planning (optional at creation)**

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| `planned_start` | Datetime picker | ✗ | ISO datetime |
| `planned_end` | Datetime picker | ✗ | Must be ≥ `planned_start` (client-side validation) |
| `expected_duration_hours` | Number input | ✗ | Hours estimate |
| `notes` | Textarea | ✗ | |

**Behaviour:**
- Create mode: calls `submitNewWo(input)` → default status = **draft**
- Edit mode: calls `updateDraft(input)` with `expected_row_version`
- Uses `type_id` select populated from `work_order_types` (list_wo_types lookup)
- Uses `urgency_id` select populated from `urgency_levels` (list_urgencies lookup)

### WoFormDialog.tsx — Dialog Wrapper

UX-DW-001 pattern:

```
<Dialog open={showCreateForm}>
  <DialogContent className="max-w-2xl max-h-[90vh]">
    <DialogHeader>
      <DialogTitle>{create ? t("page.titleNew") : t("page.titleEdit")}</DialogTitle>
    </DialogHeader>
    <WoCreateForm initial={...} onSubmitted={...} onCancel={...} />
  </DialogContent>
</Dialog>
```

### WoDiManagementPanel.tsx — DI-Sourced WO Queue

Renders a collapsible banner above the main view area for users with `ot.edit` permission.

**Condition:** Only visible when there are WOs with `source_di_id IS NOT NULL` and
`status_id IN (1, 2)` (draft or planned — not yet scheduled/assigned).

**Content:**
- Header: "OT issus de DI non programmés (3)" / "Unscheduled DI work orders (3)" + badge
- Compact table per row: WO code, linked DI code, equipment, urgency badge, created date
- "Programmer" / "Schedule" button per row → opens WoPlanningPanel (from File 02)

### WorkOrdersPage.tsx — Full Page Layout

**Structure (same pattern as RequestsPage):**

```
┌─ Page header ─────────────────────────────────────────┐
│  Wrench icon  "Ordres de travail"  [total badge]      │
│  [+ New OT]  [views: List|Kanban|Calendar|Dashboard]  │
│  [filters: search, status, type, urgency, entity]     │
│  [Refresh]                                            │
├───────────────────────────────────────────────────────┤
│  [WoDiManagementPanel — collapsible]                  │
├───────────────────────────────────────────────────────┤
│  Main view area (list / kanban / calendar / dashboard) │
└───────────────────────────────────────────────────────┘
```

**View modes:**

```tsx
type WoViewMode = "list" | "kanban" | "calendar" | "dashboard";
```

**List view:** DataTable with columns: Code, Title, Equipment, Type, Urgency, Status,
Assignee, Planned End, Actions (view, print). Row click → WoDetailDialog.

**Kanban view:** WoKanbanView (already specified in File 04).

**Calendar view:** WoCalendarView (specified below in Sprint S4 of File 04).

**Dashboard view:** WoDashboardView (specified below in Sprint S4 of File 04).

**Filters (always visible, above view area):**
- Search input (debounce 300ms, searches code + title + equipment name)
- Status multi-select (12 statuses)
- Type multi-select (7 types)
- Urgency multi-select (5 levels)
- Entity select (org nodes)
- Clear all button

**"New OT" button:**

```tsx
<Button onClick={() => openCreateForm()} className="gap-1.5">
  <Plus className="h-3.5 w-3.5" />
  {t("action.create")}
</Button>
```

Visible to users with `ot.create` permission.

### wo-store.ts Patch

Add state:
- `showCreateForm: boolean` (default false)
- `editingWo: WorkOrder | null`

Add actions:
- `openCreateForm(wo?: WorkOrder)` → `set({ showCreateForm: true, editingWo: wo ?? null })`
- `closeCreateForm()` → `set({ showCreateForm: false, editingWo: null })`
- `closeWo()` → `set({ activeWo: null })` (for detail dialog close)

### i18n Additions

```json
{
  "page": { "title": "Ordres de travail" / "Work Orders", "titleNew": "Nouvel OT" / "New Work Order", "titleEdit": "Modifier l'OT" / "Edit Work Order" },
  "action": { "create": "Nouvel OT" / "New WO", "schedule": "Programmer" / "Schedule" },
  "form": {
    "type": { "label": "Type" },
    "urgency": { "label": "Urgence" / "Urgency" },
    "equipment": { "label": "Équipement" / "Equipment", "placeholder": "Rechercher..." / "Search..." },
    "location": { "label": "Emplacement" / "Location" },
    "entity": { "label": "Entité" / "Entity" },
    "title": { "label": "Titre" / "Title" },
    "description": { "label": "Description" },
    "plannedStart": { "label": "Début prévu" / "Planned start" },
    "plannedEnd": { "label": "Fin prévue" / "Planned end" },
    "duration": { "label": "Durée estimée (h)" / "Estimated duration (h)" },
    "notes": { "label": "Remarques" / "Notes" }
  },
  "diPanel": {
    "title": "OT issus de DI non programmés" / "Unscheduled DI work orders",
    "schedule": "Programmer" / "Schedule"
  }
}
```

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- WoCreateForm renders all fields with correct types and labels
- Create mode calls submitNewWo; status defaults to draft
- Edit mode pre-fills from initial WO and calls updateDraft
- "New OT" button visible only for ot.create holders
- WoDiManagementPanel shows only DI-sourced WOs in draft/planned status
- WoDiManagementPanel "Schedule" button opens planning panel
- WorkOrdersPage has 4 view tabs with correct icons
- Filters apply to all views (list, kanban, calendar, dashboard)
- Equipment combobox search debounced at 300ms
```

### Supervisor Verification — Sprint S4

**V1 — Create WO flow.**
Click "New OT"; fill required fields; submit; verify new WO row in list with status=draft.

**V2 — DI management panel.**
Create a WO with `source_di_id` in planned status; verify it appears in the panel.
Schedule it (assign); verify it disappears from the panel.

**V3 — Permission gate.**
User WITHOUT `ot.create`; "New OT" button must not render.

**V4 — Edit draft.**
Open form with an existing draft WO; all fields pre-filled; submit calls `updateDraft`.

---

*End of Phase 2 - Sub-phase 05 - File 01*
