# Phase 2 - Sub-phase 05 - File 02
# WO Planning, Labor, Parts, and Delay Capture

## Context and Purpose

File 01 established the WO schema and 12-state machine. File 02 delivers the operational
execution backbone — the sub-entities and transitions that govern actual work:

1. **Planning gates** — prerequisite checklist, assignment, and the transitions from
   `planned` → `ready_to_schedule` → `assigned` with stage-specific field requirements
2. **Execution transitions** — start, pause, resume, mechanical completion commands with
   structured delay segment capture
3. **Labor tracking** — `work_order_interveners` records actual labor by person, skill,
   start/stop, and rate; used for MTTR, wrench time, and labor cost roll-up
4. **Parts tracking** — `work_order_parts` captures planned vs. actual consumption per WO;
   stage-gated so plan is reviewed before assignment and actuals locked at mechanical complete
5. **Delay segments** — `work_order_delay_segments` records every pause with a structured
   `delay_reason_code`; this is the analytical backbone for waiting-time KPIs
6. **Downtime segments** — `work_order_downtime_segments` records equipment downtime as a
   separate series from labor time, enabling clean OEE and availability calculations
7. **Checklist tasks** — `work_order_tasks` provides ordered, mandatory/optional task
   execution with per-task result codes and completion tracking

These sub-entities match the PRD §6.5 entity list exactly and feed the analytics layer
in File 03 and File 04.

---

## PRD Alignment Checklist

This file addresses PRD §6.5 requirements for:

- [x] `work_order_interveners`: id, work_order_id, intervener_id, skill_id, started_at,
      ended_at, hours_worked, hourly_rate, notes
- [x] `work_order_parts`: id, work_order_id, article_id, quantity_planned, quantity_used,
      unit_cost, stock_location_id
- [x] `work_order_tasks`: id, work_order_id, task_description, sequence_order,
      estimated_minutes, is_mandatory, is_completed, completed_by_id, completed_at,
      result_code, notes
- [x] `work_order_delay_segments`: id, work_order_id, started_at, ended_at,
      delay_reason_id, comment, entered_by_id
- [x] `work_order_downtime_segments`: id, work_order_id, started_at, ended_at,
      downtime_type (full/partial/standby/quality_loss), comment
- [x] Stage-gated data quality rules for planning, assignment, and start
- [x] Time segmentation: active labor time, waiting time, downtime separated
- [x] Automatic duration tracking: timer starts on In Progress; pause/resume creates segments
- [x] Three view modes (Table / Kanban / Calendar) serviced by list/filter queries

---

## Architecture Rules Applied

- **Labor timer is event-driven, not stored as raw start-end on the WO.** `work_order_interveners`
  captures individual start/end per person. The aggregate `active_labor_hours` on `work_orders`
  is computed from the sum of all intervener `hours_worked` and written back at pause/complete.
- **Delay segments are mandatory when pausing.** The `pause_wo` command requires a
  `delay_reason_id` from `delay_reason_codes`. An open delay segment (ended_at = NULL) is
  created; it is closed when `resume_wo` is called or when the WO moves to
  `mechanically_complete`.
- **Downtime segments are independent from delay segments.** A WO can have downtime without
  a delay (e.g., the asset was already down when work started). Downtime segments can be opened
  at any executing state and must be closed before mechanical completion.
- **Parts planning is separated from parts actuals.** A row in `work_order_parts` may exist
  with `quantity_planned > 0` and `quantity_used = NULL` (not yet consumed). At mechanical
  completion, at least one row must have actuals entered, or the planner must record
  `parts_none_used = true` on the WO. This is the Parts Quality Gate.
- **Mandatory tasks block mechanical completion.** If any `work_order_tasks` row has
  `is_mandatory = 1` and `is_completed = 0`, the `complete_wo_mechanically` command returns
  a blocking error with the task list.
- **Assignment propagates to primary_responsible_id.** When `assign_wo` is called with a
  `primary_responsible_id`, the WO field is written and the `wo_state_transition_log` records
  the assignee as part of the transition payload.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000022_wo_execution_sub_entities.rs` | `work_order_interveners`, `work_order_parts`, `work_order_tasks`, `work_order_delay_segments`, `work_order_downtime_segments` |
| `src-tauri/src/wo/execution.rs` | Transition functions: plan, assign, start, pause, resume, complete mechanically; prerequisite gate |
| `src-tauri/src/wo/labor.rs` | Labor entry CRUD and hours accumulation |
| `src-tauri/src/wo/parts.rs` | Parts plan/actual CRUD and quality gate check |
| `src-tauri/src/wo/tasks.rs` | Task CRUD and mandatory-task gate |
| `src-tauri/src/wo/delay.rs` | Delay and downtime segment management |
| `src-tauri/src/commands/wo.rs` (patch) | Execution IPC commands: plan, assign, start, pause, resume, add_labor, record_parts, complete_mechanically |
| `src/services/wo-execution-service.ts` | Frontend execution wrappers |
| `src/components/wo/WoPlanningPanel.tsx` | Assignment + prerequisite checklist UI |
| `src/components/wo/WoExecutionControls.tsx` | Start / Pause / Resume / Complete controls |

---

## Prerequisites

- File 01 complete: migration 021, `wo/domain.rs`, base commands working
- `delay_reason_codes` seeded in migration 021
- SP02 asset registry available; SP06 personnel module will supply `skill_id` references
  (nullable foreign key in this file; not enforced until SP06 is built)

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Sub-entity Schema and Execution Functions | migration 022 + `execution.rs` + `labor.rs` + `parts.rs` + `tasks.rs` + `delay.rs` |
| S2 | IPC Commands for All Execution Actions | `commands/wo.rs` patch |
| S3 | Planning Panel and Execution Controls | React components |

---

## Sprint S1 - Sub-entity Schema and Execution Functions

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the WO execution sub-entities and all workflow functions.

STEP 1 - CREATE src-tauri/migrations/m20260401_000022_wo_execution_sub_entities.rs

CREATE TABLE work_order_interveners (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id   INTEGER NOT NULL REFERENCES work_orders(id),
  intervener_id   INTEGER NOT NULL REFERENCES user_accounts(id),
  skill_id        INTEGER NULL,   -- FK to personnel_skills; nullable until SP06
  started_at      TEXT    NULL,
  ended_at        TEXT    NULL,
  hours_worked    REAL    NULL,   -- computed or manually entered
  hourly_rate     REAL    NULL DEFAULT 0,
  notes           TEXT    NULL
);

CREATE TABLE work_order_parts (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id    INTEGER NOT NULL REFERENCES work_orders(id),
  article_id       INTEGER NULL,   -- FK to articles; nullable until SP08
  article_ref      TEXT    NULL,   -- free-text fallback while SP08 is pending
  quantity_planned REAL    NOT NULL DEFAULT 0,
  quantity_used    REAL    NULL,
  unit_cost        REAL    NULL DEFAULT 0,
  stock_location_id INTEGER NULL,  -- FK to stock locations; nullable until SP08
  notes            TEXT    NULL
);

CREATE TABLE work_order_tasks (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id   INTEGER NOT NULL REFERENCES work_orders(id),
  task_description TEXT   NOT NULL,
  sequence_order  INTEGER NOT NULL DEFAULT 0,
  estimated_minutes INTEGER NULL,
  is_mandatory    INTEGER NOT NULL DEFAULT 0,
  is_completed    INTEGER NOT NULL DEFAULT 0,
  completed_by_id INTEGER NULL REFERENCES user_accounts(id),
  completed_at    TEXT    NULL,
  result_code     TEXT    NULL,  -- ok / nok / na / deferred
  notes           TEXT    NULL
);

CREATE TABLE work_order_delay_segments (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id   INTEGER NOT NULL REFERENCES work_orders(id),
  started_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  ended_at        TEXT    NULL,   -- NULL = still open
  delay_reason_id INTEGER NOT NULL REFERENCES delay_reason_codes(id),
  comment         TEXT    NULL,
  entered_by_id   INTEGER NULL REFERENCES user_accounts(id)
);

CREATE TABLE work_order_downtime_segments (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id INTEGER NOT NULL REFERENCES work_orders(id),
  started_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  ended_at      TEXT    NULL,
  downtime_type TEXT    NOT NULL DEFAULT 'full',
    -- full / partial / standby / quality_loss
  comment       TEXT    NULL
);

CREATE INDEX idx_woi_wo_id ON work_order_interveners(work_order_id);
CREATE INDEX idx_wop_wo_id ON work_order_parts(work_order_id);
CREATE INDEX idx_wot_wo_id ON work_order_tasks(work_order_id);
CREATE INDEX idx_wods_wo_id ON work_order_delay_segments(work_order_id);
CREATE INDEX idx_wodts_wo_id ON work_order_downtime_segments(work_order_id);


STEP 2 - CREATE src-tauri/src/wo/execution.rs

All functions use sqlx transaction. All call guard_wo_transition before writes.

--- A) plan_wo ---
Input: WoPlanInput { wo_id, actor_id, expected_row_version,
  planner_id, planned_start, planned_end, expected_duration_hours?, urgency_id? }
Guard: current_status must be in [draft, awaiting_approval] → transition to 'planned'.
Validates: planned_start and planned_end must be parseable ISO datetimes;
  planned_end must be >= planned_start.
UPDATE work_orders SET status_id=(planned), planner_id=?, planned_start=?, planned_end=?,
  urgency_id=COALESCE(?,urgency_id), row_version+1, updated_at=now WHERE id=? AND row_version=?
Write wo_state_transition_log: action='plan'.
Return updated WorkOrder.

--- B) assign_wo ---
Input: WoAssignInput { wo_id, actor_id, expected_row_version,
  assigned_group_id?, primary_responsible_id?, scheduled_at? }
Guard: current_status must be ready_to_schedule → transition to 'assigned'.
Validates: at least one of assigned_group_id or primary_responsible_id must be provided.
UPDATE: status=(assigned), assigned_group_id=?, primary_responsible_id=?,
  scheduled_at=COALESCE(?,scheduled_at), row_version+1, updated_at=now
Write wo_state_transition_log: action='assign', notes includes assignee id.

--- C) start_wo ---
Input: WoStartInput { wo_id, actor_id, expected_row_version }
Guard: current_status in [assigned, waiting_for_prerequisite] → transition to 'in_progress'.
UPDATE: status=(in_progress), actual_start=now (if NULL), row_version+1, updated_at=now.
Write wo_state_transition_log: action='start'.

--- D) pause_wo ---
Input: WoPauseInput { wo_id, actor_id, expected_row_version,
  delay_reason_id: i64, comment?: String }
Guard: current_status = in_progress → transition to 'paused'.
Validates: delay_reason_id resolves in delay_reason_codes.
INSERT work_order_delay_segments (started_at=now, ended_at=NULL, delay_reason_id, comment).
UPDATE work_orders: status=(paused), row_version+1, updated_at=now.
Write wo_state_transition_log: action='pause', reason_code=delay_reason code.

--- E) resume_wo ---
Input: WoResumeInput { wo_id, actor_id, expected_row_version }
Guard: current_status = paused → transition to 'in_progress'.
Close open delay segment: UPDATE work_order_delay_segments SET ended_at=now
  WHERE work_order_id=? AND ended_at IS NULL.
Recompute total_waiting_hours:
  SELECT SUM(ROUND((JULIANDAY(COALESCE(ended_at,strftime('%Y-%m-%dT%H:%M:%SZ','now')))
    - JULIANDAY(started_at))*24,2)) FROM work_order_delay_segments WHERE work_order_id=?
UPDATE work_orders: status=(in_progress), total_waiting_hours=computed, row_version+1.
Write wo_state_transition_log: action='resume'.

--- F) set_waiting_for_prerequisite ---
Input: WoHoldInput { wo_id, actor_id, expected_row_version, delay_reason_id, comment? }
Guard: current_status = assigned → waiting_for_prerequisite.
INSERT work_order_delay_segments (started_at=now, ended_at=NULL, ...).
UPDATE work_orders: status=(waiting_for_prerequisite), row_version+1.
Same pattern as pause.

--- G) complete_wo_mechanically ---
Input: WoMechCompleteInput { wo_id, actor_id, expected_row_version }
Guard: current_status = in_progress → mechanically_complete.

Pre-completion checks (all must pass):
1. All intervener rows for this WO have ended_at set OR hours_worked > 0
   (no open labor entries). Error: "Open labor entries must be closed first."
2. All mandatory work_order_tasks rows have is_completed = 1.
   Error: "Mandatory tasks incomplete: {task descriptions}."
3. work_order_parts row exists (any quantity_used > 0, or explicit parts_none flag on WO
   — add parts_actuals_confirmed INTEGER DEFAULT 0 to work_orders).
   Error: "Parts actuals not confirmed. Enter consumed parts or mark none used."
4. All work_order_downtime_segments have ended_at set.
   Error: "Open downtime segments must be closed before completion."

Post-checks: recompute active_labor_hours from SUM(intervener hours_worked).
UPDATE work_orders: status=(mechanically_complete), mechanically_completed_at=now,
  active_labor_hours=computed, row_version+1.
Write wo_state_transition_log: action='complete_mechanically'.


STEP 3 - CREATE src-tauri/src/wo/labor.rs

Types:
- WoIntervener (matches DDL)
- AddLaborInput: wo_id, intervener_id, skill_id?, started_at?, ended_at?,
  hours_worked? (manual), hourly_rate?, notes?

Functions:
A) add_labor_entry(pool, input) -> Result<WoIntervener>
   Validates: WO exists and is not in closed/cancelled state.
   If started_at and ended_at both set, computes hours_worked = elapsed hours.
   INSERT into work_order_interveners. Return row.

B) close_labor_entry(pool, intervener_id, ended_at: String, actor_id) -> Result<WoIntervener>
   UPDATE work_order_interveners SET ended_at=?, hours_worked=elapsed WHERE id=?.
   Validates: row belongs to an open entry (ended_at IS NULL).

C) list_labor_entries(pool, wo_id) -> Result<Vec<WoIntervener>>

D) remove_labor_entry(pool, intervener_id, actor_id) -> Result<()>
   Only allowed if WO status is in [draft, planned, assigned]. Returns error otherwise.


STEP 4 - CREATE src-tauri/src/wo/parts.rs

Types:
- WoPart (matches DDL)
- AddPartInput: wo_id, article_id?, article_ref?, quantity_planned, unit_cost?, notes?

Functions:
A) add_planned_part(pool, input) -> Result<WoPart>
   WO must not be in closed/cancelled state.
B) record_actual_usage(pool, wo_part_id: i64, quantity_used: f64, unit_cost?: f64) -> Result<WoPart>
   WO must be in in_progress or mechanically_complete state.
C) confirm_no_parts_used(pool, wo_id: i64, actor_id: i64) -> Result<()>
   UPDATE work_orders SET parts_actuals_confirmed=1 WHERE id=?.
D) list_wo_parts(pool, wo_id) -> Result<Vec<WoPart>>


STEP 5 - CREATE src-tauri/src/wo/tasks.rs

Types:
- WoTask (matches DDL)
- AddTaskInput: wo_id, task_description, sequence_order, is_mandatory, estimated_minutes?

Functions:
A) add_task(pool, input) -> Result<WoTask>
   WO must be in [draft, planned, ready_to_schedule, assigned].
B) complete_task(pool, task_id, actor_id, result_code: String, notes?) -> Result<WoTask>
   UPDATE is_completed=1, completed_by_id=?, completed_at=now, result_code=?, notes=?
C) reopen_task(pool, task_id, actor_id) -> Result<WoTask>
   Only if WO is not yet mechanically_complete.
D) list_tasks(pool, wo_id) -> Result<Vec<WoTask>>


STEP 6 - CREATE src-tauri/src/wo/delay.rs

Functions:
A) open_downtime_segment(pool, wo_id, downtime_type: String, comment?) -> Result<WoDowntimeSegment>
   WO must be executing (in_progress, paused, or assigned).
   Validates downtime_type is one of: full/partial/standby/quality_loss.
B) close_downtime_segment(pool, segment_id, ended_at: String) -> Result<WoDowntimeSegment>
C) list_delay_segments(pool, wo_id) -> Result<Vec<WoDelaySegment>>
D) list_downtime_segments(pool, wo_id) -> Result<Vec<WoDowntimeSegment>>


STEP 7 - PATCH src-tauri/src/wo/mod.rs
  pub mod execution;
  pub mod labor;
  pub mod parts;
  pub mod tasks;
  pub mod delay;

ACCEPTANCE CRITERIA
- migration 022 applies cleanly
- cargo check passes
- complete_wo_mechanically returns blocking error when mandatory task incomplete
- pause_wo inserts delay segment with ended_at = NULL
- resume_wo closes open delay segment and updates total_waiting_hours
- add_labor_entry auto-computes hours_worked when both timestamps provided
```

### Supervisor Verification - Sprint S1

**V1 - Pause creates open delay segment.**
Pause WO with reason_id; verify delay segment has ended_at = NULL and delay_reason_id set.

**V2 - Resume closes delay and updates waiting hours.**
Resume WO; verify delay segment ended_at is now set; work_orders.total_waiting_hours > 0.

**V3 - Mandatory task gate.**
Add mandatory task, leave incomplete; call complete_wo_mechanically; must return error listing
incomplete task.

**V4 - Parts gate.**
Add no parts and do not call confirm_no_parts_used; call complete_wo_mechanically; must return
parts error.

**V5 - Open downtime block.**
Open downtime segment; call complete_wo_mechanically; must return downtime error.

---

## Sprint S2 - IPC Commands for All Execution Actions

### AI Agent Prompt

```text
You are a senior Rust engineer. Add all execution IPC commands to commands/wo.rs.

PATCH src-tauri/src/commands/wo.rs

Add the following commands. Permission requirements listed per command.

A) `plan_wo` — ot.edit
   Input: WoPlanInput; Delegates to execution::plan_wo

B) `assign_wo` — ot.edit
   Input: WoAssignInput; Delegates to execution::assign_wo

C) `start_wo` — ot.edit
   Input: WoStartInput; Delegates to execution::start_wo

D) `pause_wo` — ot.edit
   Input: WoPauseInput; Delegates to execution::pause_wo

E) `resume_wo` — ot.edit
   Input: WoResumeInput; Delegates to execution::resume_wo

F) `hold_wo` — ot.edit
   Input: WoHoldInput; Delegates to execution::set_waiting_for_prerequisite

G) `complete_wo_mechanically` — ot.edit
   Input: WoMechCompleteInput; Delegates to execution::complete_wo_mechanically

H) `add_labor` — ot.edit
   Input: AddLaborInput; Delegates to labor::add_labor_entry

I) `close_labor` — ot.edit
   Input: intervener_id, ended_at, actor_id; Delegates to labor::close_labor_entry

J) `list_labor` — ot.view
   Input: wo_id; Delegates to labor::list_labor_entries

K) `add_part` — ot.edit
   Input: AddPartInput; Delegates to parts::add_planned_part

L) `record_part_usage` — ot.edit
   Input: wo_part_id, quantity_used, unit_cost?; Delegates to parts::record_actual_usage

M) `confirm_no_parts` — ot.edit
   Input: wo_id; Delegates to parts::confirm_no_parts_used

N) `add_task` — ot.edit
   Input: AddTaskInput; Delegates to tasks::add_task

O) `complete_task` — ot.edit
   Input: task_id, result_code, notes?; Delegates to tasks::complete_task

P) `list_tasks` — ot.view
   Input: wo_id; Delegates to tasks::list_tasks

Q) `open_downtime` — ot.edit
   Input: wo_id, downtime_type, comment?; Delegates to delay::open_downtime_segment

R) `close_downtime` — ot.edit
   Input: segment_id, ended_at; Delegates to delay::close_downtime_segment

S) `list_delay_segments` — ot.view
   Input: wo_id; Delegates to delay::list_delay_segments

T) `list_downtime_segments` — ot.view
   Input: wo_id; Delegates to delay::list_downtime_segments

ACCEPTANCE CRITERIA
- All 20 commands registered in invoke_handler
- cargo check passes
- complete_wo_mechanically returns structured blocking error list with specific failure reasons
```

### Supervisor Verification - Sprint S2

**V1 - Permission on complete.**
User with only `ot.view` calls `complete_wo_mechanically`; must return PermissionDenied.

**V2 - Full execute path.**
plan_wo → assign_wo → start_wo → add_labor → open_downtime → close_downtime →
confirm_no_parts → complete_wo_mechanically. Each step succeeds. WO in mechanically_complete.

**V3 - Command count.**
invoke_handler contains exactly all prior commands plus the 20 new ones; no duplicates.

---

## Sprint S3 - Planning Panel and Execution Controls

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the WO planning panel and execution controls.

CREATE src/services/wo-execution-service.ts

Types (mirror Rust execution input/output structs):
- WoPlanInput, WoAssignInput, WoStartInput, WoPauseInput, WoResumeInput,
  WoHoldInput, WoMechCompleteInput
- AddLaborInput, WoIntervener
- AddPartInput, WoPart
- AddTaskInput, WoTask, TaskResultCode ('ok'|'nok'|'na'|'deferred')
- WoDelaySegment, WoDowntimeSegment, DowntimeType

Functions (all async):
- planWo, assignWo, startWo, pauseWo, resumeWo, holdWo, completeMechanically
- addLabor, closeLabor, listLabor
- addPart, recordPartUsage, confirmNoParts, listParts
- addTask, completeTask, listTasks
- openDowntime, closeDowntime, listDelaySegments, listDowntimeSegments

CREATE src/components/wo/WoPlanningPanel.tsx

Props: wo: WorkOrder; canEdit: boolean

Sections:
1. Timing — planned_start/planned_end date pickers; expected_duration_hours input
2. Urgency — urgency level selector (5 levels with color swatches)
3. Assignment — assigned_group_id org node picker; primary_responsible_id user picker;
   scheduled_at datetime picker
4. Prerequisites checklist — editable list of tasks; add/remove; mandatory toggle
5. Parts plan — table of planned parts with quantity and estimated cost; add row button;
   running total planned cost shown

All fields disabled if !canEdit or WO is past planned status.
"Move to Ready to Schedule" button: active only when planWo requirements are met
  (planner_id set, planned_start set, planned_end set).
"Assign" button: active only when assigned_group_id or primary_responsible_id set.

CREATE src/components/wo/WoExecutionControls.tsx

Props: wo: WorkOrder; canEdit: boolean

Renders different control set based on WO status:
- in_progress: "Pause" button (opens delay reason selector popup), "Complete (Mech)" button
- paused: "Resume" button
- assigned / waiting_for_prerequisite: "Start" button, "Hold" button
- mechanically_complete: no execution controls (handled by close-out panel in File 03)

Pause flow: opens inline form with delay_reason_id selector (fetches list_delay_segments
labels) and optional comment; submits pauseWo.
Hold flow: same form as pause but calls holdWo.

Labor capture: a collapsible "Labor Entries" section within execution view.
  Start/stop labor buttons add/close intervener rows.
  Manual hours input also available.

Parts actuals: a collapsible "Parts Used" section.
  For each planned part: quantity_used input.
  "No Parts Used" button calls confirmNoParts.

Task execution: inline checklist with checkboxes; result_code dropdown; completes each task.
  Mandatory incomplete tasks are highlighted in red.

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- WoPlanningPanel disables all inputs when WO is past planned status
- WoExecutionControls shows correct control set for each status
- "Complete (Mech)" triggers pre-flight check and shows blockingErrors list if fails
- pauseWo called with selected delay_reason_id, not hardcoded
```

### Supervisor Verification - Sprint S3

**V1 - Control set by status.**
Set WO status to in_progress; WoExecutionControls shows Pause + Complete. Set to paused;
shows only Resume.

**V2 - Pause delay required.**
Click Pause without selecting a delay reason; UI must block submission (not call pauseWo).

**V3 - Planning panel disabled past planned.**
With WO in assigned status, all WoPlanningPanel inputs must be disabled.

**V4 - typecheck.**
`pnpm typecheck` — 0 errors across all new files.

---

## Sprint S4 — Shift Planning, Completion Dialog, and Execution UI Refinements

> **Gap addressed:** The web app has (1) a shift selector (morning/afternoon/night/
> day) for scheduling, (2) a completion modal (end date, hours worked, report), and
> (3) structured labor entry with start/stop buttons in the execution panel. S1–S3
> delivered the planning panel and execution controls but did not specify the shift
> field, the completion modal, or the detail dialog wrapper. This sprint adds them.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000021_wo_domain_core.rs` (patch) | Add `shift` column to `work_orders` table |
| `src-tauri/src/wo/domain.rs` (patch) | Add `WoShift` enum: `morning`, `afternoon`, `night`, `full_day` |
| `src/components/wo/WoPlanningPanel.tsx` (patch) | Add shift selector in timing section |
| `src/components/wo/WoCompletionDialog.tsx` | Completion modal: end date, hours worked, observations |
| `src/components/wo/WoDetailDialog.tsx` | Full detail dialog wrapper (UX-DW-001) hosting all WO sub-panels |
| `src/components/wo/WoExecutionControls.tsx` (patch) | Labor start/stop buttons with timer; task inline completion; parts usage input |
| `src/i18n/locale-data/{fr,en}/ot.json` (patch) | Shift labels, completion labels |

### Schema Addition: `shift` Column

Add to `work_orders` table (migration 021 patch):

```sql
ALTER TABLE work_orders ADD COLUMN shift TEXT NULL;
-- Values: 'morning' | 'afternoon' | 'night' | 'full_day'
```

Add to `WoCreateInput`, `WoDraftUpdateInput`, and `WoPlanInput` structs:

```rust
pub shift: Option<String>,  // "morning" | "afternoon" | "night" | "full_day"
```

### WoPlanningPanel.tsx Patch — Shift Selector

Add to the Timing section (below planned_end, above expected_duration):

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| `shift` | Select | ✗ | 4 options: Matin / Après-midi / Nuit / Journée (Morning / Afternoon / Night / Full Day) |

### WoCompletionDialog.tsx — Completion Confirmation

Triggered from WoExecutionControls "Complete" button (when WO is in_progress or en_attente).

**Layout (dialog `max-w-lg`):**

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| End date/time | Datetime picker | ✓ | Pre-filled with current timestamp |
| Hours worked | Number input | ✗ | Optional manual override of calculated hours |
| Report / Observations | Textarea | ✗ | Technician completion report |

**Actions (footer):**
- "Annuler" / "Cancel"
- "Marquer terminé" / "Mark Complete" (orange) — Calls `complete_wo_mechanically`

**Pre-flight error display:**
If `complete_wo_mechanically` returns blocking errors (open labor, incomplete mandatory tasks,
missing parts actuals, open downtime segments), display each as a separate error item with
descriptive text and icon.

### WoDetailDialog.tsx — Full Detail Dialog

Follows UX-DW-001 pattern (`docs/UX_DETAIL_DIALOG_PATTERN.md`).

**Layout (`max-w-4xl`, `max-h-[90vh]`):**

```
┌─ Header: WO code + title + status badge + urgency badge + X ─┐
├─────────────────────────────────────────────────────────────────┤
│  Scrollable body with Tabs:                                    │
│  ┌─────┬──────────┬──────────┬──────────┬──────────────┐      │
│  │Plan │ Execution│ Close-out│ Audit    │ Attachments  │      │
│  └─────┴──────────┴──────────┴──────────┴──────────────┘      │
│                                                                 │
│  [Tab content: WoPlanningPanel / WoExecutionControls /         │
│   WoCloseOutPanel / WoAuditTimeline / WoAttachmentPanel]       │
├─────────────────────────────────────────────────────────────────┤
│  Footer: context-appropriate buttons (Schedule, Start, Close) │
└─────────────────────────────────────────────────────────────────┘
```

**Tab visibility rules:**
- **Plan** — always visible; editable only in draft/planned/ready_to_schedule
- **Execution** — visible once assigned or later
- **Close-out** — visible once mechanically_complete or later
- **Audit** — always visible (read-only)
- **Attachments** — always visible

**Footer actions (conditional on status + permissions):**
| Status | Actions |
|--------|---------|
| draft / planned | "Programmer" (plan_wo) |
| ready_to_schedule | "Assigner" (assign_wo) |
| assigned | "Démarrer" (start_wo) + "Assigner" (re-assign) |
| in_progress | "Pause" + "Terminer" (mech complete) |
| paused | "Reprendre" (resume_wo) |
| mechanically_complete | "Vérifier" + "Clôturer" |
| technically_verified | "Clôturer" (close_wo) |

### WoExecutionControls.tsx Patch — Labor Start/Stop

Enhance the labor sub-section:
- Each labor entry row: Intervener name, skill, Start button ("Début"), Stop button ("Fin"),
  hours display (auto-computed), hourly rate, notes
- Start button: calls `add_labor(started_at = now)`; row enters "in progress" state
- Stop button: calls `close_labor(ended_at = now)`; row shows calculated hours
- Manual entry fallback: if start/stop buttons not used, user can enter hours_worked directly

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- Shift selector renders 4 options in WoPlanningPanel
- Shift value persisted via plan_wo command
- WoCompletionDialog pre-fills end date with now()
- Pre-flight errors from complete_wo_mechanically shown as individual items in dialog
- WoDetailDialog renders 5 tabs with correct visibility per status
- Labor start/stop buttons update intervener started_at / ended_at correctly
- Footer actions change based on WO status
```

### Supervisor Verification — Sprint S4

**V1 — Shift persistence.**
Select "Nuit" in planning panel; plan WO; reopen detail; verify shift shows "Nuit".

**V2 — Completion pre-flight errors.**
Attempt mech-complete with open labor entry; verify "Unclosed labor entries" error in dialog.

**V3 — Detail dialog tabs.**
Open a draft WO; only Plan + Audit + Attachments tabs visible. Open an in_progress WO;
Execution tab also visible.

**V4 — Labor start/stop.**
Click Start on a labor row; verify started_at is set. Click Stop; verify hours_worked computed.

---

*End of Phase 2 - Sub-phase 05 - File 02*
