# Phase 2 - Sub-phase 03 - File 01
# Reference Domain Model and Versioning

## Context and Purpose

Sub-phase 02 delivered the asset backbone and established a direct dependency on governed
classifications, families, statuses, and criticality semantics. Sub-phase 03 now builds
the semantic backbone itself: the Lookup and Reference Data Manager from PRD 6.13.

This module is not a dropdown editor. It is the control plane for coded meaning across
workflow, analytics, inventory traceability, reliability, and ERP mappings. If reference
domains are unmanaged, downstream modules become inconsistent and analytics lose
comparability over time.

File 01 establishes the core domain model and version lifecycle:

- reference domain catalog metadata
- reference set versioning
- reference value trees and coded semantics
- the draft -> validated -> published -> superseded progression

## PRD Alignment Checklist

This file directly implements the PRD 6.13 requirements for:

- `reference_domains`, `reference_sets`, and `reference_values`
- structure types: flat, hierarchical, versioned_code_set, unit_set, external_code_set
- governance level semantics
- the mandatory lifecycle states for reference sets

## Architecture Rules Applied

- **Domain metadata is first-class.** Every domain has structure type and governance level.
- **Sets are versioned snapshots.** Values belong to a set version, not directly to the
	domain, so historical semantics remain stable after publish.
- **Published set is immutable.** Edits happen in draft; publish creates a new active
	semantic version.
- **Hierarchy is represented in values.** Parent-child links are stored in
	`reference_values.parent_id` for hierarchical domains.
- **Code uniqueness is scoped to set.** Within a set, `code` must be unique.
- **No hard delete in protected domains.** Protected analytical values are deactivated or
	migrated, never physically removed once in use.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000013_reference_domains_core.rs` | Core reference-domain schema |
| `src-tauri/src/reference/mod.rs` | Module root and shared type exports |
| `src-tauri/src/reference/domains.rs` | Domain catalog service |
| `src-tauri/src/reference/sets.rs` | Set version lifecycle service |
| `src-tauri/src/reference/values.rs` | Value CRUD and hierarchy service |
| `src-tauri/src/commands/reference.rs` | IPC for domain/set/value operations |
| `shared/ipc-types.ts` (patch) | `ReferenceDomain`, `ReferenceSet`, `ReferenceValue` types |
| `src/services/reference-service.ts` | Frontend wrappers for core reference operations |

## Prerequisites

- Sub-phases 01 and 02 complete
- Permission framework from Phase 1 and sub-phase 01 available
- Lookup-dependent modules understand stable code semantics

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Reference Core Migration and Domain Service | migration 013, `reference/domains.rs` |
| S2 | Set Version Lifecycle Service | `reference/sets.rs` |
| S3 | Value Tree Service and IPC Integration | `reference/values.rs`, `commands/reference.rs` |

---

## Sprint S1 - Reference Core Migration and Domain Service

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the foundational schema and domain catalog for PRD 6.13.

STEP 1 - CREATE migration m20260401_000013_reference_domains_core.rs

Create table `reference_domains`:
- id (PK)
- code (TEXT UNIQUE NOT NULL)
- name (TEXT NOT NULL)
- structure_type (TEXT NOT NULL)
- governance_level (TEXT NOT NULL)
- is_extendable (INTEGER NOT NULL DEFAULT 1)
- validation_rules_json (TEXT NULL)
- created_at (TEXT NOT NULL)
- updated_at (TEXT NOT NULL)

Create table `reference_sets`:
- id (PK)
- domain_id (INTEGER NOT NULL)
- version_no (INTEGER NOT NULL)
- status (TEXT NOT NULL) // draft/validated/published/superseded
- effective_from (TEXT NULL)
- created_by_id (INTEGER NULL)
- created_at (TEXT NOT NULL)
- published_at (TEXT NULL)

Create table `reference_values`:
- id (PK)
- set_id (INTEGER NOT NULL)
- parent_id (INTEGER NULL)
- code (TEXT NOT NULL)
- label (TEXT NOT NULL)
- description (TEXT NULL)
- sort_order (INTEGER NULL)
- color_hex (TEXT NULL)
- icon_name (TEXT NULL)
- semantic_tag (TEXT NULL)
- external_code (TEXT NULL)
- is_active (INTEGER NOT NULL DEFAULT 1)
- metadata_json (TEXT NULL)

Indexes:
- unique `(set_id, code)`
- `idx_reference_values_parent_id`
- unique `(domain_id, version_no)` on sets

Register migration 013 after migration 012.

STEP 2 - CREATE src-tauri/src/reference/domains.rs

Define:
- `ReferenceDomain`
- `CreateReferenceDomainPayload`
- `UpdateReferenceDomainPayload`

Functions:
- `list_reference_domains(pool)`
- `get_reference_domain(pool, domain_id)`
- `create_reference_domain(pool, payload, actor_id)`
- `update_reference_domain(pool, domain_id, payload, actor_id)`

Validation rules:
- `code` normalized uppercase snake or dot-safe token
- `structure_type` must be one of PRD-supported types
- `governance_level` must be one of protected_analytical/tenant_managed/system_seeded/erp_synced

ACCEPTANCE CRITERIA
- migration applies with all three tables
- unique domain code enforced
- invalid structure_type rejected
```

### Supervisor Verification - Sprint S1

**V1 - Domain uniqueness.**
Create a domain code once, then try again. Second insert must fail.

**V2 - PRD type enforcement.**
Attempt unknown structure_type. Validation must reject.

**V3 - Governance-level enforcement.**
Attempt unknown governance level. Validation must reject.

---

## Sprint S2 - Set Version Lifecycle Service

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement set lifecycle rules.

CREATE src-tauri/src/reference/sets.rs

Define:
- `ReferenceSet`
- `CreateReferenceSetPayload`
- `ValidateReferenceSetPayload`

Functions:
- `list_sets_for_domain(pool, domain_id)`
- `create_draft_set(pool, domain_id, actor_id)`
- `validate_set(pool, set_id, actor_id)`
- `publish_set(pool, set_id, actor_id)`
- `supersede_previous_published(pool, domain_id, newly_published_set_id)`

Lifecycle rules:
- only draft can move to validated
- only validated can move to published
- one published set per domain
- publishing supersedes previous published set
- published set cannot be directly edited

ACCEPTANCE CRITERIA
- lifecycle transitions enforce order
- exactly one published set per domain after publish
- invalid transitions rejected
```

### Supervisor Verification - Sprint S2

**V1 - Transition ordering.**
Try publishing draft directly. Must fail.

**V2 - Single published set.**
Publish v1 then publish v2. v1 must become superseded.

**V3 - Published edit block.**
Attempt update on published set and verify rejection.

---

## Sprint S3 - Value Tree Service and IPC Integration

### AI Agent Prompt

```text
You are a Rust and TypeScript engineer. Implement value operations and command surface.

CREATE src-tauri/src/reference/values.rs

Functions:
- `list_values(pool, set_id)`
- `create_value(pool, payload, actor_id)`
- `update_value(pool, value_id, payload, actor_id)`
- `deactivate_value(pool, value_id, actor_id)`
- `move_value_parent(pool, value_id, new_parent_id, actor_id)`

Validation:
- code unique in set
- no hierarchy cycles
- parent belongs to same set
- deactivation blocked for protected/in-use values (full usage checks in File 04)

PATCH commands/reference.rs and register in lib.rs:
- list/create/update domain
- create/validate/publish set
- list/create/update/deactivate/move value

Permissions:
- read operations require `ref.view`
- write draft operations require `ref.manage`
- publish operation requires `ref.publish`

PATCH shared/ipc-types.ts and create src/services/reference-service.ts with Zod validation.

ACCEPTANCE CRITERIA
- commands compile and register
- cycle and duplicate-code rules enforced
- permission split works as defined
```

### Supervisor Verification - Sprint S3

**V1 - Duplicate code guard.**
Create same code twice in one set. Second should fail.

**V2 - Cycle guard.**
Move node under its descendant in hierarchical set. Must fail.

**V3 - Permission split.**
`ref.view` can read, `ref.manage` can edit drafts, only `ref.publish` can publish.

---

## Sprint S4 — Web-Parity Gap Closure (Reference Manager Page & Domain Browser)

> **Scope** — The entire sub‑phase 03 backend is fully specified but no page‑level
> React components exist anywhere in the roadmap. Sprint S4 in File 01 establishes
> the main page shell and domain browser — the structural foundation that Files
> 02–04 will patch with value editing, import, alias, and publish UI.

### S4‑1 — Reference Manager Page (`ReferenceManagerPage.tsx`) — GAP REF‑01

```
LOCATION   src/pages/ReferenceManagerPage.tsx
ROUTE      /lookups (replaces ModulePlaceholder)
STORE      reference-governance-store.ts (already specified in File 04)
SERVICE    reference-service.ts (already specified in File 01)

DESCRIPTION
Two-pane layout matching the admin-panel pattern established by AssetRegistryPage
and OrganizationDesignerPage:

  ┌─────────────────────┬──────────────────────────────────────┐
  │  Domain Browser     │  Value Editor Area                   │
  │  (left, 300px)      │  (right, flex-1)                     │
  │                     │                                      │
  │  🔍 Search domains  │  [Empty state when nothing selected] │
  │                     │  "Select a domain to manage its      │
  │  ▸ Equipment        │   reference values"                  │
  │    Families         │                                      │
  │    Classes          │  [ValueEditorTable when selected]    │
  │    Statuses         │  (Sprint S4 in File 02)              │
  │  ▸ Work Management  │                                      │
  │    Priority         │                                      │
  │    Failure Modes    │                                      │
  │  ▸ Organization     │                                      │
  │    Positions        │                                      │
  │    Schedules        │                                      │
  │  ▸ Personnel        │                                      │
  │    Skills           │                                      │
  │    Certifications   │                                      │
  └─────────────────────┴──────────────────────────────────────┘

  Top toolbar:
    - breadcrumb: Lookups > {selected domain} > {selected set}
    - version badge (draft / published / superseded)
    - action buttons: "New Domain" (ref.manage), "Import" (ref.manage)

Permission: ref.view to access page, ref.manage for mutations.

ACCEPTANCE CRITERIA
- route /lookups loads page with two-pane layout
- domain list loads from list_reference_domains IPC call
- selecting a domain shows its sets in a nested tree
- empty state renders when nothing selected
- page is permission-gated (ref.view)
```

### S4‑2 — Domain Browser Panel (`DomainBrowserPanel.tsx`) — GAP REF‑02

```
LOCATION   src/components/lookups/DomainBrowserPanel.tsx
STORE      reference-search-store.ts (already specified in File 03)

DESCRIPTION
Left pane of ReferenceManagerPage. Renders the domain → set hierarchy as an
accessible treegrid:

  - search input at top — client-side filters visible domains/sets by label
  - each domain node: icon (📁), name, protected badge (🔒) if analytical domain
  - expand domain → shows child reference sets
  - each set node: name, value count, version badge (draft/published)
  - click set → drives right pane to show ValueEditorTable for that set
  - right-click domain → context menu:
    - "New Set" (ref.manage)
    - "Rename Domain" (ref.manage)
  - keyboard: arrow keys, enter to select, right to expand, left to collapse

Protected analytical domains (PRD 6.13) show a lock icon and restrict
certain actions (handled by backend, browser shows visual cue only).

ACCEPTANCE CRITERIA
- domains and sets render hierarchically
- search filters visible nodes live
- protected domains show lock badge
- context menu actions are permission-gated
- keyboard navigation works
```

### Supervisor Verification — Sprint S4

**V1 — Page loads.**
Navigate to /lookups. Verify two-pane layout renders with domain list. Verify empty
state in right pane.

**V2 — Domain browsing.**
Expand a domain. See its reference sets with value counts and version badges.

**V3 — Search filtering.**
Type "fami" in search. Only "Families" domain/set visible. Clear search → all visible.

**V4 — Permission gate.**
Login as user without ref.view. Navigate to /lookups → redirected or 403 shown.

---

*End of Phase 2 - Sub-phase 03 - File 01*
