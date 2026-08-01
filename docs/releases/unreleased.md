# Unreleased

Accumulated notes from history reconstruction on `cleanup/history-reconstruction`.
Move entries into a dated version section when cutting a release.

## Added

- Shared **EmptyState** UI primitive for consistent empty rendering across list modules.
- Shared **Timeline** design-system primitive (rail, markers, day grouping, empty state) under `src/components/timeline/`.
- Shared **KanbanBoard** design-system primitive (lanes, cards, drag, counts, empty state) under `src/components/kanban/`.
- Design-system tokens `mfTimeline` / `mfKanban` and the `unified-interface-primitives` Cursor rule defining Timeline/Kanban adapter boundaries.
- `common.kanban.emptyColumn` i18n label for board empty columns.
- **ProcurementContextMenu** module (was imported by the base panel but never committed).
- Admin **CreateUserDialog** extracted as a reusable dialog component.
- RBAC permissions `per.position.view` and `per.position.manage`, including Supervisor/Planner role-template grants.
- Personnel create platform:
  - Explicit `employment_origin` / `employment_status` (Active, Inactive, Suspended, Terminated).
  - Optional external contract fields (`contract_number`, `contract_start_date`, `contract_end_date`).
  - Position as a coded business entity (code + name) with qualification requirement seed and CRUD UI.
  - Append-only **personnel assignment history** for position / team / entity / manager / schedule changes.
  - Personnel photo upload path and create-dialog image uploader (`maxItems` on `EntityFormImageUploader`).
  - Position combobox, create dialog, and positions panel surfaces gated on the new position permissions.
- Inventory procurement: requisition rejection with mandatory reason; repairable stock-leg cancel/scrap flows and UI coverage.
- **DI lifecycle & disposition** (branch `feature/p2-sp04-di-lifecycle-disposition`):
  - Closed as sole terminal operational status; outcomes carried by `disposition_code` (`DI.DISPOSITION` reference domain).
  - Migration `m20260816_000139_di_lifecycle_disposition` (columns, seed codes, status remaps, notification categories, audit/review event status remaps).
  - `disposition` + `notifications` modules, close dialog, disposition meta, updated review/conversion/stats IPC and UI.
  - AssetPicker and ProcurementArchive intentionally deferred to independent feature branches.

## Changed

- DI, WO, admin, org, assets, activity, archive, settings, sync, and profile timeline UIs now adapt to the shared Timeline primitive instead of bespoke rails.
- DI and WO Kanban boards adopt the shared KanbanBoard; WO no longer owns a bespoke board renderer.
- Prettier ignores `shared/rbac/permissions.generated.ts` so lint-staged cannot invalidate the `rbac:check` freshness gate after `rbac:generate`.
- Qualification profile upsert accepts skill reference value IDs and atomically replaces profile skill rows.
- Personnel create/update validation and IPC contracts extended for employment, contract, position, skills, certifications, and assignment reason.

## Removed

- Bespoke `WoKanbanBoard.tsx` renderer (replaced by shared KanbanBoard via `WoKanbanView`).

## Fixed

- Base branch typecheck break: `ProcurementRepairablePanel` imported missing `EmptyState` and `ProcurementContextMenu` modules.
- SQL aggregate defaults over REAL columns now use `0.0` instead of integer `0` (inventory, WO costs/plan adherence, RAMS, demo seed) so SQLite affinity and float decoding stay correct.
- WO / observability test fixtures set `planned_downtime_hours`, `origin`, and `classification_code` so test targets compile against current WO inputs.

## Breaking

- None for existing runtime data paths in this slice.
- New personnel migration (`m20260815_000138_personnel_create_platform`) adds columns and tables; deploy requires a normal migration run before using the new create/position/history commands.
- Callers creating personnel must supply the new required fields (`employment_origin`, `employment_status`, `position_id`, etc.) per the updated IPC contract.
