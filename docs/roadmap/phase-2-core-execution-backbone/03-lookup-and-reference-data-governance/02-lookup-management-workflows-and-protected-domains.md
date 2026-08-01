# Phase 2 - Sub-phase 03 - File 02
# Lookup Management Workflows and Protected Domains

## Context and Purpose

File 01 built the versioned data model. File 02 adds operational governance workflows:
draft editing, validation diagnostics, protected analytical domain controls, and safe
deactivation or migration behavior.

This is the point where the PRD distinction between ordinary local lists and protected
analytical semantics becomes enforceable.

## PRD Alignment Checklist

This file implements PRD 6.13 requirements for:

- protected analytical domains
- draft -> validate -> publish governance workflow
- usage explorer and impact preview basis
- merge and migration tools for in-use values

## Architecture Rules Applied

- **Protected analytical domains are constrained.** In-use values cannot be destructively
	deleted and must be deactivated or migrated.
- **Validation is explicit and reportable.** Set validation returns structured issues,
	not only pass/fail flags.
- **Migration maps preserve history.** Merge and replacement operations write mapping
	records so historical traceability is retained.
- **Domain policies are data-driven.** Protected behavior is determined by governance level
	plus domain-specific policy rules.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000014_reference_governance_maps.rs` | merge/migration map and validation report tables |
| `src-tauri/src/reference/validation.rs` | set validation engine |
| `src-tauri/src/reference/protected.rs` | protected-domain policy checks |
| `src-tauri/src/reference/migrations.rs` | value merge and migration map service |
| `src-tauri/src/commands/reference.rs` (patch) | validation and migration commands |
| `src/services/reference-governance-service.ts` | frontend wrappers for workflow and policy operations |

## Prerequisites

- File 01 complete
- Permission matrix includes `ref.publish`
- Modules using governed semantics expose usage probes incrementally

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Protected Domain Policy Layer | migration 014, `protected.rs` |
| S2 | Validation Workflow and Diagnostics | `validation.rs`, validation report contract |
| S3 | Merge and Migration Tools | `migrations.rs`, command integration |

---

## Sprint S1 - Protected Domain Policy Layer

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement policy controls for protected analytical domains.

STEP 1 - CREATE migration m20260401_000014_reference_governance_maps.rs

Create table `reference_value_migrations`:
- id (PK)
- domain_id (INTEGER NOT NULL)
- from_value_id (INTEGER NOT NULL)
- to_value_id (INTEGER NOT NULL)
- reason_code (TEXT NULL)
- migrated_by_id (INTEGER NULL)
- migrated_at (TEXT NOT NULL)

Create table `reference_validation_reports`:
- id (PK)
- set_id (INTEGER NOT NULL)
- status (TEXT NOT NULL) // passed/failed
- issue_count (INTEGER NOT NULL)
- blocking_count (INTEGER NOT NULL)
- report_json (TEXT NOT NULL)
- validated_by_id (INTEGER NULL)
- validated_at (TEXT NOT NULL)

STEP 2 - CREATE src-tauri/src/reference/protected.rs

Functions:
- `is_protected_domain(pool, domain_id) -> AppResult<bool>`
- `assert_can_deactivate_value(pool, value_id) -> AppResult<()>`
- `assert_can_delete_value(pool, value_id) -> AppResult<()>`

Rules:
- protected domain values cannot be hard deleted when used
- if used, require migration map or deactivation path

Usage checks (initial set):
- asset classes/families/criticality in asset registry
- work order type/urgency refs where available
- failure code domains where available

ACCEPTANCE CRITERIA
- protected-domain guard blocks forbidden delete
- non-protected domains can still use normal delete if not in use
```

### Supervisor Verification - Sprint S1

**V1 - Protected delete block.**
Try deleting in-use asset family value in protected domain. Must fail.

**V2 - Allowed non-protected delete.**
Delete unused value in tenant-managed non-protected domain. Should pass.

**V3 - Deactivate fallback.**
Protected in-use value can be deactivated only when policy allows.

---

## Sprint S2 - Validation Workflow and Diagnostics

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement set validation diagnostics.

CREATE src-tauri/src/reference/validation.rs

Types:
- `ReferenceValidationIssue`
- `ReferenceValidationResult`

Validation checks:
- duplicate codes in set
- missing labels
- hierarchy cycles
- orphan parent references
- invalid color format
- invalid external-code formatting for domains that require pattern
- protected-domain deletions without migration mapping

Functions:
- `validate_reference_set(pool, set_id, actor_id)`
- `get_latest_validation_report(pool, set_id)`

Store full issue list in `reference_validation_reports.report_json`.

PATCH lifecycle:
- set can only move to validated when blocking_count = 0

ACCEPTANCE CRITERIA
- validation returns detailed issues
- blocking issues prevent transition to validated/published
- validation reports are persisted
```

### Supervisor Verification - Sprint S2

**V1 - Duplicate code detection.**
Create duplicate code in draft set, run validate, verify blocking issue.

**V2 - Cycle detection.**
Create hierarchy cycle and verify blocking issue.

**V3 - Validation persistence.**
Check `reference_validation_reports` row after validation run.

---

## Sprint S3 - Merge and Migration Tools

### AI Agent Prompt

```text
You are a Rust and TypeScript engineer. Implement merge/migrate tools.

CREATE src-tauri/src/reference/migrations.rs

Functions:
- `merge_reference_values(pool, domain_id, from_value_id, to_value_id, actor_id)`
- `migrate_reference_usage(pool, domain_id, from_value_id, to_value_id, actor_id)`
- `list_reference_migrations(pool, domain_id, limit)`

Rules:
- source and target must belong to same domain
- target must be active
- migration writes row in `reference_value_migrations`
- source value becomes inactive after successful migration if policy allows

PATCH commands/reference.rs
- add merge/migrate/list migration endpoints

Permissions:
- merge/migrate require `ref.publish` + step-up

PATCH frontend service
- add migration/merge calls and typed responses

ACCEPTANCE CRITERIA
- migration map rows persist for each merge/migration operation
- source usage can be remapped without losing trace
- permission and step-up enforced for dangerous semantic changes
```

### Supervisor Verification - Sprint S3

**V1 - Migration map presence.**
Run merge and verify mapping row exists.

**V2 - Post-migration behavior.**
After migrate, source value appears inactive and target remains active.

**V3 - Dangerous action guard.**
Merge without step-up should fail.

---

## Sprint S4 — Web-Parity Gap Closure (Value Editor Table)

> **Scope** — Adds the primary editing surface for reference values. This component
> occupies the right pane of ReferenceManagerPage when a set is selected from the
> DomainBrowserPanel (both established in File 01 Sprint S4).

### S4‑1 — Value Editor Table (`ReferenceValueEditor.tsx`) — GAP REF‑03

```
LOCATION   src/components/lookups/ReferenceValueEditor.tsx
STORE      reference-governance-store.ts (patch — add values, editingValue, saveValue)
SERVICE    reference-service.ts + reference-governance-service.ts

DESCRIPTION
Right pane of ReferenceManagerPage. Shows all values for the selected reference set
in an editable DataTable:

  Header area:
    - Set name + domain breadcrumb
    - Version badge (draft / published / superseded)
    - "+ Add Value" button (ref.manage guard)
    - "Publish Set" button (ref.publish guard, disabled if not draft)

  Table columns:
  ┌──────┬──────────┬──────────┬──────────┬─────────┬─────────┬──────────┐
  │ Code │ Label FR │ Label EN │ Parent   │ Status  │ Usage   │ Actions  │
  ├──────┼──────────┼──────────┼──────────┼─────────┼─────────┼──────────┤
  │ FAM1 │ Pompes   │ Pumps    │ —        │ Active  │ 42      │ ✏️ 🗑️    │
  │ FAM2 │ Vannes   │ Valves   │ —        │ Active  │ 18      │ ✏️ 🗑️    │
  │ FAM3 │ Moteurs  │ Motors   │ FAM1     │ Draft   │ 0       │ ✏️ 🗑️    │
  └──────┴──────────┴──────────┴──────────┴─────────┴─────────┴──────────┘

  - Inline edit: click ✏️ → row switches to edit mode (text inputs replace labels)
  - Inline save: Enter or click checkmark; Escape to cancel
  - Add value: inserts empty row at top in edit mode
  - Delete: click 🗑️ → confirm dialog; blocked if usage > 0 (show "value in use
    by N records — deactivate instead"); deactivate option offered
  - Parent column: combobox selecting from other values in same set (hierarchy)
  - Usage column: shows count of records referencing this value (read-only)
  - Sortable by code, label, status
  - Pagination: 50 rows per page (matches DataTable default)
  - For protected analytical domains: edit actions trigger step-up auth

  Empty state (new set with no values):
    "No values yet. Click '+ Add Value' to start building this reference set."

ACCEPTANCE CRITERIA
- selecting a set in DomainBrowserPanel loads values in editor
- inline edit/save/cancel works
- delete blocked for in-use values (shows usage count)
- protected domain actions trigger step-up
- pagination works for large sets
```

### Supervisor Verification — Sprint S4

**V1 — Value CRUD.**
Select "Families" set. Add a new value with code and labels. Save. Verify it appears.
Edit its label. Delete it (if usage = 0).

**V2 — In-use protection.**
Try to delete a value with usage > 0. Verify deletion is blocked and deactivate is
offered instead.

**V3 — Protected domain guard.**
Edit a value in a protected analytical domain. Verify step-up authentication is required.

---

*End of Phase 2 - Sub-phase 03 - File 02*
