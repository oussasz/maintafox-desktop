# Phase 2 - Sub-phase 04 - File 01
# DI Domain Model and State Machine

## Context and Purpose

Sub-phase 04 delivers the Intervention Request (DI) module — the formal intake gate for all
reactive and semi-reactive maintenance demand in Maintafox Desktop.

A DI is not a miniature work order. It is a triage object that preserves the original field
signal so that demand can be reviewed, screened, approved, and converted into executable work
without losing the origin context. The immutable DI record becomes the root of the execution
chain and the first link in SLA, backlog, and reliability analytics.

This file establishes:

- the core `intervention_requests` schema (migration 017)
- the `di_state_transition_log` append-only table
- the exact 11-state machine from PRD §6.4 with transition guards
- base CRUD IPC commands
- the Rust domain types and TypeScript service/store

Files 02, 03, and 04 add the review/approval workflows, SLA/attachment/conversion machinery,
and the permission/audit layer.

---

## PRD Alignment Checklist

This file addresses PRD §6.4 requirements for:

- [x] 11-state workflow: Submitted → Pending Review → Returned for Clarification → Rejected →
      Screened → Awaiting Approval → Approved for Planning → Deferred → Converted to Work Order →
      Closed as Non-Executable → Archived
- [x] Stage gate 1 (submission): minimum valid intake context required
- [x] Data quality rule: the request remains the immutable origin record once converted
- [x] Request-to-review, review-to-approval, and approval-to-conversion timings preserved
- [x] Scope of intake: operator, technician, inspection, PM-detected, HSE, quality, production,
      IoT / external-system triggered alerts
- [x] Controlled classifications required where analytics need structured evidence
- [x] `di.*` permission domain reserved (implemented fully in File 04)

---

## Architecture Rules Applied

- **State machine is enforced in Rust, not the frontend.** All transition requests are validated
  against the allowed transition table before any write. Invalid transitions return an error.
- **`di_state_transition_log` is append-only.** No update or delete commands are implemented
  for this table. Every state movement writes a new row.
- **DI code is unique, uppercase, non-recycled.** Format: `DI-NNNN`. Sequence is not reused
  after deletion or archival.
- **No hard delete for DIs that have progressed past submission.** DIs with any state history
  beyond `submitted` are archived, not deleted. Plain submitted DIs may be deleted by
  `di.admin` with a recorded justification.
- **Cross-module FKs are nullable by design.** `converted_to_wo_id` is nullable until SP05
  implements work orders. `symptom_code_id` and `classification_code_id` reference
  `reference_values` from SP03 but are nullable to avoid blocking triage.
- **Optimistic concurrency via `row_version`.** All update commands check `expected_row_version`
  to prevent silent overwrites from concurrent sessions.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000017_di_domain_core.rs` | Core DI schema: `intervention_requests` + `di_state_transition_log` |
| `src-tauri/src/di/mod.rs` | Module root with sub-module declarations |
| `src-tauri/src/di/domain.rs` | Rust types, state enum, transition guard, code generator |
| `src-tauri/src/di/queries.rs` | List, get, search, and cross-module context queries |
| `src-tauri/src/commands/di.rs` | IPC commands: create, get, list, update draft fields |
| `src/services/di-service.ts` | Typed Tauri invoke wrappers for all DI commands |
| `src/stores/di-store.ts` | Zustand store: DI list, pagination, active DI, filters |

---

## Prerequisites

- SP01 complete: `org_nodes` table and `org.view` permission exist
- SP02 complete: `asset_registry` table exists; asset lookups are available
- SP03 complete: `reference_values` table exists; symptom codes, origin types, and impact
  levels can be resolved by stable code
- Phase 1 auth module: `user_accounts` and `session_tokens` exist; `require_permission!`
  and `require_step_up!` macros are available in `src-tauri/src/middlewares/`

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Core Schema and State Machine | migration 017 + `domain.rs` |
| S2 | Queries and IPC Commands | `queries.rs` + `commands/di.rs` |
| S3 | Frontend Service and Store | `di-service.ts` + `di-store.ts` |

---

## Sprint S1 - Core Schema and State Machine

### AI Agent Prompt

```text
You are a senior Rust engineer working on Maintafox Desktop (Tauri 2 / SQLite / sqlx).
Implement the core DI migration and domain type layer.

STEP 1 - CREATE src-tauri/migrations/m20260401_000017_di_domain_core.rs

use sea_orm_migration::prelude::*;

Create two tables:

-- TABLE 1: intervention_requests --

CREATE TABLE intervention_requests (
  id                      INTEGER PRIMARY KEY AUTOINCREMENT,
  code                    TEXT    NOT NULL UNIQUE,           -- DI-0001 format
  -- Origin context
  asset_id                INTEGER NOT NULL REFERENCES asset_registry(id),
  sub_asset_ref           TEXT    NULL,
  org_node_id             INTEGER NOT NULL REFERENCES org_nodes(id),
  -- State
  status                  TEXT    NOT NULL DEFAULT 'submitted',
  -- Triage evidence
  title                   TEXT    NOT NULL,
  description             TEXT    NOT NULL,
  origin_type             TEXT    NOT NULL,
    -- operator / technician / inspection / pm / iot / quality / hse / production / external
  symptom_code_id         INTEGER NULL REFERENCES reference_values(id),
  -- Impact flags
  impact_level            TEXT    NOT NULL DEFAULT 'unknown',
    -- unknown / none / minor / major / critical
  production_impact       INTEGER NOT NULL DEFAULT 0,        -- 0|1
  safety_flag             INTEGER NOT NULL DEFAULT 0,        -- 0|1
  environmental_flag      INTEGER NOT NULL DEFAULT 0,        -- 0|1
  quality_flag            INTEGER NOT NULL DEFAULT 0,        -- 0|1
  -- Priority
  reported_urgency        TEXT    NOT NULL DEFAULT 'medium',
    -- low / medium / high / critical
  validated_urgency       TEXT    NULL,
  -- Timing (SLA origin)
  observed_at             TEXT    NULL,
  submitted_at            TEXT    NOT NULL,
  -- Review / approval tracking
  review_team_id          INTEGER NULL REFERENCES org_nodes(id),
  reviewer_id             INTEGER NULL REFERENCES user_accounts(id),
  screened_at             TEXT    NULL,
  approved_at             TEXT    NULL,
  deferred_until          TEXT    NULL,
  declined_at             TEXT    NULL,
  closed_at               TEXT    NULL,
  archived_at             TEXT    NULL,
  -- WO linkage (nullable until SP05)
  converted_to_wo_id      INTEGER NULL,
  converted_at            TEXT    NULL,
  -- Review decision fields
  reviewer_note           TEXT    NULL,
  classification_code_id  INTEGER NULL REFERENCES reference_values(id),
  -- Recurrence
  is_recurrence_flag      INTEGER NOT NULL DEFAULT 0,
  recurrence_di_id        INTEGER NULL REFERENCES intervention_requests(id),
  -- Concurrency
  row_version             INTEGER NOT NULL DEFAULT 1,
  -- Metadata
  submitter_id            INTEGER NOT NULL REFERENCES user_accounts(id),
  created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE INDEX idx_ir_status     ON intervention_requests(status);
CREATE INDEX idx_ir_asset      ON intervention_requests(asset_id);
CREATE INDEX idx_ir_org_node   ON intervention_requests(org_node_id);
CREATE INDEX idx_ir_submitter  ON intervention_requests(submitter_id);
CREATE INDEX idx_ir_reviewer   ON intervention_requests(reviewer_id);

-- TABLE 2: di_state_transition_log (append-only) --

CREATE TABLE di_state_transition_log (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  di_id       INTEGER NOT NULL REFERENCES intervention_requests(id),
  from_status TEXT    NOT NULL,
  to_status   TEXT    NOT NULL,
  action      TEXT    NOT NULL,
    -- submit / screen / return_for_clarification / reject / approve /
    --  defer / reactivate / convert / close_non_executable / archive
  actor_id    INTEGER NULL REFERENCES user_accounts(id),
  reason_code TEXT    NULL,
  notes       TEXT    NULL,
  acted_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE INDEX idx_dstl_di_id ON di_state_transition_log(di_id);


STEP 2 - CREATE src-tauri/src/di/mod.rs

pub mod domain;
pub mod queries;


STEP 3 - CREATE src-tauri/src/di/domain.rs

Implement:

A) `DiStatus` enum - exact PRD §6.4 states:
   Submitted, PendingReview, ReturnedForClarification, Rejected,
   Screened, AwaitingApproval, ApprovedForPlanning, Deferred,
   ConvertedToWorkOrder, ClosedAsNonExecutable, Archived

B) `DiStatus::as_str()` -> &'static str using snake_case values stored in DB.
   Values: submitted / pending_review / returned_for_clarification / rejected /
           screened / awaiting_approval / approved_for_planning / deferred /
           converted_to_work_order / closed_as_non_executable / archived

C) `DiStatus::try_from(s: &str)` returning Result<DiStatus, String>.

D) `DiStatus::allowed_transitions(&self) -> &'static [DiStatus]`
   EXACT transition table:
   submitted               -> [pending_review]
   pending_review          -> [screened, returned_for_clarification, rejected]
   returned_for_clarification -> [pending_review]
   screened                -> [awaiting_approval, rejected]
   awaiting_approval       -> [approved_for_planning, deferred, rejected]
   approved_for_planning   -> [converted_to_work_order, deferred, closed_as_non_executable]
   deferred                -> [awaiting_approval]
   converted_to_work_order -> [archived]
   closed_as_non_executable -> [archived]
   rejected                -> [archived]
   archived                -> []

E) `DiStatus::is_immutable_after_conversion(&self) -> bool`
   Returns true for ConvertedToWorkOrder, ClosedAsNonExecutable, Rejected, Archived.
   These statuses lock the DI from field edits (only commentary and attachments allowed).

F) `DiStatus::requires_step_up(&self) -> bool`
   Returns true for ApprovedForPlanning (approval action), ConvertedToWorkOrder (convert).

G) `DiOriginType` enum:
   Operator, Technician, Inspection, Pm, Iot, Quality, Hse, Production, External
   with as_str() and try_from(s: &str).

H) `DiUrgency` enum: Low, Medium, High, Critical with as_str() / try_from.

I) `DiImpactLevel` enum: Unknown, None, Minor, Major, Critical with as_str() / try_from.

J) `InterventionRequest` struct (matches all DDL columns; serde Serialize/Deserialize).

K) `DiTransitionInput` struct:
   di_id: i64, to_status: DiStatus, actor_id: i64, reason_code: Option<String>,
   notes: Option<String>

L) `guard_transition(from: &DiStatus, to: &DiStatus) -> Result<(), String>`
   Checks that to is in from.allowed_transitions(); returns error with both state
   names if disallowed.

M) `generate_di_code(pool: &SqlitePool) -> Result<String, sqlx::Error>`
   SELECT COALESCE(MAX(CAST(SUBSTR(code,4) AS INT)),0)+1 FROM intervention_requests
   WHERE code LIKE 'DI-%';
   Returns "DI-" + zero-padded 4-digit number.

ACCEPTANCE CRITERIA
- All 11 DiStatus variants implemented
- Transition table matches PRD §6.4 exactly (no additions, no omissions)
- guard_transition returns error for any non-listed transition
- ConvertedToWorkOrder and Archived have empty or no outbound transitions respectively
- is_immutable_after_conversion returns true only for terminal evidence states
```

### Supervisor Verification - Sprint S1

**V1 - Migration applies cleanly.**
Run `cargo test` with migration test. Both tables created with correct columns and indexes.

**V2 - State machine coverage.**
Unit test calling `guard_transition` for every state pair; all 11 forward transitions pass;
at least 5 invalid transitions (e.g., submitted → approved, archived → submitted) return Error.

**V3 - Code generation uniqueness.**
Create two DIs concurrently in a test; codes must be DI-0001 and DI-0002 with no duplicates.

**V4 - Immutability flag.**
`is_immutable_after_conversion` returns true for ConvertedToWorkOrder, ClosedAsNonExecutable,
Rejected, Archived; false for Submitted, PendingReview, Screened, AwaitingApproval,
ApprovedForPlanning, Deferred.

---

## Sprint S2 - Queries and IPC Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement DI queries and IPC commands.

STEP 1 - CREATE src-tauri/src/di/queries.rs

Implement the following functions using sqlx and &SqlitePool:

A) `list_intervention_requests(pool, filter: DiListFilter) -> Result<DiListPage>`
   DiListFilter: status: Option<Vec<String>>, asset_id: Option<i64>,
   org_node_id: Option<i64>, submitter_id: Option<i64>, reviewer_id: Option<i64>,
   origin_type: Option<String>, urgency: Option<String>,
   search: Option<String>, limit: i64, offset: i64
   Returns DiListPage { items: Vec<InterventionRequest>, total: i64 }
   Query joins asset_registry (asset code + label) and org_nodes (node label) for display.

B) `get_intervention_request(pool, id: i64) -> Result<Option<InterventionRequest>>`
   Full row by id.

C) `get_di_transition_log(pool, di_id: i64) -> Result<Vec<DiTransitionRow>>`
   DiTransitionRow: id, from_status, to_status, action, actor_id, reason_code, notes, acted_at.

D) `get_recent_similar_dis(pool, asset_id: i64, symptom_code_id: Option<i64>, days: i64)
   -> Result<Vec<DiSummaryRow>>`
   Find DIs on same asset with same symptom (if provided) in last `days` days.
   Used to surface recurrence context to reviewers.
   Returns up to 5 results ordered by submitted_at DESC.

E) `create_intervention_request(pool, input: DiCreateInput) -> Result<InterventionRequest>`
   DiCreateInput fields: asset_id, org_node_id, title, description, origin_type,
   symptom_code_id?, impact_level, production_impact, safety_flag, environmental_flag,
   quality_flag, reported_urgency, observed_at?, submitter_id
   Generates code via generate_di_code(), sets status = 'submitted', submitted_at = now.
   Writes di_state_transition_log row: from='none', to='submitted', action='submit'.

F) `update_di_draft_fields(pool, input: DiDraftUpdateInput) -> Result<InterventionRequest>`
   DiDraftUpdateInput: id, expected_row_version, title?, description?, symptom_code_id?,
   impact_level?, production_impact?, safety_flag?, environmental_flag?, quality_flag?,
   reported_urgency?, observed_at?
   Guard: only allow if status is 'submitted' or 'returned_for_clarification'.
   Increments row_version; updates updated_at.
   Returns error if expected_row_version does not match current.
   Uses: UPDATE ... WHERE id = ? AND row_version = ?; check rows_affected == 1.

STEP 2 - CREATE src-tauri/src/commands/di.rs

Register the following Tauri commands. All require the app State<AppState>.
Pull user identity from session as in prior commands.

A) `list_di` — requires `di.view`
   Input: DiListFilter (from frontend JSON)
   Output: DiListPage

B) `get_di` — requires `di.view`
   Input: id: i64
   Output: InterventionRequest + DiTransitionRow list + recent similar DIs

C) `create_di` — requires `di.create` or `di.create.own`
   Input: DiCreateInput
   Validation before insert:
   - title not empty
   - description not empty
   - origin_type is a valid DiOriginType
   - asset_id resolves in asset_registry
   - org_node_id resolves in org_nodes
   Output: InterventionRequest

D) `update_di_draft` — requires `di.create.own` (own DI) or `di.review` (any)
   Input: DiDraftUpdateInput
   Guard: reject if status not in [submitted, returned_for_clarification]
   Output: InterventionRequest

STEP 3 - PATCH src-tauri/src/main.rs (or lib.rs invoke handler)
   Register di module and di commands in invoke_handler list.

ACCEPTANCE CRITERIA
- cargo check passes with no errors
- list_di returns paginated results filtered by status, asset, org_node, search
- create_di validates required fields and returns structured error on missing fields
- update_di_draft returns error if status is not a draft state
- update_di_draft returns concurrent-edit error if row_version mismatches
- di_state_transition_log row is written on create
```

### Supervisor Verification - Sprint S2

**V1 - Create DI end-to-end.**
Submit a new DI via `create_di`; verify row in `intervention_requests` (status = submitted,
code = DI-0001, submitted_at set) and matching row in `di_state_transition_log`.

**V2 - Draft update guard.**
Attempt `update_di_draft` on a DI in status `screened`; must return error.

**V3 - Optimistic concurrency.**
Attempt `update_di_draft` with stale `expected_row_version`; must return error.

**V4 - Search filter.**
Create 3 DIs with distinct titles; call `list_di` with `search="unique-term"`; must return
only the matching DI.

**V5 - Recurrence query.**
Create 2 DIs on same asset + symptom within last 7 days; call `get_recent_similar_dis`;
must return both rows.

---

## Sprint S3 - Frontend Service and Store

### AI Agent Prompt

```text
You are a TypeScript engineer. Implement the DI frontend service and Zustand store.

CREATE src/services/di-service.ts

Import invoke from @tauri-apps/api/core.
Define and export all input/output types matching the Rust structs.

Types to define:
- DiStatus (string union of all 11 states)
- DiOriginType (string union)
- DiUrgency (string union)
- DiImpactLevel (string union)
- DiListFilter
- InterventionRequest
- DiTransitionRow
- DiListPage
- DiCreateInput
- DiDraftUpdateInput
- DiDetailPayload: { di: InterventionRequest; transitions: DiTransitionRow[]; similar: DiSummaryRow[] }

Functions (all async, all use invoke):
- listDis(filter: DiListFilter): Promise<DiListPage>
- getDi(id: number): Promise<DiDetailPayload>
- createDi(input: DiCreateInput): Promise<InterventionRequest>
- updateDiDraft(input: DiDraftUpdateInput): Promise<InterventionRequest>

All functions must validate the response with a Zod schema (import zod).
Map Tauri error strings to typed Error objects.

CREATE src/stores/di-store.ts

Use Zustand (import { create } from 'zustand').

State shape:
- items: InterventionRequest[]
- total: number
- activeDi: DiDetailPayload | null
- filter: DiListFilter
- loading: boolean
- saving: boolean
- error: string | null

Actions:
- setFilter(filter: Partial<DiListFilter>): void — merges into current filter
- loadDis(): Promise<void> — calls listDis, sets items + total
- openDi(id: number): Promise<void> — calls getDi, sets activeDi
- submitNewDi(input: DiCreateInput): Promise<InterventionRequest> — calls createDi,
  then reloads list
- updateDraft(input: DiDraftUpdateInput): Promise<void> — calls updateDiDraft,
  refreshes activeDi

ACCEPTANCE CRITERIA
- pnpm typecheck passes with no errors
- All 4 service functions match Rust command names exactly (snake_case invoke)
- Zod validation is implemented for InterventionRequest shape
- Store sets loading=true before any async call and loading=false in finally block
- error is cleared on successful load
```

### Supervisor Verification - Sprint S3

**V1 - Type assignment.**
Assigning a string `"invalid_status"` to `DiStatus` type must produce a TypeScript error.

**V2 - Zod validation fires.**
Simulate a response missing `code` field from the mock; Zod parse must throw.

**V3 - Store filter merge.**
Call `setFilter({ status: ["submitted"] })` then `setFilter({ asset_id: 5 })`;
resulting filter must include both fields.

**V4 - Loading state.**
Intercept `loadDis()`; verify `loading = true` while awaiting and `loading = false` after.

---

## Sprint S4 — DI Create / Edit Form and Page-Level UI

> **Gap addressed:** The web app provides a full create/edit modal with 3 sections
> (requester info, equipment picker, general information + description), a "New DI"
> button in the page header, and a list context menu. S1–S3 delivered the backend,
> services, and store but no form or page-level UI component. This sprint closes
> that gap.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/di/DiCreateForm.tsx` | Multi-section create / edit form rendered inside a Dialog |
| `src/components/di/DiFormDialog.tsx` | Dialog wrapper that hosts DiCreateForm with open/close controls |
| `src/pages/RequestsPage.tsx` (patch) | "New DI" button in page header; opens DiFormDialog |
| `src/stores/di-store.ts` (patch) | `openCreateForm` / `closeCreateForm` state + `submitNewDi` wiring |
| `src/i18n/locale-data/fr/di.json` (patch) | Form labels, placeholders, validation messages |
| `src/i18n/locale-data/en/di.json` (patch) | Form labels, placeholders, validation messages |

### DiCreateForm.tsx — Component Specification

**Props:**

```ts
interface DiCreateFormProps {
  /** Pre-filled DI data when editing a draft. null = create mode. */
  initial: InterventionRequest | null;
  /** Called on successful submit. */
  onSubmitted: (di: InterventionRequest) => void;
  /** Called when user cancels. */
  onCancel: () => void;
}
```

**Layout:** Single scrollable form with 3 visual sections (inspired by web intake form).

**Section 1 — Requester** (read-only, auto-populated from session user)
- Full name (display only)
- Position / title (display only)
- Department / entity (display only)

**Section 2 — Equipment Selection** (required)
- Equipment combobox with search (loads from `list_assets` service)
  - Displays: asset code, designation, location, family
  - On select: auto-populate `asset_id` and show equipment info card
- "Clear" button to deselect

**Section 3 — General Information**

| Field | Control | Required | Max len | Notes |
|-------|---------|----------|---------|-------|
| `title` | Input | ✓ | 100 | Brief problem summary |
| `origin_type` | Select | ✓ | — | 9 values: operator, technician, inspection, pm, iot, quality, hse, production, external |
| `reported_urgency` | Select | ✓ | — | 4 levels: low, medium, high, critical |
| `impact_level` | Select | ✓ | — | 5 levels: unknown, none, minor, major, critical |
| `description` | Textarea | ✓ | 1000 | Detailed problem narrative |
| `observed_at` | Date-time picker | ✗ | — | When the problem was first noticed |
| `safety_flag` | Checkbox | ✗ | — | |
| `environmental_flag` | Checkbox | ✗ | — | |
| `quality_flag` | Checkbox | ✗ | — | |
| `production_impact` | Checkbox | ✗ | — | |
| `notes` | Textarea | ✗ | 2000 | Additional context / constraints |

**Validation (client-side):**
- `title`, `description`, `origin_type`, `reported_urgency`, `impact_level` are required;
  show inline errors on blur.
- `asset_id` / `org_node_id` required — show error if no equipment selected.
- Max-length enforced with character counter.
- Submit button disabled until all required fields pass.

**Behaviour:**
- Create mode: calls `submitNewDi(input)` → on success calls `onSubmitted(di)`.
- Edit mode (`initial != null`): calls `updateDraft(input)` → on success calls `onSubmitted(di)`.
- Uses `saving` flag from store to show spinner and disable submit button.
- On error, shows inline toast (not blocking modal).

### DiFormDialog.tsx — Dialog Wrapper

Follows UX-DW-001 pattern (`docs/UX_DETAIL_DIALOG_PATTERN.md`).

```
<Dialog open={open} onOpenChange>
  <DialogContent className="max-w-2xl max-h-[90vh]">
    <DialogHeader>
      <DialogTitle>{create ? t("page.titleNew") : t("page.titleEdit")}</DialogTitle>
    </DialogHeader>
    <DiCreateForm initial={...} onSubmitted={...} onCancel={...} />
  </DialogContent>
</Dialog>
```

### RequestsPage.tsx Patch

Add a "New DI" button next to the Refresh button in the page header:

```tsx
<Button onClick={() => openCreateForm()} className="gap-1.5">
  <Plus className="h-3.5 w-3.5" />
  {t("action.create")}
</Button>
```

Render `<DiFormDialog />` controlled by `showCreateForm` state from di-store.

### di-store.ts Patch

Add state:
- `showCreateForm: boolean` (default false)
- `editingDi: InterventionRequest | null` (null = create, non-null = edit)

Add actions:
- `openCreateForm(di?: InterventionRequest)` → `set({ showCreateForm: true, editingDi: di ?? null })`
- `closeCreateForm()` → `set({ showCreateForm: false, editingDi: null })`

### i18n Additions (both FR and EN)

```json
{
  "action": {
    "create": "Nouvelle DI" / "New Request"
  },
  "form": {
    "section": {
      "requester": "Déclarant" / "Requester",
      "equipment": "Équipement" / "Equipment",
      "general": "Informations générales" / "General Information"
    },
    "equipmentSearch": "Rechercher un équipement..." / "Search equipment...",
    "observedAt": { "label": "Date de constatation" / "Observation date" },
    "originType": { "label": "Origine" / "Origin" },
    "impactLevel": { "label": "Niveau d'impact" / "Impact level" },
    "safetyFlag": { "label": "Enjeu sécurité" / "Safety concern" },
    "environmentalFlag": { "label": "Enjeu environnemental" / "Environmental concern" },
    "qualityFlag": { "label": "Enjeu qualité" / "Quality concern" },
    "productionImpact": { "label": "Impact production" / "Production impact" },
    "notes": { "label": "Notes complémentaires" / "Additional notes", "placeholder": "Contexte, contraintes..." / "Context, constraints..." },
    "validation": {
      "titleRequired": "Le titre est requis" / "Title is required",
      "descriptionRequired": "La description est requise" / "Description is required",
      "equipmentRequired": "L'équipement est requis" / "Equipment is required",
      "originRequired": "L'origine est requise" / "Origin is required",
      "urgencyRequired": "La priorité est requise" / "Priority is required",
      "impactRequired": "Le niveau d'impact est requis" / "Impact level is required"
    }
  }
}
```

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- DiCreateForm renders all 3 sections with correct fields
- Submit with missing required fields shows inline validation errors
- Successful create calls submitNewDi and closes the dialog
- Edit mode pre-fills all fields from initial DI
- Equipment combobox filters assets on typing (debounced 300ms)
- "New DI" button visible in RequestsPage header (requires di.create or di.create.own)
- Character counter displayed for title (100) and description (1000)
```

### Supervisor Verification — Sprint S4

**V1 — Required field validation.**
Leave title empty; click submit; verify inline error appears and submit is blocked.

**V2 — Equipment combobox search.**
Type 3 characters; verify filtered list renders; select item; verify equipment
info card appears.

**V3 — Edit mode pre-fill.**
Open form with an existing draft DI; all fields must be pre-filled;
submit calls `updateDraft` (not `submitNewDi`).

**V4 — Permission gate.**
User WITHOUT di.create permission; "New DI" button must not render.

---

*End of Phase 2 - Sub-phase 04 - File 01*
