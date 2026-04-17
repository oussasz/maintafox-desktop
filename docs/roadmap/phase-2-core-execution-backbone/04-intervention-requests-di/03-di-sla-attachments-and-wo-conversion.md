# Phase 2 - Sub-phase 04 - File 03
# DI SLA, Attachments, and WO Conversion

## Context and Purpose

Files 01 and 02 established the domain model and the triage workflow. File 03 delivers
three remaining delivery areas that complete the operational DI module:

1. **SLA engine** — configurable target response and resolution times by urgency, origin type,
   and asset criticality. SLA breaches are computed and surfaced as system events.
2. **Attachment subsystem** — photos, sensor snapshots, PDFs, and other files attached to a
   DI at any point in its lifecycle. Files link to DIs immutably; they can be added but never
   overwritten by a different file under the same record.
3. **WO Conversion** — the stage 3 gate from PRD §6.4. A DI in `approved_for_planning` can
   be converted into a work order shell. After conversion the DI is locked as an immutable
   origin record. SP05 (Work Orders) will then fully populate and manage the WO record; the
   `source_di_id` FK on `work_orders` is the permanent traceability link.

These three areas together close the DI lifecycle and deliver the cross-module contracts
that SP05 and the analytics layers will consume.

---

## PRD Alignment Checklist

This file addresses PRD §6.4 requirements for:

- [x] Stage gate 3 (conversion): confirmed asset/location, classification, approved execution
      path required before converting
- [x] The request remains the immutable origin record once converted
- [x] Request-to-review, review-to-approval, and approval-to-conversion timings preserved
      (`screened_at`, `approved_at`, `converted_at` on DI record)
- [x] Photos, sensor snapshots, and free text support triage (attachment subsystem)
- [x] SLA and backlog analysis: SLA deadline, target response hours, actual response elapsed
      computable from stored timestamps
- [x] Work order receives `source_di_id` for permanent traceability (PRD §6.5 entity spec)

---

## Architecture Rules Applied

- **WO conversion is a single transaction.** DI status update, `converted_to_wo_id` and
  `converted_at` write, and WO shell insert all occur inside one sqlx transaction. Partial
  states are impossible.
- **DI becomes immutable after conversion.** After `converted_to_work_order` status, the
  only allowed writes are commentary (reviewer_note append) and new attachment uploads.
  All other field updates return an error.
- **Attachments are stored on the local filesystem under the app data directory.**
  The `di_attachments` table stores the relative path, not an absolute OS path. The Tauri
  app resolves the absolute path at runtime using the app data directory from `AppHandle`.
  Files are never deleted from disk when the attachment record is removed; they are orphaned
  and can be purged by an admin cleanup task.
- **SLA rules are reference data managed by an admin.** `di_sla_rules` rows are seeded with
  sensible defaults and can be updated by a user with `di.admin`. They are not in the
  `reference_values` system but follow the same governed-edit pattern.
- **WO shell creation is intentionally minimal.** SP04 creates only the fields needed for
  traceability: `source_di_id`, `asset_id`, `org_node_id`, `title`, `urgency`, `status =
  draft`. SP05 fills all planning, execution, and cost fields. This avoids SP04 taking a
  dependency on the full WO schema before SP05 is built.
- **source_di_id on work_orders is nullable in migration 017 and becomes populated in
  SP05.** SP04 writes a placeholder row format that SP05's migration will extend.

---

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000019_di_attachments_sla.rs` | `di_attachments` + `di_sla_rules` tables |
| `src-tauri/src/di/attachments.rs` | Attachment upload validation, save, list, and delete-record functions |
| `src-tauri/src/di/sla.rs` | SLA rule resolution, deadline computation, and breach detection |
| `src-tauri/src/di/conversion.rs` | WO conversion transaction: DI lock + WO shell creation |
| `src-tauri/src/commands/di.rs` (patch) | New IPC commands: upload_di_attachment, list_di_attachments, convert_di_to_wo, get_sla_status |
| `src/services/di-attachment-service.ts` | Attachment upload and list wrappers |
| `src/services/di-conversion-service.ts` | WO conversion command wrapper and result type |
| `src/components/di/DiAttachmentPanel.tsx` | Attachment list + upload drop zone UI component |
| `src/components/di/WoConversionModal.tsx` | Conversion confirmation modal with checklist |

---

## Prerequisites

- Files 01 and 02 complete
- Migration 017 and 018 applied
- `di.convert` and `di.admin` permissions exist in the system seed (seeded in SP06 or
  inline in this sprint)
- Step-up token validation available from Phase 1 auth module
- Tauri `AppHandle` accessible within commands for resolving the app data directory

---

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | SLA Engine and Attachment Schema | migration 019 + `sla.rs` + `attachments.rs` |
| S2 | WO Conversion Transaction and Commands | `conversion.rs` + `commands/di.rs` patch |
| S3 | Attachment Panel and Conversion Modal UI | React components |

---

## Sprint S1 - SLA Engine and Attachment Schema

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the DI SLA engine and attachment subsystem.

STEP 1 - CREATE src-tauri/migrations/m20260401_000019_di_attachments_sla.rs

use sea_orm_migration::prelude::*;

-- TABLE: di_attachments --

CREATE TABLE di_attachments (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  di_id            INTEGER NOT NULL REFERENCES intervention_requests(id),
  file_name        TEXT    NOT NULL,
  relative_path    TEXT    NOT NULL UNIQUE,   -- path relative to appDataDir/di_attachments/
  mime_type        TEXT    NOT NULL DEFAULT 'application/octet-stream',
  size_bytes       INTEGER NOT NULL DEFAULT 0,
  attachment_type  TEXT    NOT NULL DEFAULT 'other',
    -- photo / sensor_snapshot / pdf / other
  uploaded_by_id   INTEGER NULL REFERENCES user_accounts(id),
  uploaded_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  notes            TEXT    NULL
);

CREATE INDEX idx_da_di_id ON di_attachments(di_id);

-- TABLE: di_sla_rules --

CREATE TABLE di_sla_rules (
  id                        INTEGER PRIMARY KEY AUTOINCREMENT,
  name                      TEXT    NOT NULL,
  urgency_level             TEXT    NOT NULL,
    -- low / medium / high / critical
  origin_type               TEXT    NULL,    -- NULL = applies to all origins
  asset_criticality_class   TEXT    NULL,    -- NULL = applies to all classes
  target_response_hours     INTEGER NOT NULL,   -- hours until first review expected
  target_resolution_hours   INTEGER NOT NULL,   -- hours from submit to WO conversion expected
  escalation_threshold_hours INTEGER NOT NULL,  -- hours after which escalation fires
  is_active                 INTEGER NOT NULL DEFAULT 1
);

-- Seed default SLA rules --
INSERT INTO di_sla_rules (name, urgency_level, target_response_hours,
  target_resolution_hours, escalation_threshold_hours) VALUES
  ('Critical - All Origins',  'critical', 1,   8,   4),
  ('High - All Origins',      'high',     4,   24,  8),
  ('Medium - All Origins',    'medium',   24,  72,  48),
  ('Low - All Origins',       'low',      72,  168, 120);


STEP 2 - PATCH src-tauri/src/di/mod.rs
  Add: pub mod attachments;
       pub mod sla;


STEP 3 - CREATE src-tauri/src/di/sla.rs

Types:
- `DiSlaRule` struct matching di_sla_rules DDL
- `DiSlaStatus` struct:
    rule_id: Option<i64>,
    target_response_hours: Option<i64>,
    target_resolution_hours: Option<i64>,
    sla_deadline: Option<String>,          // ISO datetime = submitted_at + target_response_hours
    response_elapsed_hours: Option<f64>,   // submitted_at → screened_at
    resolution_elapsed_hours: Option<f64>, // submitted_at → converted_at or now
    is_response_breached: bool,
    is_resolution_breached: bool

Functions:

A) `resolve_sla_rule(pool, urgency: &str, origin_type: &str, criticality_class: Option<&str>)
   -> Result<Option<DiSlaRule>>`
   SELECT from di_sla_rules WHERE urgency_level = ? and is_active = 1.
   Priority: exact match (urgency + origin + criticality) > partial (urgency + origin, NULL class)
   > broad (urgency only, NULL origin, NULL class).
   Return the most specific matching active rule or None.

B) `compute_sla_status(pool, di: &InterventionRequest) -> Result<DiSlaStatus>`
   1. resolve_sla_rule for di.reported_urgency, di.origin_type, and optional criticality
      from asset_registry (join asset_criticality_class).
   2. Compute sla_deadline = submitted_at + target_response_hours.
   3. Compute response_elapsed_hours = screened_at.unwrap_or(now) − submitted_at.
   4. Compute resolution_elapsed_hours = converted_at.unwrap_or(now) − submitted_at.
   5. is_response_breached = response_elapsed_hours > target_response_hours AND screened_at is None.
   6. is_resolution_breached = resolution_elapsed_hours > target_resolution_hours AND converted_at is None.
   7. Return DiSlaStatus.

C) `list_sla_rules(pool) -> Result<Vec<DiSlaRule>>`
   SELECT all from di_sla_rules.

D) `update_sla_rule(pool, input: SlaRuleUpdateInput) -> Result<DiSlaRule>`
   Requires: name, urgency_level, target_response_hours, target_resolution_hours,
   escalation_threshold_hours, is_active.
   Requires di.admin role check performed in the command layer, not here.


STEP 4 - CREATE src-tauri/src/di/attachments.rs

Types:
- `DiAttachment` struct matching di_attachments DDL

Functions:

A) `save_di_attachment(pool, app_data_dir: &Path, input: DiAttachmentInput) -> Result<DiAttachment>`
   DiAttachmentInput: di_id, file_name, file_bytes: Vec<u8>, mime_type, attachment_type, notes?,
   uploaded_by_id.
   Logic:
   1. Verify DI exists and status is not Archived (attachments allowed on all non-archived states).
   2. Generate relative_path: "di_attachments/{di_id}/{uuid}-{file_name}"
   3. Compute absolute path: app_data_dir.join(&relative_path)
   4. Create directories if missing (std::fs::create_dir_all).
   5. Write bytes to disk.
   6. INSERT into di_attachments.
   7. Return DiAttachment.

B) `list_di_attachments(pool, di_id: i64) -> Result<Vec<DiAttachment>>`
   SELECT from di_attachments WHERE di_id = ? ORDER BY uploaded_at DESC.

C) `delete_di_attachment_record(pool, attachment_id: i64, actor_id: i64) -> Result<()>`
   DELETE from di_attachments WHERE id = ?. Does NOT delete the file from disk.
   Caller (command layer) must hold di.admin to invoke this.

ACCEPTANCE CRITERIA
- migration 019 applies cleanly (both tables + seed data)
- compute_sla_status returns is_response_breached = true when screened_at is NULL and
  elapsed > target_response_hours
- resolve_sla_rule returns critical rule for urgency = critical, not medium rule
- save_di_attachment creates directories and writes file before inserting row
- save_di_attachment returns error if DI does not exist
```

### Supervisor Verification - Sprint S1

**V1 - SLA rule resolution priority.**
Create a rule for urgency=high + origin=iot (target 2h) and a broad rule for high (target 4h).
Call `resolve_sla_rule` with urgency=high + origin=iot; must return the 2h rule, not the 4h.

**V2 - SLA breach flag.**
Set submitted_at 6 hours ago on a DI with a critical rule (target_response_hours = 1) and
screened_at = NULL; `compute_sla_status` must return is_response_breached = true.

**V3 - Attachment write path.**
Upload a small attachment; verify file exists at app_data_dir/di_attachments/{di_id}/{file};
verify row in di_attachments with matching relative_path.

**V4 - Attachment on archived DI.**
Attempt `save_di_attachment` on an archived DI; must return error.

---

## Sprint S2 - WO Conversion Transaction and Commands

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the WO conversion transaction and command layer.

STEP 1 - CREATE src-tauri/src/di/conversion.rs

Types:
- `WoConversionInput`:
    di_id: i64,
    actor_id: i64,
    expected_row_version: i64,
    step_up_token: String,
    conversion_notes: Option<String>

- `WoConversionResult`:
    di: InterventionRequest,      // updated DI with converted status
    wo_id: i64,                   // new WO id
    wo_code: String               // new WO code (e.g., WOR-0001)

Functions:

`convert_di_to_work_order(pool, input: WoConversionInput) -> Result<WoConversionResult>`

Logic inside a single sqlx transaction:

1. Load DI; call guard_transition(current_status, &DiStatus::ConvertedToWorkOrder).
   Return error if fails.

2. Validate step_up_token via validate_step_up_token(pool, &input.step_up_token, input.actor_id).
   Return PermissionDenied if invalid.

3. Validate conversion prerequisites:
   - DI must have asset_id set (not NULL)
   - DI must have classification_code_id set (not NULL)
   Error descriptions must be specific: "Asset context required", "Classification required".

4. Generate WO code:
   SELECT COALESCE(MAX(CAST(SUBSTR(code,5) AS INT)),0)+1
   FROM work_orders WHERE code LIKE 'WOR-%';
   Format as "WOR-" + zero-padded 4 digits.
   NOTE: If work_orders table does not yet exist (SP05 not built), INSERT into a
   minimal `work_order_stubs` table with same schema subset. This table will be
   replaced by SP05's full work_orders table. Use CREATE TABLE IF NOT EXISTS inside
   migration 019 for `work_order_stubs`:
     id INTEGER PK, code TEXT UNIQUE, source_di_id INTEGER, asset_id INTEGER,
     org_node_id INTEGER, title TEXT, urgency TEXT, status TEXT DEFAULT 'draft',
     created_at TEXT DEFAULT (strftime(...))

5. INSERT into work_order_stubs (or work_orders if it exists via SP05):
   code = generated WO code, source_di_id = di.id, asset_id = di.asset_id,
   org_node_id = di.org_node_id, title = di.title, urgency = di.validated_urgency
   (fallback to reported_urgency), status = 'draft', created_at = now.

6. UPDATE intervention_requests SET
     status = 'converted_to_work_order',
     converted_to_wo_id = new_wo_id,
     converted_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
     row_version = row_version + 1,
     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
   WHERE id = ? AND row_version = ?; check rows_affected == 1.

7. INSERT into di_state_transition_log:
   di_id, from = 'approved_for_planning', to = 'converted_to_work_order',
   action = 'convert', actor_id, notes = conversion_notes.

8. INSERT into di_review_events:
   event_type = 'converted', step_up_used = 1.

9. Commit transaction. Return WoConversionResult.


STEP 2 - PATCH src-tauri/src/di/mod.rs
  Add: pub mod conversion;


STEP 3 - PATCH src-tauri/src/commands/di.rs

Add the following commands:

A) `upload_di_attachment`
   Permission: di.create.own (own DI) OR di.review (any)
   Input: di_id: i64, file_name: String, file_bytes: Vec<u8>, mime_type: String,
          attachment_type: String, notes: Option<String>
   Delegates to: attachments::save_di_attachment
   Gets app_data_dir from AppHandle: app.path().app_data_dir()

B) `list_di_attachments`
   Permission: di.view
   Input: di_id: i64
   Delegates to: attachments::list_di_attachments

C) `delete_di_attachment`
   Permission: di.admin
   Input: attachment_id: i64
   Delegates to: attachments::delete_di_attachment_record

D) `convert_di_to_wo`
   Permission: di.convert
   Input: WoConversionInput (includes step_up_token)
   Delegates to: conversion::convert_di_to_work_order
   Returns: WoConversionResult

E) `get_sla_status`
   Permission: di.view
   Input: di_id: i64
   Loads DI then calls sla::compute_sla_status
   Returns: DiSlaStatus

F) `list_sla_rules`
   Permission: di.view
   Delegates to: sla::list_sla_rules

G) `update_sla_rule`
   Permission: di.admin
   Input: SlaRuleUpdateInput
   Delegates to: sla::update_sla_rule

ACCEPTANCE CRITERIA
- cargo check passes
- convert_di_to_wo writes DI update, WO stub, state log, and review event in one transaction
- upload_di_attachment receives file bytes and writes to disk before inserting DB row
- get_sla_status returns is_response_breached correctly
- list_sla_rules returns default seeded rules
```

### Supervisor Verification - Sprint S2

**V1 - Conversion atomicity.**
Simulate a DB error after WO stub insert but before DI update; confirm transaction rollback
(DI still in approved_for_planning, no WO stub row).

**V2 - Conversion step-up guard.**
Call `convert_di_to_wo` with invalid step_up_token; must return PermissionDenied.

**V3 - Conversion missing classification.**
Attempt conversion on a DI with classification_code_id = NULL; must return descriptive error.

**V4 - WO stub created.**
Successful conversion; verify row in work_order_stubs with source_di_id = DI.id and
status = 'draft'.

**V5 - DI locked after conversion.**
Attempt `update_di_draft` on a converted DI; must return error (immutable state).

---

## Sprint S3 - Attachment Panel and Conversion Modal UI

### AI Agent Prompt

```text
You are a TypeScript / React engineer. Build the DI attachment panel and WO conversion modal.

CREATE src/services/di-attachment-service.ts

Types:
- DiAttachment (matches di_attachments DDL)
- DiAttachmentUploadInput: { diId: number; fileName: string; fileBytes: number[];
    mimeType: string; attachmentType: 'photo'|'sensor_snapshot'|'pdf'|'other'; notes?: string }

Functions:
- uploadDiAttachment(input: DiAttachmentUploadInput): Promise<DiAttachment>
  Before invoking, convert File object to Uint8Array and then Array<number> for Tauri.
- listDiAttachments(diId: number): Promise<DiAttachment[]>
- deleteDiAttachment(attachmentId: number): Promise<void>

CREATE src/services/di-conversion-service.ts

Types:
- WoConversionInput: { diId: number; expectedRowVersion: number; stepUpToken: string;
    conversionNotes?: string }
- WoConversionResult: { di: InterventionRequest; woId: number; woCode: string }

Functions:
- convertDiToWo(input: WoConversionInput): Promise<WoConversionResult>
- getSlaStatus(diId: number): Promise<DiSlaStatus>
- listSlaRules(): Promise<DiSlaRule[]>

CREATE src/components/di/DiAttachmentPanel.tsx

Props: diId: number; canUpload: boolean; canDelete: boolean

Behavior:
- On mount, call listDiAttachments and render list with thumbnail for photos,
  icon for other types, file name, upload date, and uploader name.
- Drop zone: accepts drag-and-drop or click-to-browse; reads File via FileReader as
  ArrayBuffer; converts to number[] for uploadDiAttachment call.
- Show upload progress state (uploading=true) while invoke is pending.
- Show delete button only when canDelete = true; confirm before calling deleteDiAttachment.
- On upload or delete success, reload list.
- Max file size validation: 20 MB; display inline error if exceeded.

CREATE src/components/di/WoConversionModal.tsx

Props: di: InterventionRequest; onConverted: (result: WoConversionResult) => void; onClose: () => void

Behavior:
- Show a pre-conversion checklist derived from DI fields:
    [x] Asset confirmed: {asset_id is set}
    [x] Classification set: {classification_code_id is set}
    [x] Urgency validated: {validated_urgency is set}
- If any checklist item is false, show it as a blocker and disable the Convert button.
- Step-up token input: prompt user to enter step-up PIN (passes raw string to backend;
  backend validates — no client-side PIN verification).
- "Convert to Work Order" button:  calls convertDiToWo; on success calls onConverted(result)
  with navigation hint to the new WO code.
- Show saving spinner while converting; display error if conversion fails.

ACCEPTANCE CRITERIA
- pnpm typecheck passes
- DiAttachmentPanel converts File to number[] correctly before invoke
- WoConversionModal disables Convert button when asset_id or classification_code_id is null
- convertDiToWo result includes woCode for UI navigation
- Max 20MB file validation fires before any invoke call
```

### Supervisor Verification - Sprint S3

**V1 - Checklist blocker.**
Open WoConversionModal with a DI missing `classification_code_id`; Convert button must be
disabled and checklist item marked as incomplete.

**V2 - File size guard.**
Attempt to drag a 25 MB file into DiAttachmentPanel; must show inline error without calling
uploadDiAttachment.

**V3 - Post-conversion feedback.**
Successful conversion; modal must call `onConverted` with `woCode = "WOR-0001"` visible to
the caller for navigation.

**V4 - typecheck.**
Run `pnpm typecheck`; zero errors in the two new service files and two new component files.

---

## Sprint S4 — Lookup Managers and SLA Rules Admin Panel

> **Gap addressed:** The web app provides inline lookup-manager modals for failure
> modes and production impacts (with suggestion chips and CRUD). It also lets admins
> manage SLA rules via UI. S1–S3 delivered `update_sla_rule` and `list_sla_rules`
> commands but no admin UI. This sprint closes that gap.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/di/DiLookupManagerDialog.tsx` | Reusable inline lookup-manager modal for DI reference values (failure modes, production impacts) |
| `src/components/di/DiSlaRulesPanel.tsx` | Admin panel for viewing / editing SLA rules (requires `di.admin`) |
| `src/pages/RequestsPage.tsx` (patch) | Gear icon in header → opens SLA rules panel (visible to `di.admin`) |
| `src/components/di/DiCreateForm.tsx` (patch) | "Manage" buttons next to failure mode & production impact selectors that open DiLookupManagerDialog |
| `src/i18n/locale-data/{fr,en}/di.json` (patch) | Lookup labels, SLA labels |

### DiLookupManagerDialog.tsx — Reference Value CRUD

Reusable for any reference_values domain (failure_mode, production_impact, symptom, etc.).

**Props:**

```ts
interface DiLookupManagerDialogProps {
  open: boolean;
  onClose: () => void;
  domain: string;         // e.g. "failure_mode", "production_impact"
  title: string;          // Dialog header text
  onValueSelected?: (id: number) => void;  // Optional: pick mode for form integration
}
```

**Layout (split):**
- **Left pane:** Existing items list with edit/delete buttons per row
- **Right pane:** Create/edit form:
  - `name` input (required, max 100)
  - `description` textarea (optional, max 500)
  - Suggestion chips (pre-defined common values for quick fill)
  - Character counters
  - Save / Cancel buttons

**Behaviour:**
- Create → calls reference_values CRUD (via reference service, already built in SP03)
- Edit → pre-fills form, calls update
- Delete → confirmation dialog, calls delete
- On item click (pick mode) → calls `onValueSelected(id)`, closes dialog

### DiSlaRulesPanel.tsx — SLA Configuration

Visible only to users with `di.admin` permission.

**Layout:** Table of SLA rules with inline edit:

| Column | Type | Notes |
|--------|------|-------|
| Rule name | Text | Read-only label |
| Urgency level | Badge | Color-coded |
| Target response (hours) | Editable number | |
| Target resolution (hours) | Editable number | |
| Escalation threshold (hours) | Editable number | |
| Active | Toggle switch | |
| Actions | Save button | Calls `update_sla_rule` |

**Data source:** `list_sla_rules` command (from S2).

### Acceptance Criteria

```
- pnpm typecheck passes
- DiLookupManagerDialog creates, edits, and deletes reference values
- Suggestion chips populate the name field on click
- DiSlaRulesPanel renders only for di.admin users
- SLA rule changes persist via update_sla_rule command
- "Manage" buttons in DiCreateForm open the lookup dialog for the correct domain
```

### Supervisor Verification — Sprint S4

**V1 — Lookup create.**
Open failure mode manager; create a new entry; verify it appears in the list.

**V2 — SLA admin gate.**
User WITHOUT `di.admin`: gear icon and SLA panel must not render.

**V3 — Suggestion chips.**
Click a suggestion chip; verify the name field is populated.

---

*End of Phase 2 - Sub-phase 04 - File 03*
