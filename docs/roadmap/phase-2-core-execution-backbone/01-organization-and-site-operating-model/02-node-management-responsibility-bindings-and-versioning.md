# Phase 2 - Sub-phase 01 - File 02
# Node Management, Responsibility Bindings, and Versioning

## Context and Purpose

File 01 established the configurable structure schema: structure models, node types,
and parent-child relationship rules. File 02 turns that schema into operational data.

This is the first point where the tenant's configured operating model becomes real:
sites, plants, workshops, zones, warehouses, departments, or any other tenant-defined
node vocabulary are instantiated as `org_nodes`, linked to named ownership bindings,
and mapped to external identifiers used by ERP, legacy systems, and later integration
modules.

This file is critical because it is where the PRD's design rule becomes concrete:
physical structure and responsibility structure are related, but not assumed to be the
same thing. An org node can represent physical location, operational scope, functional
ownership, or another tenant-defined structural concept. Responsibility assignments and
external bindings then attach the governance meaning around that node.

## Architecture Rules Applied

- **Nodes are governed records, not decorative tree items.** A node is a control point
	for work routing, asset anchoring, permit scope, KPI rollup, and cost visibility.
- **No destructive delete in live use.** Org nodes are deactivated or closed with
	`status`, `effective_to`, and `row_version` updates. `deleted_at` is reserved for
	explicit archival or local developer reset utilities, not normal admin behavior.
- **Relationship rules come from the active structure model.** Node create and move
	actions validate parent-child compatibility against `org_type_relationship_rules`
	attached to the currently active model from File 01.
- **Tree integrity is stored explicitly.** `ancestor_path` and `depth` are maintained in
	the same transaction as create or move operations so descendant queries remain fast and
	deterministic.
- **Optimistic concurrency is required.** Node update, move, and deactivate commands must
	require `expected_row_version` and reject stale writes.
- **Responsibilities are effective-dated.** `org_node_responsibilities` tracks named
	owner roles over time. Historical responsibility context is preserved by ending a prior
	assignment with `valid_to` instead of overwriting it.
- **Bindings are non-destructive and unique.** `org_entity_bindings` are used for ERP
	plant codes, cost centers, SAP functional locations, and legacy references. Only one
	primary binding per `(node_id, binding_type, external_system)` is allowed at a time.
- **Permission split:** `org.view` reads the structure, `org.manage` performs ordinary
	node and binding maintenance, and dangerous structural actions are escalated to
	`org.admin` plus step-up in File 04.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/org/nodes.rs` | Org node lifecycle, tree loading, path maintenance, optimistic concurrency |
| `src-tauri/src/org/responsibilities.rs` | Named ownership bindings with effective dating |
| `src-tauri/src/org/entity_bindings.rs` | External identifier mapping and primary-binding rules |
| `src-tauri/src/commands/org.rs` (patch) | Node, responsibility, and binding IPC commands |
| `shared/ipc-types.ts` (patch) | `OrgNode`, `OrgNodeResponsibility`, `OrgEntityBinding`, payload types |
| `src/services/org-node-service.ts` | Frontend IPC wrappers for node management |
| `src/stores/org-node-store.ts` | Zustand store for tree state, selected node context, responsibilities, bindings |

## Prerequisites

- File 01 complete: active structure model, node types, and relationship rules services
- Phase 1 migration 004 complete: `org_nodes`, `org_node_responsibilities`, and
	`org_entity_bindings` tables exist
- Phase 1 migration 006 complete: teams and workforce/person tables exist for
	responsibility assignments
- SP04-F03 complete: `require_permission!` and `require_step_up!` macros available

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Org Node Lifecycle and Tree Integrity | `org/nodes.rs`, create/update/move/deactivate + tree projection |
| S2 | Responsibility Bindings and External Mappings | `org/responsibilities.rs`, `org/entity_bindings.rs` |
| S3 | IPC, Frontend Services, and Version-Safe Editing | `commands/org.rs`, `org-node-service.ts`, `org-node-store.ts` |

---

## Sprint S1 - Org Node Lifecycle and Tree Integrity

### AI Agent Prompt

```text
You are a senior Rust engineer. File 01 delivered the configurable org schema.
Your task is to implement the operational node layer on top of `org_nodes`.

STEP 1 - CREATE src-tauri/src/org/nodes.rs

Create a Rust module that defines the node entity structs and all lifecycle operations.

Use these core types:

```rust
use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNode {
		pub id: i64,
		pub sync_id: String,
		pub code: String,
		pub name: String,
		pub node_type_id: i64,
		pub parent_id: Option<i64>,
		pub ancestor_path: String,
		pub depth: i64,
		pub description: Option<String>,
		pub cost_center_code: Option<String>,
		pub external_reference: Option<String>,
		pub status: String,
		pub effective_from: Option<String>,
		pub effective_to: Option<String>,
		pub erp_reference: Option<String>,
		pub notes: Option<String>,
		pub created_at: String,
		pub updated_at: String,
		pub deleted_at: Option<String>,
		pub row_version: i64,
		pub origin_machine_id: Option<String>,
		pub last_synced_checkpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgTreeRow {
		pub node: OrgNode,
		pub node_type_code: String,
		pub node_type_label: String,
		pub can_host_assets: bool,
		pub can_own_work: bool,
		pub can_carry_cost_center: bool,
		pub can_aggregate_kpis: bool,
		pub can_receive_permits: bool,
		pub child_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateOrgNodePayload {
		pub code: String,
		pub name: String,
		pub node_type_id: i64,
		pub parent_id: Option<i64>,
		pub description: Option<String>,
		pub cost_center_code: Option<String>,
		pub external_reference: Option<String>,
		pub effective_from: Option<String>,
		pub erp_reference: Option<String>,
		pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateOrgNodeMetadataPayload {
		pub node_id: i64,
		pub name: Option<String>,
		pub description: Option<Option<String>>,
		pub cost_center_code: Option<Option<String>>,
		pub external_reference: Option<Option<String>>,
		pub erp_reference: Option<Option<String>>,
		pub notes: Option<Option<String>>,
		pub status: Option<String>,
		pub expected_row_version: i64,
}

#[derive(Debug, Deserialize)]
pub struct MoveOrgNodePayload {
		pub node_id: i64,
		pub new_parent_id: Option<i64>,
		pub expected_row_version: i64,
		pub effective_from: Option<String>,
}
```

Required functions:

1. `list_active_org_tree(pool: &SqlitePool) -> AppResult<Vec<OrgTreeRow>>`
	 - Join `org_nodes` with `org_node_types`
	 - Only return rows where `deleted_at IS NULL`
	 - Sort by `ancestor_path ASC, name ASC`
	 - Include `child_count` for quick UI rendering

2. `get_org_node_by_id(pool, node_id) -> AppResult<OrgNode>`

3. `create_org_node(pool, payload, created_by_id) -> AppResult<OrgNode>`
	 Validation rules:
	 - `code` must be non-empty, trimmed, and unique across active nodes
	 - `node_type_id` must belong to the active structure model
	 - If `parent_id` is `None`, the node type must have `is_root_type = 1`
	 - If `parent_id` exists, the parent node must exist and the parent-child pair must be
		 allowed by `org_type_relationship_rules` in the active model
	 - If `cost_center_code` is present, the node type must have `can_carry_cost_center = 1`
	 - Root node depth = 0 and ancestor_path = `/{id}/`
	 - Child node depth = `parent.depth + 1` and ancestor_path = `{parent.ancestor_path}{id}/`

4. `update_org_node_metadata(pool, payload) -> AppResult<OrgNode>`
	 - Reject if `row_version` does not match `expected_row_version`
	 - Allow metadata changes only; parent changes are handled by `move_org_node`
	 - Increment `row_version` on every successful update
	 - Reject `cost_center_code` if the node type cannot carry cost centers

5. `move_org_node(pool, payload, moved_by_id) -> AppResult<OrgNode>`
	 Validation rules:
	 - Reject stale `row_version`
	 - Reject moving a node under itself or any descendant
	 - Reject root-to-child transitions if the node type is marked `is_root_type`
	 - Validate the new parent-child type pair against active relationship rules
	 - Recompute `ancestor_path` and `depth` for the moved node and all descendants
	 - Increment `row_version` for every row touched in the subtree

6. `deactivate_org_node(pool, node_id, expected_row_version, deactivated_by_id) -> AppResult<OrgNode>`
	 Validation rules:
	 - Reject if active descendants exist
	 - Reject if active responsibility assignments exist with `valid_to IS NULL`
	 - Set `status = 'inactive'`, `effective_to = now`, increment `row_version`
	 - Do not set `deleted_at`

Implementation guidance:

```rust
fn bool_from_i64(value: i64) -> bool {
		value != 0
}

async fn get_active_model_id(pool: &SqlitePool) -> AppResult<i64> {
		sqlx::query_scalar!(
				"SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1"
		)
		.fetch_optional(pool)
		.await?
		.ok_or_else(|| AppError::ValidationFailed(vec![
				"no active org structure model exists".to_string(),
		]))
}

async fn assert_parent_child_allowed(
		pool: &SqlitePool,
		model_id: i64,
		parent_type_id: i64,
		child_type_id: i64,
) -> AppResult<()> {
		let allowed: i64 = sqlx::query_scalar!(
				r#"SELECT COUNT(*)
					 FROM org_type_relationship_rules
					 WHERE structure_model_id = ?
						 AND parent_type_id = ?
						 AND child_type_id = ?"#,
				model_id,
				parent_type_id,
				child_type_id
		)
		.fetch_one(pool)
		.await?;

		if allowed == 0 {
				return Err(AppError::ValidationFailed(vec![
						"parent-child node type combination is not allowed by the active model".to_string(),
				]));
		}
		Ok(())
}
```

Transaction rule:
- `create_org_node`, `move_org_node`, and `deactivate_org_node` must each run inside a
	SQL transaction because path/depth updates and status changes must remain atomic.

STEP 2 - Add minimal tests in the same module or `src-tauri/tests/org_nodes.rs`

Required test cases:
- create root node -> depth 0 and `ancestor_path` looks like `/{id}/`
- create child node -> depth increments and path appends correctly
- move node under descendant -> returns `AppError::ValidationFailed`
- stale `expected_row_version` -> returns `AppError::ValidationFailed`
- deactivate node with active child -> returns `AppError::ValidationFailed`

ACCEPTANCE CRITERIA
- `cargo check` passes with 0 errors
- `cargo test` passes for node lifecycle tests
- `list_active_org_tree()` returns rows sorted by `ancestor_path`
- root and child depth/path are computed correctly
- stale writes are rejected via `expected_row_version`
```

### Supervisor Verification - Sprint S1

**V1 - Root creation path check.**
Create a draft model, add a root node type, publish the model, then create a root node.
Inspect the inserted row in `org_nodes`. The `depth` column must be `0` and the
`ancestor_path` must be `/{id}/`.

**V2 - Child move guard.**
Create a root node and two descendants beneath it. Attempt to move the root beneath its
grandchild. The operation must fail with a validation error about cycles or descendants.

**V3 - Optimistic concurrency guard.**
Read a node, record its `row_version`, then update it once successfully. Re-submit the
same update payload using the old `expected_row_version`. The second call must fail.

---

## Sprint S2 - Responsibility Bindings and External Mappings

### AI Agent Prompt

```text
You are a senior Rust engineer. The org node lifecycle service exists. Your task is to
implement effective-dated responsibility bindings and external entity mappings.

STEP 1 - CREATE src-tauri/src/org/responsibilities.rs

Use these structs:

```rust
use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNodeResponsibility {
		pub id: i64,
		pub node_id: i64,
		pub responsibility_type: String,
		pub person_id: Option<i64>,
		pub team_id: Option<i64>,
		pub valid_from: Option<String>,
		pub valid_to: Option<String>,
		pub created_at: String,
		pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AssignResponsibilityPayload {
		pub node_id: i64,
		pub responsibility_type: String,
		pub person_id: Option<i64>,
		pub team_id: Option<i64>,
		pub valid_from: Option<String>,
		pub valid_to: Option<String>,
}
```

Required functions:

1. `list_node_responsibilities(pool, node_id, include_inactive) -> AppResult<Vec<OrgNodeResponsibility>>`

2. `assign_responsibility(pool, payload, actor_id) -> AppResult<OrgNodeResponsibility>`
	 Validation rules:
	 - `responsibility_type` must be non-empty
	 - exactly one of `person_id` or `team_id` must be set
	 - `node_id` must reference an active node
	 - no overlapping active assignment for the same `(node_id, responsibility_type)`
		 window; handover must be explicit

3. `end_responsibility_assignment(pool, assignment_id, valid_to, actor_id) -> AppResult<OrgNodeResponsibility>`
	 - set `valid_to`
	 - reject if `valid_to` is earlier than `valid_from`

4. `resolve_current_responsibility(pool, node_id, responsibility_type, at_ts) -> AppResult<Option<OrgNodeResponsibility>>`
	 - return the active assignment at a point in time

Business rules:
- File 02 must seed or assume these default responsibility codes exist in the lookup layer:
	`maintenance_owner`, `production_owner`, `hse_owner`, `planner`, `approver`
- later modules may add more codes, but these five are the baseline set expected by the PRD

STEP 2 - CREATE src-tauri/src/org/entity_bindings.rs

Use these structs:

```rust
use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgEntityBinding {
		pub id: i64,
		pub node_id: i64,
		pub binding_type: String,
		pub external_system: String,
		pub external_id: String,
		pub is_primary: bool,
		pub valid_from: Option<String>,
		pub valid_to: Option<String>,
		pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UpsertOrgEntityBindingPayload {
		pub node_id: i64,
		pub binding_type: String,
		pub external_system: String,
		pub external_id: String,
		pub is_primary: bool,
		pub valid_from: Option<String>,
		pub valid_to: Option<String>,
}
```

Required functions:

1. `list_entity_bindings(pool, node_id, include_inactive) -> AppResult<Vec<OrgEntityBinding>>`

2. `upsert_entity_binding(pool, payload, actor_id) -> AppResult<OrgEntityBinding>`
	 Validation rules:
	 - node must exist and not be deactivated
	 - `binding_type`, `external_system`, and `external_id` must be non-empty
	 - active `(external_system, external_id)` must be unique across the tenant
	 - if `is_primary = true`, clear any previous primary binding for the same
		 `(node_id, binding_type, external_system)` in the same transaction

3. `expire_entity_binding(pool, binding_id, valid_to, actor_id) -> AppResult<OrgEntityBinding>`

Recommended binding examples:
- `binding_type = 'site_reference', external_system = 'erp', external_id = 'PLANT-100'`
- `binding_type = 'cost_center', external_system = 'erp', external_id = 'CC-2100'`
- `binding_type = 'functional_location', external_system = 'sap', external_id = 'FL-01-03-02'`
- `binding_type = 'legacy_code', external_system = 'legacy_cmms', external_id = 'MEC-A1'`

STEP 3 - Add tests for responsibility overlap and primary-binding uniqueness

Required test cases:
- assigning both `person_id` and `team_id` fails
- assigning neither `person_id` nor `team_id` fails
- overlapping `maintenance_owner` assignments on the same node fail
- creating a second primary ERP binding for the same binding type clears the previous primary

ACCEPTANCE CRITERIA
- `cargo check` passes with 0 errors
- `cargo test` passes for responsibility and binding rules
- only one active assignee exists per `(node_id, responsibility_type)`
- only one primary binding exists per `(node_id, binding_type, external_system)`
```

### Supervisor Verification - Sprint S2

**V1 - Responsibility exclusivity.**
Attempt to assign `maintenance_owner` to the same node twice without ending the first
assignment. The second call must fail. Then end the first assignment and verify the
second call succeeds.

**V2 - Team/person XOR rule.**
Submit one responsibility payload with both `person_id` and `team_id`, and one with
neither. Both must fail.

**V3 - Primary binding uniqueness.**
Create a primary ERP plant binding for a node, then create another primary ERP plant
binding for the same node and binding type. After the second insert, query the table:
only the latest row should have `is_primary = 1`.

---

## Sprint S3 - IPC, Frontend Services, and Version-Safe Editing

### AI Agent Prompt

```text
You are a Rust and TypeScript engineer. The org node, responsibility, and binding
services are complete. Expose them to the frontend and build the state layer.

STEP 1 - PATCH src-tauri/src/commands/org.rs

Add these command groups:

Read commands (`org.view`):
- `list_org_tree`
- `get_org_node`
- `list_org_node_responsibilities`
- `list_org_entity_bindings`

Manage commands (`org.manage`):
- `create_org_node`
- `update_org_node_metadata`
- `assign_org_node_responsibility`
- `end_org_node_responsibility`
- `upsert_org_entity_binding`
- `expire_org_entity_binding`

Dangerous structural commands (temporary baseline):
- `move_org_node`
- `deactivate_org_node`

Implementation rule:
- All reads require `require_session!` and `require_permission!(user, "org.view")`
- Ordinary maintenance commands require `require_permission!(user, "org.manage")`
- `move_org_node` and `deactivate_org_node` should already use `org.admin`; File 04 will
	add mandatory `require_step_up!` and audit logging

STEP 2 - PATCH shared/ipc-types.ts

Add these interfaces:

```typescript
export interface OrgNode {
	id: number;
	sync_id: string;
	code: string;
	name: string;
	node_type_id: number;
	parent_id: number | null;
	ancestor_path: string;
	depth: number;
	description: string | null;
	cost_center_code: string | null;
	external_reference: string | null;
	status: string;
	effective_from: string | null;
	effective_to: string | null;
	erp_reference: string | null;
	notes: string | null;
	created_at: string;
	updated_at: string;
	deleted_at: string | null;
	row_version: number;
	origin_machine_id: string | null;
	last_synced_checkpoint: string | null;
}

export interface OrgTreeRow {
	node: OrgNode;
	node_type_code: string;
	node_type_label: string;
	can_host_assets: boolean;
	can_own_work: boolean;
	can_carry_cost_center: boolean;
	can_aggregate_kpis: boolean;
	can_receive_permits: boolean;
	child_count: number;
}

export interface OrgNodeResponsibility {
	id: number;
	node_id: number;
	responsibility_type: string;
	person_id: number | null;
	team_id: number | null;
	valid_from: string | null;
	valid_to: string | null;
	created_at: string;
	updated_at: string;
}

export interface OrgEntityBinding {
	id: number;
	node_id: number;
	binding_type: string;
	external_system: string;
	external_id: string;
	is_primary: boolean;
	valid_from: string | null;
	valid_to: string | null;
	created_at: string;
}
```

STEP 3 - CREATE src/services/org-node-service.ts

Required functions:
- `listOrgTree()`
- `getOrgNode(nodeId)`
- `createOrgNode(payload)`
- `updateOrgNodeMetadata(payload)`
- `moveOrgNode(payload)`
- `deactivateOrgNode(nodeId, expectedRowVersion)`
- `listOrgNodeResponsibilities(nodeId)`
- `assignOrgNodeResponsibility(payload)`
- `endOrgNodeResponsibility(assignmentId, validTo?)`
- `listOrgEntityBindings(nodeId)`
- `upsertOrgEntityBinding(payload)`
- `expireOrgEntityBinding(bindingId, validTo?)`

Service rules:
- every IPC result must be validated with Zod
- `moveOrgNode` and `deactivateOrgNode` must surface version-conflict errors distinctly so
	the UI can prompt a refresh instead of showing a generic failure

STEP 4 - CREATE src/stores/org-node-store.ts

State shape:

```typescript
interface OrgNodeStoreState {
	treeRows: OrgTreeRow[];
	selectedNodeId: number | null;
	selectedNode: OrgNode | null;
	responsibilities: OrgNodeResponsibility[];
	bindings: OrgEntityBinding[];
	loading: boolean;
	saving: boolean;
	error: string | null;

	loadTree: () => Promise<void>;
	selectNode: (nodeId: number | null) => Promise<void>;
	refreshSelectedNodeContext: () => Promise<void>;
}
```

Behavior rules:
- `loadTree()` fetches the full tree once and keeps the flattened `ancestor_path` ordering
- `selectNode(nodeId)` loads node details, responsibilities, and bindings in parallel
- after a successful node mutation, the store reloads the tree and selected node context

STEP 5 - Add a smoke test for stale-row-version behavior in the frontend service

ACCEPTANCE CRITERIA
- `cargo check` passes with 0 errors
- `pnpm run typecheck` passes with 0 errors
- `listOrgTree()` returns an empty array on a fresh tenant with no nodes
- after creating a root node and a child, `loadTree()` returns two rows in ancestor order
- stale row-version errors propagate to the store without being swallowed
```

### Supervisor Verification - Sprint S3

**V1 - Permission split.**
Log in as a user with `org.view` only. Confirm `list_org_tree` succeeds and
`create_org_node` fails. Then log in as a user with `org.manage` and confirm node create
and responsibility assignment succeed.

**V2 - Tree reload after mutation.**
Create a root and a child, then inspect the Zustand store after `loadTree()`. Two rows
must be present and ordered by hierarchy. Rename the child and confirm the store reloads
the selected node state.

**V3 - Version conflict path.**
Perform two edits on the same node from separate browser windows or test harness calls.
The second stale submission must surface a row-version error and leave the store in a
recoverable state.

---

*End of Phase 2 - Sub-phase 01 - File 02*
