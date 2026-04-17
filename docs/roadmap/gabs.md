# Consolidated Sprint S4 Gap Backlog

This document consolidates Sprint S4 gap backlog sections found under `desktop/docs/roadmap`.
Each section is copied verbatim from the first `## Sprint S4` heading to the end of its source file.
Use this single view for review and planning across roadmap documents.

Generated: 2026-04-08 | Total Sprint S4 sections: 21

## phase-1-secure-foundation/06-settings-updater-diagnostics-backup-and-restore-preflight/01-settings-core-and-policy-loading.md

## Sprint S4 — Web-Parity Gap Closure (Settings Page & Policy Editor UI)

> **Scope** — This file explicitly states "the actual UI surface is a Phase 2
> deliverable" but no Phase 2 file ever defines it. Sprint S4 closes this gap
> by specifying the SettingsPage shell and policy editor panels that consume
> the backend built in Sprints S1–S3.

### S4‑1 — Settings Page (`SettingsPage.tsx`) — GAP SET‑01

```
LOCATION   src/pages/SettingsPage.tsx
ROUTE      /settings (replaces ModulePlaceholder)
STORE      settings-store.ts (already exists — add activeCategory, editingPolicy)
SERVICE    settings-service.ts (already exists)
GUARD      adm.settings permission

DESCRIPTION
Vertical-tab layout with category groups in left sidebar:

  ┌─────────────────────┬──────────────────────────────────────────┐
  │  Settings            │  General Settings                        │
  │                      │                                          │
  │  ▸ General           │  ┌─────────────────┬───────────────────┐ │
  │    App name          │  │ Setting         │ Value             │ │
  │    Default language  │  ├─────────────────┼───────────────────┤ │
  │    Date format       │  │ App name        │ Maintafox CMMS    │ │
  │  ▸ Security          │  │ Default lang    │ [FR ▼]            │ │
  │    Session policy    │  │ Date format     │ [DD/MM/YYYY ▼]   │ │
  │    Password policy   │  │ Timezone        │ [Auto ▼]         │ │
  │    Device trust      │  └─────────────────┴───────────────────┘ │
  │  ▸ Maintenance       │                                          │
  │    SLA defaults      │  [ Save Changes ] (disabled when clean)  │
  │    WO numbering      │                                          │
  │  ▸ Integration       │                                          │
  │    ERP connector     │                                          │
  │    IoT connector     │                                          │
  │  ▸ Backup            │                                          │
  │    Backup schedule   │                                          │
  │    Retention policy  │                                          │
  └─────────────────────┴──────────────────────────────────────────┘

- Categories loaded from settings domains (dynamic, not hardcoded)
- Direct-apply settings: edited inline, saved immediately with toast
- Governed settings (session policy, backup policy, etc.): use Draft → Test →
  Activate workflow via PolicyEditorPanel (see S4-2)
- Governed settings show current active value + "(draft pending)" badge if draft exists
- All setting changes are audited (audit_events table from Sprint S1)
- Search box at top of left sidebar — filters visible categories/settings

ACCEPTANCE CRITERIA
- route /settings loads with category sidebar and settings table
- direct-apply settings save on change with success toast
- governed settings show PolicyEditorPanel (not inline edit)
- page is permission-gated (adm.settings)
- settings changes appear in audit log
```

### S4‑2 — Policy Editor Panels — GAP SET‑02

```
LOCATION   src/components/settings/PolicyEditorPanel.tsx
STORE      settings-store.ts (patch — add draftPolicy, testResults, activatePolicy)
SERVICE    settings-service.ts (patch — add draft/test/activate IPC wrappers)

DESCRIPTION
Replaces inline editing for governed settings (session policy, password policy,
backup policy, connector credentials). Renders inside the right pane of SettingsPage
when a governed setting category is selected:

  ┌───────────────────────────────────────────────────────────────┐
  │  Session Policy                              Status: Active   │
  │                                                               │
  │  Active Configuration:          Draft (if exists):            │
  │  ┌──────────────────────┐       ┌──────────────────────┐     │
  │  │ Max session: 8h      │       │ Max session: 4h      │     │
  │  │ Idle timeout: 30min  │  →    │ Idle timeout: 15min  │     │
  │  │ Step-up: 120s        │       │ Step-up: 60s         │     │
  │  └──────────────────────┘       └──────────────────────┘     │
  │                                                               │
  │  [ Edit Draft ]  [ Test Draft ]  [ Activate ]  [ Discard ]   │
  │                                                               │
  │  Test Results (if run):                                       │
  │  ✅ Session timeout validation passed                         │
  │  ✅ Idle lock threshold within bounds                         │
  │  ⚠️ Step-up window very short (60s) — consider user impact    │
  │                                                               │
  │  Change History:                                              │
  │  2026-04-07 14:00 — admin — Activated v3                     │
  │  2026-04-07 13:55 — admin — Tested draft v4                  │
  │  2026-04-06 09:30 — admin — Created draft v4                 │
  └───────────────────────────────────────────────────────────────┘

Workflow:
  1. "Edit Draft" — opens form fields for the policy (pre-filled from active or
     existing draft). Saves as draft snapshot.
  2. "Test Draft" — runs backend validation rules. Shows test results panel with
     pass/warn/fail indicators.
  3. "Activate" — requires step-up auth for security policies. Promotes draft to
     active. Old active becomes superseded.
  4. "Discard" — deletes draft (confirm dialog).

Side-by-side diff: active vs draft, highlighting changed values in amber.

Policy-specific form fields vary by policy type:
  - Session: max_session_hours, idle_timeout_minutes, step_up_window_seconds
  - Password: min_length, require_uppercase, require_number, max_age_days,
    history_count
  - Backup: schedule_cron, retention_days, include_photos, compression_level

ACCEPTANCE CRITERIA
- draft → test → activate workflow works end-to-end
- test results show pass/warn/fail per validation rule
- activate requires step-up for security policies
- side-by-side diff highlights changed values
- change history loads from policy_snapshots + audit_events
```

### Supervisor Verification — Sprint S4

**V1 — Settings page navigation.**
Login as admin. Navigate to /settings. Verify category sidebar loads. Click "General" →
direct-apply settings appear. Click "Security" → governed settings show policy editor.

**V2 — Direct-apply setting.**
Change "Default language" to EN. Verify toast confirms save. Refresh page → setting
persists.

**V3 — Policy lifecycle.**
Edit session policy draft (change idle timeout). Test draft → verify test results. Activate
→ step-up required → policy becomes active. Verify old active is superseded in change
history.

**V4 — Permission guard.**
Login as non-admin user. Navigate to /settings → redirected or 403.

---

*End of Phase 1 · Sub-phase 06 · File 01*

---

## phase-2-core-execution-backbone/00-shared-ui-component-foundation/02-data-table-and-chart-primitives.md

## Sprint S4 — Web-Parity Gap Closure (Dashboard KPI Shell)

> **Scope** — The current DashboardPage shows only app version and online status.
> Full analytics is Phase 5, but the web reference has a functional KPI card + chart
> layout. Sprint S4 adds a reusable KPI card shell and a basic dashboard layout
> that Phase 2 modules can plug into as they ship.

### S4‑1 — Dashboard KPI Shell — GAP DSH‑01

```
LOCATION   src/pages/DashboardPage.tsx (patch — replace minimal content)
COMPONENT  src/components/dashboard/KpiCard.tsx
COMPONENT  src/components/dashboard/DashboardWorkloadChart.tsx
STORE      No new store — each KPI card calls its own IPC command
SERVICE    src/services/dashboard-service.ts (new — aggregation IPC wrappers)
COMMANDS   get_dashboard_kpis (Rust — queries counts from DI, WO, Asset, PM tables)

DESCRIPTION
Replaces the minimal DashboardPage with a KPI grid + workload chart:

  ┌────────────────────────────────────────────────────────────┐
  │  Dashboard                          Welcome, {display_name}│
  │                                                            │
  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐     │
  │  │ Open DIs │ │ Open WOs │ │ Assets   │ │ Overdue  │     │
  │  │    12    │ │     8    │ │   247    │ │    3     │     │
  │  │  ↑ 2     │ │  ↓ 1    │ │  — 0    │ │  ↑ 1    │     │
  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘     │
  │                                                            │
  │  ┌──────────────────────────────────────────────────────┐  │
  │  │  Workload — Last 7 Days                    [7d|30d]  │  │
  │  │                                                      │  │
  │  │  ██████                                              │  │
  │  │  ██████  ████                                        │  │
  │  │  ██████  ████  ██████  ████  ██████  ████  ██████    │  │
  │  │  Mon     Tue    Wed    Thu    Fri    Sat    Sun      │  │
  │  │                                                      │  │
  │  │  ■ DI Created  ■ WO Completed  ■ PM Due             │  │
  │  └──────────────────────────────────────────────────────┘  │
  │                                                            │
  │  Quick Actions                                             │
  │  [ + New DI ]  [ + New WO ]  [ + New Asset ]               │
  └────────────────────────────────────────────────────────────┘

KpiCard.tsx — Reusable component:
  Props: title, value (number), trend (number, positive=up), icon, color
  Renders: shadcn Card with icon, large value, trend arrow with delta
  Trend: ↑ green if positive change is good (assets), ↑ red if positive change
  is bad (overdue). Configurable via trendDirection: 'up-good' | 'up-bad'.

DashboardWorkloadChart.tsx:
  Reuses BarChart.tsx (from Sprint S2) with stacked bars
  Period selector: 7d / 30d toggle (segmented control)
  Data: get_dashboard_workload_chart IPC → {date, di_created, wo_completed, pm_due}[]
  Empty state: "Not enough data yet" with illustration

Quick Actions: permission-gated buttons linking to create flows:
  - "+ New DI" (di.create) → /requests?action=create
  - "+ New WO" (ot.create) → /work-orders?action=create (placeholder until WO ships)
  - "+ New Asset" (eq.manage) → /equipment?action=create

KPI data refreshes on page mount + every 5 minutes (setInterval).

ACCEPTANCE CRITERIA
- 4 KPI cards render with real counts from database
- trend arrows show change vs previous period
- bar chart renders with real workload data
- period toggle switches between 7d and 30d
- quick action buttons are permission-gated
- empty state renders gracefully when no data exists
```

### Supervisor Verification — Sprint S4

**V1 — KPI cards.**
With 12 open DIs and 8 open WOs in database, verify cards show correct counts. Verify
trend arrows show delta vs previous period.

**V2 — Workload chart.**
Verify stacked bar chart renders for last 7 days. Toggle to 30d. Verify chart updates.

**V3 — Quick actions.**
Login as user with di.create but not ot.create. Verify "+ New DI" visible, "+ New WO"
hidden.

---

*End of Phase 2 · Sub-phase 00 · File 02*

---

## phase-2-core-execution-backbone/01-organization-and-site-operating-model/03-org-designer-ui-and-impact-preview.md

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

---

## phase-2-core-execution-backbone/02-equipment-and-asset-registry-backbone/02-lifecycle-history-meters-and-document-links.md

## Sprint S4 — Web-Parity Gap Closure (Frontend Lifecycle & Health)

> **Scope** — Three web‑parity features missing from the roadmap: a decommission /
> retire modal with dependency analysis, a computed health‑score indicator, and a
> photo gallery for asset images. All backend foundations exist (lifecycle events,
> meters, document links); Sprint S4 adds the frontend surfaces.

### S4‑1 — Decommission / Retire Modal (`AssetDecommissionModal.tsx`) — GAP EQ‑04

```
LOCATION   src/components/assets/AssetDecommissionModal.tsx
STORE      asset-store.ts (patch — add decommissionAsset action + confirm state)
SERVICE    asset-lifecycle-service.ts (patch — add decommission_asset IPC wrapper)

DESCRIPTION
Triggered from AssetDetailPanel action menu or future tree context menu.
Shows:
  - asset identity header (code, name, class, status badge)
  - binding dependency list — counts from AssetBindingSummary domains:
    ┌───────────────┬──────────┬──────────────────────────────┐
    │ Domain        │ Count    │ Detail                       │
    ├───────────────┼──────────┼──────────────────────────────┤
    │ Open DIs      │ n        │ blocks if n > 0              │
    │ Open WOs      │ n        │ blocks if n > 0              │
    │ Active PMs    │ n        │ warning — will be suspended  │
    │ IoT bindings  │ n        │ warning — will be unlinked   │
    │ Documents     │ n        │ info — remain archived       │
    └───────────────┴──────────┴──────────────────────────────┘
  - blocker banner: "Cannot decommission — n open work items. Close them first."
  - reason textarea (required when no blockers)
  - target state selector: Retired | Scrapped | Transferred
  - confirm button disabled when blockers > 0 or reason empty

ACCEPTANCE CRITERIA
- open DI/WO blocks decommission
- reason is stored in lifecycle_events as event_data JSON
- after confirm, asset status changes and detail panel refreshes
```

### S4‑2 — Health Score Indicator — GAP EQ‑07

```
LOCATION   src/components/assets/AssetHealthBadge.tsx
COMMAND    get_asset_health_score (Rust — reads lifecycle events + meter readings)
SERVICE    asset-service.ts (patch — add getHealthScore IPC wrapper)

DESCRIPTION
Composite 0–100 score computed from:
  - time since last lifecycle event (age factor)
  - latest meter readings vs threshold (meter factor)
  - open DI/WO count (workload factor)
Displayed as a colored badge on AssetResultTable and AssetDetailPanel:
  - 80–100  green   "Good"
  - 50–79   amber   "Fair"
  - 0–49    red     "Poor"
  - null    gray    "No data"

Uses Tailwind badge variants from shadcn/ui Badge component.

ACCEPTANCE CRITERIA
- score computes from real lifecycle + meter data
- assets with no lifecycle events show "No data" (not 0)
- badge renders on both result table and detail panel
```

### S4‑3 — Photo Gallery (`AssetPhotoGallery.tsx`) — GAP EQ‑05

```
LOCATION   src/components/assets/AssetPhotoGallery.tsx
STORE      asset-store.ts (patch — add photos: AssetPhoto[], uploadPhoto, deletePhoto)
COMMAND    upload_asset_photo, list_asset_photos, delete_asset_photo
MIGRATION  patch document_links table or add asset_photos table

DESCRIPTION
Tab on AssetDetailPanel ("Photos") alongside existing Hierarchy/Lifecycle/Meters/Docs
tabs. Layout:
  - thumbnail grid (4 columns, aspect-ratio 1:1, object-cover)
  - click thumbnail → lightbox overlay with prev/next arrows + close button
  - upload button (eq.manage guard) → file picker restricted to image/* MIME types
  - max file size: 5 MB, validated client-side before upload
  - photos stored in app_data_dir/photos/{asset_id}/{uuid}.{ext}
  - delete button on lightbox (eq.manage guard + confirm dialog)
  - empty state: camera icon + "No photos" + upload CTA

ACCEPTANCE CRITERIA
- upload stores file to disk and creates DB row
- thumbnail grid lazy-loads (IntersectionObserver or native loading="lazy")
- lightbox navigates with keyboard arrows
- permission-gated: view needs eq.view, upload/delete needs eq.manage
```

### Supervisor Verification — Sprint S4

**V1 — Decommission blockers.**
Create an asset with 1 open DI. Open decommission modal → verify blocker banner shows and
confirm button is disabled. Close the DI. Reopen modal → confirm button enabled.

**V2 — Health score display.**
Asset with recent lifecycle event and normal meter readings shows green badge. Asset with
no events shows gray "No data".

**V3 — Photo upload cycle.**
Upload a 3 MB JPEG to an asset. Verify thumbnail appears. Click to open lightbox. Delete
photo and confirm it disappears from grid.

---

*End of Phase 2 - Sub-phase 02 - File 02*

---

## phase-2-core-execution-backbone/02-equipment-and-asset-registry-backbone/03-asset-ui-search-and-cross-module-bindings.md

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

---

## phase-2-core-execution-backbone/03-lookup-and-reference-data-governance/01-reference-domain-model-and-versioning.md

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

---

## phase-2-core-execution-backbone/03-lookup-and-reference-data-governance/02-lookup-management-workflows-and-protected-domains.md

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

---

## phase-2-core-execution-backbone/03-lookup-and-reference-data-governance/03-aliases-imports-and-search-behavior.md

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

---

## phase-2-core-execution-backbone/03-lookup-and-reference-data-governance/04-reference-validation-publish-controls-and-audit.md

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

---

## phase-2-core-execution-backbone/04-intervention-requests-di/01-di-domain-model-and-state-machine.md

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

---

## phase-2-core-execution-backbone/04-intervention-requests-di/02-di-intake-review-and-approval-flows.md

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

---

## phase-2-core-execution-backbone/04-intervention-requests-di/03-di-sla-attachments-and-wo-conversion.md

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

---

## phase-2-core-execution-backbone/04-intervention-requests-di/04-di-permissions-tests-and-audit-coverage.md

## Sprint S4 — Dashboard, Calendar View, Archive View, Context Menu, and Stats Endpoint

> **Gap addressed:** The web app has 4 views (list, kanban, calendar, dashboard)
> plus an archive section, context menus, and a /stats analytics endpoint. S1–S3
> delivered list + kanban + detail dialog but no dashboard, calendar, archive, or
> context menu. The backend also lacks a stats aggregation command. This sprint
> closes all remaining UI and analytics gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/di/stats.rs` | DI statistics aggregation queries |
| `src-tauri/src/commands/di.rs` (patch) | `get_di_stats` IPC command |
| `src/services/di-stats-service.ts` | Frontend wrapper for stats endpoint |
| `src/stores/di-stats-store.ts` | Zustand store for dashboard data |
| `src/components/di/DiDashboardView.tsx` | KPI cards + charts dashboard |
| `src/components/di/DiCalendarView.tsx` | Month / week / day calendar view |
| `src/components/di/DiArchivePanel.tsx` | Archive table for rejected + archived DIs |
| `src/components/di/DiContextMenu.tsx` | Right-click context menu for DI rows / cards |
| `src/pages/RequestsPage.tsx` (patch) | Add Dashboard + Calendar view tabs; archive section; context menu integration |
| `src/i18n/locale-data/{fr,en}/di.json` (patch) | Dashboard, calendar, archive labels |

### get_di_stats — Backend Stats Command

`get_di_stats` — requires `di.view`

**Input:** `DiStatsFilter`

```rust
struct DiStatsFilter {
    date_from: Option<String>,   // ISO date
    date_to: Option<String>,     // ISO date
    entity_id: Option<i64>,      // Org node scope
}
```

**Output:** `DiStatsPayload`

```rust
struct DiStatsPayload {
    // KPI card values
    total: i64,
    pending: i64,
    in_progress: i64,        // submitted + pending_review + screened + awaiting_approval
    closed: i64,             // closed_as_non_executable + archived
    closed_this_month: i64,
    overdue: i64,            // SLA breached
    sla_met_count: i64,      // DIs resolved within SLA
    sla_total: i64,          // DIs with SLA rules applied
    safety_issues: i64,      // safety_flag = 1

    // Distributions
    status_distribution: Vec<StatusCount>,      // { status, count }
    priority_distribution: Vec<PriorityCount>,  // { priority, count }
    type_distribution: Vec<TypeCount>,          // { origin_type, count }

    // Trend (adaptive granularity: daily if ≤93 days, monthly otherwise)
    monthly_trend: Vec<TrendPoint>,   // { period, created, closed }
    available_years: Vec<i32>,

    // Age stats (for open DIs)
    avg_age_days: f64,
    max_age_days: f64,

    // Tables
    top_equipment: Vec<EquipmentCount>,  // { asset_id, asset_label, count, percentage }
    overdue_dis: Vec<OverdueDi>,         // { id, code, title, priority, days_overdue }
}
```

### DiDashboardView.tsx — Dashboard Component

**Layout:**

**Row 1 — KPI Cards** (8 cards, responsive grid):
| Card | Value | Colour |
|------|-------|--------|
| Total | `total` | neutral |
| En attente | `pending` | amber |
| En cours | `in_progress` | blue |
| SLA respecté | `sla_met_count / sla_total` (%) | green if ≥80%, red otherwise |
| Clôturées (mois) | `closed_this_month` | teal |
| En retard | `overdue` | red |
| Enjeux sécurité | `safety_issues` | destructive |
| Âge moyen | `avg_age_days` days | muted |

**Row 2 — Charts** (responsive 2-col grid):
- **Status doughnut** — status_distribution (Recharts PieChart)
- **Priority bar** — priority_distribution (horizontal BarChart)

**Row 3 — Trend** (full width):
- Monthly trend (ComposedChart: bars = "created", line = "closed")
- Period pills: 3M / 6M / 12M / 24M / All
- Year picker + month picker for drill-down

**Row 4 — Tables** (2-col):
- **Top 10 Equipment** — asset_label, count, percentage bar
- **Overdue DIs** — code, title, priority badge, days overdue (red)

### DiCalendarView.tsx — Calendar View

**Props:** Uses `items` from di-store.

**Modes (toggle buttons):**
- **Month** — 7-day grid; DIs indexed by `submitted_at`; up to 3 chips per day cell, "+N" overflow
- **Week** — 7-day × 24-hour grid; all-day row above; DIs positioned by `submitted_at`
- **Day** — Full 24-hour timeline for single day

**Chip rendering:** Each DI chip shows code + priority color bar.
- Click chip → opens DiDetailDialog
- Right-click → DiContextMenu

**Navigation:** Prev / Next buttons, "Today" button, month/week/day toggle.

### DiArchivePanel.tsx — Archive View

Collapsible section at the bottom of RequestsPage.

**Content:**
- Header: "Archive" with badge count
- DataTable: code, title, equipment, status (Rejected / Archived), requester, date, reason
- Filters: search, status (rejected / archived)
- No edit actions — read-only

**Data source:** `loadDis({ status: ['rejected', 'archived'] })`.

### DiContextMenu.tsx — Right-Click Menu

Uses Radix ContextMenu or custom dropdown.

**Menu items (conditional on permissions + DI status):**

| Item | Condition | Action |
|------|-----------|--------|
| Voir détail | always | opens DiDetailDialog |
| Modifier | `di.create.own` + status=submitted/pending draft | opens DiFormDialog in edit mode |
| Supprimer | `di.admin` + status=submitted | delete confirmation dialog |
| Approuver | `di.approve` + status=awaiting_approval | opens DiApprovalDialog |
| Rejeter | `di.review` + status in review states | opens DiRejectionDialog |
| Renvoyer | `di.review` + status=pending_review | opens DiReturnDialog |
| Assigner | `ot.assign` (future) | opens assignment picker |

Integrate into:
- DiKanbanBoard (right-click on card)
- DataTable list rows (right-click on row)
- DiCalendarView (right-click on chip)

### RequestsPage.tsx Patch — View Tabs

Update the view toggle to include 4 modes:

```tsx
type ViewMode = "list" | "kanban" | "calendar" | "dashboard";
```

Add icons: List, Columns3, CalendarDays, BarChart3.

Below the main view area, render `<DiArchivePanel />` as a collapsible section.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors (Rust + TypeScript)
- get_di_stats returns correct KPI counts matching database state
- DiDashboardView renders 8 KPI cards and 4 chart/table sections
- DiCalendarView renders month/week/day modes with DI chips on correct dates
- DiArchivePanel shows only rejected + archived DIs
- DiContextMenu respects permission gates (items hidden for unauthorized users)
- Trend granularity switches: daily when range ≤ 93 days, monthly otherwise
- SLA % card shows green when ≥ 80%, red otherwise
- RequestsPage has 4 view tabs: List, Kanban, Calendar, Dashboard
```

### Supervisor Verification — Sprint S4

**V1 — Dashboard KPIs.**
Create 5 DIs with mixed statuses; verify KPI cards show correct counts.

**V2 — Calendar chip click.**
Click a DI chip in month view; DiDetailDialog must open.

**V3 — Context menu permission.**
Right-click a DI as a user without `di.approve`; "Approuver" must not appear.

**V4 — Archive isolation.**
DiArchivePanel must not show submitted/pending DIs; only rejected and archived.

**V5 — Stats trend granularity.**
Request stats with a 30-day range; verify daily data points. Request with 180-day range;
verify monthly aggregation.

---

*End of Phase 2 - Sub-phase 04 - File 04*

---

## phase-2-core-execution-backbone/05-work-orders-ot/01-wo-domain-model-and-execution-states.md

## Sprint S4 — WO Create / Edit Form, WorkOrdersPage, and DI Management Panel

> **Gap addressed:** S1–S3 defined the backend (`create_wo`, `update_wo_draft`),
> the service, and the store — but no page-level layout or creation form. The web
> app provides a "Créer un OT" button, a create modal with equipment/type/urgency/
> dates/description fields, a DI management panel (unscheduled DI-sourced WOs), and
> a WorkOrdersPage with list/kanban/calendar tabs. This sprint delivers the
> equivalent desktop components.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/wo/WoCreateForm.tsx` | Create / edit-draft form for a work order |
| `src/components/wo/WoFormDialog.tsx` | Dialog wrapper for WoCreateForm (UX-DW-001) |
| `src/components/wo/WoDiManagementPanel.tsx` | Banner showing unscheduled DI-sourced WOs with quick "Schedule" action |
| `src/pages/WorkOrdersPage.tsx` | Full WO workspace page with 4 view tabs, filters, DI panel |
| `src/stores/wo-store.ts` (patch) | `openCreateForm`, `closeCreateForm`, `closeWo` actions |
| `src/i18n/locale-data/fr/ot.json` (patch) | Page, form, action, DI panel labels |
| `src/i18n/locale-data/en/ot.json` (patch) | Page, form, action, DI panel labels |

### WoCreateForm.tsx — Create/Edit Form

**Props:**

```ts
interface WoCreateFormProps {
  initial: WorkOrder | null;   // null = create, non-null = edit draft
  onSubmitted: (wo: WorkOrder) => void;
  onCancel: () => void;
}
```

**Layout:** Single scrollable form, 2 sections.

**Section 1 — Work Order Info**

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| `type_id` | Select (7 WO types) | ✓ | Correctif, Préventif, Prédictif, Amélioratif, Inspection, Overhaul, Condition-Based |
| `equipment_id` | Asset combobox (search) | ✗ | Same pattern as DiCreateForm equipment picker |
| `location_id` | Org-node picker | ✗ | |
| `entity_id` | Org-node picker | ✗ | |
| `urgency_id` | Select (5 urgency levels) | ✗ | Color-coded: VeryLow → Critical |
| `title` | Input | ✓ | Max 200 chars |
| `description` | Textarea | ✗ | |

**Section 2 — Planning (optional at creation)**

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| `planned_start` | Datetime picker | ✗ | ISO datetime |
| `planned_end` | Datetime picker | ✗ | Must be ≥ `planned_start` (client-side validation) |
| `expected_duration_hours` | Number input | ✗ | Hours estimate |
| `notes` | Textarea | ✗ | |

**Behaviour:**
- Create mode: calls `submitNewWo(input)` → default status = **draft**
- Edit mode: calls `updateDraft(input)` with `expected_row_version`
- Uses `type_id` select populated from `work_order_types` (list_wo_types lookup)
- Uses `urgency_id` select populated from `urgency_levels` (list_urgencies lookup)

### WoFormDialog.tsx — Dialog Wrapper

UX-DW-001 pattern:

```
<Dialog open={showCreateForm}>
  <DialogContent className="max-w-2xl max-h-[90vh]">
    <DialogHeader>
      <DialogTitle>{create ? t("page.titleNew") : t("page.titleEdit")}</DialogTitle>
    </DialogHeader>
    <WoCreateForm initial={...} onSubmitted={...} onCancel={...} />
  </DialogContent>
</Dialog>
```

### WoDiManagementPanel.tsx — DI-Sourced WO Queue

Renders a collapsible banner above the main view area for users with `ot.edit` permission.

**Condition:** Only visible when there are WOs with `source_di_id IS NOT NULL` and
`status_id IN (1, 2)` (draft or planned — not yet scheduled/assigned).

**Content:**
- Header: "OT issus de DI non programmés (3)" / "Unscheduled DI work orders (3)" + badge
- Compact table per row: WO code, linked DI code, equipment, urgency badge, created date
- "Programmer" / "Schedule" button per row → opens WoPlanningPanel (from File 02)

### WorkOrdersPage.tsx — Full Page Layout

**Structure (same pattern as RequestsPage):**

```
┌─ Page header ─────────────────────────────────────────┐
│  Wrench icon  "Ordres de travail"  [total badge]      │
│  [+ New OT]  [views: List|Kanban|Calendar|Dashboard]  │
│  [filters: search, status, type, urgency, entity]     │
│  [Refresh]                                            │
├───────────────────────────────────────────────────────┤
│  [WoDiManagementPanel — collapsible]                  │
├───────────────────────────────────────────────────────┤
│  Main view area (list / kanban / calendar / dashboard) │
└───────────────────────────────────────────────────────┘
```

**View modes:**

```tsx
type WoViewMode = "list" | "kanban" | "calendar" | "dashboard";
```

**List view:** DataTable with columns: Code, Title, Equipment, Type, Urgency, Status,
Assignee, Planned End, Actions (view, print). Row click → WoDetailDialog.

**Kanban view:** WoKanbanView (already specified in File 04).

**Calendar view:** WoCalendarView (specified below in Sprint S4 of File 04).

**Dashboard view:** WoDashboardView (specified below in Sprint S4 of File 04).

**Filters (always visible, above view area):**
- Search input (debounce 300ms, searches code + title + equipment name)
- Status multi-select (12 statuses)
- Type multi-select (7 types)
- Urgency multi-select (5 levels)
- Entity select (org nodes)
- Clear all button

**"New OT" button:**

```tsx
<Button onClick={() => openCreateForm()} className="gap-1.5">
  <Plus className="h-3.5 w-3.5" />
  {t("action.create")}
</Button>
```

Visible to users with `ot.create` permission.

### wo-store.ts Patch

Add state:
- `showCreateForm: boolean` (default false)
- `editingWo: WorkOrder | null`

Add actions:
- `openCreateForm(wo?: WorkOrder)` → `set({ showCreateForm: true, editingWo: wo ?? null })`
- `closeCreateForm()` → `set({ showCreateForm: false, editingWo: null })`
- `closeWo()` → `set({ activeWo: null })` (for detail dialog close)

### i18n Additions

```json
{
  "page": { "title": "Ordres de travail" / "Work Orders", "titleNew": "Nouvel OT" / "New Work Order", "titleEdit": "Modifier l'OT" / "Edit Work Order" },
  "action": { "create": "Nouvel OT" / "New WO", "schedule": "Programmer" / "Schedule" },
  "form": {
    "type": { "label": "Type" },
    "urgency": { "label": "Urgence" / "Urgency" },
    "equipment": { "label": "Équipement" / "Equipment", "placeholder": "Rechercher..." / "Search..." },
    "location": { "label": "Emplacement" / "Location" },
    "entity": { "label": "Entité" / "Entity" },
    "title": { "label": "Titre" / "Title" },
    "description": { "label": "Description" },
    "plannedStart": { "label": "Début prévu" / "Planned start" },
    "plannedEnd": { "label": "Fin prévue" / "Planned end" },
    "duration": { "label": "Durée estimée (h)" / "Estimated duration (h)" },
    "notes": { "label": "Remarques" / "Notes" }
  },
  "diPanel": {
    "title": "OT issus de DI non programmés" / "Unscheduled DI work orders",
    "schedule": "Programmer" / "Schedule"
  }
}
```

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- WoCreateForm renders all fields with correct types and labels
- Create mode calls submitNewWo; status defaults to draft
- Edit mode pre-fills from initial WO and calls updateDraft
- "New OT" button visible only for ot.create holders
- WoDiManagementPanel shows only DI-sourced WOs in draft/planned status
- WoDiManagementPanel "Schedule" button opens planning panel
- WorkOrdersPage has 4 view tabs with correct icons
- Filters apply to all views (list, kanban, calendar, dashboard)
- Equipment combobox search debounced at 300ms
```

### Supervisor Verification — Sprint S4

**V1 — Create WO flow.**
Click "New OT"; fill required fields; submit; verify new WO row in list with status=draft.

**V2 — DI management panel.**
Create a WO with `source_di_id` in planned status; verify it appears in the panel.
Schedule it (assign); verify it disappears from the panel.

**V3 — Permission gate.**
User WITHOUT `ot.create`; "New OT" button must not render.

**V4 — Edit draft.**
Open form with an existing draft WO; all fields pre-filled; submit calls `updateDraft`.

---

*End of Phase 2 - Sub-phase 05 - File 01*

---

## phase-2-core-execution-backbone/05-work-orders-ot/02-wo-planning-labor-parts-and-delay-capture.md

## Sprint S4 — Shift Planning, Completion Dialog, and Execution UI Refinements

> **Gap addressed:** The web app has (1) a shift selector (morning/afternoon/night/
> day) for scheduling, (2) a completion modal (end date, hours worked, report), and
> (3) structured labor entry with start/stop buttons in the execution panel. S1–S3
> delivered the planning panel and execution controls but did not specify the shift
> field, the completion modal, or the detail dialog wrapper. This sprint adds them.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000021_wo_domain_core.rs` (patch) | Add `shift` column to `work_orders` table |
| `src-tauri/src/wo/domain.rs` (patch) | Add `WoShift` enum: `morning`, `afternoon`, `night`, `full_day` |
| `src/components/wo/WoPlanningPanel.tsx` (patch) | Add shift selector in timing section |
| `src/components/wo/WoCompletionDialog.tsx` | Completion modal: end date, hours worked, observations |
| `src/components/wo/WoDetailDialog.tsx` | Full detail dialog wrapper (UX-DW-001) hosting all WO sub-panels |
| `src/components/wo/WoExecutionControls.tsx` (patch) | Labor start/stop buttons with timer; task inline completion; parts usage input |
| `src/i18n/locale-data/{fr,en}/ot.json` (patch) | Shift labels, completion labels |

### Schema Addition: `shift` Column

Add to `work_orders` table (migration 021 patch):

```sql
ALTER TABLE work_orders ADD COLUMN shift TEXT NULL;
-- Values: 'morning' | 'afternoon' | 'night' | 'full_day'
```

Add to `WoCreateInput`, `WoDraftUpdateInput`, and `WoPlanInput` structs:

```rust
pub shift: Option<String>,  // "morning" | "afternoon" | "night" | "full_day"
```

### WoPlanningPanel.tsx Patch — Shift Selector

Add to the Timing section (below planned_end, above expected_duration):

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| `shift` | Select | ✗ | 4 options: Matin / Après-midi / Nuit / Journée (Morning / Afternoon / Night / Full Day) |

### WoCompletionDialog.tsx — Completion Confirmation

Triggered from WoExecutionControls "Complete" button (when WO is in_progress or en_attente).

**Layout (dialog `max-w-lg`):**

| Field | Control | Required | Notes |
|-------|---------|----------|-------|
| End date/time | Datetime picker | ✓ | Pre-filled with current timestamp |
| Hours worked | Number input | ✗ | Optional manual override of calculated hours |
| Report / Observations | Textarea | ✗ | Technician completion report |

**Actions (footer):**
- "Annuler" / "Cancel"
- "Marquer terminé" / "Mark Complete" (orange) — Calls `complete_wo_mechanically`

**Pre-flight error display:**
If `complete_wo_mechanically` returns blocking errors (open labor, incomplete mandatory tasks,
missing parts actuals, open downtime segments), display each as a separate error item with
descriptive text and icon.

### WoDetailDialog.tsx — Full Detail Dialog

Follows UX-DW-001 pattern (`docs/UX_DETAIL_DIALOG_PATTERN.md`).

**Layout (`max-w-4xl`, `max-h-[90vh]`):**

```
┌─ Header: WO code + title + status badge + urgency badge + X ─┐
├─────────────────────────────────────────────────────────────────┤
│  Scrollable body with Tabs:                                    │
│  ┌─────┬──────────┬──────────┬──────────┬──────────────┐      │
│  │Plan │ Execution│ Close-out│ Audit    │ Attachments  │      │
│  └─────┴──────────┴──────────┴──────────┴──────────────┘      │
│                                                                 │
│  [Tab content: WoPlanningPanel / WoExecutionControls /         │
│   WoCloseOutPanel / WoAuditTimeline / WoAttachmentPanel]       │
├─────────────────────────────────────────────────────────────────┤
│  Footer: context-appropriate buttons (Schedule, Start, Close) │
└─────────────────────────────────────────────────────────────────┘
```

**Tab visibility rules:**
- **Plan** — always visible; editable only in draft/planned/ready_to_schedule
- **Execution** — visible once assigned or later
- **Close-out** — visible once mechanically_complete or later
- **Audit** — always visible (read-only)
- **Attachments** — always visible

**Footer actions (conditional on status + permissions):**
| Status | Actions |
|--------|---------|
| draft / planned | "Programmer" (plan_wo) |
| ready_to_schedule | "Assigner" (assign_wo) |
| assigned | "Démarrer" (start_wo) + "Assigner" (re-assign) |
| in_progress | "Pause" + "Terminer" (mech complete) |
| paused | "Reprendre" (resume_wo) |
| mechanically_complete | "Vérifier" + "Clôturer" |
| technically_verified | "Clôturer" (close_wo) |

### WoExecutionControls.tsx Patch — Labor Start/Stop

Enhance the labor sub-section:
- Each labor entry row: Intervener name, skill, Start button ("Début"), Stop button ("Fin"),
  hours display (auto-computed), hourly rate, notes
- Start button: calls `add_labor(started_at = now)`; row enters "in progress" state
- Stop button: calls `close_labor(ended_at = now)`; row shows calculated hours
- Manual entry fallback: if start/stop buttons not used, user can enter hours_worked directly

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- Shift selector renders 4 options in WoPlanningPanel
- Shift value persisted via plan_wo command
- WoCompletionDialog pre-fills end date with now()
- Pre-flight errors from complete_wo_mechanically shown as individual items in dialog
- WoDetailDialog renders 5 tabs with correct visibility per status
- Labor start/stop buttons update intervener started_at / ended_at correctly
- Footer actions change based on WO status
```

### Supervisor Verification — Sprint S4

**V1 — Shift persistence.**
Select "Nuit" in planning panel; plan WO; reopen detail; verify shift shows "Nuit".

**V2 — Completion pre-flight errors.**
Attempt mech-complete with open labor entry; verify "Unclosed labor entries" error in dialog.

**V3 — Detail dialog tabs.**
Open a draft WO; only Plan + Audit + Attachments tabs visible. Open an in_progress WO;
Execution tab also visible.

**V4 — Labor start/stop.**
Click Start on a labor row; verify started_at is set. Click Stop; verify hours_worked computed.

---

*End of Phase 2 - Sub-phase 05 - File 02*

---

## phase-2-core-execution-backbone/05-work-orders-ot/03-wo-closeout-verification-and-cost-posting-hooks.md

## Sprint S4 — Print WO Fiche and Cost Display Panel

> **Gap addressed:** The web app provides a "Print" button on each WO detail that
> opens a professional A4 document (company header, reference strip, equipment,
> planning, description, visa/signatures, footer). It also displays cost summary
> inline in the detail view. S1–S3 specified `get_cost_summary` but no print
> component and no cost display panel. This sprint closes both gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/wo/WoPrintFiche.tsx` | Print-ready A4 WO fiche layout |
| `src/components/wo/WoCostSummaryCard.tsx` | Inline cost display in WO detail |
| `src/components/wo/WoDetailDialog.tsx` (patch) | "Print" button in footer; cost summary card in close-out tab |
| `src/i18n/locale-data/{fr,en}/ot.json` (patch) | Print and cost labels |

### WoPrintFiche.tsx — Professional A4 Document

Triggered by "Imprimer" / "Print" button in WoDetailDialog footer.

**Behaviour:** Opens a new browser window (via `window.open`) with print-optimized HTML.

**Layout (A4 portrait, CSS `@media print` optimized):**

```
┌─────────────────────────────────────────────┐
│  Company logo (left)   Company name (right)  │
│  Address, contact info                       │
│  DOCUMENT TITLE: "Fiche d'Ordre de Travail" │
│  Reference: WOR-0001  |  Date: 08/04/2026   │
├─────────────────────────────────────────────┤
│  IDENTIFICATION                              │
│  Code: WOR-0001                              │
│  Type: Correctif   Urgence: Haute            │
│  Statut: En cours  Créé le: 01/04/2026       │
├─────────────────────────────────────────────┤
│  ÉQUIPEMENT CONCERNÉ                         │
│  Désignation: Pompe centrifuge #12           │
│  Entité: Usine Nord                          │
├─────────────────────────────────────────────┤
│  PLANIFICATION                               │
│  Début prévu: 02/04/2026  08:00              │
│  Fin prévue:  03/04/2026  16:00              │
│  Poste: Matin                                │
│  Durée estimée: 8h                           │
├─────────────────────────────────────────────┤
│  DEMANDEUR & DESCRIPTION                     │
│  Demandeur: Jean Dupont                      │
│  Description: [full text]                    │
│  Source DI: DI-0042                          │
├─────────────────────────────────────────────┤
│  TÂCHES                                      │
│  1. Démontage pompe  [✓ OK]                  │
│  2. Remplacement joint  [✓ OK]               │
│  3. Test étanchéité  [ ]                      │
├─────────────────────────────────────────────┤
│  INTERVENANTS                                │
│  Dupont J. (Responsable), Heures: 4h         │
│  Martin P. (Exécutant), Heures: 6h           │
├─────────────────────────────────────────────┤
│  PIÈCES UTILISÉES                            │
│  Joint SKF 6205 — Qté: 2                     │
│  Roulement NTN 7208 — Qté: 1                │
├─────────────────────────────────────────────┤
│  VISAS & SIGNATURES                          │
│  ┌───────────┬───────────┬───────────┐      │
│  │ Demandeur │ Exécutant │ Resp.Mnt. │      │
│  │           │           │           │      │
│  │ _______   │ _______   │ _______   │      │
│  │ Date:     │ Date:     │ Date:     │      │
│  └───────────┴───────────┴───────────┘      │
├─────────────────────────────────────────────┤
│  Réf: WOR-0001 | Maintafox | 08/04/2026     │
│  Document confidentiel                       │
└─────────────────────────────────────────────┘
```

**CSS:** `@page { size: A4; margin: 15mm; }` — no page break inside signature table.
**Data:** Loads tasks, interveners, parts via existing list commands.
**Company info:** Read from settings store (`settings.company.*`).

### WoCostSummaryCard.tsx — Inline Cost Display

**Props:** `{ woId: number }`

**Layout (Card):**
| Row | Value | Notes |
|-----|-------|-------|
| Main-d'œuvre / Labor | `labor_cost` | Sum of interveners (hours × rate) |
| Pièces / Parts | `parts_cost` | Sum of parts (qty × unit_cost) |
| Services / Services | `service_cost` | Manual vendor entry |
| **Total** | `total_cost` | **Bold**, highlighted |
| Écart / Variance | `expected_duration - actual_duration` | Show ▲/▼ arrow + colour |

**Data:** Calls `get_cost_summary(woId)` on mount and when WO status changes.

Render inside WoCloseOutPanel (Section 3 bottom) and in WoDetailDialog as a summary badge.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- WoPrintFiche opens a new window with A4-optimized layout
- Print includes all sections: identification, equipment, planning, description, tasks, interveners, parts, signatures
- Company header populated from settings
- WoCostSummaryCard shows correct cost breakdown matching get_cost_summary values
- Variance shows positive/negative indicator with appropriate colour
- Print CSS prevents page breaks inside signature table
```

### Supervisor Verification — Sprint S4

**V1 — Print fiche.**
Open WO detail; click Print; verify new window with correct A4 layout, company header,
and signature table.

**V2 — Cost summary.**
WO with labor 4h × 50 = 200, parts 2 × 45 = 90, service = 30; cost card shows total = 320.

**V3 — Variance.**
WO with expected_duration=8h, actual_duration=10h; variance shows "+2h" in red.

---

*End of Phase 2 - Sub-phase 05 - File 03*

---

## phase-2-core-execution-backbone/05-work-orders-ot/04-wo-permissions-tests-and-analytics-readiness.md

## Sprint S4 — Calendar View, Dashboard / Stats, Context Menu, and Archive

> **Gap addressed:** The web app provides three presentation modes (List / Kanban /
> Calendar) plus a dashboard section with KPIs, and a right-click context menu
> for quick actions. S1–S3 specified `get_wo_analytics_snapshot` but no calendar
> component, no dashboard view, no context menu, and no archive panel. This sprint
> closes all four gaps and wires the WorkOrdersPage view selector created in
> File 01 — Sprint S4.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/components/wo/WoCalendarView.tsx` | Month/week calendar with urgency-coloured chips |
| `src/components/wo/WoDashboardView.tsx` | KPI cards + charts (status, urgency, trend, backlog) |
| `src/components/wo/WoContextMenu.tsx` | Right-click quick-action menu on WO rows/cards |
| `src/components/wo/WoArchivePanel.tsx` | Archived (Fermé/Annulé) WO browser |
| Backend: `get_wo_stats` IPC command (optional extension) | Aggregated KPI data for dashboard |
| `src/i18n/locale-data/{fr,en}/ot.json` (patch) | Calendar, dashboard, and context menu labels |

### WoCalendarView.tsx — Calendar Presentation

**Props:** `{ workOrders: WorkOrder[], onSelect: (wo) => void }`

**Behaviour:**
- Default view: **Month**. Toggle to **Week** via top-right button.
- Each WO renders as a coloured chip on its `planned_start` date cell.
- **Chip colour** follows urgency:
  - Basse → `bg-emerald-100 text-emerald-800`
  - Normale → `bg-blue-100 text-blue-800`
  - Haute → `bg-amber-100 text-amber-800`
  - Critique → `bg-red-100 text-red-800`
- Click chip → calls `onSelect(wo)` (opens WoDetailDialog via wo-store).
- Month navigation via ← / → arrows.
- "Today" button resets to current month/week.

**Layout (Month):**
```
┌──────────────────────────────────────────────────┐
│  ← April 2026 →                [Week] [Month]   │
├──────┬──────┬──────┬──────┬──────┬──────┬──────┤
│ Lun  │ Mar  │ Mer  │ Jeu  │ Ven  │ Sam  │ Dim  │
├──────┼──────┼──────┼──────┼──────┼──────┼──────┤
│      │  1   │  2   │  3   │  4   │  5   │  6   │
│      │      │ [WO] │      │ [WO] │      │      │
│      │      │ [WO] │      │      │      │      │
├──────┼──────┼──────┼──────┼──────┼──────┼──────┤
│  7   │  8   │  9   │ 10   │ 11   │ 12   │ 13   │
│      │ [WO] │      │      │ [WO] │      │      │
│      │      │      │      │ [WO] │      │      │
│      │      │      │      │ [WO] │      │      │
└──────┴──────┴──────┴──────┴──────┴──────┴──────┘
```

### WoDashboardView.tsx — KPI Cards and Charts

**Props:** `{ period?: { from: Date, to: Date } }`

Renders inside WorkOrdersPage when view selector = "Dashboard".

**KPI Cards (top row, 4 cards):**

| Card | Value | Icon | Colour |
|------|-------|------|--------|
| Total OT | `stats.total` | `ClipboardList` | blue |
| En cours | `stats.in_progress` | `PlayCircle` | amber |
| Terminés | `stats.completed` | `CheckCircle2` | emerald |
| En retard | `stats.overdue` | `AlertTriangle` | red |

**Charts (2 × 2 grid, using Recharts):**

| Position | Chart | Type |
|----------|-------|------|
| Top-left | Status Distribution | Donut (PieChart) |
| Top-right | Urgency Breakdown | Horizontal bar (BarChart) |
| Bottom-left | Completion Trend (30 days) | Area chart (AreaChart) |
| Bottom-right | Backlog Heatmap by Entity | Treemap or bar |

**Data source:** Calls `get_wo_analytics_snapshot` (already spec'd in S1). If additional
aggregations are needed, extend with a `get_wo_stats` command returning
`WoStatsPayload { total, in_progress, completed, overdue, by_status, by_urgency,
daily_completed: Vec<(String, i32)>, by_entity: Vec<(String, i32)> }`.

### WoContextMenu.tsx — Right-Click Quick Actions

**Props:** `{ wo: WorkOrder, position: { x, y }, onClose: () => void }`

Triggered on right-click from WoKanbanCard and WO list rows.

**Menu Items (conditional on status and permissions):**

| Label | Icon | Condition | Action |
|-------|------|-----------|--------|
| Voir détail / View detail | `Eye` | always | `openWo(wo.id)` |
| Modifier / Edit | `Pencil` | status ∈ {Brouillon, Planifié} | open WoFormDialog in edit mode |
| Démarrer / Start | `Play` | status = Planifié | `start_wo` |
| Compléter / Complete | `CheckCircle` | status = En_cours | open WoCompletionDialog |
| Imprimer / Print | `Printer` | always | open WoPrintFiche |
| Dupliquer / Duplicate | `Copy` | always | pre-fill WoFormDialog with WO data |
| Annuler / Cancel | `XCircle` | status ∉ {Fermé, Annulé} | confirm dialog → `cancel_wo` |

**Implementation:** Use Radix `ContextMenu` primitive (`@radix-ui/react-context-menu`).

### WoArchivePanel.tsx — Archived WO Browser

Accessible via an "Archive" toggle at the bottom of WorkOrdersPage or as a 5th tab
alongside List/Kanban/Calendar/Dashboard.

**Behaviour:**
- Lists WOs with `status ∈ {Fermé, Annulé}`.
- Sortable columns: code, type, closed_at, urgency, entity.
- Click row → opens WoDetailDialog (read-only).
- "Restaurer" button available for role `ADMIN` to reopen a WO.

**Backend:** Reuses `list_work_orders` with filter `{ archived: true }` or status
filter. No new IPC command required, but extended filter support already exists in
the list command specification.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- WoCalendarView renders WOs on correct dates with urgency colours
- Month ← / → navigation works; "Today" button resets view
- Click chip opens WoDetailDialog
- WoDashboardView shows 4 KPI cards with correct counts
- 4 charts render: status donut, urgency bar, trend area, backlog
- WoContextMenu appears on right-click with correct items per status
- Context menu actions trigger correct store actions/dialogs
- WoArchivePanel lists only Fermé/Annulé WOs
- Archive WOs open in read-only detail view
```

### Supervisor Verification — Sprint S4

**V1 — Calendar view.**
Switch WorkOrdersPage to Calendar; verify WO chips appear on planned_start dates;
verify urgency colour mapping; click a chip and confirm WoDetailDialog opens.

**V2 — Dashboard KPIs.**
Create 3 WOs (1 En_cours, 1 Terminé, 1 En_retard); open Dashboard; verify card
counts = 3 total, 1 en cours, 1 terminé, 1 en retard.

**V3 — Dashboard charts.**
Verify status donut shows 3 segments; urgency bar shows correct distribution;
trend area chart renders last 30 days correctly.

**V4 — Context menu.**
Right-click a Planifié WO; verify "Démarrer" option present; right-click a Fermé WO;
verify "Démarrer" option absent; click "Imprimer" and verify print fiche opens.

**V5 — Archive panel.**
Fermer 2 WOs and Cancel 1; switch to Archive tab; verify 3 WOs listed; click to
open in read-only mode; verify "Restaurer" button visible for admin role only.

---

*End of Phase 2 - Sub-phase 05 - File 04*

---

## phase-2-core-execution-backbone/06-users-roles-permissions-and-admin-governance/01-user-admin-model-and-role-structure.md

## Sprint S4 — AdminPage Layout, User Metrics, Role Chips, and User-Create Password

> **Gaps addressed:** The roadmap specifies 6+ admin panels (UserListPanel, RoleEditorPanel,
> PermissionCatalogPanel, SessionVisibilityPanel, DelegationManagerPanel,
> EmergencyElevationPanel, RoleImportExportPanel, AdminAuditTimeline) but no `AdminPage.tsx`
> that wires them together. The web app has 4 KPI metric cards above the user table; the
> desktop has no equivalent. The web's role list shows domain-coloured permission chips
> (`DI (5)`, `OT (7)`) — the desktop spec only describes the tree editor, not the list view.
> Finally, `create_user` omits an initial password field, making local-user creation
> impossible. This sprint closes all four gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/pages/AdminPage.tsx` | Tab-routed page replacing the `UsersPage` placeholder |
| `src/components/admin/AdminMetricCards.tsx` | KPI cards at the top of AdminPage |
| `src/components/admin/RoleEditorPanel.tsx` (patch) | Permission domain chips in role list |
| `src-tauri/src/commands/admin_users.rs` (patch) | `initial_password` field on `create_user` |
| `src-tauri/src/commands/admin_stats.rs` | `get_admin_stats` IPC command |
| `src/services/rbac-service.ts` (patch) | Typed wrappers for admin stats + updated `CreateUserInput` |
| `src/i18n/locale-data/{fr,en}/admin.json` (patch) | Admin page, tab, and metric labels |

### GAP-05 — AdminPage.tsx — Tab-Routed Admin Shell

Replaces the current `UsersPage.tsx` placeholder.

**Layout:**
```
┌──────────────────────────────────────────────────────────────────┐
│  Page Header: "Administration"                                   │
│  [AdminMetricCards — 4 KPI cards]                                │
├──────────────────────────────────────────────────────────────────┤
│  Tab Bar (permission-gated):                                     │
│  [Users] [Roles] [Permissions] [Sessions] [Delegation]           │
│  [Emergency] [Import/Export] [Audit]                             │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Active Tab → renders corresponding panel component              │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

**Tab configuration:**

| Tab Label (FR / EN) | Panel Component | Required Permission | Default |
|---------------------|-----------------|--------------------:|---------|
| Utilisateurs / Users | `UserListPanel` | `adm.users` | ✅ first visible |
| Rôles / Roles | `RoleEditorPanel` | `adm.roles` | |
| Permissions | `PermissionCatalogPanel` | `adm.permissions` | |
| Sessions | `SessionVisibilityPanel` | `adm.users` | |
| Délégation / Delegation | `DelegationManagerPanel` | `adm.roles` | |
| Urgence / Emergency | `EmergencyElevationPanel` | `adm.users` | |
| Import / Export | `RoleImportExportPanel` | `adm.roles` | |
| Audit | `AdminAuditTimeline` | `adm.users` OR `adm.roles` | |

**Behaviour:**
- Each tab is conditionally rendered using `<PermissionGate>`.
- Default active tab = first tab the user has permission for.
- Tab state stored in URL search param (`?tab=roles`) for deep-linking.
- If user has no `adm.*` permissions, `AdminPage` shows a 403 message.
- Page registered in router at `/admin` and sidebar nav item requires `adm.users` OR `adm.roles`.

**Implementation:**
- Use Radix `Tabs` primitive (`@radix-ui/react-tabs`) for accessible keyboard nav.
- Lazy-load each panel component via `React.lazy()`.

### GAP-06 — AdminMetricCards.tsx — KPI Cards

**Layout (horizontal row, 4 cards):**

| Card | Value Source | Icon | Colour |
|------|-------------|------|--------|
| Utilisateurs actifs / Active Users | `stats.active_users` | `Users` | blue |
| Rôles / Roles | `stats.total_roles` (badge: `system_roles` / `custom_roles`) | `Shield` | indigo |
| Sessions actives / Active Sessions | `stats.active_sessions` | `MonitorSmartphone` | emerald |
| Sans affectation / Unassigned | `stats.unassigned_users` (users with 0 scope assignments) | `AlertTriangle` | amber (red if > 0) |

**Data source:** Calls `get_admin_stats` IPC command on mount and every 60 seconds.

**`get_admin_stats` backend command:**

```rust
// src-tauri/src/commands/admin_stats.rs

#[derive(Serialize)]
pub struct AdminStatsPayload {
    pub active_users: i64,
    pub inactive_users: i64,
    pub total_roles: i64,
    pub system_roles: i64,
    pub custom_roles: i64,
    pub active_sessions: i64,
    pub unassigned_users: i64,       // users with 0 active scope assignments
    pub emergency_grants_active: i64, // active emergency elevations
}

// Permission: adm.users (tenant scope)
// Single query with CTEs for each metric
```

### GAP-11 — Role Permission Chips in Role List

**Patch to `RoleEditorPanel.tsx` — left panel (role list):**

Currently the spec describes a role list with type/status badges. Add domain-coloured
permission chips next to each role.

**Chip rendering logic:**
1. Group role permissions by domain prefix (e.g., `ot.*`, `di.*`, `eq.*`).
2. Count only action permissions (exclude the `*.view` base if domain has other actions).
3. Render as compact chips: `DI (5)` `OT (7)` `EQ (3)` with domain-specific colours.

**Domain colour mapping:**

| Domain | Chip Colour |
|--------|------------|
| `di` | `bg-blue-100 text-blue-800` |
| `ot` | `bg-emerald-100 text-emerald-800` |
| `eq` | `bg-orange-100 text-orange-800` |
| `pm` | `bg-violet-100 text-violet-800` |
| `inv` | `bg-amber-100 text-amber-800` |
| `per` | `bg-cyan-100 text-cyan-800` |
| `org` | `bg-pink-100 text-pink-800` |
| `ref` | `bg-slate-100 text-slate-800` |
| `adm` | `bg-red-100 text-red-800` |
| Other | `bg-gray-100 text-gray-800` |

**Chip overflow:** If role has > 6 domain chips, show first 5 + `+N more` chip that
expands on hover (tooltip listing all domains).

**Full permissions = special badge:** If a role has ALL permissions, show a single
`Accès complet / Full access` badge in gold instead of individual chips.

### GAP-12 — User-Create Password Flow

**Problem:** `CreateUserInput` has `username, identity_mode, personnel_id, force_password_change`
but no `initial_password`. A local-mode user cannot be created without a password.

**Patch to `src-tauri/src/commands/admin_users.rs` — `create_user`:**

```rust
pub struct CreateUserInput {
    pub username: String,
    pub identity_mode: String,     // "local" | "sso" | "hybrid"
    pub personnel_id: Option<i64>,
    pub initial_password: Option<String>, // NEW — required when identity_mode = "local"
    pub force_password_change: Option<bool>, // defaults to true
}
```

**Validation rules:**
- If `identity_mode = "local"`: `initial_password` is **required**, min 8 chars,
  must contain at least one uppercase, one lowercase, one digit.
- If `identity_mode = "sso"`: `initial_password` must be `None` (SSO users don't have passwords).
- If `identity_mode = "hybrid"`: `initial_password` is optional (can login via SSO or password).
- `force_password_change` defaults to `true` when `initial_password` is provided.
- Password is hashed with argon2id (reuse `hash_password()` from `auth/password.rs`).

**Patch to `UserListPanel.tsx` — Create User modal:**

Add to the create-user form:
- **Password** field (required for local, hidden for SSO): `type="password"`, min 8 chars.
- **Confirm Password** field: must match.
- **Strength indicator**: visual bar (weak/medium/strong) using zxcvbn or simple regex check.
- **Force change on first login** checkbox: defaults to checked.

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- AdminPage renders with tab bar; tabs filtered by permission
- Default tab = first tab user has permission for
- URL deep-linking works (?tab=roles navigates to Roles tab)
- AdminMetricCards shows 4 KPI cards with correct values
- Unassigned users card turns red when count > 0
- Auto-refresh every 60s without UI flicker
- Role list shows domain-coloured permission chips
- Full-access roles show gold "Accès complet" badge
- Chip overflow shows "+N more" with hover tooltip
- create_user with identity_mode="local" requires initial_password
- create_user with identity_mode="sso" rejects initial_password
- Password hashed with argon2id, force_password_change defaults true
- Create User modal shows password strength indicator
- cargo check passes with zero errors
```

### Supervisor Verification — Sprint S4

**V1 — AdminPage tabs.**
Login as Administrateur; verify all 8 tabs visible. Login as Technicien; verify AdminPage
shows 403 (no `adm.*` permissions).

**V2 — Metrics.**
With 3 active users, 2 custom roles, 1 unassigned user: verify cards show correct numbers;
unassigned card is amber/red.

**V3 — Permission chips.**
View Supervisor role in list; verify chips show `DI (5)` `OT (6)` etc. View Superadmin;
verify single gold "Accès complet" badge.

**V4 — Create local user.**
Click "Create User"; set identity_mode=local; leave password empty → form validation blocks
save. Enter valid password → user created with `force_password_change=true`. Login as new
user → ForcePasswordChangePage appears.

**V5 — Create SSO user.**
Click "Create User"; set identity_mode=sso; password field hidden. User created successfully
without password.

### S4‑6 — Profile Page (`ProfilePage.tsx`) — GAP PRO‑01

```
LOCATION   src/pages/ProfilePage.tsx
ROUTE      /profile (replaces ModulePlaceholder)
STORE      No new store — reads from auth session + settings-store
SERVICE    user-service.ts (patch — add getMyProfile, updateMyProfile IPC wrappers)
COMMAND    get_my_profile, update_my_profile (Rust — reads/writes own user record)

DESCRIPTION
Current-user self-service page (no admin permission required — any authenticated user).

Layout — single-column, centered max-w-2xl:
  ┌───────────────────────────────────────────────────────────┐
  │  ┌────────┐                                               │
  │  │ Avatar │  {display_name}                               │
  │  │  (XL)  │  {role_name} · {email}                        │
  │  └────────┘  Member since {created_at}                    │
  │                                                           │
  │  ─────────────────────────────────────────────────────    │
  │                                                           │
  │  Personal Information                        [ Edit ]     │
  │  ┌──────────────┬──────────────────────────────────────┐  │
  │  │ Display name │ Jean Dupont                          │  │
  │  │ Email        │ jean@example.com                     │  │
  │  │ Phone        │ +33 6 12 34 56 78                    │  │
  │  │ Language     │ Français                             │  │
  │  └──────────────┴──────────────────────────────────────┘  │
  │                                                           │
  │  Security                                                 │
  │  ┌──────────────────────────────────────────────────────┐ │
  │  │ Password        Last changed: 2026-03-15  [ Change ] │ │
  │  │ PIN unlock      Enabled                   [ Manage ] │ │
  │  │ Trusted devices 2 devices      [ View / Revoke ]     │ │
  │  └──────────────────────────────────────────────────────┘ │
  │                                                           │
  │  Notification Preferences                                 │
  │  ┌──────────────────────────────────────────────────────┐ │
  │  │ Links to NotificationPreferencesPanel (SP07-F01)     │ │
  │  └──────────────────────────────────────────────────────┘ │
  │                                                           │
  │  Session History (last 10)                                │
  │  ┌──────────┬──────────┬──────────┬──────────┐           │
  │  │ Date     │ Device   │ Duration │ Status   │           │
  │  │ Apr 8    │ Desktop  │ 2h 15m   │ Active   │           │
  │  │ Apr 7    │ Desktop  │ 6h 30m   │ Closed   │           │
  │  └──────────┴──────────┴──────────┴──────────┘           │
  └───────────────────────────────────────────────────────────┘

- "Edit" personal info: opens inline edit with save/cancel
- "Change Password": opens change-password dialog (current + new + confirm)
- "Manage PIN": opens enable/disable/change PIN dialog
- "View / Revoke" trusted devices: lists device names with revoke button
- Session history: read-only, last 10 sessions from session_events table
- Notification preferences: reuses NotificationPreferencesPanel from SP07-F01
  (or shows "Available after notification module" placeholder if SP07 not yet built)

ACCEPTANCE CRITERIA
- any authenticated user can access /profile
- personal info edit saves via update_my_profile
- password change requires current password validation
- trusted device revocation works
- session history loads from session_events
```

### Supervisor Verification — Sprint S4 (continued)

**V6 — Profile page access.**
Login as any user. Navigate to /profile. Verify personal info, security section, and
session history render correctly.

**V7 — Profile edit.**
Edit display name. Save. Verify TopBar user menu shows updated name.

**V8 — Password change.**
Click "Change Password". Enter wrong current password → error. Enter correct current
password + valid new password → success toast.

---

*End of Phase 2 - Sub-phase 06 - File 01*

---

## phase-2-core-execution-backbone/06-users-roles-permissions-and-admin-governance/02-permission-domains-and-scope-enforcement.md

## Sprint S4 — PermissionProvider Context, Permission Cache, and Per-Route Guards

> **Gaps addressed:** (1) Each `usePermissions()` hook instance fires an independent IPC
> call — the code itself notes "Phase 2 will introduce a PermissionProvider context" but
> no spec exists. (2) Every `check_permission()` call in Rust queries SQLite; with scoped
> resolution + dependency checks, this is expensive. No in-memory cache layer is specified.
> (3) The desktop has menu filtering via `requiredPermission` on `NavItem`, but no per-route
> guards — a user who knows the URL path can navigate to any route. This sprint closes all
> three performance and security gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src/contexts/PermissionContext.tsx` | React context + provider for centralized permission state |
| `src/hooks/use-permissions.ts` (rewrite) | Thin wrapper over context instead of independent IPC |
| `src/components/auth/PermissionRoute.tsx` | Route-level permission guard component |
| `src/router.tsx` (patch) | Wrap protected routes with `<PermissionRoute>` |
| `src-tauri/src/rbac/cache.rs` | In-memory permission cache with event-driven invalidation |
| `src-tauri/src/rbac/resolver.rs` (patch) | Use cache layer instead of raw SQL on every check |
| `src-tauri/src/commands/admin_users.rs` (patch) | Emit `rbac-changed` event on role/assignment mutation |
| `src/i18n/locale-data/{fr,en}/auth.json` (patch) | Unauthorized page labels |

### GAP-01 — PermissionContext.tsx — Centralized Permission State

**Problem:** Every component calling `usePermissions()` triggers `invoke("get_my_permissions")`.
On a page with 10 `<PermissionGate>` components, that's 10 IPC round-trips.

**Solution:** Single `PermissionProvider` at the `AuthGuard` level loads permissions once,
stores in React context, and exposes the same `can()` API.

**Architecture:**

```
<AuthGuard>
  <PermissionProvider>       ← NEW: loads permissions once
    <ShellLayout>
      <Outlet />
    </ShellLayout>
  </PermissionProvider>
</AuthGuard>
```

**PermissionContext API:**

```typescript
interface PermissionContextValue {
  permissions: PermissionRecord[];
  isLoading: boolean;
  can: (permissionName: string) => boolean;
  canAny: (...permissionNames: string[]) => boolean;
  canAll: (...permissionNames: string[]) => boolean;
  refresh: () => Promise<void>;
}
```

**Behaviour:**
1. On mount: calls `get_my_permissions` once. Stores result in `useState`.
2. Listens for Tauri event `rbac-changed` (emitted by backend on any role/assignment mutation).
   On receive → calls `refresh()` to reload permissions.
3. Listens for `session-unlocked` event → calls `refresh()` (permissions may have changed
   while session was locked if admin modified roles in another session).
4. `can()`, `canAny()`, `canAll()` are memoized with `useMemo` over the permissions array.
5. While `isLoading`, all `can()` calls return `false` (deny-by-default).

**`usePermissions()` hook rewrite:**

```typescript
export function usePermissions() {
  const context = useContext(PermissionContext);
  if (!context) {
    throw new Error('usePermissions must be used within <PermissionProvider>');
  }
  return context;
}
```

No more independent `invoke()` calls. All consumers share one permission set.

**`<PermissionGate>` stays unchanged** — it already uses `usePermissions().can()`.

### GAP-02 — Permission Cache Layer (Rust)

**Problem:** `check_permission()` in `rbac.rs` runs a SQL query joining
`user_scope_assignments → role_permissions → permissions` on every call. Commands that
check multiple permissions (e.g., a page load checking 5 permissions) hit the DB 5 times.

**Solution:** In-memory cache inside `AppState` with event-driven invalidation.

**New file: `src-tauri/src/rbac/cache.rs`**

```rust
pub struct PermissionCache {
    /// Current user's effective permissions per scope key.
    /// Key: (user_id, scope_key) where scope_key = "tenant" | "entity:{id}" | "site:{id}"
    /// Value: (HashSet<String>, Instant)  — permissions + load timestamp
    entries: HashMap<(i64, String), (HashSet<String>, Instant)>,
    /// Maximum age before forced refresh (fallback safety net)
    max_age: Duration,  // default: 120 seconds
}

impl PermissionCache {
    pub fn new(max_age_secs: u64) -> Self { ... }

    /// Get cached permissions. Returns None if not cached or expired.
    pub fn get(&self, user_id: i64, scope_key: &str) -> Option<&HashSet<String>> { ... }

    /// Store permissions for a user+scope pair.
    pub fn put(&mut self, user_id: i64, scope_key: String, perms: HashSet<String>) { ... }

    /// Invalidate ALL entries for a given user (called on role/assignment change).
    pub fn invalidate_user(&mut self, user_id: i64) { ... }

    /// Invalidate ALL entries (called on role definition change affecting multiple users).
    pub fn invalidate_all(&mut self) { ... }
}
```

**Integration with `AppState`:**

```rust
pub struct AppState {
    pub db: SqlitePool,
    pub session: Arc<RwLock<SessionManager>>,
    pub permission_cache: Arc<RwLock<PermissionCache>>,  // NEW
    // ...
}
```

**Integration with `resolver.rs`:**

Patch `effective_permissions()` and `user_has_permission()`:
1. Check `permission_cache.get(user_id, scope_key)` first.
2. On cache hit → return cached set.
3. On cache miss → run SQL query → `permission_cache.put()` → return.

**Invalidation triggers (in `admin_users.rs` commands):**
- `assign_role_scope` → `cache.invalidate_user(target_user_id)` + emit `rbac-changed` event
- `revoke_role_scope` → `cache.invalidate_user(target_user_id)` + emit `rbac-changed` event
- `update_role` (permissions changed) → `cache.invalidate_all()` + emit `rbac-changed` event
- `delete_role` → `cache.invalidate_all()` + emit `rbac-changed` event
- `grant_emergency_elevation` → `cache.invalidate_user(target)` + emit `rbac-changed` event
- `revoke_emergency_elevation` → `cache.invalidate_user(target)` + emit `rbac-changed` event
- `deactivate_user` → `cache.invalidate_user(target)` + emit `rbac-changed` event

**Tauri event emission:**

```rust
// After any RBAC mutation:
app_handle.emit("rbac-changed", RbacChangedPayload {
    affected_user_id: Some(target_user_id), // or None for global changes
    action: "role_assigned",
})?;
```

This event is consumed by both:
- The Rust cache (invalidation)
- The frontend `PermissionProvider` (refresh)

### GAP-07 — PermissionRoute.tsx — Per-Route Permission Guards

**Problem:** Desktop has only sidebar filtering. A user who types `/admin` in the address bar
(or bookmarks it) bypasses menu-level filtering. Menu hiding is UX, not access control.

**Solution:** `<PermissionRoute>` component wrapping protected route segments.

**Component API:**

```typescript
interface PermissionRouteProps {
  permission?: string;       // single permission check
  anyOf?: string[];          // any of these permissions
  allOf?: string[];          // all of these permissions
  fallback?: React.ReactNode; // custom fallback (default: UnauthorizedPage)
}
```

**Behaviour:**
1. Uses `usePermissions()` from context.
2. While loading → renders `<LoadingSpinner />`.
3. If permission check fails → renders `<UnauthorizedPage />` (or custom fallback).
4. If permission check passes → renders `<Outlet />`.

**New component: `src/pages/UnauthorizedPage.tsx`**

Simple page with:
- Lock icon
- "Accès non autorisé / Unauthorized Access" heading
- "Vous n'avez pas les permissions nécessaires pour accéder à cette page."
- "Retour au tableau de bord / Return to Dashboard" link

**Router patch (`src/router.tsx`):**

```tsx
// Before (current):
<Route element={<AuthGuard />}>
  <Route element={<ShellLayout />}>
    <Route path="/admin" element={<AdminPage />} />
    <Route path="/requests" element={<RequestsPage />} />
    ...
  </Route>
</Route>

// After:
<Route element={<AuthGuard />}>
  <Route element={<PermissionProvider />}>   {/* NEW wrapper */}
    <Route element={<ShellLayout />}>
      <Route element={<PermissionRoute anyOf={["adm.users", "adm.roles"]} />}>
        <Route path="/admin" element={<AdminPage />} />
      </Route>
      <Route element={<PermissionRoute permission="di.view" />}>
        <Route path="/requests" element={<RequestsPage />} />
      </Route>
      <Route element={<PermissionRoute permission="ot.view" />}>
        <Route path="/work-orders" element={<WorkOrdersPage />} />
      </Route>
      <Route element={<PermissionRoute permission="eq.view" />}>
        <Route path="/equipment" element={<EquipmentPage />} />
      </Route>
      {/* Routes without permission guard: dashboard, settings, profile */}
      <Route path="/dashboard" element={<DashboardPage />} />
      ...
    </Route>
  </Route>
</Route>
```

**Route-to-permission mapping:**

| Route | Permission Guard |
|-------|-----------------|
| `/admin` | `anyOf: ["adm.users", "adm.roles"]` |
| `/requests` | `di.view` |
| `/work-orders` | `ot.view` |
| `/equipment` | `eq.view` |
| `/organization` | `org.view` |
| `/personnel` | `per.view` |
| `/reference` | `ref.view` |
| `/planning` | `plan.view` |
| `/reports` | `rep.view` |
| `/settings` | `adm.settings` |
| `/dashboard` | *(none — always accessible)* |
| `/unauthorized` | *(none — always accessible)* |

### Acceptance Criteria

```
- pnpm typecheck passes with zero errors
- cargo check passes with zero errors
- PermissionProvider loads permissions once on mount (verified via IPC call count)
- 10 PermissionGate components on a page result in 1 IPC call, not 10
- rbac-changed event triggers immediate permission refresh in frontend
- Permission cache hit rate > 90% on repeated check_permission calls for same user+scope
- Cache invalidation on role_assigned / role_revoked clears correct user entries
- Cache invalidation on update_role clears ALL entries
- PermissionRoute blocks navigation to /admin for user without adm.* permissions
- PermissionRoute renders UnauthorizedPage (not blank screen)
- Direct URL navigation to /requests without di.view shows UnauthorizedPage
- Dashboard route accessible to all authenticated users (no guard)
```

### Supervisor Verification — Sprint S4

**V1 — PermissionProvider deduplication.**
Open DevTools → Network/IPC tab. Navigate to RequestsPage with 8 PermissionGate components.
Verify only 1 `get_my_permissions` call (not 8).

**V2 — Event-driven refresh.**
Login as Alice (Technicien). In a separate admin session, add `adm.users` to Alice's role.
Backend emits `rbac-changed`. Alice's sidebar should show the Admin menu item within seconds
without manual page refresh.

**V3 — Cache performance.**
Add console timing around `check_permission()` calls. First call: ~2ms (DB hit). Subsequent
calls for same user+scope: < 0.1ms (cache hit).

**V4 — Route guard.**
Login as Technicien (no `adm.*`). Type `/admin` in address bar. Verify UnauthorizedPage
renders with lock icon and French text. Verify browser back button works.

**V5 — Route guard pass-through.**
Login as Administrateur. Navigate to `/admin`. Verify AdminPage loads normally.

---

*End of Phase 2 - Sub-phase 06 - File 02*

---

## phase-2-core-execution-backbone/06-users-roles-permissions-and-admin-governance/03-admin-flows-session-visibility-and-delegation.md

## Sprint S4 — Account Lockout, StepUpDialog, and Online Presence

> **Gaps addressed:** (1) `user_accounts` has `failed_login_attempts` and `locked_until`
> columns since migration 002 but no enforcement logic exists — this is an OWASP requirement.
> (2) `verifyStepUp` IPC command and `rbac-service.ts` wrapper exist, but no reusable UI
> dialog triggers them — every dangerous action would need to independently build a
> re-authentication prompt. (3) The web tracks online users with a heartbeat; the desktop
> has `SessionVisibilityPanel` for admin view but no lightweight presence indicator for
> general UI (e.g., green dot on user avatars in assignment dropdowns). This sprint closes
> all three gaps.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|

## phase-2-core-execution-backbone/06-users-roles-permissions-and-admin-governance/02-permission-domains-and-scope-enforcement.md

## SP06-F02 — Deferred Gaps (Permission Catalog & RBAC Sprint)

> **Source:** Professional audit of SP06-F02 implementation performed after the
> full permission catalog migration (029), scope chain, RBAC cache, route guards,
> admin panels, and emergency elevation commands were completed and verified
> (580 tests pass, cargo check clean, pnpm typecheck clean).

### Deferred Items

| # | Gap | Priority | Scope | Notes |
|---|-----|----------|-------|-------|
| 1 | **PermissionCatalogPanel PDF export** | Low | Frontend | CSV export works; PDF export button should use `@react-pdf/renderer` or `jsPDF`. The spec mentions PDF but CSV covers the primary use case. |
| 2 | **EmergencyElevationPanel full UI** | Medium | Frontend | `grant_emergency_elevation` and `revoke_emergency_elevation` IPC commands + service bindings are implemented. A stub panel exists but needs full form (user picker, role picker, scope, reason, expiry time-picker) and confirmation dialog with step-up. |
| 3 | **i18n key consolidation** | Low | Frontend | Some emergency / elevation keys may be duplicated across `auth.json` and `admin.json` locale files. Consolidate during next i18n cleanup pass. |
| 4 | **StepUpDialog integration for dangerous PermissionCatalog actions** | Medium | Frontend | `createCustomPermission` is flagged `adm.permissions` (dangerous + step-up) but the panel's "Add Custom" flow does not yet trigger `StepUpDialog` before the IPC call. |
| 5 | **Permission dependency graph visualization** | Low | Frontend | The dependency viewer in `PermissionCatalogPanel` is tabular. A visual DAG (e.g., using `reactflow`) would improve UX for complex dependency chains. |
| `src-tauri/src/auth/lockout.rs` | Account lockout enforcement logic |
| `src-tauri/src/commands/auth.rs` (patch) | Integrate lockout into `login` command |
| `src-tauri/src/rbac/cache.rs` (patch) | `rbac_settings` cache for lockout config |
| `src/components/auth/StepUpDialog.tsx` | Reusable step-up re-authentication dialog |
| `src/hooks/use-step-up.ts` | Hook wrapping StepUpDialog for action execution |
| `src/components/admin/OnlinePresenceIndicator.tsx` | Green/gray dot component |
| `src-tauri/src/commands/admin_users.rs` (patch) | `unlock_user_account` command |
| `src/i18n/locale-data/{fr,en}/auth.json` (patch) | Lockout and step-up dialog labels |

### GAP-03 — Account Lockout Enforcement

**Problem:** The `login` command in `auth.rs` validates password but never increments
`failed_login_attempts` or checks `locked_until`. An attacker can brute-force passwords
indefinitely.

**New file: `src-tauri/src/auth/lockout.rs`**

```rust
pub struct LockoutPolicy {
    pub max_attempts: i32,         // default: 5
    pub lockout_minutes: i64,      // default: 15
    pub progressive: bool,         // default: true — doubles lockout on repeated lockouts
}

impl LockoutPolicy {
    /// Load from rbac_settings table with fallback defaults
    pub async fn load(pool: &SqlitePool) -> Self { ... }
}

/// Check if the account is currently locked.
/// Returns Ok(()) if unlocked, Err(AppError::AccountLocked { until }) if locked.
pub async fn check_lockout(
    pool: &SqlitePool,
    user_id: i64,
) -> AppResult<()> {
    // SELECT locked_until FROM user_accounts WHERE id = ?
    // If locked_until IS NOT NULL AND locked_until > datetime('now') → Err
    // If locked_until IS NOT NULL AND locked_until <= datetime('now') → auto-unlock:
    //   UPDATE user_accounts SET locked_until = NULL, failed_login_attempts = 0
}

/// Record a failed login attempt. Locks account if threshold exceeded.
pub async fn record_failed_attempt(
    pool: &SqlitePool,
    user_id: i64,
    policy: &LockoutPolicy,
) -> AppResult<()> {
    // UPDATE user_accounts SET failed_login_attempts = failed_login_attempts + 1
    // If new count >= max_attempts:
    //   Calculate lockout_duration:
    //     base = lockout_minutes
    //     if progressive: multiply by 2^(consecutive_lockouts - 1), cap at 24h
    //   UPDATE SET locked_until = datetime('now', '+N minutes')
    //   Write audit event: action='account_locked', summary includes attempt count
}

/// Reset failed attempts on successful login.
pub async fn reset_attempts(pool: &SqlitePool, user_id: i64) -> AppResult<()> {
    // UPDATE user_accounts SET failed_login_attempts = 0, locked_until = NULL
}
```

**Patch to `src-tauri/src/commands/auth.rs` — `login` command:**

```rust
// BEFORE password verification:
let user = find_user_by_username(pool, &input.username).await?;
check_lockout(pool, user.id).await?;  // NEW — returns AccountLocked error if locked

// AFTER password verification failure:
record_failed_attempt(pool, user.id, &policy).await?;  // NEW

// AFTER password verification success:
reset_attempts(pool, user.id).await?;  // NEW
```

**Frontend handling:**
The `LoginPage.tsx` already handles `AppError` variants. Add handling for `AccountLocked`:
- Show message: "Compte verrouillé. Réessayez dans X minutes." / "Account locked. Try again in X minutes."
- Show `locked_until` timestamp.
- No countdown timer (to avoid giving attackers timing information — security best practice).

**`rbac_settings` entries (add to migration 028 seed or new migration):**

```sql
INSERT OR IGNORE INTO rbac_settings (key, value, description) VALUES
  ('lockout_max_attempts', '5', 'Failed login attempts before account lockout'),
  ('lockout_base_minutes', '15', 'Base lockout duration in minutes'),
  ('lockout_progressive', '1', '1 = double lockout on repeated lockouts, capped at 24h');
```

**Admin unlock command:**

```rust
// src-tauri/src/commands/admin_users.rs — patch

#[tauri::command]
pub async fn unlock_user_account(
    state: tauri::State<'_, AppState>,
    user_id: i64,
) -> AppResult<()> {
    let admin = require_session!(state);
    require_permission!(state, &admin, "adm.users", PermissionScope::Global);
    require_step_up!(state);
    
    // UPDATE user_accounts SET failed_login_attempts = 0, locked_until = NULL
    // Write admin_change_event: action='account_unlocked'
}
```

Exposed in `UserListPanel` as a "Déverrouiller / Unlock" button on locked user rows.

### GAP-04 — StepUpDialog.tsx — Reusable Re-Authentication

**Problem:** Dangerous actions (role mutation, emergency elevation, user deactivation) require
step-up but there's no shared UI component. Without it, each panel would need to independently
build a password prompt + retry loop.

**Component: `src/components/auth/StepUpDialog.tsx`**

```typescript
interface StepUpDialogProps {
  open: boolean;
  onVerified: () => void;      // called after successful step-up
  onCancel: () => void;
  title?: string;              // e.g., "Confirm role deletion"
  description?: string;        // e.g., "This action requires re-authentication"
}
```

**Layout (Radix Dialog):**
```
┌──────────────────────────────────────────┐
│  🔒 Re-authentication Required           │
│                                          │
│  {title}                                 │
│  {description}                           │
│                                          │
│  ┌──────────────────────────────────┐    │
│  │  Password: ••••••••              │    │
│  └──────────────────────────────────┘    │
│                                          │
│  ⚠️ Step-up window: 120 seconds          │
│                                          │
│  [Cancel]                  [Verify]      │
└──────────────────────────────────────────┘
```

**Behaviour:**
1. Renders only when `open = true`.
2. Password field auto-focused.
3. On submit → calls `verifyStepUp({ password })` via rbac-service.
4. On success → calls `onVerified()`, dialog closes.
5. On failure → shows inline error "Mot de passe incorrect / Incorrect password".
6. After 3 failed attempts in the dialog → shows warning + disables for 30 seconds.
7. Enter key submits; Escape cancels.

**Hook: `src/hooks/use-step-up.ts`**

```typescript
interface UseStepUpReturn {
  /** Wraps an async action that needs step-up. Shows dialog if step-up not fresh. */
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
  /** The dialog element to render (place once in page layout) */
  StepUpDialogElement: React.ReactElement;
}

function useStepUp(): UseStepUpReturn {
  // 1. Try the action directly.
  // 2. If backend returns StepUpRequired error → open StepUpDialog.
  // 3. On verification → retry the action.
  // 4. Return the action's result.
}
```

**Usage pattern (in any panel):**

```tsx
function EmergencyElevationPanel() {
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const handleGrant = async () => {
    await withStepUp(() => grantEmergencyElevation(input));
    // Only reaches here if step-up succeeded and action completed
  };

  return (
    <>
      <Button onClick={handleGrant}>Grant Emergency Access</Button>
      {StepUpDialogElement}
    </>
  );
}
```

### GAP-10 — Online Presence Indicator

**Problem:** The web shows a live "online" dot + "active since" label for each user.
The desktop `SessionVisibilityPanel` shows sessions for admins, but no lightweight presence
indicator exists for general UI contexts (e.g., showing who is currently logged in when
assigning an OT to a technician).

**Component: `src/components/admin/OnlinePresenceIndicator.tsx`**

```typescript
interface OnlinePresenceIndicatorProps {
  userId: number;
  size?: 'sm' | 'md';  // default 'sm'
}
```

**Rendering:**
- **Active** (session exists, `last_activity_at` within 5 minutes): Green dot (`bg-emerald-500`)
- **Idle** (session exists, `last_activity_at` > 5 min but session not expired): Amber dot
- **Offline** (no active session, or session expired): Gray dot (`bg-gray-300`)

**Data source:** `list_active_sessions` is admin-only. For non-admin contexts, add a
lightweight command:

```rust
// src-tauri/src/commands/admin_users.rs — patch

#[tauri::command]
pub async fn get_user_presence(
    state: tauri::State<'_, AppState>,
    user_ids: Vec<i64>,
) -> AppResult<Vec<UserPresence>> {
    let _user = require_session!(state);
    // No specific permission required — any authenticated user can see presence
    // (presence is not sensitive; it's equivalent to seeing someone in an office)
    
    // Query: SELECT user_id, last_activity_at FROM app_sessions
    //   WHERE user_id IN (?) AND is_revoked = 0 AND expires_at > datetime('now')
    // Return: Vec<UserPresence { user_id, status: "active"|"idle"|"offline", last_activity_at }>
}
```

**Caching:** Presence data cached in a Zustand atom for 30 seconds. Batch-fetch for all
visible user IDs in a single IPC call.

**Usage contexts:**
- `UserListPanel` — presence dot next to each username
- OT intervener assignment dropdown — show who is currently active
- DI assignment dropdown — show reviewer availability
- `SessionVisibilityPanel` — already has full session data (no change needed)

### Acceptance Criteria

```
- cargo check passes with zero errors
- pnpm typecheck passes with zero errors
- Login with wrong password 5 times → account locked, error message shows lockout
- Login with correct password after lockout expires → succeeds, counter reset
- Progressive lockout: 2nd lockout = 30min, 3rd = 60min (capped at 24h)
- Admin can unlock account via UserListPanel "Unlock" button
- StepUpDialog opens on dangerous action; password verification works
- StepUpDialog closes after successful verification; original action completes
- StepUpDialog shows inline error on wrong password
- useStepUp hook integrates transparently — action code doesn't know about step-up
- OnlinePresenceIndicator shows green/amber/gray dots correctly
- get_user_presence batch-fetches for multiple user IDs in one call
- Presence dot updates within 30 seconds of user activity change
```

### Supervisor Verification — Sprint S4

**V1 — Account lockout.**
Attempt 5 wrong passwords for user "technicien". Verify 6th attempt shows
"Compte verrouillé" with time remaining. Wait 15 minutes (or admin unlock) → login succeeds.

**V2 — Progressive lockout.**
Lock account again after first lockout clears. Verify second lockout duration = 30 minutes.

**V3 — Admin unlock.**
Login as admin. Open Users tab. Find locked user. Click "Déverrouiller". Verify
`failed_login_attempts = 0` and `locked_until = NULL`. User can login immediately.

**V4 — StepUpDialog.**
Login as admin. Go to Roles tab. Click "Delete" on a custom role. Verify StepUpDialog
appears. Enter wrong password → inline error. Enter correct password → role deleted.

**V5 — Step-up window.**
Perform a dangerous action (triggers step-up dialog, enter correct password).
Immediately perform another dangerous action within 120 seconds. Verify no second
step-up prompt (window is still fresh).

**V6 — Presence indicator.**
Login as admin and technician on same machine (sequential sessions). Open UserListPanel.
Verify admin shows green dot. Logout admin. Verify admin dot turns gray within 30 seconds.

---

*End of Phase 2 - Sub-phase 06 - File 03*

---

## phase-2-core-execution-backbone/06-users-roles-permissions-and-admin-governance/04-governance-testing-and-security-audit-controls.md

## Sprint S4 — Password Expiry Policy and PIN-Based Fast Unlock

> **Gaps addressed:** (1) `user_accounts` has a `password_changed_at` column since migration
> 002 but no expiry policy enforces periodic password rotation — a requirement for regulated
> industries (ISO 55001, IEC 62443). (2) `user_accounts` has a `pin_hash` column but no
> PIN creation, update, or fast-unlock flow exists. For field technicians using shared
> workstations, re-entering a full password to unlock an idle-locked session on every return
> is a significant workflow friction. This sprint closes both gaps and adds 4 new tests to
> the RBAC suite.

### What This Sprint Adds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/auth/password_policy.rs` | Password expiry evaluation + enforcement |
| `src-tauri/src/commands/auth.rs` (patch) | Integrate expiry check into `login` and `get_session_info` |
| `src-tauri/src/auth/pin.rs` | PIN hash, verify, and unlock logic |
| `src-tauri/src/commands/auth.rs` (patch) | `set_pin`, `unlock_session_with_pin` commands |
| `src/pages/auth/LockScreen.tsx` (patch) | PIN entry option alongside password unlock |
| `src/components/admin/PasswordPolicyPanel.tsx` | Admin UI for configuring expiry settings |
| `src-tauri/src/rbac/tests.rs` (patch) | 4 new tests (RBAC-16 through RBAC-19) |
| `src/i18n/locale-data/{fr,en}/auth.json` (patch) | Password expiry and PIN labels |

### GAP-08 — Password Expiry Policy

**Problem:** `password_changed_at` is populated on password change but nothing ever checks
whether the password has expired. Users can keep the same password indefinitely.

**New file: `src-tauri/src/auth/password_policy.rs`**

```rust
pub struct PasswordPolicy {
    pub max_age_days: i64,              // default: 90; 0 = no expiry
    pub warn_days_before_expiry: i64,   // default: 14
    pub min_length: usize,              // default: 8
    pub require_uppercase: bool,        // default: true
    pub require_lowercase: bool,        // default: true
    pub require_digit: bool,            // default: true
    pub require_special: bool,          // default: false
}

impl PasswordPolicy {
    /// Load from rbac_settings table with fallback defaults
    pub async fn load(pool: &SqlitePool) -> Self { ... }
}

pub enum PasswordExpiryStatus {
    /// Password is valid; no action needed
    Valid,
    /// Password expires within warn_days_before_expiry
    ExpiringSoon { days_remaining: i64 },
    /// Password has expired; force_password_change must be set
    Expired,
    /// No password_changed_at recorded (legacy user); treat as expired
    NeverSet,
}

/// Check if a user's password has expired based on policy.
pub async fn check_password_expiry(
    pool: &SqlitePool,
    user_id: i64,
    policy: &PasswordPolicy,
) -> AppResult<PasswordExpiryStatus> {
    // SELECT password_changed_at FROM user_accounts WHERE id = ?
    // If max_age_days = 0 → return Valid (expiry disabled)
    // If password_changed_at IS NULL → return NeverSet
    // Compute age = now - password_changed_at in days
    // If age > max_age_days → return Expired
    // If age > (max_age_days - warn_days_before_expiry) → return ExpiringSoon
    // Else → return Valid
}

/// Validate password strength against policy rules.
pub fn validate_password_strength(
    password: &str,
    policy: &PasswordPolicy,
) -> Result<(), Vec<String>> {
    // Returns Ok(()) or Err(list of violated rules)
    // e.g., ["Minimum 8 characters", "Must contain uppercase letter"]
}
```

**Patch to `login` command:**

```rust
// After successful authentication + lockout check:
let policy = PasswordPolicy::load(&state.db).await?;
match check_password_expiry(&state.db, user.id, &policy).await? {
    PasswordExpiryStatus::Expired | PasswordExpiryStatus::NeverSet => {
        // Set force_password_change = 1 in DB
        // Session is created but ForcePasswordChangePage will intercept
    },
    PasswordExpiryStatus::ExpiringSoon { days_remaining } => {
        // Include warning in SessionInfo DTO (new field: password_expires_in_days)
        // Frontend shows a non-blocking toast: "Votre mot de passe expire dans X jours"
    },
    PasswordExpiryStatus::Valid => { /* no action */ }
}
```

**Patch to `SessionInfo` DTO:**

```rust
pub struct SessionInfo {
    // ... existing fields ...
    pub password_expires_in_days: Option<i64>,  // NEW — None if no expiry or not expiring soon
}
```

**Frontend toast (patch to `ShellLayout.tsx` or `AuthGuard`):**

When `sessionInfo.password_expires_in_days` is `Some(n)` and `n <= 14`:
- Show persistent toast (not auto-dismiss): "Votre mot de passe expire dans {n} jours.
  [Changer maintenant]"
- "Changer maintenant" link navigates to profile/password-change page.

**Patch to `force_change_password` command:**

Add password strength validation:
```rust
let policy = PasswordPolicy::load(&state.db).await?;
validate_password_strength(&input.new_password, &policy)
    .map_err(|violations| AppError::Validation(violations.join(", ")))?;
```

**`rbac_settings` entries:**

```sql
INSERT OR IGNORE INTO rbac_settings (key, value, description) VALUES
  ('password_max_age_days',      '90',    'Days before password expiry (0 = disabled)'),
  ('password_warn_days',         '14',    'Days before expiry to show warning'),
  ('password_min_length',        '8',     'Minimum password length'),
  ('password_require_uppercase', '1',     'Require at least one uppercase letter'),
  ('password_require_lowercase', '1',     'Require at least one lowercase letter'),
  ('password_require_digit',     '1',     'Require at least one digit'),
  ('password_require_special',   '0',     'Require at least one special character');
```

**Admin UI: `PasswordPolicyPanel.tsx`**

Small panel accessible from AdminPage → Settings sub-tab or inline in the Audit tab:
- Form fields for each `rbac_settings` password policy key
- Live preview: "Current policy: min 8 chars, uppercase + lowercase + digit, expires every 90 days"
- Save button → updates `rbac_settings` rows
- Permission: `adm.settings`

### GAP-09 — PIN-Based Fast Unlock

**Problem:** Field technicians on shared workstations must re-enter a full password every
time the 30-minute idle lock triggers. A 4–6 digit PIN provides faster unlock for idle-locked
sessions without compromising the full password.

**Scope:** PIN is ONLY for idle lock unlock. It cannot be used for:
- Initial login (always requires full password)
- Step-up re-authentication (always requires full password)
- Password changes (always requires current full password)

**New file: `src-tauri/src/auth/pin.rs`**

```rust
/// Hash a PIN using argon2id with reduced memory (16 MiB) since PINs are short.
/// Uses the same argon2id as password but with adjusted params for PIN-length inputs.
pub fn hash_pin(pin: &str) -> AppResult<String> {
    // argon2id, m=16384, t=3, p=1
    // Returns PHC-format hash string
}

/// Verify a PIN against a stored hash.
pub fn verify_pin(pin: &str, hash: &str) -> AppResult<bool> {
    // Constant-time comparison via argon2::verify
}

/// Validate PIN format: 4-6 digits only.
pub fn validate_pin_format(pin: &str) -> AppResult<()> {
    // Must be 4-6 characters, all digits
    // No sequential patterns (1234, 0000) — optional hardening
}
```

**New IPC commands (patch `src-tauri/src/commands/auth.rs`):**

```rust
#[tauri::command]
pub async fn set_pin(
    state: tauri::State<'_, AppState>,
    input: SetPinInput,  // { current_password: String, new_pin: String }
) -> AppResult<()> {
    let user = require_session!(state);
    // Verify current_password first (full password required to set/change PIN)
    verify_password(&input.current_password, &user_record.password_hash)?;
    validate_pin_format(&input.new_pin)?;
    let pin_hash = hash_pin(&input.new_pin)?;
    // UPDATE user_accounts SET pin_hash = ? WHERE id = ?
    // Write audit event: action='pin_set'
}

#[tauri::command]
pub async fn clear_pin(
    state: tauri::State<'_, AppState>,
    input: ClearPinInput,  // { current_password: String }
) -> AppResult<()> {
    let user = require_session!(state);
    verify_password(&input.current_password, &user_record.password_hash)?;
    // UPDATE user_accounts SET pin_hash = NULL WHERE id = ?
    // Write audit event: action='pin_cleared'
}

#[tauri::command]
pub async fn unlock_session_with_pin(
    state: tauri::State<'_, AppState>,
    input: PinUnlockInput,  // { pin: String }
) -> AppResult<SessionInfo> {
    // Session must exist AND be locked (is_locked = true)
    let session = get_locked_session!(state)?;
    
    // Load user's pin_hash
    let user = get_user_by_id(&state.db, session.user.user_id).await?;
    let pin_hash = user.pin_hash.ok_or(AppError::Auth("No PIN configured".into()))?;
    
    // Verify PIN
    if !verify_pin(&input.pin, &pin_hash)? {
        // Increment failed_pin_attempts (separate counter, or reuse failed_login_attempts)
        // After 3 failed PIN attempts → require full password (disable PIN unlock for this lock)
        return Err(AppError::Auth("Invalid PIN".into()));
    }
    
    // Unlock session
    session_manager.unlock()?;
    // Write audit event: action='session_unlocked_with_pin'
    
    Ok(session_info)
}
```

**Patch to `LockScreen.tsx`:**

```
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│       🔒  Session verrouillée / Session Locked               │
│                                                              │
│       Bonjour, {displayName}                                 │
│                                                              │
│  ┌─────────────────────────────────────────────────────┐     │
│  │  [PIN mode]  or  [Password mode]    ← toggle        │     │
│  └─────────────────────────────────────────────────────┘     │
│                                                              │
│  PIN mode (if pin_hash exists):                              │
│  ┌──┐ ┌──┐ ┌──┐ ┌──┐ ┌──┐ ┌──┐                            │
│  │  │ │  │ │  │ │  │ │  │ │  │  ← 4-6 digit boxes          │
│  └──┘ └──┘ └──┘ └──┘ └──┘ └──┘                            │
│  Auto-submit when PIN length reached                         │
│                                                              │
│  Password mode (always available):                           │
│  ┌──────────────────────────────────────────────┐            │
│  │  Password: ••••••••                          │            │
│  └──────────────────────────────────────────────┘            │
│  [Déverrouiller / Unlock]                                    │
│                                                              │
│  (switch) Utiliser le mot de passe / Use PIN                 │
│                                                              │
│  ⚠️ 3 failed PIN attempts → switch to password required      │
│                                                              │
│  [Se déconnecter / Sign out]                                 │
└──────────────────────────────────────────────────────────────┘
```

**Behaviour:**
- Default unlock mode: PIN if `pin_hash` configured, else password.
- Toggle link: "Utiliser le mot de passe" / "Utiliser le PIN" switches mode.
- PIN auto-submits when the configured length is reached (no button needed).
- After 3 failed PIN attempts → force switch to password mode, disable PIN toggle.
- PIN entry boxes use `inputMode="numeric"` for mobile keyboard compatibility.

**PIN Setup UI (Profile settings — not AdminPage):**

Add a "PIN de déverrouillage rapide / Quick Unlock PIN" section to the user's profile
settings page:
- Current state badge: "PIN configuré / PIN not set"
- "Configurer le PIN / Set PIN" button → modal:
  1. Enter current password (required)
  2. Enter new PIN (4-6 digits)
  3. Confirm PIN
- "Supprimer le PIN / Remove PIN" button → requires current password

### New Tests (RBAC-16 through RBAC-19)

Add to `src-tauri/src/rbac/tests.rs`:

```
test_rbac_16_password_expiry_enforced
  Create user with password_changed_at = 91 days ago.
  Load policy with max_age_days = 90.
  Call check_password_expiry.
  Assert: returns PasswordExpiryStatus::Expired.

test_rbac_17_password_expiry_warning
  Create user with password_changed_at = 80 days ago.
  Load policy with max_age_days = 90, warn_days = 14.
  Call check_password_expiry.
  Assert: returns PasswordExpiryStatus::ExpiringSoon { days_remaining: 10 }.

test_rbac_18_pin_unlock_success
  Create user with pin_hash set.
  Create a locked session.
  Call unlock_session_with_pin with correct PIN.
  Assert: session unlocked, audit event written.

test_rbac_19_pin_unlock_failure_locks_to_password
  Create user with pin_hash set.
  Create a locked session.
  Call unlock_session_with_pin with wrong PIN 3 times.
  Assert: 3rd call returns error indicating "PIN disabled, use password".
  Call unlock_session with correct password.
  Assert: session unlocked.
```

### Updated RBAC Test Count

Total tests after Sprint S4: **19** (original 15 + 4 new).

### Acceptance Criteria

```
- cargo check passes with zero errors
- pnpm typecheck passes with zero errors
- User with password_changed_at > 90 days ago → ForcePasswordChangePage on next login
- User with password_changed_at = 80 days ago → toast warning "expires in 10 days"
- Password expiry disabled when max_age_days = 0
- force_change_password validates password strength per policy
- PasswordPolicyPanel can update all policy settings (adm.settings required)
- set_pin requires current password verification
- PIN format validation: 4-6 digits only, rejects "123" and "1234567"
- unlock_session_with_pin succeeds with correct PIN
- 3 failed PIN attempts → PIN mode disabled, password required
- LockScreen shows PIN input boxes when pin_hash is configured
- LockScreen auto-submits PIN when digit count reached
- PIN toggle between PIN mode and password mode works
- All 19 tests pass: cargo test rbac::tests
```

### Supervisor Verification — Sprint S4

**V1 — Password expiry.**
Set password_max_age_days=1. Change a user's password_changed_at to 2 days ago.
Login as that user. Verify ForcePasswordChangePage appears.

**V2 — Expiry warning.**
Set password_max_age_days=30, warn_days=14. Set password_changed_at to 20 days ago.
Login. Verify toast: "Votre mot de passe expire dans 10 jours."

**V3 — Password strength.**
On ForcePasswordChangePage, enter "abc" → validation error list shown (too short, no uppercase,
no digit). Enter "Abcdef1!" → accepted.

**V4 — PIN setup.**
Go to profile settings. Click "Set PIN". Enter current password + "1234".
Verify pin_hash is now set in DB. Lock session (wait 30min or trigger manually).
Enter 1234 on lock screen → session unlocked.

**V5 — PIN lockout.**
Lock session. Enter wrong PIN 3 times. Verify PIN mode disabled, password field shown.
Enter correct password → session unlocked.

**V6 — Test suite.**
Run `cargo test rbac::tests -- --list` → shows 19 tests.
Run `cargo test rbac::tests` → 0 failures.

---

*End of Phase 2 - Sub-phase 06 - File 04*

---

## Resolved Gaps — Session 2026-04-09

> **Scope** — Bug fixes and missing backend implementations discovered during
> integration testing of DI, WO, Asset, and Profile modules.

### RG‑01 — DI / WO / Asset Create Form Silent Failures — RESOLVED

```
SYMPTOM    Clicking "Create" on DI, WO, or Asset forms did nothing — no error
           message, no feedback. Form appeared frozen.

ROOT CAUSE
  1. DiCreateForm.tsx: org_node_id used `selectedAsset.org_node_id ?? 0`, sending
     org_node_id=0 to Rust backend which rejected it (FK constraint) — empty catch
     block swallowed the error silently.
  2. WoCreateForm.tsx: same silent catch pattern — errors from store.createWo() were
     never surfaced to the user.
  3. AssetCreateForm.tsx: submitCreate() was unguarded await — error caused closeForm()
     to still execute, hiding the failure.

FIX
  - DiCreateForm: null-check org_node_id before submit; show validation error if
    missing; surface store errors + catch errors in error banner above footer.
  - WoCreateForm: added storeError reading, submitError state, error banner.
  - AssetCreateForm: wrapped submitCreate in try/catch; closeForm only on success;
    error banner added.
  - Added i18n keys "assetMissingOrgNode" in EN and FR di.json.

FILES CHANGED
  src/components/di/DiCreateForm.tsx
  src/components/wo/WoCreateForm.tsx
  src/components/assets/AssetCreateForm.tsx
  src/i18n/locale-data/en/di.json
  src/i18n/locale-data/fr/di.json
```

### RG‑02 — Zod `is_modified` Validation Error on DI List — RESOLVED

```
SYMPTOM    DI list/detail dialogs showed red error wall: "is_modified: Expected
           boolean, received undefined" for every DI record.

ROOT CAUSE
  The Zod schemas in di-service.ts, di-conversion-service.ts, and
  di-review-service.ts declared `is_modified: z.boolean()` (required), but the
  Rust backend never selects this column (it does not exist in the DB schema).
  Every IPC response failed Zod validation.

FIX
  Changed `is_modified: z.boolean()` → `is_modified: z.boolean().default(false)`
  in all three service files so the field defaults gracefully when absent.

FILES CHANGED
  src/services/di-service.ts
  src/services/di-conversion-service.ts
  src/services/di-review-service.ts
  src/stores/__tests__/di-store.test.ts (fixture updated)
```

### RG‑03 — Profile Page Infinite Loading — RESOLVED

```
SYMPTOM    Profile page (/profile) showed infinite loading spinner. Console
           showed "command get_my_profile not found" Tauri IPC error.

ROOT CAUSE
  The frontend (ProfilePage.tsx, user-service.ts) called 4 Tauri IPC commands
  that had no Rust implementation:
    - get_my_profile
    - update_my_profile
    - change_password
    - get_session_history
  The invoke() promises never resolved → loading state stuck.

FIX
  Created src-tauri/src/commands/profile.rs implementing all 4 commands:
    - get_my_profile: queries user_accounts + roles via user_scope_assignments
    - update_my_profile: dynamic SET for display_name (email/phone/language
      deferred until DB migration adds those columns)
    - change_password: verifies current password with Argon2id, hashes new,
      emits audit event
    - get_session_history: queries app_sessions + trusted_devices
  Registered module in commands/mod.rs and all 4 commands in lib.rs
  invoke_handler.

  Also fixed frontend type mismatch: app_sessions.id is TEXT (UUID) not
  INTEGER, so SessionHistorySchema.id changed from z.number() to
  z.union([z.number(), z.string()]) and SessionHistoryEntry.id changed from
  number to number | string.

FILES CHANGED
  src-tauri/src/commands/profile.rs (NEW)
  src-tauri/src/commands/mod.rs
  src-tauri/src/lib.rs
  src/services/user-service.ts
  shared/ipc-types.ts
```

---

*End of Resolved Gaps — Session 2026-04-09*

---
