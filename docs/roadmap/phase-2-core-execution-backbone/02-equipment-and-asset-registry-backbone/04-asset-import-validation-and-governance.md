# Phase 2 - Sub-phase 02 - File 04
# Asset Import, Validation, and Governance

## Context and Purpose

Files 01 to 03 established the data contracts and operational UI for governed asset
management. File 04 closes sub-phase 02 by implementing controlled import pathways,
validation gates, and auditability.

This is where the `eq.import` permission domain becomes operational. The objective is to
allow bulk onboarding and synchronization without compromising data quality, identity
stability, or historical trace integrity.

## Architecture Rules Applied

- **Import is staged, not direct-write.** Files are parsed into staging tables and
	validated before commit.
- **Idempotent upsert by external key policy.** Import behavior is deterministic and
	replay-safe.
- **No silent reclassification drift.** Changes to class/family/criticality from import
	are flagged and require explicit policy handling.
- **Conflict classes are explicit.** Duplicate code, org mismatch, hierarchy cycle risk,
	and forbidden decommission transitions are distinct error categories.
- **Audit every import batch.** Summary, actor, source file hash, validation counts,
	and apply result are recorded.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000012_asset_import_and_audit.rs` | Import batch, staging row, and import event tables |
| `src-tauri/src/assets/import.rs` | Parse/validate/preview/apply pipeline |
| `src-tauri/src/assets/governance.rs` | Validation policy engine and conflict classifier |
| `src-tauri/src/commands/assets.rs` (patch) | Import preview/apply/list batch commands |
| `src/services/asset-import-service.ts` | Frontend wrappers for import workflow |
| `src/stores/asset-import-store.ts` | Import wizard state and conflict resolution state |
| `src/pages/assets/AssetImportPage.tsx` | Import and validation workspace |

## Prerequisites

- Files 01 to 03 complete
- `eq.import` permission seeded and assigned appropriately
- Lookup governance active for class/family/criticality/status domains

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Import Staging and Validation Pipeline | migration 012, import parser, validation preview |
| S2 | Apply Engine, Idempotency, and Audit | apply command, conflict policy, import audit rows |
| S3 | Import UI Workflow and Governance Controls | import page/store, preview table, apply controls |

---

## Sprint S1 - Import Staging and Validation Pipeline

### AI Agent Prompt

```text
You are a senior Rust engineer. Build staged import validation for assets.

STEP 1 - CREATE migration m20260401_000012_asset_import_and_audit.rs

Add tables:

`asset_import_batches`
- id (PK)
- source_filename (TEXT NOT NULL)
- source_sha256 (TEXT NOT NULL)
- initiated_by_id (INTEGER NULL)
- status (TEXT NOT NULL) // uploaded/validated/applied/failed/cancelled
- total_rows (INTEGER NOT NULL DEFAULT 0)
- valid_rows (INTEGER NOT NULL DEFAULT 0)
- warning_rows (INTEGER NOT NULL DEFAULT 0)
- error_rows (INTEGER NOT NULL DEFAULT 0)
- created_at (TEXT NOT NULL)
- updated_at (TEXT NOT NULL)

`asset_import_staging`
- id (PK)
- batch_id (INTEGER NOT NULL)
- row_no (INTEGER NOT NULL)
- raw_json (TEXT NOT NULL)
- normalized_asset_code (TEXT NULL)
- normalized_external_key (TEXT NULL)
- validation_status (TEXT NOT NULL) // valid/warning/error
- validation_messages_json (TEXT NOT NULL)
- proposed_action (TEXT NULL) // create/update/skip/conflict

`asset_import_events`
- id (PK)
- batch_id (INTEGER NOT NULL)
- event_type (TEXT NOT NULL)
- summary_json (TEXT NULL)
- created_by_id (INTEGER NULL)
- created_at (TEXT NOT NULL)

STEP 2 - CREATE src-tauri/src/assets/governance.rs

Define conflict categories:
- `DuplicateAssetCode`
- `UnknownLookupCode`
- `OrgNodeMissing`
- `HierarchyCycleRisk`
- `ForbiddenStatusTransition`
- `ReclassificationRequiresReview`

Provide function:
- `validate_import_row(pool, normalized_row) -> ValidationOutcome`

STEP 3 - CREATE src-tauri/src/assets/import.rs

Functions:
- `create_import_batch(pool, filename, file_sha256, actor_id)`
- `stage_import_rows(pool, batch_id, rows)`
- `validate_import_batch(pool, batch_id)`
- `get_import_preview(pool, batch_id)`

ACCEPTANCE CRITERIA
- batch and staging rows created
- validation summary counts populate batch table
- preview returns row-level status and conflict classes
```

### Supervisor Verification - Sprint S1

**V1 - Batch metadata captured.**
Upload CSV and verify filename + SHA-256 stored in batch row.

**V2 - Validation counts.**
Run validation and confirm valid/warning/error counts are populated.

**V3 - Conflict classes visible.**
At least one intentionally bad row should show explicit conflict category.

---

## Sprint S2 - Apply Engine, Idempotency, and Audit

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement controlled apply from staging to registry.

STEP 1 - In assets/import.rs add `apply_import_batch(pool, batch_id, policy, actor_id)`

Rules:
- can only apply from `validated` state
- rows with `error` are skipped unless explicit override policy supports them
- upsert by deterministic key policy:
	- preferred external id key if present
	- fallback asset_code
- apply must be transactional per row and recorded in event log

STEP 2 - Add idempotency guard
- if same batch SHA and same source rows reapplied, no duplicate asset rows should be created

STEP 3 - Add audit recording
- insert summary event in `asset_import_events`
- include counts: created, updated, skipped, conflicted

STEP 4 - PATCH commands/assets.rs
Add commands:
- `create_asset_import_batch`
- `validate_asset_import_batch`
- `get_asset_import_preview`
- `apply_asset_import_batch`
- `list_asset_import_batches`

Permission rules:
- all import commands require `eq.import`

ACCEPTANCE CRITERIA
- apply creates/updates expected rows
- rerun does not duplicate unchanged rows
- import events capture apply summary
```

### Supervisor Verification - Sprint S2

**V1 - Idempotent replay.**
Apply same batch twice and verify no duplicate assets are created.

**V2 - Permission gate.**
User without `eq.import` cannot call import commands.

**V3 - Event summary.**
After apply, `asset_import_events` contains created/updated/skipped counts.

---

## Sprint S3 - Import UI Workflow and Governance Controls

### AI Agent Prompt

```text
You are a TypeScript and React engineer. Build import workflow UI and state.

STEP 1 - CREATE src/services/asset-import-service.ts
Expose typed wrappers for all import commands.

STEP 2 - CREATE src/stores/asset-import-store.ts
State:
- selected file metadata
- current batch id
- validation preview rows
- summary counts
- apply policy toggles
- loading/saving/error

Methods:
- `uploadAndCreateBatch`
- `validateBatch`
- `loadPreview`
- `applyBatch`
- `resetFlow`

STEP 3 - CREATE src/pages/assets/AssetImportPage.tsx
Workflow:
1. upload file
2. run validation
3. inspect conflict table
4. apply with policy options
5. view summary outcome

STEP 4 - add localization keys and warning copy
- explicit warnings for reclassification and decommission transitions

STEP 5 - add minimal tests
- summary rendering
- conflict table rendering
- apply button disabled when validation not complete

ACCEPTANCE CRITERIA
- typecheck passes
- import flow works end-to-end in dev
- blocked/conflict rows are clearly visible before apply
```

### Supervisor Verification - Sprint S3

**V1 - Guided flow enforcement.**
Apply button must remain disabled until validation step is complete.

**V2 - Conflict transparency.**
Conflict rows show category and message, not generic failure text.

**V3 - Outcome summary.**
After apply, UI shows created/updated/skipped counts matching backend event summary.

---

*End of Phase 2 - Sub-phase 02 - File 04*
