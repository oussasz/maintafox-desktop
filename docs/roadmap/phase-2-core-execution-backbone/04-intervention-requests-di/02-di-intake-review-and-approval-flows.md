# Phase 2 - Sub-phase 04 - File 02
# DI Intake, Review, and Approval Flows

## Context and Purpose

File 01 established the schema and state machine. File 02 delivers the operational workflows
that move a DI through its triage lifecycle:

1. **Submission validation** — stage gate 1: minimum intake fields enforced before persisting
2. **Review queue** — screener-assigned queue with context enrichment for reviewers
3. **Screen action** — validates priority, queue ownership, and classification before advancing
4. **Return for clarification** — sends DI back to submitter with structured note
5. **Reject action** — terminates the request with a required reason
6. **Approve for planning** — confirms execution path; unlocks conversion
7. **Defer action** — holds approved DI with a structured until-date and reason

These flows enforce the three stage gates from PRD §6.4:
- Submission gate: title, description, origin, asset or location, urgency
- Review gate: validated priority, queue ownership, triage decision, classification
- Conversion gate: implemented in File 03

An append-only `di_review_events` table (migration 018) records every triage decision with
actor, timestamp, and reasoning so backlogs and SLA breaches are always traceable.

---

## PRD Alignment Checklist

This file addresses PRD §6.4 requirements for:

- [x] Stage gate 2 (review): validated priority, queue ownership, triage decision required
      before advancing beyond Pending Review
- [x] Review-to-approval timing preserved (screened_at, approved_at stored on DI)
- [x] Repeat-issue detection surfaced to reviewer as context (not a blocker)
- [x] Request-to-review elapsed time computable (submitted_at → screened_at)
- [x] Scope of intake — all 9 origin types handled and validated at submission
- [x] Controlled classifications required for analytics — `classification_code_id` mandatory
      before screen proceeds to approval queue
- [x] Deferred state — requires structured `deferred_until` date and reason

---

## Architecture Rules Applied

- **Transition commands are separate from CRUD commands.** `screen_di`, `return_di`,
  `reject_di`, `approve_di`, `defer_di` each call `guard_transition` from `domain.rs`
  before writing any data. No raw status string updates bypass the state machine.
- **Review queue uses org_node scoping.** A reviewer sees all DIs whose `review_team_id`
  matches one of their assigned org nodes. Global reviewers (`di.review` global scope) see
  all queues.
- **Repeat-issue context is non-blocking.** `get_recent_similar_dis` runs as a read-only
  enrichment when a reviewer opens a DI detail. Similar DIs are surfaced as information; they
  do not block any transition.
- **screened_at and approved_at are written once and never overwritten.** Subsequent
  reactivation of a deferred DI does not reset these timestamps; the review trail is preserved.
- **di_review_events is append-only.** No update or delete commands exist for this table.
- **SLA clock is initialized at screen time.** The target response deadline is computed in
  File 03 (`sla.rs`) but the `di_review_events` row captures the clock-start timestamp.
- **Screen action auto-advances through Screened → AwaitingApproval atomically.** The PRD
  state machine defines both intermediate states, but the screen action advances through
  `PendingReview → Screened → AwaitingApproval` in a single transaction. Two
  `di_review_events` rows are written (`screened` + `advanced_to_approval`) for full audit
  traceability. The DI never rests in the `screened` state; `screened_at` is still recorded
  and the intermediate state appears in the event log.
- **Step-up reauthentication uses session state, not tokens.** The `require_step_up!` macro
  validates that the caller's session has active elevated authentication. No explicit
  `step_up_token` parameter flows through domain functions; the IPC command layer enforces
  step-up before delegating to `review.rs`.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000018_di_review_events.rs` | Append-only review decision log |
| `src-tauri/src/di/review.rs` | Triage workflow functions: screen / return / reject / approve / defer / reactivate |
| `src-tauri/src/commands/di.rs` (patch) | New IPC commands: screen_di, return_di, reject_di, approve_di, defer_di, reactivate_di |
| `src/services/di-review-service.ts` | Frontend wrappers for all review transition commands |
| `src/stores/di-review-store.ts` | Zustand state for review queue, triage actions, and similar-DI context |

---

## Prerequisites

- File 01 complete: migration 017, `di/domain.rs`, `di/queries.rs`, `commands/di.rs` working
- `guard_transition` function available from `di/domain.rs`
- `require_permission!` macro available; `di.review` and `di.approve` permissions seed data
  exist in the permissions table (seeded in SP06 or seeded as part of this subphase's S1)

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Review Event Log and Workflow Functions | migration 018 + `review.rs` |
| S2 | IPC Commands for All Triage Actions | `commands/di.rs` patch |
| S3 | Review Queue UI State and Service | `di-review-service.ts` + `di-review-store.ts` |

---

## Sprint S1 - Review Event Log and Workflow Functions

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the DI review event log and all triage functions.

STEP 1 - CREATE src-tauri/migrations/m20260401_000018_di_review_events.rs

use sea_orm_migration::prelude::*;

CREATE TABLE di_review_events (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  di_id           INTEGER NOT NULL REFERENCES intervention_requests(id),
  event_type      TEXT    NOT NULL,
    -- submitted / screened / advanced_to_approval / returned_for_clarification /
    --  rejected / approved / deferred / reactivated / sla_initialized
  actor_id        INTEGER NULL REFERENCES user_accounts(id),
  acted_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  -- Decision data
  from_status     TEXT    NOT NULL,
  to_status       TEXT    NOT NULL,
  reason_code     TEXT    NULL,
  notes           TEXT    NULL,
  -- SLA context (set on screen action)
  sla_target_hours INTEGER NULL,
  sla_deadline    TEXT    NULL,
  -- Step-up used
  step_up_used    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_dre_di_id    ON di_review_events(di_id);
CREATE INDEX idx_dre_actor    ON di_review_events(actor_id);
CREATE INDEX idx_dre_type     ON di_review_events(event_type);


STEP 2 - CREATE src-tauri/src/di/review.rs

Implement the following async functions. All take &SqlitePool and an input struct.
All begin a sqlx transaction, perform state guard, write the DI update, and write
the di_review_events row inside the same transaction.

--- A) screen_di ---

Input: DiScreenInput {
  di_id: i64,
  actor_id: i64,
  expected_row_version: i64,
  validated_urgency: String,          // must be valid DiUrgency
  review_team_id: Option<i64>,        // org node responsible for approval
  classification_code_id: i64,        // required — reference_values FK
  reviewer_note: Option<String>,
}

Logic:
1. Load DI; call guard_transition(&current_status, &DiStatus::Screened); error if fails.
2. Call guard_transition(&DiStatus::Screened, &DiStatus::AwaitingApproval); error if fails.
   Both guards are validated upfront before any write.
3. Validate validated_urgency is a valid DiUrgency value.
4. Validate classification_code_id resolves in reference_values table.
5. UPDATE intervention_requests SET
     status = 'awaiting_approval',        -- final persisted state (auto-advanced)
     validated_urgency = ?,
     review_team_id = COALESCE(?, review_team_id),
     classification_code_id = ?,
     reviewer_note = COALESCE(?, reviewer_note),
     reviewer_id = ?,
     screened_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
     row_version = row_version + 1,
     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
   WHERE id = ? AND row_version = ?;
   Check rows_affected == 1 (concurrency guard).
6. Insert di_review_events row 1: event_type = 'screened',
   from = 'pending_review', to = 'screened'.
7. Insert di_review_events row 2: event_type = 'advanced_to_approval',
   from = 'screened', to = 'awaiting_approval'.
8. Commit. Return updated InterventionRequest (status = 'awaiting_approval').

--- B) return_di_for_clarification ---

Input: DiReturnInput {
  di_id: i64,
  actor_id: i64,
  expected_row_version: i64,
  reviewer_note: String,    // required — must not be empty
}

Logic:
1. Load DI; guard_transition to ReturnedForClarification.
2. reviewer_note must not be empty; return error if blank.
3. UPDATE: status = 'returned_for_clarification', reviewer_note = ?, reviewer_id = ?,
   row_version + 1, updated_at = now.
4. Insert di_review_events row: event_type = 'returned_for_clarification'.
5. Commit. Return updated DI.

--- C) reject_di ---

Input: DiRejectInput {
  di_id: i64,
  actor_id: i64,
  expected_row_version: i64,
  reason_code: String,    // required
  notes: Option<String>,
}

Logic:
1. Load DI; guard_transition to Rejected.
2. reason_code must not be empty.
3. UPDATE: status = 'rejected', declined_at = now, reviewer_id = ?,
   reviewer_note = COALESCE(notes, reviewer_note), row_version + 1, updated_at = now.
4. Insert di_review_events row: event_type = 'rejected', reason_code = ?.
5. Commit.

--- D) approve_di_for_planning ---

Input: DiApproveInput {
  di_id: i64,
  actor_id: i64,
  expected_row_version: i64,
  notes: Option<String>,
  // No step_up_token field — step-up is enforced at the IPC command layer
  // via require_step_up!(state) before this function is called.
}

Logic:
1. Load DI; guard_transition to ApprovedForPlanning.
2. (Step-up already enforced by IPC layer — no token validation here.)
3. UPDATE: status = 'approved_for_planning', approved_at = now,
   reviewer_note = COALESCE(notes, reviewer_note), row_version + 1, updated_at = now.
4. Insert di_review_events row: event_type = 'approved', step_up_used = 1.
5. Commit.

--- E) defer_di ---

Input: DiDeferInput {
  di_id: i64,
  actor_id: i64,
  expected_row_version: i64,
  deferred_until: String,    // ISO date string, must be in the future
  reason_code: String,       // required
  notes: Option<String>,
}

Logic:
1. Load DI; guard_transition to Deferred.
2. Validate deferred_until is a future date (parse and compare with now).
3. reason_code must not be empty.
4. UPDATE: status = 'deferred', deferred_until = ?, row_version + 1, updated_at = now.
5. Insert di_review_events row: event_type = 'deferred'.
6. Commit.

--- F) reactivate_deferred_di ---

Input: DiReactivateInput {
  di_id: i64,
  actor_id: i64,
  expected_row_version: i64,
  notes: Option<String>,
}

Logic:
1. Load DI; guard_transition from Deferred to AwaitingApproval.
2. UPDATE: status = 'awaiting_approval', deferred_until = NULL,
   row_version + 1, updated_at = now.
3. Insert di_review_events: event_type = 'reactivated'.
4. Commit.

--- G) get_review_events ---

fn get_review_events(pool, di_id: i64) -> Result<Vec<DiReviewEvent>>
SELECT * FROM di_review_events WHERE di_id = ? ORDER BY acted_at ASC.

ACCEPTANCE CRITERIA
- Each function uses sqlx::Transaction; all writes commit atomically
- guard_transition is always called before any DB write
- screen_di validates both transitions (→Screened, →AwaitingApproval) and writes 2 event rows
- approve_di relies on IPC-layer step-up guard (require_step_up!); no token field in input
- deferred_until is validated as a future date; past dates return error
- reviewer_note and reason_code required fields return descriptive errors when empty
- di_review_events rows written for every successful transition
```

### Supervisor Verification - Sprint S1

**V1 - Screen action atomicity.**
Kill the process between DI update and event write (simulate panic); confirm on restart that
the DI is still in `pending_review` (transaction rolled back).

**V2 - Return requires note.**
Call `return_di_for_clarification` with an empty `reviewer_note`; must return validation error.

**V3 - Approve step-up guard.**
Call `approve_di` IPC command without an active step-up session; must return `StepUpRequired`.

**V4 - Defer future-date guard.**
Call `defer_di` with `deferred_until` set to yesterday; must return validation error.

**V5 - Reactivation from deferred.**
Create DI → screen → approve → defer → reactivate; final status must be `awaiting_approval`
with deferred_until = NULL and full event log of 6 rows (screen writes 2: screened +
advanced_to_approval).

---

## Sprint S2 - IPC Commands for All Triage Actions

### AI Agent Prompt

```text
You are a senior Rust engineer. Add triage IPC commands to the DI command module.

PATCH src-tauri/src/commands/di.rs

Add the following Tauri commands. Each follows the same pattern:
1. Resolve session user from state
2. Check required permission with require_permission! macro
3. Delegate to the matching review.rs function
4. Return serialized result or CommandError

A) `screen_di`
   Permission required: di.review
   Delegates to: review::screen_di(pool, input)
   Input: DiScreenInput (from frontend JSON)

B) `return_di`
   Permission required: di.review
   Delegates to: review::return_di_for_clarification(pool, input)
   Input: DiReturnInput

C) `reject_di`
   Permission required: di.review
   Delegates to: review::reject_di(pool, input)
   Input: DiRejectInput

D) `approve_di`
   Permission required: di.approve
   Step-up: enforced via require_step_up!(state) before delegating — session must have
   active elevated auth. No token field in DiApproveInput.
   Delegates to: review::approve_di_for_planning(pool, input)
   Input: DiApproveInput

E) `defer_di`
   Permission required: di.approve
   Delegates to: review::defer_di(pool, input)
   Input: DiDeferInput

F) `reactivate_di`
   Permission required: di.approve
   Delegates to: review::reactivate_deferred_di(pool, input)
   Input: DiReactivateInput

G) `get_di_review_events`
   Permission required: di.view
   Delegates to: review::get_review_events(pool, di_id)
   Input: di_id: i64

ALSO: Register di::review in src-tauri/src/di/mod.rs (pub mod review;)

ACCEPTANCE CRITERIA
- cargo check passes
- All 7 commands registered in invoke_handler
- screen_di, reject_di, return_di guarded by di.review
- approve_di, defer_di, reactivate_di guarded by di.approve
- get_di_review_events guarded by di.view
```

### Supervisor Verification - Sprint S2

**V1 - Permission guard on approve.**
Call `approve_di` with a user who only has `di.view`; must return PermissionDenied.

**V2 - screen_di permission.**
Call `screen_di` with a user who has `di.review`; must succeed if DI is in pending_review.

**V3 - Command registration.**
`tauri::Builder` has no duplicate command names; `cargo check` is clean.

---

## Sprint S3 - Review Queue UI State and Service

### AI Agent Prompt

```text
You are a TypeScript engineer. Implement the DI review service and Zustand store.

CREATE src/services/di-review-service.ts

Import invoke from @tauri-apps/api/core.

Define types:
- DiScreenInput, DiReturnInput, DiRejectInput, DiApproveInput, DiDeferInput,
  DiReactivateInput (match Rust structs)
- DiReviewEvent (matches di_review_events row)
- DiSummaryRow (matching get_recent_similar_dis output from File 01)

Functions (all async):
- screenDi(input: DiScreenInput): Promise<InterventionRequest>
- returnDi(input: DiReturnInput): Promise<InterventionRequest>
- rejectDi(input: DiRejectInput): Promise<InterventionRequest>
- approveDi(input: DiApproveInput): Promise<InterventionRequest>
- deferDi(input: DiDeferInput): Promise<InterventionRequest>
- reactivateDi(input: DiReactivateInput): Promise<InterventionRequest>
- getDiReviewEvents(diId: number): Promise<DiReviewEvent[]>

All functions use Zod to validate response shape before returning.

CREATE src/stores/di-review-store.ts

State shape:
- reviewQueue: InterventionRequest[]           // DIs in pending_review for user's org nodes
- activeReviewDi: InterventionRequest | null
- reviewEvents: DiReviewEvent[]
- similarDis: DiSummaryRow[]
- saving: boolean
- error: string | null

Actions:
- loadReviewQueue(): Promise<void>
  Calls listDis({ status: ['pending_review', 'returned_for_clarification'] }) from di-service
- openForReview(id: number): Promise<void>
  Loads DI detail + review events + similar DIs in parallel
- screen(input: DiScreenInput): Promise<void>
  Calls screenDi; refreshes activeReviewDi and reviewQueue
- returnForClarification(input: DiReturnInput): Promise<void>
  Calls returnDi; refreshes
- reject(input: DiRejectInput): Promise<void>
  Calls rejectDi; refreshes
- approve(input: DiApproveInput): Promise<void>
  Calls approveDi; refreshes
- defer(input: DiDeferInput): Promise<void>
  Calls deferDi; refreshes
- reactivate(input: DiReactivateInput): Promise<void>
  Calls reactivateDi; refreshes

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- All 7 service functions invoke matching Rust command names
- Store openForReview loads events and similar DIs in Promise.all (parallel)
- saving = true set before any mutating action; cleared in finally
- error cleared on successful action
```

### Supervisor Verification - Sprint S3

**V1 - Parallel load.**
`openForReview` must fire getDi and getDiReviewEvents and listRecentSimilar in a single
`Promise.all`; verify no sequential await chain.

**V2 - Type safety on approve.**
`DiApproveInput` must NOT include `step_up_token`; step-up is enforced server-side via
session state. Confirm the type has only `di_id`, `actor_id`, `expected_row_version`, `notes`.

**V3 - Queue filter.**
`loadReviewQueue` must set `status: ['pending_review', 'returned_for_clarification']`
in the list filter; confirm with mock interception.

---

## Sprint S4 — Review UI: Management Panel, Approval / Rejection Modals, Modified Badge

> **Gap addressed:** S1–S3 built the backend review commands and the store. The web
> app provides an approver management panel (sorted pending queue), an approval
> confirmation modal with conversion preview + print, a rejection modal with reason
> textarea, and a "modifiée" badge when the creator edits a pending DI. This sprint
> delivers the equivalent desktop UI components.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/di/DiReviewPanel.tsx` | Approver-facing management panel: sorted queue of pending DIs |
| `src/components/di/DiApprovalDialog.tsx` | Approval confirmation modal with DI summary, conversion preview, approval note, print button |
| `src/components/di/DiRejectionDialog.tsx` | Rejection confirmation modal with DI summary, reason textarea, warning |
| `src/components/di/DiReturnDialog.tsx` | Return-for-clarification dialog with required note |
| `src/pages/RequestsPage.tsx` (patch) | Integrate DiReviewPanel as a collapsible section above the list/kanban view (visible to `di.review` holders) |
| `src/stores/di-review-store.ts` (patch) | `openApproval`, `openRejection`, `openReturn` dialog state + handlers |
| `src/i18n/locale-data/{fr,en}/di.json` (patch) | Review labels, modal titles, action confirmations |

### DiReviewPanel.tsx — Approver Queue

Renders a collapsible card above the main view for users with `di.review` permission.

**Content:**
- Header with badge count: "DI en attente de validation (8)" / "Pending DIs (8)"
- Sortable/filterable list (sort by: priority desc → submitted_at asc → equipment → requester)
- Each row shows: code, title, priority badge, equipment label, requester name, submitted date,
  "modifiée" badge (when `is_modified = 1`)
- Actions per row:
  - **Validate** (green) → opens DiApprovalDialog
  - **Reject** (red) → opens DiRejectionDialog
  - **Return** (amber) → opens DiReturnDialog
  - **View** → opens DiDetailDialog

**Data source:** `di-review-store.loadReviewQueue()` (already specified in S3).

### DiApprovalDialog.tsx — Approval Confirmation

Follows UX-DW-001 pattern. Large dialog (`max-w-3xl`).

**Layout:**
1. **Conversion banner** — `DI-0001 → OT-0001` (code preview)
2. **DI info card** — Code, title, type, priority, status badges
3. **Equipment info** — Designation, entity, location
4. **Requester info** — Name, position, department
5. **Description + notes** — Full text
6. **Approval note** — Textarea (optional, max 2000 chars)
7. **Approver signature section** — Name (from session), timestamp (current)

**Actions (footer):**
- "Imprimer" / "Print" (secondary) — Opens print-friendly approval sheet
- "Approuver & Convertir" / "Approve & Convert" (primary, green) — Calls `approve(input)` then triggers DI→WO conversion

**Print function:**
- Opens a new print-ready window with:
  - Company header (from settings)
  - Document title: "Fiche de validation — Demande d'Intervention"
  - Reference number, issue date
  - DI summary (all fields)
  - Conversion notice: "Transférée à l'OT" + OT code
  - Visa/signature section: Requester, Approver, Maintenance Responsible (3 columns, blank lines)
  - Footer: Document ref, company name, confidentiality notice

### DiRejectionDialog.tsx — Rejection Confirmation

Small dialog (`max-w-lg`).

**Layout:**
1. **DI summary** — Code, title, priority badge
2. **Rejection reason** — Textarea (optional but recommended, max 2000 chars)
3. **Warning** — "Cette action est irréversible." / "This action is irreversible."

**Actions (footer):**
- "Annuler" / "Cancel"
- "Rejeter & Confirmer" / "Reject & Confirm" (destructive red) — Calls `reject(input)` with reason_code + notes

### DiReturnDialog.tsx — Return for Clarification

Small dialog (`max-w-lg`).

**Layout:**
1. **DI summary** — Code, title, requester name
2. **Clarification note** — Textarea (required, non-empty)
3. **Info** — "La DI sera renvoyée au déclarant." / "The request will be returned to the requester."

**Actions (footer):**
- "Annuler" / "Cancel"
- "Renvoyer" / "Return" (amber) — Calls `returnForClarification(input)`

### Modified Badge

When `di.is_modified = 1`, display a small badge "Modifiée" / "Modified" (amber background)
next to the DI code in:
- DiReviewPanel rows
- DiKanbanBoard cards
- DataTable list rows
- DiDetailDialog header

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- DiReviewPanel renders only for users with di.review permission
- DiReviewPanel shows correct badge count matching total pending DIs
- DiApprovalDialog shows conversion preview with generated OT code
- DiApprovalDialog print opens a browser print window with full approval sheet
- DiRejectionDialog submit calls reject with reason_code and notes
- DiReturnDialog requires non-empty clarification note (submit blocked if empty)
- "modifiée" badge appears on DIs where is_modified = 1
- Sort order: priority desc → submitted_at asc → equipment → requester
```

### Supervisor Verification — Sprint S4

**V1 — Review panel permission gate.**
User WITHOUT `di.review`: DiReviewPanel must not render in RequestsPage.

**V2 — Modified badge visibility.**
DI with `is_modified = 1` must show "Modifiée" badge in review panel, kanban card, and list row.

**V3 — Rejection blocks on empty reason_code.**
Attempt reject without `reason_code`; submit must be blocked with inline error.

**V4 — Print approval sheet.**
Click Print in DiApprovalDialog; verify new window opens with document title, DI summary,
signatures section, company header.

---

*End of Phase 2 - Sub-phase 04 - File 02*
