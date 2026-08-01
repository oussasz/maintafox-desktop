# Phase 2 - Sub-phase 03 - File 03
# Aliases, Imports, and Search Behavior

## Context and Purpose

File 03 operationalizes terminology stability. PRD 6.13 requires alias and legacy mapping
to keep search, migration, and integration stable when labels and codes evolve.

This file adds:

- alias and synonym governance
- import/export pathways with row-level diagnostics
- reference-aware search behavior that understands aliases and locale preferences

## PRD Alignment Checklist

This file addresses PRD 6.13 requirements for:

- `reference_aliases`
- alias types for legacy/import/search
- import/export templates with validation feedback
- search resilience during terminology transitions

## Architecture Rules Applied

- **Alias is explicit metadata.** Alias records are typed and locale-aware.
- **Preferred alias selection is deterministic.** At most one preferred alias per
	`(reference_value_id, locale, alias_type)`.
- **Import is template-governed.** Domain import format is explicit and validated.
- **Search ranks canonical then alias matches.** Exact code or canonical label wins,
	then preferred aliases, then legacy aliases.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000015_reference_aliases_and_imports.rs` | alias and import batch tables |
| `src-tauri/src/reference/aliases.rs` | alias CRUD and preferred-alias rules |
| `src-tauri/src/reference/imports.rs` | import/export service for reference domains |
| `src-tauri/src/reference/search.rs` | alias-aware search/read model |
| `src-tauri/src/commands/reference.rs` (patch) | alias/import/search commands |
| `src/services/reference-alias-service.ts` | frontend wrappers |
| `src/stores/reference-search-store.ts` | search state for lookup manager UI |

## Prerequisites

- Files 01 and 02 complete
- Import governance patterns from Phase 2 sub-phase 02 available as reference

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Alias Governance Service | migration 015, `aliases.rs` |
| S2 | Import and Export Pipelines | `imports.rs` |
| S3 | Alias-Aware Search Behavior | `search.rs`, service/store integration |

---

## Sprint S1 - Alias Governance Service

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement alias domain support.

STEP 1 - CREATE migration m20260401_000015_reference_aliases_and_imports.rs

Create table `reference_aliases`:
- id (PK)
- reference_value_id (INTEGER NOT NULL)
- alias_label (TEXT NOT NULL)
- locale (TEXT NOT NULL)
- alias_type (TEXT NOT NULL) // legacy/import/search
- is_preferred (INTEGER NOT NULL DEFAULT 0)
- created_at (TEXT NOT NULL)

Indexes:
- `idx_reference_aliases_value`
- unique `(reference_value_id, locale, alias_type, alias_label)`

STEP 2 - CREATE src-tauri/src/reference/aliases.rs

Functions:
- `list_aliases(pool, reference_value_id)`
- `create_alias(pool, payload, actor_id)`
- `update_alias(pool, alias_id, payload, actor_id)`
- `delete_alias(pool, alias_id, actor_id)`

Rules:
- alias label non-empty
- one preferred alias per `(reference_value_id, locale, alias_type)`
- deleting preferred alias should auto-promote another alias or clear preferred state

ACCEPTANCE CRITERIA
- alias uniqueness enforced
- preferred-alias rule enforced
```

### Supervisor Verification - Sprint S1

**V1 - Preferred alias uniqueness.**
Try marking two aliases preferred for same value/locale/type. Only one should remain preferred.

**V2 - Duplicate alias guard.**
Duplicate alias label in same scope should fail.

**V3 - Delete behavior.**
Delete preferred alias and verify deterministic fallback behavior.

---

## Sprint S2 - Import and Export Pipelines

### AI Agent Prompt

```text
You are a Rust engineer. Implement reference import/export support.

STEP 1 - In migration 015 add:

`reference_import_batches`
- id (PK)
- domain_id (INTEGER NOT NULL)
- source_filename (TEXT NOT NULL)
- source_sha256 (TEXT NOT NULL)
- status (TEXT NOT NULL) // uploaded/validated/applied/failed
- total_rows/valid_rows/warning_rows/error_rows
- initiated_by_id (INTEGER NULL)
- created_at (TEXT NOT NULL)
- updated_at (TEXT NOT NULL)

`reference_import_rows`
- id (PK)
- batch_id (INTEGER NOT NULL)
- row_no (INTEGER NOT NULL)
- raw_json (TEXT NOT NULL)
- normalized_code (TEXT NULL)
- validation_status (TEXT NOT NULL)
- messages_json (TEXT NOT NULL)
- proposed_action (TEXT NULL)

STEP 2 - CREATE src-tauri/src/reference/imports.rs

Functions:
- `create_import_batch`
- `stage_import_rows`
- `validate_import_batch`
- `apply_import_batch`
- `export_domain_set`

Rules:
- apply only from validated batches
- protected-domain changes route through policy checks
- preserve mapping trail for replaced codes

ACCEPTANCE CRITERIA
- import validation returns row-level diagnostics
- export includes canonical values and aliases
- apply updates target draft set deterministically
```

### Supervisor Verification - Sprint S2

**V1 - Row-level diagnostics.**
Import malformed rows and confirm row-specific error output.

**V2 - Protected-policy integration.**
Protected domain import conflicts should show governance failures.

**V3 - Export completeness.**
Export contains both canonical values and alias data.

---

## Sprint S3 - Alias-Aware Search Behavior

### AI Agent Prompt

```text
You are a Rust and TypeScript engineer. Implement reference search behavior.

CREATE src-tauri/src/reference/search.rs

Functions:
- `search_reference_values(pool, domain_code, query, locale, limit)`

Ranking rules:
1. exact canonical code match
2. exact canonical label match
3. preferred alias match in locale
4. other alias match in locale
5. fallback alias matches in other locales

Return fields:
- value id/code/label
- matched_text
- match_source (canonical_code/canonical_label/alias)
- alias_type if alias match

PATCH commands/reference.rs and shared types

CREATE src/stores/reference-search-store.ts
- query
- locale
- domain
- results
- loading/error

ACCEPTANCE CRITERIA
- alias-aware search returns stable ranked results
- locale-specific preferred aliases are favored
- search works when canonical labels changed but legacy aliases remain
```

### Supervisor Verification - Sprint S3

**V1 - Legacy alias continuity.**
Search using old term mapped as legacy alias. Correct value should still be found.

**V2 - Locale ranking.**
Preferred alias in current locale should rank above non-preferred aliases.

**V3 - Canonical precedence.**
Exact code match should always rank first.

---

## Sprint S4 — Web-Parity Gap Closure (Import Wizard & Alias Manager UI)

> **Scope** — File 03 built alias and import Rust services with no frontend
> surfaces. Sprint S4 adds the UI panels for both: a CSV import wizard integrated
> into ReferenceManagerPage, and an alias management panel per reference value.

### S4‑1 — Reference Import Wizard (`ReferenceImportWizard.tsx`) — GAP REF‑05

```
LOCATION   src/components/lookups/ReferenceImportWizard.tsx
SERVICE    reference-alias-service.ts (patch — add importValues IPC wrapper)

DESCRIPTION
Sheet / dialog opened from ReferenceManagerPage toolbar "Import" button (ref.manage
guard). Three-step wizard:

  Step 1 — Upload
    - drop zone or file picker (CSV / XLSX)
    - target domain + set selectors (pre-filled if set already selected)
    - "Next" button

  Step 2 — Map & Validate
    - column mapping table: source column → target field (code, label_fr, label_en,
      parent_code)
    - preview table showing first 10 rows with mapped values
    - validation diagnostics panel:
      ┌────────┬──────────────────────────────────────┐
      │ Row 3  │ ⚠️ Duplicate code "FAM1" — will skip │
      │ Row 7  │ ❌ Missing required field "code"      │
      │ Row 12 │ ⚠️ Parent "XXX" not found — orphaned │
      └────────┴──────────────────────────────────────┘
    - summary: N valid, N warnings, N errors
    - "Back" / "Import N valid rows" buttons

  Step 3 — Result
    - success/skip/error counts
    - downloadable error report (CSV)
    - "Done" button returns to editor

ACCEPTANCE CRITERIA
- CSV parsing handles UTF-8 with BOM
- column mapping is persisted per domain for repeat imports
- rows with errors are skipped, not imported
- error report is downloadable
```

### S4‑2 — Alias Manager Panel (`ReferenceAliasPanel.tsx`) — GAP REF‑06

```
LOCATION   src/components/lookups/ReferenceAliasPanel.tsx
SERVICE    reference-alias-service.ts

DESCRIPTION
Sub-panel within ReferenceValueEditor — opened when clicking a value row's "Aliases"
button (or expandable section below inline edit row):

  ┌──────────────────────────────────────────────────────┐
  │  Aliases for: FAM1 — Pompes                          │
  │                                                      │
  │  ┌──────┬──────────┬──────────┬──────────┬─────────┐ │
  │  │ Alias│ Locale   │ Type     │ Preferred│ Actions │ │
  │  ├──────┼──────────┼──────────┼──────────┼─────────┤ │
  │  │ Pump │ en       │ synonym  │ ✓        │ ✏️ 🗑️   │ │
  │  │ P001 │ —        │ legacy   │          │ ✏️ 🗑️   │ │
  │  └──────┴──────────┴──────────┴──────────┴─────────┘ │
  │                                                      │
  │  [ + Add Alias ]                                     │
  └──────────────────────────────────────────────────────┘

- Alias types: synonym, legacy, import, abbreviation
- Locale: fr, en, or null (locale-independent)
- Preferred flag: one preferred per locale (radio behavior)
- Add/edit/delete with inline editing (same pattern as value editor)

ACCEPTANCE CRITERIA
- aliases list loads for selected value
- add/edit/delete inline works
- preferred alias enforces one-per-locale
- alias types match backend enum
```

### Supervisor Verification — Sprint S4

**V1 — Import CSV.**
Prepare a 20-row CSV with 2 intentional errors (missing code, duplicate). Run import
wizard. Verify 18 imported, 2 in error report. Download error CSV.

**V2 — Alias management.**
Open aliases for a value. Add a "legacy" alias. Mark it preferred. Add another preferred
for same locale. Verify first one is un-preferred (radio behavior).

---

*End of Phase 2 - Sub-phase 03 - File 03*
