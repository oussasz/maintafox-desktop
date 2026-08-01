# Phase 2 - Sub-phase 05 - File 03
# WO Close-Out, Verification, and Cost Posting Hooks

## Context and Purpose

Files 01 and 02 established the WO schema, state machine, and execution machinery. File 03
delivers the three closing stages of the work order lifecycle:

1. **Structured close-out** — the four-section evidence panel required by PRD §6.5 and the
   research brief: confirmed symptom/condition, diagnosed failure mode/cause, action performed
   (temporary or permanent), and verification of restoration + recurrence risk. This evidence
   feeds the reliability engineering module (SP: RAMS) and repeat-failure detection.
2. **Technical verification** — the stage gate between `mechanically_complete` and
   `technically_verified`. A verifier (separate from the technician) confirms return-to-service,
   reviews all captured evidence, and records a verification result (pass/fail/monitor). This
   step is mandatory for corrective and emergency work types.
3. **Closure quality gate and cost roll-up** — the final closure command that runs
   mandatory field checks, computes final cost accumulators (labor + parts + service), and
   writes the WO to the analytics feed. The WO becomes a read-only historical record after
   closure. Cost posting hooks provide a structured interface for SP (Budget/Cost §6.24) to
   consume without requiring SP05 to know about cost center structures.
4. **Attachments** — `work_order_attachments` for photos, reports, and PDF work sheets
   captured during execution or at close-out.
5. **Reopen logic** — supervisors can reopen recently closed WOs within a configurable
   recurrence window (default 7 days) while preserving all close-out evidence and
   verification history.

---

## PRD Alignment Checklist

This file addresses PRD §6.5 requirements for:

- [x] `work_order_failure_details`: id, work_order_id, symptom_id, failure_mode_id,
      failure_cause_id, failure_effect_id, is_temporary_repair, is_permanent_repair,
      cause_not_determined, notes
- [x] `work_order_verifications`: id, work_order_id, verified_by_id, verified_at,
      result (pass/fail/monitor), return_to_service_confirmed, recurrence_risk_level, notes
- [x] `work_order_attachments`: id, work_order_id, file_path, file_name, uploaded_by_id,
      uploaded_at
- [x] Structured close-out panel: four sections (symptom, failure, action, verification)
- [x] Mandatory closure quality gate: labor actuals, parts actuals, failure details for
      corrective work, downtime closed, verification result captured
- [x] Cost accumulation: labor + parts + service = total_cost; plan-vs-actual variance
- [x] Reopen logic: recurrence window configurable; original evidence preserved
- [x] Closed WO analytics feed: structured output for reliability, cost, and schedule analytics

---

## Architecture Rules Applied

- **`work_order_failure_details` uses reference_values FKs.** `symptom_id`, `failure_mode_id`,
  `failure_cause_id`, `failure_effect_id` all reference `reference_values(id)` from SP03.
  This is the bridge between the WO execution record and the structured failure taxonomy that
  feeds RAMS in later subphases. These fields are nullable but at least one failure detail row
  is required for corrective and emergency WO types before closure.
- **Verification is not self-verification.** The `verified_by_id` on `work_order_verifications`
  must differ from `primary_responsible_id` on the WO (enforced in the verify command).
  For WOs with no primary responsible (group-assigned only), this check is skipped.
- **Cost roll-up is computed, not manually entered for accumulators.** `compute_and_post_costs`
  reads `work_order_interveners.hours_worked * hourly_rate` for labor, sums `work_order_parts.
  quantity_used * unit_cost` for parts, and writes results to `work_orders.labor_cost`,
  `parts_cost`, `total_cost`. `service_cost` is manually entered on the WO before this
  computation. The total is available as a cost-posting hook for SP §6.24.
- **Closure is terminal and irreversible except via reopen.** After `close_wo` succeeds, all
  field-edit commands return an error. The only allowed writes are attachment uploads and
  reopen (within the recurrence window).
- **Reopen increments a `reopen_count` field.** Each reopen adds 1 to `reopen_count` on
  the WO and writes a `wo_state_transition_log` row. This counter is visible in the close-out
  audit and flags repeat-closure issues.
- **Analytics feed is a computed view, not a separate table.** `get_wo_analytics_snapshot`
  returns a denormalized JSON payload assembled from the closed WO and its sub-entities.
  The payload format is the contract that future analytics modules consume. It does not write
  a new table — it reads from the existing tables and projects the result.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000023_wo_closeout_and_attachments.rs` | `work_order_failure_details`, `work_order_verifications`, `work_order_attachments`; `reopen_count` + `parts_actuals_confirmed` + `service_cost_input` columns on `work_orders` |
| `src-tauri/src/wo/closeout.rs` | Failure detail CRUD, save_verification, close_wo quality gate, reopen_wo |
| `src-tauri/src/wo/costs.rs` | compute_and_post_costs, get_cost_summary, cost-posting hook payload |
| `src-tauri/src/wo/attachments.rs` | Attachment upload/list/delete-record functions |
| `src-tauri/src/wo/analytics.rs` | get_wo_analytics_snapshot — denormalized closed-WO payload |
| `src-tauri/src/commands/wo.rs` (patch) | Close-out IPC commands: save_failure_detail, save_verification, close_wo, reopen_wo, upload_attachment, get_analytics_snapshot |
| `src/services/wo-closeout-service.ts` | Close-out command wrappers |
| `src/components/wo/WoCloseOutPanel.tsx` | Four-section close-out UI |
| `src/components/wo/WoVerificationPanel.tsx` | Technical verification form |

---

## Prerequisites

- Files 01 and 02 complete
- SP03 complete: `reference_values` table for failure taxonomy FKs
- Phase 1 auth for step-up on close

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Close-Out Schema and Logic | migration 023 + `closeout.rs` + `costs.rs` + `attachments.rs` |
| S2 | Analytics Snapshot and IPC Commands | `analytics.rs` + `commands/wo.rs` patch |
| S3 | Close-Out and Verification UI | React panels |

---

## Sprint S1 - Close-Out Schema and Logic

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the WO close-out schema and all closing functions.

STEP 1 - CREATE src-tauri/migrations/m20260401_000023_wo_closeout_and_attachments.rs

CREATE TABLE work_order_failure_details (
  id                  INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id       INTEGER NOT NULL REFERENCES work_orders(id),
  symptom_id          INTEGER NULL REFERENCES reference_values(id),
  failure_mode_id     INTEGER NULL REFERENCES reference_values(id),
  failure_cause_id    INTEGER NULL REFERENCES reference_values(id),
  failure_effect_id   INTEGER NULL REFERENCES reference_values(id),
  is_temporary_repair INTEGER NOT NULL DEFAULT 0,
  is_permanent_repair INTEGER NOT NULL DEFAULT 0,
  cause_not_determined INTEGER NOT NULL DEFAULT 0,
  notes               TEXT    NULL
);

CREATE TABLE work_order_verifications (
  id                         INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id              INTEGER NOT NULL REFERENCES work_orders(id),
  verified_by_id             INTEGER NOT NULL REFERENCES user_accounts(id),
  verified_at                TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  result                     TEXT    NOT NULL,   -- pass / fail / monitor
  return_to_service_confirmed INTEGER NOT NULL DEFAULT 0,
  recurrence_risk_level      TEXT    NULL,       -- none / low / medium / high
  notes                      TEXT    NULL
);

CREATE TABLE work_order_attachments (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  work_order_id  INTEGER NOT NULL REFERENCES work_orders(id),
  file_name      TEXT    NOT NULL,
  relative_path  TEXT    NOT NULL UNIQUE,
  mime_type      TEXT    NOT NULL DEFAULT 'application/octet-stream',
  size_bytes     INTEGER NOT NULL DEFAULT 0,
  uploaded_by_id INTEGER NULL REFERENCES user_accounts(id),
  uploaded_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  notes          TEXT    NULL
);

-- Add columns to work_orders --
ALTER TABLE work_orders ADD COLUMN parts_actuals_confirmed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE work_orders ADD COLUMN service_cost_input REAL NULL DEFAULT 0;
ALTER TABLE work_orders ADD COLUMN reopen_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE work_orders ADD COLUMN last_closed_at TEXT NULL;

CREATE INDEX idx_wofd_wo_id ON work_order_failure_details(work_order_id);
CREATE INDEX idx_wov_wo_id  ON work_order_verifications(work_order_id);
CREATE INDEX idx_woa_wo_id  ON work_order_attachments(work_order_id);


STEP 2 - CREATE src-tauri/src/wo/closeout.rs

Types:
- WoFailureDetail (matches DDL)
- WoVerification (matches DDL)
- SaveFailureDetailInput: wo_id, symptom_id?, failure_mode_id?, failure_cause_id?,
  failure_effect_id?, is_temporary_repair, is_permanent_repair, cause_not_determined, notes?
- SaveVerificationInput: wo_id, verified_by_id, result: String, return_to_service_confirmed,
  recurrence_risk_level?, notes?
- WoCloseInput: wo_id, actor_id, expected_row_version, step_up_token: String

Functions:

--- A) save_failure_detail ---
Upsert failure detail: if one exists for wo_id, update it; if not, insert.
WO must not be in closed/cancelled state.
Validates: result is one of pass/fail/monitor.
Validates: for is_temporary_repair=1 AND is_permanent_repair=1 return error
  ("Action cannot be both temporary and permanent").
Return WoFailureDetail.

--- B) save_verification ---
WO must be in mechanically_complete.
Validates: result in [pass, fail, monitor].
Validates: verified_by_id != WO.primary_responsible_id (no self-verification).
  If WO.primary_responsible_id IS NULL, skip this check.
INSERT into work_order_verifications.
If result = 'pass' AND return_to_service_confirmed = 1:
  UPDATE work_orders SET recurrence_risk_level = COALESCE(recurrence_risk_level, ?)
Transition WO: mechanically_complete → technically_verified.
  UPDATE work_orders SET status=(technically_verified), technically_verified_at=now,
  row_version+1. Write wo_state_transition_log: action='verify'.
Return (WoVerification, updated WorkOrder).

--- C) close_wo (quality gate) ---
Input: WoCloseInput (includes step_up_token).

1. Load WO; guard_wo_transition(technically_verified, closed).
2. Validate step_up_token.
3. Quality gate — all checks must pass (collect all failures, return list if any fail):
   a. active_labor_hours > 0 OR at least one work_order_interveners row exists.
      Error: "Labor actuals required."
   b. parts_actuals_confirmed = 1 OR at least one work_order_parts row has quantity_used > 0.
      Error: "Parts actuals required."
   c. If WO type is corrective or emergency:
      At least one work_order_failure_details row exists with (failure_mode_id IS NOT NULL
      OR cause_not_determined = 1).
      Error: "Failure coding required for corrective/emergency work."
   d. root_cause_summary IS NOT NULL AND LENGTH(root_cause_summary) > 0
      (for corrective/emergency types only).
      Error: "Root cause summary required."
   e. At least one work_order_verifications row with result IN ('pass','monitor').
      Error: "Technical verification required."

4. If all checks pass: compute final costs.
   SELECT SUM(hours_worked * COALESCE(hourly_rate,0)) FROM work_order_interveners WHERE wo_id=?
   → labor_cost.
   SELECT SUM(quantity_used * COALESCE(unit_cost,0)) FROM work_order_parts WHERE wo_id=?
   → parts_cost.
   service_cost = COALESCE(service_cost_input, 0).
   total_cost = labor_cost + parts_cost + service_cost.
   Compute actual_duration_hours:
     ROUND((JULIANDAY(now) - JULIANDAY(actual_start)) * 24, 2) if actual_start set.

5. UPDATE work_orders SET status=(closed), closed_at=now,
   labor_cost=?, parts_cost=?, service_cost=?, total_cost=?,
   actual_duration_hours=?, row_version+1, updated_at=now
   WHERE id=? AND row_version=?.

6. Write wo_state_transition_log: action='close'.
7. Return updated WorkOrder.

--- D) reopen_wo ---
Input: WoReopenInput { wo_id, actor_id, expected_row_version, reason: String,
  step_up_token: String }

1. Load WO; must be in closed status.
2. Validate step_up_token.
3. Check recurrence window: closed_at must be within last N days
   (N from app settings, default 7). If outside window, return error.
   Pull setting via: SELECT value FROM app_settings WHERE key='wo_reopen_window_days'
   If setting absent, default to 7.
4. Transition to technically_verified (revert to last stage before closure).
5. UPDATE work_orders SET status=(technically_verified), reopen_count=reopen_count+1,
   last_closed_at=closed_at, closed_at=NULL, row_version+1.
6. Write wo_state_transition_log: action='reopen', notes=reason.
7. Return updated WorkOrder.

--- E) Helper: get_failure_details(pool, wo_id) -> Result<Vec<WoFailureDetail>> ---

--- F) Helper: get_verifications(pool, wo_id) -> Result<Vec<WoVerification>> ---


STEP 3 - CREATE src-tauri/src/wo/costs.rs

Types:
- WoCostSummary:
    wo_id: i64,
    labor_cost: f64,
    parts_cost: f64,
    service_cost: f64,
    total_cost: f64,
    expected_duration_hours: Option<f64>,
    actual_duration_hours: Option<f64>,
    active_labor_hours: f64,
    total_waiting_hours: f64,
    duration_variance_hours: Option<f64>,  // actual - expected (positive = overrun)

- CostPostingHook:
    wo_id: i64,
    wo_code: String,
    entity_id: Option<i64>,
    asset_id: Option<i64>,
    type_code: String,
    urgency_level: Option<i64>,
    total_cost: f64,
    labor_cost: f64,
    parts_cost: f64,
    service_cost: f64,
    closed_at: String

Functions:
A) get_cost_summary(pool, wo_id) -> Result<WoCostSummary>
   Reads work_orders + computes variance.

B) get_cost_posting_hook(pool, wo_id) -> Result<CostPostingHook>
   Assembles CostPostingHook payload for SP §6.24 consumption.
   WO must be in closed or technically_verified state.

C) update_service_cost(pool, wo_id, service_cost: f64, actor_id) -> Result<()>
   UPDATE work_orders.service_cost_input. WO must not be closed/cancelled.


STEP 4 - CREATE src-tauri/src/wo/attachments.rs

Functions matching SP04 DI attachment pattern:
A) save_wo_attachment(pool, app_data_dir, input: WoAttachmentInput) -> Result<WoAttachment>
   Store under app_data_dir/wo_attachments/{wo_id}/{uuid}-{file_name}.
   WO must not be in cancelled state (allow adding even after closure).

B) list_wo_attachments(pool, wo_id) -> Result<Vec<WoAttachment>>

C) delete_wo_attachment_record(pool, attachment_id) -> Result<()>
   Does NOT delete from disk. Requires ot.admin in command layer.

PATCH src-tauri/src/wo/mod.rs
  pub mod closeout;
  pub mod costs;
  pub mod attachments;

ACCEPTANCE CRITERIA
- migration 023 applies with all ALTER TABLE statements
- cargo check passes
- close_wo returns list of all blocking issues when multiple quality gates fail
- save_verification rejects self-verification when primary_responsible_id is set
- reopen_wo fails after recurrence window
- compute_and_post_costs computes total_cost = labor + parts + service
```

### Supervisor Verification - Sprint S1

**V1 - Multi-gate failure list.**
On a corrective WO with no labor, no parts, no failure detail, call close_wo; must return
3 or more blocking errors in one response (not just the first one found).

**V2 - Self-verification guard.**
Set primary_responsible_id = actor_id on WO; call save_verification with same id; must fail.

**V3 - Cost roll-up.**
Add 2 labor entries (2h × 50 = 100, 3h × 40 = 120), 1 part (5 × 20 = 100), service_cost_input = 50;
close WO; verify labor_cost=220, parts_cost=100, service_cost=50, total_cost=370.

**V4 - Reopen window.**
Close WO; set closed_at = 10 days ago via direct DB update; call reopen_wo; must return
recurrence window error.

---

## Sprint S2 - Analytics Snapshot and IPC Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the WO analytics snapshot and all close-out IPC commands.

STEP 1 - CREATE src-tauri/src/wo/analytics.rs

Types:

`WoAnalyticsSnapshot`:
  wo_id: i64,
  wo_code: String,
  type_code: String,
  asset_id: Option<i64>,
  asset_code: Option<String>,
  entity_id: Option<i64>,
  urgency_level: Option<i64>,
  source_di_id: Option<i64>,
  -- Execution times
  submitted_at: Option<String>,        -- creation date of the WO
  actual_start: Option<String>,
  actual_end: Option<String>,
  mechanically_completed_at: Option<String>,
  technically_verified_at: Option<String>,
  closed_at: Option<String>,
  -- Time segments
  expected_duration_hours: Option<f64>,
  actual_duration_hours: Option<f64>,
  active_labor_hours: f64,
  total_waiting_hours: f64,
  downtime_hours: f64,
  schedule_deviation_hours: Option<f64>, -- actual_start - planned_start
  -- Costs
  labor_cost: f64,
  parts_cost: f64,
  service_cost: f64,
  total_cost: f64,
  -- Close-out evidence
  recurrence_risk_level: Option<String>,
  root_cause_summary: Option<String>,
  corrective_action_summary: Option<String>,
  failure_details: Vec<WoFailureDetail>,
  verifications: Vec<WoVerification>,
  -- Counts
  reopen_count: i64,
  labor_entries_count: i64,
  parts_entries_count: i64,
  attachment_count: i64,
  task_count: i64,
  mandatory_task_count: i64,
  completed_task_count: i64,
  delay_segment_count: i64,
  downtime_segment_count: i64,
  -- Planning quality
  was_planned: bool,         -- planned_start was set before actual_start
  parts_actuals_confirmed: bool,
  -- Module integration stubs
  pm_occurrence_id: Option<i64>,   -- placeholder for SP09 PM linkage
  permit_ids: Vec<i64>,            -- placeholder for SP23 Work Permit linkage

Functions:
`get_wo_analytics_snapshot(pool, wo_id: i64) -> Result<WoAnalyticsSnapshot>`
  Assembles the full snapshot from work_orders + all sub-entities.
  WO must be in closed or technically_verified state.
  schedule_deviation_hours: if actual_start and planned_start both set,
    ROUND((JULIANDAY(actual_start)-JULIANDAY(planned_start))*24, 2).


STEP 2 - PATCH src-tauri/src/commands/wo.rs

Add the following commands:

A) `save_failure_detail` — ot.edit
   Input: SaveFailureDetailInput; Delegates to closeout::save_failure_detail

B) `save_verification` — ot.edit + step-up
   Input: SaveVerificationInput + step_up_token
   Delegates to closeout::save_verification
   Step-up validated inside save_verification function.

C) `close_wo` — ot.edit + step-up
   Input: WoCloseInput; Delegates to closeout::close_wo

D) `reopen_wo` — ot.admin + step-up
   Input: WoReopenInput; Delegates to closeout::reopen_wo

E) `upload_wo_attachment` — ot.edit
   Input: wo_id, file_name, file_bytes, mime_type, notes?
   Gets app_data_dir from AppHandle; delegates to attachments::save_wo_attachment

F) `list_wo_attachments` — ot.view
   Input: wo_id; Delegates to attachments::list_wo_attachments

G) `delete_wo_attachment` — ot.admin
   Input: attachment_id; Delegates to attachments::delete_wo_attachment_record

H) `get_cost_summary` — ot.view
   Input: wo_id; Delegates to costs::get_cost_summary

I) `update_service_cost` — ot.edit
   Input: wo_id, service_cost; Delegates to costs::update_service_cost

J) `get_cost_posting_hook` — ot.view
   Input: wo_id; Delegates to costs::get_cost_posting_hook
   Returns: CostPostingHook (used by SP §6.24 Budget module)

K) `get_wo_analytics_snapshot` — ot.view
   Input: wo_id; Delegates to analytics::get_wo_analytics_snapshot

ACCEPTANCE CRITERIA
- All 11 commands registered; cargo check passes
- get_wo_analytics_snapshot returns WoAnalyticsSnapshot with all nested sub-entities
- save_verification transitions WO to technically_verified
- close_wo is blocked with structured list when quality gate fails
- get_cost_posting_hook returns correct total_cost payload
```

### Supervisor Verification - Sprint S2

**V1 - Analytics snapshot completeness.**
Close a WO with 2 labor entries, 1 part, 1 task, 1 failure detail, 1 verification;
call get_wo_analytics_snapshot; verify all count fields > 0 and costs match.

**V2 - Cost posting hook.**
Call get_cost_posting_hook on a closed WO; verify wo_code, total_cost, type_code, entity_id
are correct.

**V3 - reopen_wo permission.**
User with ot.edit (not ot.admin) calls reopen_wo; must return PermissionDenied.

---

## Sprint S3 - Close-Out and Verification UI

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the WO close-out panel and verification panel.

CREATE src/services/wo-closeout-service.ts

Types:
- SaveFailureDetailInput, WoFailureDetail
- SaveVerificationInput, WoVerification
- WoCloseInput, WoReopenInput
- WoCostSummary, CostPostingHook
- WoAttachment, WoAttachmentUploadInput
- WoAnalyticsSnapshot

Functions (all async):
- saveFailureDetail, saveVerification, closeWo, reopenWo
- listWoAttachments, uploadWoAttachment, deleteWoAttachment
- getCostSummary, updateServiceCost
- getWoAnalyticsSnapshot

CREATE src/components/wo/WoCloseOutPanel.tsx

Props: wo: WorkOrder; canEdit: boolean; onClosed: () => void

Four visible sections (always expanded for closing WOs):

SECTION 1 — Observed Symptom & Condition
  - Symptom code: reference_values selector (domain = symptom codes)
  - Narrative: free text field (maps to failure_details notes if no failure mode selected)

SECTION 2 — Failure Analysis
  - Failure mode: reference_values selector (domain = failure modes)
  - Failure cause: reference_values selector (domain = failure causes)
  - Failure effect: reference_values selector (domain = failure effects)
  - "Cause not determined" checkbox — disables mode/cause/effect selectors when checked

SECTION 3 — Action Performed
  - Corrective action summary: multiline text
  - Root cause summary: multiline text
  - Repair type: radio — Temporary / Permanent / Not Applicable
  - Recurrence risk: selector — None / Low / Medium / High
  - Service cost: number input

SECTION 4 — Return to Service
  - "Mark Mechanically Complete" button (only if WO in in_progress)
  - Verification result (if WO in mechanically_complete): pass/fail/monitor selector
  - Return to service confirmed: checkbox
  - Verification notes: text

"Close Work Order" button:
  - Only visible when WO in technically_verified
  - Prompts step-up PIN input
  - Calls closeWo; shows blocking error list if quality gate fails
  - Calls onClosed on success

Pre-flight error list rendered below the button when quality gate fails.
Each error item shown with icon and descriptive text.

CREATE src/components/wo/WoAttachmentPanel.tsx

Same pattern as DiAttachmentPanel from SP04.
Props: woId: number; canUpload: boolean; canDelete: boolean
Max file size: 25 MB (WOs may have larger PDFs than DI photos).

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- WoCloseOutPanel "Cause not determined" disables failure code selectors
- Close button requires step-up token input; empty token produces validation error
- Pre-flight errors are shown individually, not as one combined string
- WoAttachmentPanel max file size 25 MB enforced before any invoke
```

### Supervisor Verification - Sprint S3

**V1 - Cause not determined disables selectors.**
Check "Cause not determined"; verify failure_mode, failure_cause, failure_effect selectors
are disabled.

**V2 - Pre-flight error list.**
Mock closeWo to return 3 quality gate errors; verify 3 separate error items render.

**V3 - Step-up required.**
Click Close without entering step-up PIN; "Close Work Order" must be disabled.

**V4 - typecheck.**
`pnpm typecheck` — 0 errors across all new service and component files.

---

## Sprint S4 — Print WO Fiche and Cost Display Panel

> **Gap addressed:** The web app provides a "Print" button on each WO detail that
> opens a professional A4 document (company header, reference strip, equipment,
> planning, description, visa/signatures, footer). It also displays cost summary
> inline in the detail view. S1–S3 specified `get_cost_summary` but no print
> component and no cost display panel. This sprint closes both gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/wo/WoPrintFiche.tsx` | Print-ready A4 WO fiche layout |
| `src/components/wo/WoCostSummaryCard.tsx` | Inline cost display in WO detail |
| `src/components/wo/WoDetailDialog.tsx` (patch) | "Print" button in footer; cost summary card in close-out tab |
| `src/i18n/locale-data/{fr,en}/ot.json` (patch) | Print and cost labels |

### WoPrintFiche.tsx — Professional A4 Document

Triggered by "Imprimer" / "Print" button in WoDetailDialog footer.

**Behaviour:** Opens a new browser window (via `window.open`) with print-optimized HTML.

**Layout (A4 portrait, CSS `@media print` optimized):**

```
┌─────────────────────────────────────────────┐
│  Company logo (left)   Company name (right)  │
│  Address, contact info                       │
│  DOCUMENT TITLE: "Fiche d'Ordre de Travail" │
│  Reference: WOR-0001  |  Date: 08/04/2026   │
├─────────────────────────────────────────────┤
│  IDENTIFICATION                              │
│  Code: WOR-0001                              │
│  Type: Correctif   Urgence: Haute            │
│  Statut: En cours  Créé le: 01/04/2026       │
├─────────────────────────────────────────────┤
│  ÉQUIPEMENT CONCERNÉ                         │
│  Désignation: Pompe centrifuge #12           │
│  Entité: Usine Nord                          │
├─────────────────────────────────────────────┤
│  PLANIFICATION                               │
│  Début prévu: 02/04/2026  08:00              │
│  Fin prévue:  03/04/2026  16:00              │
│  Poste: Matin                                │
│  Durée estimée: 8h                           │
├─────────────────────────────────────────────┤
│  DEMANDEUR & DESCRIPTION                     │
│  Demandeur: Jean Dupont                      │
│  Description: [full text]                    │
│  Source DI: DI-0042                          │
├─────────────────────────────────────────────┤
│  TÂCHES                                      │
│  1. Démontage pompe  [✓ OK]                  │
│  2. Remplacement joint  [✓ OK]               │
│  3. Test étanchéité  [ ]                      │
├─────────────────────────────────────────────┤
│  INTERVENANTS                                │
│  Dupont J. (Responsable), Heures: 4h         │
│  Martin P. (Exécutant), Heures: 6h           │
├─────────────────────────────────────────────┤
│  PIÈCES UTILISÉES                            │
│  Joint SKF 6205 — Qté: 2                     │
│  Roulement NTN 7208 — Qté: 1                │
├─────────────────────────────────────────────┤
│  VISAS & SIGNATURES                          │
│  ┌───────────┬───────────┬───────────┐      │
│  │ Demandeur │ Exécutant │ Resp.Mnt. │      │
│  │           │           │           │      │
│  │ _______   │ _______   │ _______   │      │
│  │ Date:     │ Date:     │ Date:     │      │
│  └───────────┴───────────┴───────────┘      │
├─────────────────────────────────────────────┤
│  Réf: WOR-0001 | Maintafox | 08/04/2026     │
│  Document confidentiel                       │
└─────────────────────────────────────────────┘
```

**CSS:** `@page { size: A4; margin: 15mm; }` — no page break inside signature table.
**Data:** Loads tasks, interveners, parts via existing list commands.
**Company info:** Read from settings store (`settings.company.*`).

### WoCostSummaryCard.tsx — Inline Cost Display

**Props:** `{ woId: number }`

**Layout (Card):**
| Row | Value | Notes |
|-----|-------|-------|
| Main-d'œuvre / Labor | `labor_cost` | Sum of interveners (hours × rate) |
| Pièces / Parts | `parts_cost` | Sum of parts (qty × unit_cost) |
| Services / Services | `service_cost` | Manual vendor entry |
| **Total** | `total_cost` | **Bold**, highlighted |
| Écart / Variance | `expected_duration - actual_duration` | Show ▲/▼ arrow + colour |

**Data:** Calls `get_cost_summary(woId)` on mount and when WO status changes.

Render inside WoCloseOutPanel (Section 3 bottom) and in WoDetailDialog as a summary badge.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- WoPrintFiche opens a new window with A4-optimized layout
- Print includes all sections: identification, equipment, planning, description, tasks, interveners, parts, signatures
- Company header populated from settings
- WoCostSummaryCard shows correct cost breakdown matching get_cost_summary values
- Variance shows positive/negative indicator with appropriate colour
- Print CSS prevents page breaks inside signature table
```

### Supervisor Verification — Sprint S4

**V1 — Print fiche.**
Open WO detail; click Print; verify new window with correct A4 layout, company header,
and signature table.

**V2 — Cost summary.**
WO with labor 4h × 50 = 200, parts 2 × 45 = 90, service = 30; cost card shows total = 320.

**V3 — Variance.**
WO with expected_duration=8h, actual_duration=10h; variance shows "+2h" in red.

---

## Sprint S5 — Frontend Gap Remediation

### Context

During the Phase 2 SP05 File 02 professional gap review (session: p2-sp05-f02), 15 gaps were
identified across the WO frontend components. The 5 critical/quick fixes were resolved inline
(GA-001 through GA-003, GA-006, GA-014, GA-015). The remaining 10 gaps are documented here
as structured work items for the next agent session.

---

### GA-004 — WoPauseInput / WoHoldInput Missing `delay_reason_id`

**File:** `src/shared/ipc-types.ts`

**Problem:** `WoPauseInput` and `WoHoldInput` carry only `{ id, expected_row_version }`.
The Rust `pause_wo` command requires `delay_reason_id: Option<i64>` (used to categorize
the pause for downtime analytics). Without it, every pause stores a null reason, which
breaks scheduled-downtime KPIs.

**Fix:**
```typescript
// shared/ipc-types.ts
export interface WoPauseInput {
  id: number;
  expected_row_version: number;
  delay_reason_id: number | null;   // ← add
}

export interface WoHoldInput {
  id: number;
  expected_row_version: number;
  delay_reason_id: number | null;   // ← add
}
```

Add a `DelayReasonSelector` dropdown to `WoExecutionControls` — shown only when user
initiates a pause — populated via `list_reference_values({ category: "delay_reason" })`.

**Acceptance:**
- `pnpm typecheck` passes
- Pause dialog shows delay-reason dropdown
- `wo_delay_segments.delay_reason_id` is populated in SQLite after pause

---

### GA-005 — Completion Dialog Fields Not Persisted by Rust

**Files:** `src-tauri/src/wo/execution.rs` + `src/shared/ipc-types.ts`

**Problem:** `WoMechCompleteInput` carries `actual_end`, `actual_duration_hours`, and
`conclusion` in TypeScript but the Rust `complete_wo_mechanically` command ignores them.
These fields are defined on `work_orders` (nullable) and should be written at mech-complete
time to support plan-vs-actual variance reporting.

**Fix (Rust side):**
```rust
// In MechCompleteCmd, add fields:
pub actual_end: Option<String>,           // ISO-8601
pub actual_duration_hours: Option<f64>,
pub conclusion: Option<String>,

// In execute(), apply to work_orders UPDATE:
if let Some(ae) = &cmd.actual_end { ... }
if let Some(adh) = cmd.actual_duration_hours { ... }
if let Some(c) = &cmd.conclusion { ... }
```

**Acceptance:**
- After `complete_wo_mechanically`, `work_orders.actual_end` and `work_orders.conclusion`
  match the submitted values
- `get_work_order` returns the persisted values
- Rust unit test `s5_v1_mech_complete_persists_actuals` passes

---

### GA-007 — `formatShiftLabel` and SHIFT_OPTIONS Use Hardcoded French

**File:** `src/components/wo/WoPlanningPanel.tsx`

**Problem:** Shift display labels (`"Matin"`, `"Après-midi"`, `"Nuit"`, `"Journée complète"`)
are hardcoded in French regardless of the active locale. This breaks English-locale users.

**Fix:** Use i18n keys already present in `fr/ot.json` and `en/ot.json`:
```typescript
// WoPlanningPanel.tsx
const { t } = useTranslation("ot");

const SHIFT_OPTIONS = [
  { value: "morning",   label: t("shift.morning") },
  { value: "afternoon", label: t("shift.afternoon") },
  { value: "night",     label: t("shift.night") },
  { value: "full_day",  label: t("shift.full_day") },
];

function formatShiftLabel(shift: WoShift, t: TFunction): string {
  return t(`shift.${shift}`);
}
```

Ensure `shift.morning`, `shift.afternoon`, `shift.night`, `shift.full_day` keys exist in
both `en/ot.json` and `fr/ot.json`.

**Acceptance:**
- Switch app locale to EN → shift dropdown shows "Morning / Afternoon / Night / Full day"
- Switch to FR → shows "Matin / Après-midi / Nuit / Journée complète"

---

### GA-008 — WoPlanningPanel and WoExecutionControls Section Headings Not i18n'd

**Files:** `src/components/wo/WoPlanningPanel.tsx`, `src/components/wo/WoExecutionControls.tsx`

**Problem:** Several section headings, button labels, and status messages are hardcoded
English strings:
- `"Planning"`, `"Assignment"`, `"Scheduling"`, `"Assign Planner"`, `"Assign Team"`,
  `"Plan this WO"`, `"No execution actions at this status."`

**Fix:** Replace all hardcoded strings with `t("plan.*")` / `t("exec.*")` keys. Add
corresponding keys to both locale files.

**Acceptance:**
- No hardcoded English UI strings remain in these two components
- `src/__tests__/i18n/wo-panels.test.ts` asserts FR / EN key parity for all new keys

---

### GA-009 — `formatDate` in WoDetailDialog Hardcodes `fr-FR` Locale

**File:** `src/components/wo/WoDetailDialog.tsx`

**Problem:** `formatDate` calls `new Date(s).toLocaleDateString("fr-FR", ...)` regardless
of the active app locale, showing French date format for English-locale users.

**Fix:**
```typescript
import { useTranslation } from "react-i18next";

// In component:
const { i18n } = useTranslation();

function formatDate(s: string | null | undefined): string {
  if (!s) return "—";
  return new Date(s).toLocaleDateString(i18n.language, {
    year: "numeric", month: "short", day: "2-digit",
  });
}
```

**Acceptance:**
- EN locale → dates formatted as "Jan 15, 2025"
- FR locale → dates formatted as "15 janv. 2025"

---

### GA-010 — Build `WoAuditTimeline` Component

**File (new):** `src/components/wo/WoAuditTimeline.tsx`

**Problem:** The Audit tab in `WoDetailDialog` shows raw date strings for `created_at`,
`planned_start`, `actual_start`, `planned_end`, `actual_end`. There is no visual timeline
or state-transition history.

**Deliverable:**
- Component reads `wo_state_transition_log` via `get_wo_state_log(woId)` IPC command
- Renders a vertical timeline: each row = `{ from_status, to_status, actor, timestamp, notes }`
- Color-code each status badge using the existing `WO_STATUS_COLOR` map
- Falls back to a minimal summary (4 key dates) if `get_wo_state_log` is not yet implemented

**Rust prerequisite:** `get_wo_state_log(wo_id: i64) -> Result<Vec<WoStateLogEntry>>` command
reading `wo_state_transition_log` table (built in SP05 File 01).

**Acceptance:**
- Audit tab shows timeline with at least "Created → Drafted → ..." entries
- Each row shows actor name, not just actor ID
- Timestamps respect app locale (GA-009 fix applies here too)

---

### GA-011 — Build `WoAttachmentPanel` Component

**File (new):** `src/components/wo/WoAttachmentPanel.tsx`

**Problem:** The Attachments tab shows only a `<p>Pièces jointes à venir</p>` placeholder.

**Deliverable:**
- List attachments via `list_wo_attachments(woId)` → `WoAttachment[]`
- Upload button triggers `upload_wo_attachment(woId, fileData)` (Tauri file dialog)
- Each attachment row: file name, uploader, date, delete button (permission-gated)
- Attachment schema defined in SP05 File 03 Sprint S1

**Prerequisite:** File 03 Sprint S1 (`work_order_attachments` migration and commands) must
be complete before this panel can be wired to real data. Interim: render empty state UX.

**Acceptance:**
- Upload flow works end-to-end with Tauri file dialog
- Uploaded files appear in list without page refresh
- Delete removes the record and hides the file from the list

---

### GA-012 — Implement Verification Flow: `mechanically_complete` → `technically_verified`

**Files:** `src/components/wo/WoVerificationPanel.tsx` (new), `src/components/wo/WoDetailDialog.tsx`

**Problem:** In `WoDetailDialog` the footer "Verify" button for `mechanically_complete` WOs
currently calls `onClose` (placeholder). The verification stage is a required gate: a separate
verifier must confirm return-to-service before the WO can be closed.

**Deliverable:**
- `WoVerificationPanel.tsx`: form with `result` (pass/fail/monitor), `return_to_service_confirmed`
  checkbox, `recurrence_risk_level` select, `notes` textarea, "Submit Verification" button
- `save_verification` IPC command (File 03 Sprint S1) wired to the panel
- Footer behaviour: "Verify" button for `mechanically_complete` opens an inline panel or
  side-sheet; submitting the form calls `save_verification` then transitions to `technically_verified`
- "Close" button for `technically_verified` calls `close_wo` (not `onClose` which dismisses the dialog)

**Acceptance:**
- Self-verification is rejected (`verified_by_id === primary_responsible_id`)
- After verification, WO status transitions to `technically_verified`
- After close, WO is read-only and shows status badge "Closed"

---

### GA-013 — Consolidate Duplicate Type Definitions

**Files:** `src/services/wo-execution-service.ts`, `src/shared/ipc-types.ts`

**Problem:** `wo-execution-service.ts` defines local types `WoTask`, `WoDelaySegment`,
`WoDowntimeSegment`, `WoIntervener` that duplicate or diverge from equivalent types in
`shared/ipc-types.ts`. This creates a maintenance burden and subtle runtime mismatches.

**Fix:**
1. Move canonical `WoTask`, `WoDelaySegment`, `WoDowntimeSegment`, `WoIntervener` definitions
   to `shared/ipc-types.ts`
2. Re-export them from `wo-execution-service.ts` for backward compatibility
3. Consolidate `WoExecutionInput` (uses `wo_id` + `actor_id`) and align with Rust backend field names

**Acceptance:**
- No duplicate interface definitions across service files
- `pnpm typecheck` passes
- No runtime field-name mismatches between TS invocations and Rust deserialization

---

### Acceptance Criteria for Sprint S5

```
- pnpm typecheck passes with zero errors after all fixes
- GA-004: Pause dialog shows delay-reason selector; wo_delay_segments.delay_reason_id populated
- GA-005: Mech-complete persists actual_end, actual_duration_hours, conclusion; Rust test passes
- GA-007/GA-008: No hardcoded French/English UI strings in WoPlanningPanel or WoExecutionControls
- GA-009: Date formatting respects active i18n locale in WoDetailDialog
- GA-010: Audit tab shows state-transition timeline with actor names
- GA-011: Attachments tab uploads and lists files via Tauri file dialog
- GA-012: Verification form wired to save_verification; self-verification blocked; close_wo called on close
- GA-013: No duplicate type definitions; ipc-types.ts is single source of truth for WO sub-entity types
```

### Supervisor Verification — Sprint S5

**V1 — i18n parity.**
Switch locale to EN; open a WO in planning state; verify all labels in WoPlanningPanel are
in English (shifts, section headings, button labels). Switch to FR; verify French labels.

**V2 — Pause with delay reason.**
Start a WO; click Pause; verify delay-reason dropdown appears; select a reason; confirm pause
stored the reason ID in SQLite `wo_delay_segments`.

**V3 — Mech-complete actuals persisted.**
Complete a WO mechanically with `actual_end` and `conclusion` filled in; open SQLite and verify
`work_orders.actual_end` and `work_orders.conclusion` match the submitted values.

**V4 — Verification gate.**
As the WO's primary_responsible, try to verify the WO; expect rejection. As a different user
with `ot.verify` permission, submit verification; confirm status moves to `technically_verified`.

**V5 — Audit timeline.**
Close a WO; open it; go to Audit tab; verify timeline shows all state transitions with actor
names and timestamps in the active locale format.

---

*End of Phase 2 - Sub-phase 05 - File 03*
