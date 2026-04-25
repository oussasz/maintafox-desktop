# Phase 2 - Sub-phase 02 - File 03
# Asset UI, Search, and Cross-Module Bindings

## Context and Purpose

Files 01 and 02 provided backend identity, hierarchy, lifecycle, meter, and document
contracts. File 03 makes those contracts operational in the desktop UI and prepares the
registry to be consumed by downstream modules.

The core requirement is discoverability and binding fidelity: users must find the right
asset quickly, and other modules must bind to a stable governed asset context rather than
ad-hoc text fields.

## Architecture Rules Applied

- **Search is domain-aware.** Asset search ranks code, name, org location, class, family,
	status, and external ids, not just plain text.
- **Context panel is evidence-oriented.** Asset details include current identity, recent
	lifecycle events, meter summary, and bindings.
- **Bindings use stable IDs.** Cross-module references should store `asset_id` and
	optional boundary hints, not denormalized names.
- **UI state is robust under stale updates.** Version conflicts surface clearly so users
	can refresh and retry.
- **Bilingual UI parity.** All labels and filters are localized in FR/EN.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src/services/asset-search-service.ts` | Search and suggestion wrappers |
| `src/stores/asset-search-store.ts` | Search query/result state |
| `src/pages/assets/AssetRegistryPage.tsx` | Asset list and detail workspace |
| `src/components/assets/AssetFilterBar.tsx` | Multi-criteria filtering |
| `src/components/assets/AssetResultTable.tsx` | Search result grid |
| `src/components/assets/AssetDetailPanel.tsx` | Asset identity/lifecycle/meter snapshot panel |
| `src/components/assets/AssetBindingSummary.tsx` | Cross-module binding summary cards |
| `shared/ipc-types.ts` (patch) | search and binding DTOs |

## Prerequisites

- Files 01 and 02 complete
- Existing navigation shell from Phase 1 SP02
- i18n baseline from Phase 1 SP05

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Search Contracts and Store | search IPC/service/store |
| S2 | Asset Registry Workspace UI | page and core components |
| S3 | Cross-Module Binding Readiness | binding summary contracts and integration hooks |

---

## Sprint S1 - Search Contracts and Store

### AI Agent Prompt

```text
You are a TypeScript engineer. Implement asset search services and state.

STEP 1 - PATCH commands/assets.rs and backend service
Add read command `search_assets` with params:
- query: string
- classCodes?: string[]
- familyCodes?: string[]
- statusCodes?: string[]
- orgNodeIds?: number[]
- includeDecommissioned?: boolean
- limit?: number

Return DTO:
- asset identity core fields
- org node label
- parent asset code/name
- latest meter summary (if any)
- external id count

STEP 2 - CREATE src/services/asset-search-service.ts
Expose:
- `searchAssets(filters)`
- `suggestAssetCodes(prefix)`
- `suggestAssetNames(prefix)`

Use Zod validation for all responses.

STEP 3 - CREATE src/stores/asset-search-store.ts
State:
- filters
- result list
- selected result id
- loading/error

Methods:
- `runSearch`
- `clearFilters`
- `selectAsset`

ACCEPTANCE CRITERIA
- typecheck passes
- search returns empty array safely for no results
- filter changes trigger deterministic search refresh
```

### Supervisor Verification - Sprint S1

**V1 - Code-priority search.**
Searching exact asset code returns target row first.

**V2 - Multi-filter search.**
Apply class + status filters and verify results are constrained correctly.

**V3 - Empty-state behavior.**
Search nonsense query and verify UI/store handles zero results without error.

---

## Sprint S2 - Asset Registry Workspace UI

### AI Agent Prompt

```text
You are a React engineer. Build the main registry workspace.

STEP 1 - CREATE src/pages/assets/AssetRegistryPage.tsx

Use a two-pane layout:
- left pane: filter bar + result table
- right pane: detail panel for selected asset

STEP 2 - CREATE components

`AssetFilterBar.tsx`
- query input
- class, family, status, org filters
- clear/apply buttons

`AssetResultTable.tsx`
- columns: code, name, class, family, status, org, criticality
- row click selects asset
- status badges

`AssetDetailPanel.tsx`
- identity block
- hierarchy block
- latest lifecycle events
- latest meter values
- document link count

Localization:
- add translation keys under `assets` namespace for both FR and EN

ACCEPTANCE CRITERIA
- page renders with no selected asset
- selecting row loads detail panel
- localization keys exist for all visible labels
```

### Supervisor Verification - Sprint S2

**V1 - Selection flow.**
Select an asset in table and verify detail panel updates.

**V2 - Filter and clear flow.**
Apply filters then clear; table resets correctly.

**V3 - Locale parity.**
Switch language and verify key labels on page and components.

---

## Sprint S3 - Cross-Module Binding Readiness

### AI Agent Prompt

```text
You are a full-stack engineer. Add cross-module binding summary contracts.

STEP 1 - Backend read model
Create read function `get_asset_binding_summary(asset_id)` returning counts/flags:
- linked_di_count
- linked_wo_count
- linked_pm_plan_count
- linked_failure_event_count
- linked_document_count
- linked_iot_signal_count
- linked_erp_mapping_count

In this phase, unavailable domains should return status placeholders:
- status: unavailable/not_implemented
- count: null

STEP 2 - PATCH shared/ipc-types.ts
Add `AssetBindingSummary` DTO with per-domain status.

STEP 3 - CREATE `AssetBindingSummary.tsx`
Render cards with count or placeholder note.

STEP 4 - integrate into detail panel
Show summary below lifecycle and meter blocks.

ACCEPTANCE CRITERIA
- binding summary renders even when many domains not yet implemented
- no runtime errors when counts are null
- placeholder notes clearly indicate future module dependencies
```

### Supervisor Verification - Sprint S3

**V1 - Placeholder tolerance.**
If WO or PM domains are not implemented yet, cards still render with clear unavailable state.

**V2 - Implemented-domain counts.**
For document links, count should reflect actual linked rows.

**V3 - Detail panel resilience.**
Switch between assets rapidly; summary panel should not flicker into error state.

---

## Sprint S4 — Web-Parity Gap Closure (Asset CRUD, Tree & Identification)

> **Scope** — Six web‑parity gaps closing the path from read‑only registry to
> full asset lifecycle management: create form, edit form, hierarchical tree
> navigator with context menu, criticality badge components, and QR code
> identification.

### S4‑1 — Asset Create Form (`AssetCreateForm.tsx`) — GAP EQ‑01

```
LOCATION   src/components/assets/AssetCreateForm.tsx
STORE      asset-store.ts (patch — add createDraft, submitCreate, createFormOpen)
SCHEMA     src/schemas/asset-create.schema.ts (Zod)
SERVICE    asset-service.ts (already has createAsset IPC wrapper)

DESCRIPTION
Sheet / dialog triggered from a "+ New Asset" button on AssetRegistryPage (top-right,
eq.manage permission guard).

Form layout — single-column, scrollable:
  ┌─────────────────────────────────────────────────────┐
  │  Code *           [auto-suggest or manual]          │
  │  Name *           [text]                            │
  │  Class *          [select — lookup: asset_classes]  │
  │  Family *         [select — lookup: families]       │
  │  Status           [select — defaults to "Active"]   │
  │  Criticality *    [select — A / B / C / D]          │
  │  Parent asset     [combobox — asset search]         │
  │  Organization     [combobox — org node search]      │
  │  Location         [select — lookup: locations]      │
  │  Description      [textarea]                        │
  └─────────────────────────────────────────────────────┘
  [ Cancel ]                              [ Create Asset ]

- All lookup selects use reference-search-store for typeahead
- Parent combobox calls search_assets with query debounce (300ms)
- Organization combobox calls search_nodes
- Zod schema enforces required fields; form uses useZodForm() from SP00-F01
- On success: toast, close sheet, select new asset in table

ACCEPTANCE CRITERIA
- mandatory field validation blocks submit when empty
- lookup selects load from reference-service
- parent select excludes self (N/A on create but prepared for edit re-use)
- created asset appears in result table without full re-fetch (optimistic)
```

### S4‑2 — Asset Edit Form (`AssetEditForm.tsx`) — GAP EQ‑02

```
LOCATION   src/components/assets/AssetEditForm.tsx
STORE      asset-store.ts (patch — add editDraft, submitUpdate, editFormOpen)
SCHEMA     src/schemas/asset-edit.schema.ts (Zod — reuses create fields + id)

DESCRIPTION
Sheet / dialog triggered from AssetDetailPanel "Edit" button (eq.manage guard).
Pre-populated with current asset data. Same field layout as create form except:
  - Code field is read-only (immutable after creation)
  - Parent select excludes self AND own descendants (prevent cycles)
  - "Last modified" info line at bottom

Dirty tracking via useZodForm defaultValues comparison → unsaved-changes confirmation
on close attempt.

ACCEPTANCE CRITERIA
- code field is read-only
- parent cycle prevention works (cannot select self or descendant)
- unsaved-changes prompt appears on dirty close
- optimistic update in table after save
```

### S4‑3 — Hierarchical Tree Navigator (`AssetTreeNavigator.tsx`) — GAP EQ‑03

```
LOCATION   src/components/assets/AssetTreeNavigator.tsx
STORE      asset-store.ts (patch — add treeExpandedIds, treeSelectedId, loadChildren)
COMMAND    get_asset_children (Rust — returns direct children of parent_asset_id)

DESCRIPTION
Optional LEFT pane on AssetRegistryPage — toggled via a "Tree / Table" segmented control
in the toolbar. When tree mode is active:
  ┌───────────────┬──────────────────────────────────┐
  │  Tree pane    │   Detail panel (existing)        │
  │               │                                  │
  │  ▸ Site A     │   [AssetDetailPanel.tsx]          │
  │    ▸ Line 1   │                                  │
  │      Motor 1  │                                  │
  │      Motor 2  │                                  │
  │    ▸ Line 2   │                                  │
  │  ▸ Site B     │                                  │
  └───────────────┴──────────────────────────────────┘

- Root level: assets where parent_asset_id IS NULL
- Expand on click: lazy-loads children via get_asset_children
- Tree item shows: code, name, status mini-badge, criticality dot
- Selected node drives AssetDetailPanel
- Keyboard: Arrow keys for navigation, Enter to select, Right to expand, Left to collapse
- Search box at top of tree pane — filters visible nodes (client-side match on code/name)
- aria-role="treegrid" with proper ARIA attributes

ACCEPTANCE CRITERIA
- root assets load on mount
- expand node lazy-loads children
- keyboard navigation works (arrow keys + enter)
- selecting tree node shows detail panel for that asset
```

### S4‑4 — Tree Context Menu — GAP EQ‑06

```
LOCATION   src/components/assets/AssetTreeContextMenu.tsx (uses shadcn ContextMenu)

DESCRIPTION
Right-click on a tree node shows context menu with permission-gated items:
  ┌────────────────────────────────┐
  │  ➕ Add child asset  (eq.manage)│
  │  ✏️ Edit             (eq.manage)│
  │  📋 Copy code        (eq.view) │
  │  ─────────────────────────     │
  │  🔄 Move to…         (eq.manage)│
  │  ⚠️ Decommission…    (eq.manage)│
  └────────────────────────────────┘

- "Add child asset" opens AssetCreateForm with parent pre-filled
- "Edit" opens AssetEditForm
- "Move to…" opens a node-picker combobox dialog (cycle prevention enforced)
- "Decommission…" opens AssetDecommissionModal (from file 02 S4-1)
- Items hidden when user lacks the required permission

ACCEPTANCE CRITERIA
- menu appears on right-click
- hidden items for insufficient permissions
- "Add child" pre-fills parent field
```

### S4‑5 — Criticality Badge Component — GAP EQ‑08

```
LOCATION   src/components/assets/CriticalityBadge.tsx

DESCRIPTION
Standardized badge for asset criticality grades used in AssetResultTable,
AssetDetailPanel, and AssetTreeNavigator:
  - A (Critical)  → red-500 bg, white text
  - B (Important) → orange-400 bg, white text
  - C (Standard)  → blue-400 bg, white text
  - D (Low)       → gray-300 bg, gray-800 text

Renders as shadcn Badge variant with Tailwind color classes. Accepts
criticality: 'A' | 'B' | 'C' | 'D' | null prop. Null renders gray "—".

ACCEPTANCE CRITERIA
- all 4 grades render with distinct colors
- null renders graceful fallback
- used in at least 3 locations (table, detail, tree)
```

### S4‑6 — QR Code Generation (`AssetQrCode.tsx`) — GAP EQ‑09

```
LOCATION   src/components/assets/AssetQrCode.tsx
DEPENDENCY qrcode (npm package — generates SVG QR codes, ~28 KB gzip)

DESCRIPTION
Button on AssetDetailPanel action bar: "QR Code" (eq.view guard).
Opens a popover showing:
  - QR code SVG encoding: maintafox://asset/{asset_id}
  - asset code + name below QR
  - "Download PNG" button (canvas render → download)
  - "Print" button (opens print dialog with QR + label)

Size: 200×200 px default with option to scale.

ACCEPTANCE CRITERIA
- QR encodes correct asset ID
- PNG download produces valid image file
- print layout is clean (QR + code + name only, no page chrome)
```

### Supervisor Verification — Sprint S4

**V1 — Create flow.**
Click "+ New Asset". Fill required fields. Submit. Verify asset appears in table and detail
panel shows the new asset.

**V2 — Edit flow.**
Select an asset. Click "Edit". Change name. Save. Verify name updates in table. Try to
change code → field is read-only.

**V3 — Tree navigation.**
Toggle to tree mode. Expand a parent. Select a child. Verify detail panel updates. Use
keyboard arrows to navigate.

**V4 — Context menu.**
Right-click a tree node. Verify "Add child" pre-fills parent. Verify menu items hidden
for users without eq.manage.

**V5 — Criticality badges.**
View assets with criticality A, B, C, D in table. Verify color coding matches spec.

**V6 — QR code.**
Open QR for an asset. Download PNG. Scan with phone → verify encoded URI.

---

*End of Phase 2 - Sub-phase 02 - File 03*
