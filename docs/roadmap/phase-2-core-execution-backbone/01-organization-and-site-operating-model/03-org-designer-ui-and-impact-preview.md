# Phase 2 - Sub-phase 01 - File 03
# Org Designer UI and Impact Preview

## Context and Purpose

Files 01 and 02 created the backend contracts for structure-model governance and live
node maintenance. File 03 creates the administrative design surface that makes those
contracts usable.

This is not a decorative org-chart page. The PRD and research direction are explicit:
the organization model is the tenant's operating backbone. That means the UI must do
more than render a tree. It must help the admin understand type semantics, node
capabilities, responsibility coverage, and the operational impact of moving or disabling
part of the structure before they commit the change.

The impact preview is especially important. Structural mistakes are expensive because
later modules will anchor assets, work requests, work orders, permits, inventory zones,
and budgets to `org_nodes`. A safe designer warns early, shows what is affected, and
blocks silent structural damage.

## Architecture Rules Applied

- **Tree-first, not org-chart-first.** The UI uses an operational tree workspace with
	searchable rows, inspector panels, and preview drawers. Free-form chart layout is not
	the primary interaction model.
- **Preview before dangerous action.** Move, deactivate, and responsibility reassignment
	must call a preview endpoint before the final mutation command is enabled.
- **Future-module aware previews.** Phase 2 SP01 can only compute org-local impacts
	directly, but the preview contract is shaped so later modules can contribute counts for
	assets, open work, permits, stock, or budget scope.
- **Guided editing over unconstrained drag-drop.** Drag handles may open a move flow,
	but the actual save path remains guided and validated. The admin never commits a blind
	drag operation.
- **Bilingual from the start.** All labels, node-type badges, preview messages, and
	warnings go through `useT()` and ship in French and English.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/org/tree_queries.rs` | Flattened tree projections for the designer workspace |
| `src-tauri/src/org/impact_preview.rs` | Preview contract for move, deactivate, and reassignment operations |
| `src-tauri/src/commands/org.rs` (patch) | `get_org_designer_snapshot`, `preview_org_change` |
| `src/services/org-designer-service.ts` | IPC wrappers for tree snapshot and impact preview |
| `src/stores/org-designer-store.ts` | Designer workspace state, filters, preview drawer state |
| `src/pages/admin/OrganizationDesignerPage.tsx` | Main admin designer screen |
| `src/components/org/OrganizationTreePanel.tsx` | Tree grid with capability badges and filters |
| `src/components/org/NodeInspectorPanel.tsx` | Detail editor and node context panel |
| `src/components/org/ImpactPreviewDrawer.tsx` | Preview UI for move/deactivate/reassignment |
| `public/locales/en/org-designer.json` and `public/locales/fr/org-designer.json` | Bilingual text for the designer |

## Prerequisites

- File 01 complete: structure-model configuration services
- File 02 complete: node lifecycle, responsibilities, bindings, and frontend services
- SP05 complete: `useT()` and locale infrastructure available

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Tree Snapshot and Impact Preview Backend | `tree_queries.rs`, `impact_preview.rs`, preview IPC |
| S2 | Organization Designer Workspace UI | page, tree panel, inspector, i18n |
| S3 | Guarded Interaction Flows and Preview-Driven Actions | preview drawer, move flow, deactivate flow, responsibility reassignment flow |

---

## Sprint S1 - Tree Snapshot and Impact Preview Backend

### AI Agent Prompt

```text
You are a senior Rust engineer. Files 01 and 02 delivered structure and node services.
Your task is to add read-optimized projections for the designer UI and a preview engine
that estimates operational impact before structural mutations.

STEP 1 - CREATE src-tauri/src/org/tree_queries.rs

Implement a designer snapshot query layer.

Use these types:

```rust
use crate::errors::AppResult;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgDesignerNodeRow {
		pub node_id: i64,
		pub parent_id: Option<i64>,
		pub ancestor_path: String,
		pub depth: i64,
		pub code: String,
		pub name: String,
		pub status: String,
		pub row_version: i64,
		pub node_type_id: i64,
		pub node_type_code: String,
		pub node_type_label: String,
		pub can_host_assets: bool,
		pub can_own_work: bool,
		pub can_carry_cost_center: bool,
		pub can_aggregate_kpis: bool,
		pub can_receive_permits: bool,
		pub child_count: i64,
		pub active_responsibility_count: i64,
		pub active_binding_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgDesignerSnapshot {
		pub active_model_id: Option<i64>,
		pub active_model_version: Option<i64>,
		pub nodes: Vec<OrgDesignerNodeRow>,
}
```

Required functions:
- `get_org_designer_snapshot(pool) -> AppResult<OrgDesignerSnapshot>`
- `search_nodes(pool, query, status_filter, type_filter) -> AppResult<Vec<OrgDesignerNodeRow>>`

Query rules:
- rows are flattened and ordered by `ancestor_path`
- counts for responsibilities and bindings should only include active assignments/bindings
- if no active model exists, return `active_model_id = None` and `nodes = []`

STEP 2 - CREATE src-tauri/src/org/impact_preview.rs

Implement a preview contract that can be reused by the UI before dangerous changes.

Use these types:

```rust
use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrgPreviewAction {
		MoveNode,
		DeactivateNode,
		ReassignResponsibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgImpactDependencySummary {
		pub domain: String,
		pub status: String,
		pub count: Option<i64>,
		pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgImpactPreview {
		pub action: OrgPreviewAction,
		pub subject_node_id: i64,
		pub affected_node_count: i64,
		pub descendant_count: i64,
		pub active_responsibility_count: i64,
		pub active_binding_count: i64,
		pub blockers: Vec<String>,
		pub warnings: Vec<String>,
		pub dependencies: Vec<OrgImpactDependencySummary>,
}

#[derive(Debug, Deserialize)]
pub struct PreviewOrgChangePayload {
		pub action: String,
		pub node_id: i64,
		pub new_parent_id: Option<i64>,
		pub responsibility_type: Option<String>,
		pub replacement_person_id: Option<i64>,
		pub replacement_team_id: Option<i64>,
}
```

Preview requirements:

1. `preview_move_node(pool, node_id, new_parent_id) -> AppResult<OrgImpactPreview>`
	 - compute descendant count and affected subtree size
	 - add blocker if `new_parent_id` is inside the subtree
	 - add blocker if parent-child type rule is invalid
	 - add warning if any descendant currently has active responsibilities or external bindings

2. `preview_deactivate_node(pool, node_id) -> AppResult<OrgImpactPreview>`
	 - count descendants, active responsibilities, active bindings
	 - add blocker if active descendants exist
	 - add blocker if active responsibilities exist

3. `preview_responsibility_reassignment(pool, node_id, responsibility_type, replacement_person_id, replacement_team_id)`
	 - add blocker if replacement target is missing
	 - add warning if the current assignment is already ended

Dependency placeholders:
- the preview must include future dependency summaries even before those modules exist
- for now return entries like:
	- `{ domain: "assets", status: "unavailable", count: null, note: "Module 6.3 not yet implemented" }`
	- `{ domain: "open_work", status: "unavailable", count: null, note: "Modules 6.4/6.5 not yet implemented" }`
	- `{ domain: "permits", status: "unavailable", count: null, note: "Module 6.23 not yet implemented" }`
	- `{ domain: "inventory", status: "unavailable", count: null, note: "Module 6.8 not yet implemented" }`

STEP 3 - PATCH src-tauri/src/commands/org.rs

Add these commands:
- `get_org_designer_snapshot`
- `preview_org_change`

Permission rules:
- both require `org.view`
- `preview_org_change` is read-only and should not mutate data

ACCEPTANCE CRITERIA
- `cargo check` passes with 0 errors
- `get_org_designer_snapshot` returns flattened rows ordered by `ancestor_path`
- `preview_org_change` returns blockers for invalid move and deactivate scenarios
- dependency placeholders appear even when downstream modules are not implemented yet
```

### Supervisor Verification - Sprint S1

**V1 - Snapshot ordering.**
Create a small tree with one root and two nested descendants. Call
`get_org_designer_snapshot`. The `nodes` array must be ordered by `ancestor_path` and the
depth values must match the tree.

**V2 - Move preview blocker.**
Preview moving a node beneath one of its descendants. The preview must return at least
one blocker explaining that a cycle would be created.

**V3 - Future-domain placeholders.**
Call `preview_org_change` on any valid node and inspect the `dependencies` array. It
must include placeholders for assets, open work, permits, and inventory with status
`unavailable` rather than omitting them.

---

## Sprint S2 - Organization Designer Workspace UI

### AI Agent Prompt

```text
You are a React and TypeScript engineer. The backend now provides designer snapshots and
preview payloads. Build the admin workspace for designing and reviewing the org model.

STEP 1 - CREATE src/services/org-designer-service.ts

Expose these functions with Zod validation:
- `getOrgDesignerSnapshot()`
- `previewOrgChange(payload)`

STEP 2 - CREATE src/stores/org-designer-store.ts

State shape:

```typescript
interface OrgDesignerStoreState {
	snapshot: OrgDesignerSnapshot | null;
	filterText: string;
	statusFilter: string | null;
	typeFilter: string | null;
	selectedNodeId: number | null;
	preview: OrgImpactPreview | null;
	previewOpen: boolean;
	loading: boolean;
	previewLoading: boolean;
	error: string | null;

	loadSnapshot: () => Promise<void>;
	setFilterText: (value: string) => void;
	setSelectedNodeId: (nodeId: number | null) => void;
	openPreview: (payload: PreviewOrgChangePayload) => Promise<void>;
	closePreview: () => void;
}
```

STEP 3 - CREATE src/pages/admin/OrganizationDesignerPage.tsx

Use a three-pane layout:
- left: structure model summary and filters
- center: tree workspace
- right: selected node inspector

UI direction:
- this is an operations workspace, not a playful diagram
- show node code, name, type, status, and capability badges in the tree
- use badges for capability flags: `ASSET`, `WORK`, `COST`, `KPI`, `PERMIT`
- show a model-version chip like `Model v3 - Active`
- include a warning banner when no active model exists

STEP 4 - CREATE src/components/org/OrganizationTreePanel.tsx

Requirements:
- accessible treegrid or nested list, keyboard navigable
- search box filters by code, name, and type label
- each row shows indentation by `depth`
- node rows surface child count and warning state for inactive nodes

STEP 5 - CREATE src/components/org/NodeInspectorPanel.tsx

Tabs or sections:
- Details
- Responsibilities
- External Bindings
- Preview Actions

The inspector must consume `selectedNodeId` and show placeholder content if nothing is selected.

STEP 6 - Add i18n files

Create `public/locales/en/org-designer.json` and `public/locales/fr/org-designer.json`.
At minimum include keys for:
- title
- noActiveModel
- searchPlaceholder
- details
- responsibilities
- externalBindings
- previewActions
- moveNode
- deactivateNode
- capability badges
- preview drawer labels

ACCEPTANCE CRITERIA
- `pnpm run typecheck` passes with 0 errors
- `pnpm run i18n:check` passes with 0 missing keys
- the page renders when an active model exists and when no model exists
- selecting a row updates the inspector panel without console errors
```

### Supervisor Verification - Sprint S2

**V1 - Empty-state rendering.**
Start with no active model. Open the Organization Designer page. The page must render a
clear empty-state banner instead of crashing.

**V2 - Bilingual labels.**
Switch the application between French and English. Confirm the page title, filters,
panel headings, and preview labels change language correctly.

**V3 - Tree interaction.**
Create a three-level tree and open the page. The tree panel must indent rows correctly,
allow searching by code or name, and update the inspector when a row is selected.

---

## Sprint S3 - Guarded Interaction Flows and Preview-Driven Actions

### AI Agent Prompt

```text
You are a React engineer. The designer UI exists. Add preview-driven action flows so the
admin can evaluate impact before attempting a structural change.

STEP 1 - CREATE src/components/org/ImpactPreviewDrawer.tsx

This drawer or side sheet shows:
- action label
- affected node count
- descendant count
- active responsibility count
- active binding count
- blockers
- warnings
- dependency placeholders for future modules

Behavior rules:
- if blockers exist, disable the final confirm button
- if only warnings exist, allow confirm but require explicit acknowledgement

STEP 2 - Wire preview actions into the inspector

From `NodeInspectorPanel.tsx`, add buttons that open preview for:
- move node
- deactivate node
- reassign responsibility

Implementation rule:
- preview opens first
- final mutation command is not available until the preview payload has been shown
- guided move uses a target-node picker, not free-form drop save

STEP 3 - Optional drag interaction

If the team uses a tree drag library already, a drag gesture may be added, but it must
only initiate the preview workflow. A drag gesture must never save immediately.

STEP 4 - Add a component test or story-level smoke test

Test cases:
- preview with blockers disables confirm
- preview with warnings shows warning list and enabled confirm
- closing the drawer clears preview state in the store

ACCEPTANCE CRITERIA
- `pnpm run typecheck` passes with 0 errors
- preview drawer appears before move/deactivate/reassign actions
- blocker scenarios disable the confirm action
- store preview state clears cleanly on close
```

### Supervisor Verification - Sprint S3

**V1 - Blocker disables confirm.**
Pick a node with active descendants and trigger deactivate preview. The preview drawer
must show blockers and the final confirm button must be disabled.

**V2 - Warning-only preview.**
Pick a node move scenario that is valid but has responsibilities or bindings in the
subtree. The drawer must show warnings while still allowing the user to continue.

**V3 - Preview state reset.**
Open and close the preview drawer repeatedly. Confirm that old blockers and warnings do
not leak into the next preview request.

---

## Sprint S4 — Web-Parity Gap Closure (Node Type Admin, Equipment Toggle & Export)

> **Scope** — Three web‑parity features not covered by the current roadmap:
> a dedicated admin panel for managing organization node types with icons and
> colors, a per‑node equipment assignment widget, and a print / export function
> for the org chart.

### S4‑1 — Node Type Manager Panel (`NodeTypeManagerPanel.tsx`) — GAP ORG‑01

```
LOCATION   src/components/org/NodeTypeManagerPanel.tsx
STORE      org-store.ts (patch — add nodeTypes, createNodeType, updateNodeType,
           deleteNodeType)
SERVICE    org-service.ts (patch — add CRUD wrappers for node types)

DESCRIPTION
Accessible from OrganizationDesignerPage toolbar: "Manage Types" button (org.manage
permission guard). Opens as a Sheet (side panel) or routed sub-view.

Layout:
  ┌───────────────────────────────────────────────────────────────┐
  │  Organization Node Types                                      │
  │                                                               │
  │  ┌──────┬──────────┬──────┬──────────┬─────────────┬────────┐ │
  │  │ Icon │ Label    │ Color│ Capabilities           │ Actions│ │
  │  ├──────┼──────────┼──────┼─────────────────────────┼────────┤ │
  │  │ 🏭   │ Site     │ 🔵   │ ASSET WORK COST KPI    │ ✏️ 🗑️  │ │
  │  │ 🔧   │ Line     │ 🟢   │ ASSET WORK             │ ✏️ 🗑️  │ │
  │  │ 📋   │ Zone     │ 🟡   │ ASSET                  │ ✏️ 🗑️  │ │
  │  │ 👤   │ Team     │ 🟣   │ WORK COST              │ ✏️ 🗑️  │ │
  │  └──────┴──────────┴──────┴─────────────────────────┴────────┘ │
  │                                                               │
  │  [ + Add Type ]                                               │
  └───────────────────────────────────────────────────────────────┘

  Edit row (inline or dialog):
    - Label (text input, required)
    - Icon picker: grid of Lucide icon names, search by keyword
    - Color picker: 12 preset swatches (Tailwind palette) + custom hex input
    - Capability toggles: ASSET, WORK, COST, KPI, PERMIT (checkbox group)
    - allowed_children: multi-select of other node types

  Delete: blocked if node type is in use by > 0 nodes (show usage count).

ACCEPTANCE CRITERIA
- node types list loads from backend
- add / edit / delete with inline or dialog editing
- icon picker shows Lucide icons with search
- color picker shows 12 swatches + custom hex
- capability toggles map to backend flags
- delete blocked for in-use types
```

### S4‑2 — Equipment Assignment Widget — GAP ORG‑02

```
LOCATION   NodeInspectorPanel.tsx (patch — add "Equipment" tab)
STORE      org-node-store.ts (patch — add assignedAssets, assignAsset, unassignAsset)
SERVICE    org-node-service.ts (patch — add asset assignment IPC wrappers)

DESCRIPTION
New tab "Equipment" on NodeInspectorPanel (visible only when node type has ASSET
capability). Shows assets assigned to the selected organization node:

  ┌───────────────────────────────────────────────┐
  │  Equipment — Line 1 (3 assets)                │
  │                                               │
  │  ┌──────┬───────────┬───────────┬───────────┐ │
  │  │ Code │ Name      │ Status    │ Actions   │ │
  │  ├──────┼───────────┼───────────┼───────────┤ │
  │  │ P001 │ Pump A    │ 🟢 Active │ ❌ Remove │ │
  │  │ M003 │ Motor B   │ 🟡 Maint. │ ❌ Remove │ │
  │  └──────┴───────────┴───────────┴───────────┘ │
  │                                               │
  │  [ + Assign Asset ] (combobox → asset search) │
  └───────────────────────────────────────────────┘

  - "+ Assign Asset" opens a combobox searching unassigned assets
  - "Remove" unlinks asset from node (confirm dialog)
  - Assets assigned elsewhere show warning: "Currently assigned to {other node}"
  - Tab shows count badge: "Equipment (3)"

ACCEPTANCE CRITERIA
- tab visible only for ASSET-capable node types
- assign/remove updates asset's org_node_id
- reassignment shows current-assignment warning
- count badge updates on assign/remove
```

### S4‑3 — Org Chart Print / Export — GAP ORG‑03

```
LOCATION   src/components/org/OrgExportMenu.tsx
DEPENDENCY None (uses browser print API + canvas)

DESCRIPTION
Dropdown button on OrganizationDesignerPage toolbar: "Export" with options:
  - "Print Org Chart" → window.print() with @media print stylesheet
    that renders the tree in a clean hierarchical layout
  - "Export as PNG" → html-to-image (or dom-to-image-more) renders the
    OrganizationTreePanel to a canvas → download as PNG
  - "Export as CSV" → flat list: node_code, node_name, type, parent_code,
    capabilities, depth — downloaded as UTF-8 CSV

Print stylesheet (src/styles/org-print.css or Tailwind @media print):
  - hides sidebar, toolbar, inspector panel
  - tree renders full-width with indentation preserved
  - each node shows: icon, name, type badge, code
  - page breaks between depth-2 subtrees

ACCEPTANCE CRITERIA
- print preview shows clean tree without app chrome
- PNG export captures full tree (even if scrolled)
- CSV export includes all nodes with correct hierarchy
```

### Supervisor Verification — Sprint S4

**V1 — Node type CRUD.**
Open "Manage Types". Add a new type "Department" with blue color and WORK + COST
capabilities. Verify it appears. Edit its icon. Delete it (if unused).

**V2 — Equipment assignment.**
Select a node with ASSET capability. Go to Equipment tab. Assign an asset via
combobox. Verify count badge shows (1). Remove it. Verify badge goes away.

**V3 — Export.**
Click Export → Print. Verify print preview shows clean tree. Click Export → CSV.
Open downloaded CSV and verify hierarchy is correct.

---

*End of Phase 2 - Sub-phase 01 - File 03*
