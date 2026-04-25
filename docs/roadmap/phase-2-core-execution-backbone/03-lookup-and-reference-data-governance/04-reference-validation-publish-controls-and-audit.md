# Phase 2 - Sub-phase 03 - File 04
# Reference Validation, Publish Controls, and Audit

## Context and Purpose

Files 01 through 03 established schema, workflows, aliases, import/export, and search.
File 04 closes sub-phase 03 with strict publish governance and auditability.

This file ensures PRD 6.13 is enforced as intended:

- only valid sets can be published
- impacts are previewed before publish
- protected analytical semantics are guarded
- every governance action is auditable

## PRD Alignment Checklist

This file addresses PRD 6.13 requirements for:

- draft-validate-publish governance gates
- usage explorer and impact preview
- protected analytical controls
- `reference_change_events` audit trail

## Architecture Rules Applied

- **Publish runs on validated evidence.** Publish requires a fresh validation report with
	zero blocking issues.
- **Impact preview is mandatory for protected domains.** Publishing semantic changes in
	protected domains requires dependency impact summary.
- **Append-only governance audit.** Every create/update/deactivate/merge/migrate/import/
	publish action writes change events.
- **Step-up for high-risk semantic changes.** Merge, migration, and protected-domain
	publish require elevated authorization.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000016_reference_change_events.rs` | immutable governance event table |
| `src-tauri/src/reference/publish.rs` | publish guardrail and impact preview engine |
| `src-tauri/src/reference/audit.rs` | audit writer for reference governance actions |
| `src-tauri/src/commands/reference.rs` (patch) | publish validation/preview/audit commands |
| `src/services/reference-publish-service.ts` | frontend wrappers for publish readiness and events |
| `src/stores/reference-governance-store.ts` | UI state for publish readiness and change timeline |

## Prerequisites

- Files 01 to 03 complete
- Permissions seeded for `ref.view`, `ref.manage`, `ref.publish`
- Step-up controls available from prior security modules

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Publish Validation and Impact Preview | `publish.rs` |
| S2 | Audit Trail and Dangerous-Action Guards | migration 016 + `audit.rs` |
| S3 | Governance UI Integration and Final Controls | service/store integration |

---

## Sprint S1 - Publish Validation and Impact Preview

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement publish readiness and impact preview.

CREATE src-tauri/src/reference/publish.rs

Types:
- `ReferencePublishIssue`
- `ReferencePublishReadiness`
- `ReferenceImpactSummary`

Functions:
- `compute_publish_readiness(pool, set_id)`
- `preview_publish_impact(pool, set_id)`
- `publish_reference_set(pool, set_id, actor_id)`

Readiness checks:
- set exists and status is validated
- latest validation report exists and has zero blockers
- if protected domain, impact preview must run and include dependency summary
- no unresolved migration-required issues

Impact summary dimensions:
- assets
- work_orders
- pm_plans
- inventory
- reliability_events
- external_integrations

For unavailable modules, return explicit placeholder status instead of omission.

ACCEPTANCE CRITERIA
- publish blocked when blockers exist
- protected-domain publish requires impact preview
- successful publish supersedes previous set and preserves history
```

### Supervisor Verification - Sprint S1

**V1 - Blocked publish.**
Try publishing set with blocking validation issues. Must fail.

**V2 - Impact preview requirement.**
Protected-domain publish without preview readiness should fail.

**V3 - Successful publish transition.**
Validated set with no blockers publishes and supersedes prior set.

---

## Sprint S2 - Audit Trail and Dangerous-Action Guards

### AI Agent Prompt

```text
You are a senior Rust engineer. Add immutable governance auditing and strict permission guards.

STEP 1 - CREATE migration m20260401_000016_reference_change_events.rs

Create table `reference_change_events`:
- id (PK)
- set_id (INTEGER NULL)
- action (TEXT NOT NULL) // create/update/deactivate/merge/migrate/import/publish
- action_by_id (INTEGER NULL)
- action_at (TEXT NOT NULL)
- summary (TEXT NULL)
- details_json (TEXT NULL)
- requires_step_up (INTEGER NOT NULL DEFAULT 0)
- apply_result (TEXT NOT NULL DEFAULT 'applied')

STEP 2 - CREATE src-tauri/src/reference/audit.rs

Functions:
- `record_reference_change_event(pool, input)`
- `list_reference_change_events(pool, set_id, limit)`

STEP 3 - PATCH commands/reference.rs

Permission rules:
- reads: `ref.view`
- draft edits/import validate/apply: `ref.manage`
- publish/merge/migrate in protected domains: `ref.publish` + step-up

Audit rules:
- record event for successful and blocked publish attempts
- record event for merge/migrate/import apply actions
- no update/delete path for change events

ACCEPTANCE CRITERIA
- migration 016 applies
- dangerous semantic actions require publish permission and step-up
- change events persist for applied and blocked actions
```

### Supervisor Verification - Sprint S2

**V1 - Step-up enforcement.**
Attempt protected merge without step-up. Must fail.

**V2 - Audit row for blocked action.**
Blocked publish attempt should create change event with apply_result blocked.

**V3 - Audit immutability.**
No update/delete commands exist for change events.

---

## Sprint S3 - Governance UI Integration and Final Controls

### AI Agent Prompt

```text
You are a TypeScript engineer. Integrate publish readiness and audit timeline into UI state.

CREATE src/services/reference-publish-service.ts
- `computePublishReadiness(setId)`
- `previewPublishImpact(setId)`
- `publishReferenceSet(setId)`
- `listReferenceChangeEvents(setId, limit?)`

CREATE src/stores/reference-governance-store.ts
State:
- readiness
- impact summary
- change events
- loading/saving/error

Methods:
- `loadReadiness`
- `loadImpact`
- `publish`
- `loadEvents`

PATCH lookup manager page
- publish readiness banner
- blocker list
- publish button disabled when blockers exist
- change timeline panel

ACCEPTANCE CRITERIA
- typecheck passes
- publish button disabled on blockers
- events timeline renders applied and blocked actions
```

### Supervisor Verification - Sprint S3

**V1 - Readiness gate UI.**
With blocker issues, publish button remains disabled.

**V2 - Publish refresh path.**
After successful publish, readiness and timeline refresh automatically.

**V3 - Timeline integrity.**
Applied and blocked actions both appear in timeline with timestamps.

---

## Sprint S4 — Web-Parity Gap Closure (Publish Workflow UI)

> **Scope** — File 04 Sprint S3 references a "lookup manager page" with readiness
> banner and timeline but never names a `.tsx` component. Sprint S4 formalizes the
> publish workflow UI that patches ReferenceManagerPage with governance controls.

### S4‑1 — Publish Readiness Panel (`PublishReadinessPanel.tsx`) — GAP REF‑04

```
LOCATION   src/components/lookups/PublishReadinessPanel.tsx
STORE      reference-governance-store.ts (readiness, impactSummary, changeEvents)
SERVICE    reference-publish-service.ts

DESCRIPTION
Panel that appears at the top of ReferenceValueEditor when viewing a draft set
(replaces the generic version badge area):

  ┌─────────────────────────────────────────────────────────────┐
  │  📋 Publish Readiness — Families (draft v3)                 │
  │                                                             │
  │  Status: ⚠️ 2 blockers found                                │
  │                                                             │
  │  Blockers:                                                  │
  │    ❌ Value "FAM3" missing required label_en                 │
  │    ❌ Circular parent reference detected: FAM5 → FAM2 → FAM5│
  │                                                             │
  │  Warnings:                                                  │
  │    ⚠️ 3 values have no aliases — search discoverability low  │
  │                                                             │
  │  Impact Preview:                                            │
  │    - 42 assets reference values in this set                 │
  │    - 3 new values will become available                     │
  │    - 0 values deactivated                                   │
  │                                                             │
  │  [ Preview Full Impact ]     [ Publish Set ] (disabled)     │
  └─────────────────────────────────────────────────────────────┘

- "Preview Full Impact" opens a detail dialog showing per-value usage counts
- "Publish Set" button:
  - disabled when blockers > 0
  - enabled when 0 blockers → triggers step-up auth (protected domain) or
    simple confirm dialog (ordinary domain)
  - on success: version increments, badge changes to "published", toast

Change Timeline (below the editor table):
  ┌──────────────────────────────────────────────────────────┐
  │  📅 Change Timeline                                      │
  │                                                          │
  │  2026-04-08 14:32 — admin — Published v2                 │
  │  2026-04-08 14:30 — admin — Validated draft v3           │
  │  2026-04-07 09:15 — tech1 — Added value "FAM3"          │
  │  2026-04-07 09:10 — tech1 — Modified label for "FAM1"   │
  │  2026-04-05 11:00 — admin — Published v1                 │
  └──────────────────────────────────────────────────────────┘

- Infinite scroll, most recent first
- Each entry: timestamp, actor, action description, optional value reference
- Filter by action type (create/modify/delete/publish/validate)

ACCEPTANCE CRITERIA
- readiness panel appears only for draft sets
- blockers disable publish button
- impact preview loads from computePublishReadiness IPC
- publish triggers step-up for protected domains
- change timeline loads from listReferenceChangeEvents
- after publish, page refreshes to show new version
```

### Supervisor Verification — Sprint S4

**V1 — Blocker detection.**
Create a draft set with one value missing label_en. Verify readiness panel shows blocker
and publish is disabled.

**V2 — Successful publish.**
Fix all blockers. Click "Publish Set". For protected domain: step-up auth required. For
ordinary: confirm dialog. After publish, version badge updates.

**V3 — Change timeline.**
Perform 3 actions (add value, edit value, publish). Verify all 3 appear in timeline
with correct timestamps and actors.

---

*End of Phase 2 - Sub-phase 03 - File 04*
