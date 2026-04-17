# Phase 2 - Sub-phase 02 - File 02
# Lifecycle History, Meters, and Document Links

## Context and Purpose

File 01 created core identity and hierarchy contracts. File 02 adds temporal evidence:
what changed, when it changed, and under what technical context.

This is the part that makes analytics and reliability trustworthy. Without lifecycle
events, meter history, and governed document links, later modules cannot answer key
questions like when an asset was replaced, whether runtime exposure data is sufficient,
or which approved technical dossier applied at the time of failure.

## Architecture Rules Applied

- **Lifecycle is append-first.** Identity records hold current state, while event tables
	store the historical timeline.
- **Meters are time-series data.** Current counter values are derived from readings,
	not blindly overwritten without trace.
- **Primary meter semantics are explicit.** Assets can have multiple counters, but one
	meter may be designated as primary for PM and reliability denominator calculations.
- **Document links are governed references.** Links point to managed docs by stable ids,
	not ad-hoc URLs, and include purpose codes.
- **No evidence destruction.** Lifecycle events, readings, and document links are ended
	or superseded, not hard-deleted when referenced.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000011_asset_lifecycle_meter_docs.rs` | Lifecycle event, meter, meter reading, and document-link tables |
| `src-tauri/src/assets/lifecycle.rs` | Move/install/replace/reclassify/decommission event service |
| `src-tauri/src/assets/meters.rs` | Meter definitions and reading ingestion service |
| `src-tauri/src/assets/documents.rs` | Technical dossier and document linkage service |
| `src-tauri/src/commands/assets.rs` (patch) | Lifecycle, meter, and document-link IPC commands |
| `shared/ipc-types.ts` (patch) | lifecycle/meter/document types |
| `src/services/asset-lifecycle-service.ts` | Frontend wrappers for event and meter operations |

## Prerequisites

- File 01 complete
- SP01 org governance complete
- PRD references for document and support module context understood

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Lifecycle Event Timeline | migration 011 + lifecycle service |
| S2 | Meter and Reading Governance | `assets/meters.rs` and reading contracts |
| S3 | Document Links and IPC Integration | `assets/documents.rs` and command wiring |

---

## Sprint S1 - Lifecycle Event Timeline

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement lifecycle history for assets.

STEP 1 - CREATE migration m20260401_000011_asset_lifecycle_meter_docs.rs

Add table `asset_lifecycle_events`:
- id (PK)
- asset_id (INTEGER NOT NULL)
- event_type (TEXT NOT NULL) // install/move/replace/reclassify/preserve/decommission/recommission
- from_org_node_id (INTEGER NULL)
- to_org_node_id (INTEGER NULL)
- from_status_code (TEXT NULL)
- to_status_code (TEXT NULL)
- from_class_code (TEXT NULL)
- to_class_code (TEXT NULL)
- related_asset_id (INTEGER NULL) // replacement counterpart
- reason_code (TEXT NULL)
- notes (TEXT NULL)
- event_at (TEXT NOT NULL)
- approved_by_id (INTEGER NULL)
- created_by_id (INTEGER NULL)
- created_at (TEXT NOT NULL)

Index by `asset_id,event_at` and `event_type`.

STEP 2 - CREATE src-tauri/src/assets/lifecycle.rs

Types:
- `AssetLifecycleEvent`
- `RecordLifecycleEventPayload`

Functions:
- `list_asset_lifecycle_events(pool, asset_id, limit)`
- `record_lifecycle_event(pool, payload, actor_id)`

Validation:
- `event_type` must be in governed set
- `asset_id` must exist and not be deleted
- move event requires both `from_org_node_id` and `to_org_node_id`
- replacement event requires `related_asset_id`
- reclassify event requires both class codes
- decommission event sets `asset_registry.status_code = decommissioned` and `decommissioned_at`

ACCEPTANCE CRITERIA
- lifecycle events insert correctly
- decommission event updates current status and timeline
- invalid payload combinations are rejected
```

### Supervisor Verification - Sprint S1

**V1 - Move event integrity.**
Record a move event and verify both old and new org node ids are saved.

**V2 - Replacement traceability.**
Record replacement event with related asset id and verify linkage exists.

**V3 - Decommission behavior.**
Record decommission event and verify current status changes plus historical row preserved.

---

## Sprint S2 - Meter and Reading Governance

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement meter definitions and readings.

STEP 1 - In migration 011 add:

`asset_meters`
- id (PK)
- asset_id (INTEGER NOT NULL)
- meter_code (TEXT NOT NULL)
- meter_type (TEXT NOT NULL) // hours/cycles/km/output
- unit_code (TEXT NOT NULL)
- is_primary (INTEGER NOT NULL DEFAULT 0)
- rollover_value (REAL NULL)
- is_active (INTEGER NOT NULL DEFAULT 1)
- created_at (TEXT NOT NULL)

`asset_meter_readings`
- id (PK)
- meter_id (INTEGER NOT NULL)
- reading_value (REAL NOT NULL)
- reading_at (TEXT NOT NULL)
- source_type (TEXT NOT NULL) // manual/iot/import
- source_reference (TEXT NULL)
- quality_flag (TEXT NOT NULL DEFAULT 'accepted')
- created_by_id (INTEGER NULL)
- created_at (TEXT NOT NULL)

Indexes on `(meter_id,reading_at)` and `(asset_id,is_primary)`.

STEP 2 - CREATE src-tauri/src/assets/meters.rs

Functions:
- `list_asset_meters(pool, asset_id)`
- `create_asset_meter(pool, payload, actor_id)`
- `record_meter_reading(pool, payload, actor_id)`
- `get_latest_meter_value(pool, meter_id)`

Validation rules:
- one primary meter per asset
- reading timestamps must be monotonic unless `quality_flag = corrected`
- negative readings rejected
- if rollover configured and value drops, create derived delta accordingly

ACCEPTANCE CRITERIA
- primary meter uniqueness enforced
- readings stored with source metadata
- latest value query returns most recent accepted reading
```

### Supervisor Verification - Sprint S2

**V1 - Primary meter uniqueness.**
Try creating two primary meters for one asset. Second must fail.

**V2 - Reading monotonic rule.**
Insert reading 100 then 90 for same meter without corrected flag. Must fail.

**V3 - Source metadata capture.**
Insert manual and import readings and verify `source_type` and `source_reference` fields.

---

## Sprint S3 - Document Links and IPC Integration

### AI Agent Prompt

```text
You are a Rust and TypeScript engineer. Add governed document links and command wiring.

STEP 1 - In migration 011 add table `asset_document_links`
- id (PK)
- asset_id (INTEGER NOT NULL)
- document_ref (TEXT NOT NULL)
- link_purpose (TEXT NOT NULL) // technical_dossier/manual/warranty/certificate/inspection_pack
- is_primary (INTEGER NOT NULL DEFAULT 0)
- valid_from (TEXT NULL)
- valid_to (TEXT NULL)
- created_by_id (INTEGER NULL)
- created_at (TEXT NOT NULL)

STEP 2 - CREATE src-tauri/src/assets/documents.rs

Functions:
- `list_asset_document_links(pool, asset_id, include_inactive)`
- `upsert_asset_document_link(pool, payload, actor_id)`
- `expire_asset_document_link(pool, link_id, valid_to, actor_id)`

Validation:
- one primary link per `(asset_id, link_purpose)`
- document ref must not be empty

STEP 3 - PATCH commands/assets.rs
Add lifecycle/meter/document commands and permission guards:
- reads: `eq.view`
- writes: `eq.manage`

STEP 4 - PATCH shared/ipc-types.ts and create `asset-lifecycle-service.ts`
Expose all new contracts with Zod validation.

ACCEPTANCE CRITERIA
- commands compile and are registered
- document-link primary uniqueness enforced
- frontend service returns typed lifecycle/meter/document data
```

### Supervisor Verification - Sprint S3

**V1 - Document primary rule.**
Two primary links for same purpose must not both remain active.

**V2 - Command registration.**
Invoke lifecycle and meter read commands in devtools; commands must resolve.

**V3 - Typed service contract.**
Typecheck passes with lifecycle, meter, and document types.

---

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
