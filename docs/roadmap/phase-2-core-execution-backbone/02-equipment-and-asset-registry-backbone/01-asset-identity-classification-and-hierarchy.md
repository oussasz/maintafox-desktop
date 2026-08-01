# Phase 2 - Sub-phase 02 - File 01
# Asset Identity, Classification, and Hierarchy

## Context and Purpose

Sub-phase 01 delivered the organization operating backbone: configurable structure models,
live org nodes, and governance controls for structural changes. Sub-phase 02 now anchors
equipment and maintainable assets to that backbone.

This file establishes the governed identity layer for assets. It is the foundation for
DI and WO context, PM targeting, reliability evidence, and cost attribution. The PRD is
explicit that this module is not a flat list. It must preserve maintainable boundaries,
classification semantics, and historical identity even as assets are moved, replaced,
or reclassified later.

File 01 focuses on three essentials:

1. stable asset identity and status model
2. governed classification and family semantics
3. parent-child and org-node hierarchy binding rules

## Architecture Rules Applied

- **Identity is stable and non-recycled.** Asset code and `sync_id` are durable keys.
	Once assigned, they are never reused by another physical item.
- **Classification is governed data.** Class, family, and criticality are controlled by
	lookup domains, not free-text fields.
- **Maintainable boundary is explicit.** A registry row includes a boundary flag so
	analytics can separate true maintainable units from purely structural components.
- **Org linkage is required.** Every active asset is bound to a valid active org node
	from sub-phase 01. This gives immediate routing and ownership context.
- **No hard delete for evidence-bearing assets.** If an asset is referenced by work,
	permit, cost, PM, or reliability records, it can be retired but not deleted.
- **Hierarchy integrity must be acyclic.** Parent-child asset relations cannot create
	cycles and must enforce depth and class compatibility policies.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000010_asset_registry_core.rs` | Core asset registry and hierarchy tables |
| `src-tauri/src/assets/mod.rs` | Module root and shared types |
| `src-tauri/src/assets/identity.rs` | Asset identity and classification service |
| `src-tauri/src/assets/hierarchy.rs` | Parent-child and org-node binding service |
| `src-tauri/src/commands/assets.rs` | IPC for create/read/update/list asset identity and hierarchy |
| `shared/ipc-types.ts` (patch) | `Asset`, `AssetHierarchyRow`, payload contracts |
| `src/services/asset-service.ts` | Frontend IPC wrappers for core asset operations |
| `src/stores/asset-store.ts` | State for asset list and selected asset context |

## Prerequisites

- Sub-phase 01 complete: org nodes and validation flow are available
- Phase 1 lookup governance complete: reference domains for class/family/criticality
- SP04 permission framework complete: `eq.view`, `eq.manage`, `eq.import` permissions

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Core Asset Registry Schema and Identity Service | migration 010, `assets/identity.rs` |
| S2 | Asset Hierarchy and Org Binding Rules | `assets/hierarchy.rs`, hierarchy validation |
| S3 | IPC, Frontend Services, and Store | `commands/assets.rs`, `asset-service.ts`, `asset-store.ts` |

---

## Sprint S1 - Core Asset Registry Schema and Identity Service

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement the governed asset identity backbone.

STEP 1 - CREATE src-tauri/migrations/m20260401_000010_asset_registry_core.rs

Create these tables:

1. `asset_registry`
	 - id (PK)
	 - sync_id (TEXT UNIQUE NOT NULL)
	 - asset_code (TEXT UNIQUE NOT NULL)
	 - asset_name (TEXT NOT NULL)
	 - class_code (TEXT NOT NULL)
	 - family_code (TEXT NOT NULL)
	 - criticality_code (TEXT NOT NULL)
	 - status_code (TEXT NOT NULL) // planned/commissioned/in_service/standby/out_of_service/preserved/decommissioned
	 - manufacturer (TEXT NULL)
	 - model (TEXT NULL)
	 - serial_number (TEXT NULL)
	 - maintainable_boundary (INTEGER NOT NULL DEFAULT 1)
	 - org_node_id (INTEGER NOT NULL)
	 - commissioned_at (TEXT NULL)
	 - decommissioned_at (TEXT NULL)
	 - created_at (TEXT NOT NULL)
	 - updated_at (TEXT NOT NULL)
	 - deleted_at (TEXT NULL)
	 - row_version (INTEGER NOT NULL DEFAULT 1)

2. `asset_external_ids`
	 - id (PK)
	 - asset_id (INTEGER NOT NULL)
	 - system_code (TEXT NOT NULL)
	 - external_id (TEXT NOT NULL)
	 - is_primary (INTEGER NOT NULL DEFAULT 0)
	 - valid_from (TEXT NULL)
	 - valid_to (TEXT NULL)
	 - created_at (TEXT NOT NULL)

3. `asset_hierarchy`
	 - id (PK)
	 - parent_asset_id (INTEGER NOT NULL)
	 - child_asset_id (INTEGER NOT NULL)
	 - relation_type (TEXT NOT NULL) // installed_component/functional_child/contains
	 - effective_from (TEXT NULL)
	 - effective_to (TEXT NULL)
	 - created_at (TEXT NOT NULL)

Create indexes:
- `idx_asset_registry_org_node_id`
- `idx_asset_registry_status_code`
- `idx_asset_hierarchy_parent`
- `idx_asset_hierarchy_child`

Register migration 010 in the migrator after 009.

STEP 2 - CREATE src-tauri/src/assets/identity.rs

Define:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
		pub id: i64,
		pub sync_id: String,
		pub asset_code: String,
		pub asset_name: String,
		pub class_code: String,
		pub family_code: String,
		pub criticality_code: String,
		pub status_code: String,
		pub manufacturer: Option<String>,
		pub model: Option<String>,
		pub serial_number: Option<String>,
		pub maintainable_boundary: bool,
		pub org_node_id: i64,
		pub commissioned_at: Option<String>,
		pub decommissioned_at: Option<String>,
		pub created_at: String,
		pub updated_at: String,
		pub deleted_at: Option<String>,
		pub row_version: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateAssetPayload {
		pub asset_code: String,
		pub asset_name: String,
		pub class_code: String,
		pub family_code: String,
		pub criticality_code: String,
		pub status_code: String,
		pub manufacturer: Option<String>,
		pub model: Option<String>,
		pub serial_number: Option<String>,
		pub maintainable_boundary: bool,
		pub org_node_id: i64,
		pub commissioned_at: Option<String>,
}
```

Required functions:
- `list_assets(pool, status_filter, org_node_filter, query, limit)`
- `get_asset_by_id(pool, asset_id)`
- `create_asset(pool, payload, actor_id)`
- `update_asset_identity(pool, asset_id, payload, expected_row_version, actor_id)`

Validation rules:
- `asset_code` must be uppercase and unique among non-deleted assets
- `org_node_id` must reference an active org node
- `class_code`, `family_code`, `criticality_code`, and `status_code` must exist in governed lookup domains
- if `status_code = decommissioned`, then `decommissioned_at` is required
- `maintainable_boundary` cannot be false when status is `in_service` and class policy says maintainable required

ACCEPTANCE CRITERIA
- migration 010 applies successfully
- `create_asset` inserts a row with `row_version = 1`
- duplicate `asset_code` is rejected with `ValidationFailed`
- invalid lookup codes are rejected
```

### Supervisor Verification - Sprint S1

**V1 - Asset creation and uniqueness.**
Create an asset with code `PMP-1001`, then attempt another with the same code.
Second insert must fail.

**V2 - Org linkage guard.**
Attempt to create an asset with an inactive org node id.
The command must reject with validation error.

**V3 - Lookup governance guard.**
Attempt to create with unknown `criticality_code`.
The command must reject until the code is available in lookup domains.

---

## Sprint S2 - Asset Hierarchy and Org Binding Rules

### AI Agent Prompt

```text
You are a senior Rust engineer. Implement parent-child asset hierarchy and integrity rules.

STEP 1 - CREATE src-tauri/src/assets/hierarchy.rs

Define:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetHierarchyRow {
		pub relation_id: i64,
		pub parent_asset_id: i64,
		pub child_asset_id: i64,
		pub relation_type: String,
		pub effective_from: Option<String>,
		pub effective_to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LinkAssetPayload {
		pub parent_asset_id: i64,
		pub child_asset_id: i64,
		pub relation_type: String,
		pub effective_from: Option<String>,
}
```

Required functions:
- `list_asset_children(pool, parent_asset_id)`
- `list_asset_parents(pool, child_asset_id)`
- `link_asset_hierarchy(pool, payload, actor_id)`
- `unlink_asset_hierarchy(pool, relation_id, effective_to, actor_id)`
- `move_asset_org_node(pool, asset_id, new_org_node_id, expected_row_version, actor_id)`

Validation rules:
- parent and child cannot be the same asset
- no hierarchy cycles allowed
- child can have at most one active parent for relation types that enforce single parent
- parent and child must be active unless relation is historical backfill
- moving asset org node must preserve evidence and increment `row_version`
- cannot move decommissioned assets without admin override (File 04)

STEP 2 - add transaction integrity
- link and unlink operations must run in transactions when they update related timestamps

STEP 3 - add tests
- cycle prevention
- single-parent rule
- move org node increments row version

ACCEPTANCE CRITERIA
- cycle creation attempts fail
- hierarchy relations are effective-dated, not destructively deleted
- org-node moves preserve asset row and increment version
```

### Supervisor Verification - Sprint S2

**V1 - Cycle prevention.**
Create A -> B then try B -> A. Operation must fail.

**V2 - Effective dating behavior.**
Unlink a relation and verify `effective_to` is set while row remains in table.

**V3 - Move version increment.**
Move an asset to another org node and confirm `row_version` increments by 1.

---

## Sprint S3 - IPC, Frontend Services, and Store

### AI Agent Prompt

```text
You are a Rust and TypeScript engineer. Expose asset identity and hierarchy functions to frontend.

STEP 1 - CREATE src-tauri/src/commands/assets.rs

Commands:
- `list_assets`
- `get_asset_by_id`
- `create_asset`
- `update_asset_identity`
- `list_asset_children`
- `link_asset_hierarchy`
- `unlink_asset_hierarchy`
- `move_asset_org_node`

Permissions:
- reads require `eq.view`
- create/update/link/move require `eq.manage`

STEP 2 - PATCH src-tauri/src/commands/mod.rs and lib.rs
- add `pub mod assets;`
- register commands in `invoke_handler`

STEP 3 - PATCH shared/ipc-types.ts
- add `Asset`, `AssetHierarchyRow`, and payload interfaces

STEP 4 - CREATE src/services/asset-service.ts
- all IPC wrappers with Zod validation

STEP 5 - CREATE src/stores/asset-store.ts
- state: list, selected asset, hierarchy, loading, saving, error
- methods: `loadAssets`, `selectAsset`, `createAsset`, `updateAsset`, `linkChild`, `unlinkChild`, `moveAssetOrgNode`

ACCEPTANCE CRITERIA
- `cargo check` passes
- `pnpm run typecheck` passes
- user with `eq.view` can list assets but cannot mutate
- user with `eq.manage` can create/update/link assets
```

### Supervisor Verification - Sprint S3

**V1 - Permission split check.**
`eq.view` user can call list/get only. Mutations fail.

**V2 - Store flow check.**
Create asset through store, then select it and link a child. Store reloads list and hierarchy.

**V3 - Type safety check.**
Run `pnpm run typecheck`; no asset service/store errors should appear.

---

*End of Phase 2 - Sub-phase 02 - File 01*