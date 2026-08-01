# Phase 2 - Sub-phase 01 - File 04
# Organization Validation, Permissions, and Audit

## Context and Purpose

Files 01 through 03 provided the schema layer, the live node-management layer, and the
designer UI with impact preview. File 04 closes the sub-phase by adding the publish-time
validation engine, dangerous-action guardrails, and an explicit audit trail for
structural changes.

This file is where the PRD's strongest governance requirements become enforceable:

- invalid structures cannot be published
- major structural changes are versioned and auditable
- historical meaning is preserved across renames, moves, and deactivations
- sensitive structural actions require stronger authorization than ordinary edits

This is also where the gap between File 01 model versioning and File 02 live nodes is
resolved. Because `org_node_types` are version-scoped to a structure model while
`org_nodes` reference a specific `node_type_id`, publishing a new model must remap live
nodes from old type IDs to new type IDs by semantic `code`. Without that remap, the new
model would become active while live nodes still pointed to superseded node-type rows.

## Architecture Rules Applied

- **Publish validation is mandatory.** A draft structure model cannot be activated until
	the validator confirms that live nodes can be mapped into it safely.
- **Node-type codes are semantic identifiers.** Labels, icons, and capability flags may
	evolve between model versions, but type `code` is the stable bridge used to remap live
	nodes during publish.
- **One active model remains the runtime truth.** After a publish succeeds, all live
	`org_nodes.node_type_id` values are remapped to the corresponding rows in the newly
	active model inside the same transaction that supersedes the old model.
- **Dangerous-action split:** ordinary maintenance is `org.manage`; model publish,
	subtree move, node deactivation with operational impact, and bulk structural changes
	are `org.admin` and require step-up.
- **Audit is append-only.** Structural changes write immutable org audit rows that record
	the action, actor, preview summary, and result. No update/delete path exists for audit.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000009_org_change_events.rs` | Append-only audit table for structural changes |
| `src-tauri/src/org/validation.rs` | Publish validator and node-type remap planning |
| `src-tauri/src/org/audit.rs` | Org audit writer for publish, move, deactivate, responsibility, and binding events |
| `src-tauri/src/commands/org.rs` (patch) | Validation and audit IPC commands; stronger permission gates |
| `src/services/org-governance-service.ts` | Frontend wrappers for validation results and audit timeline |
| `src/stores/org-governance-store.ts` | UI state for publish readiness and audit timeline |

## Prerequisites

- Files 01 to 03 complete
- SP04 complete: dangerous-action and step-up macros available
- SP06-F01 complete: settings-change audit pattern available as a reference for append-only governance events

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Publish Validator and Node-Type Remap | `validation.rs`, publish-readiness contract, live remap transaction |
| S2 | Dangerous-Action Permissions and Audit Trail | migration 009, `audit.rs`, command hardening |
| S3 | Governance UI State and Verification Harness | frontend services/store, publish-readiness banner, audit timeline access |

---

## Sprint S1 - Publish Validator and Node-Type Remap

### AI Agent Prompt

```text
You are a senior Rust engineer. Your task is to implement the structure-model validator
that must run before a draft model is published, and the transactional remap that moves
live nodes onto the new node-type rows.

STEP 1 - CREATE src-tauri/src/org/validation.rs

Use these types:

```rust
use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgValidationIssue {
		pub code: String,
		pub severity: String,
		pub message: String,
		pub related_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgPublishValidationResult {
		pub model_id: i64,
		pub can_publish: bool,
		pub issue_count: i64,
		pub blocking_count: i64,
		pub issues: Vec<OrgValidationIssue>,
		pub remap_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTypeRemap {
		pub old_type_id: i64,
		pub old_type_code: String,
		pub new_type_id: i64,
		pub new_type_code: String,
}
```

Required functions:

1. `validate_draft_model_for_publish(pool, model_id) -> AppResult<OrgPublishValidationResult>`

The validator must check all of the following:

- the target model exists and is in `draft` status
- exactly one root node type exists in the draft model
- no duplicate node-type codes exist in the draft model
- every draft node type is reachable from the root through relationship rules
- the relationship-rule graph contains no cycles
- every active live node's current type code exists in the draft model
- every active live node's parent-child pair remains allowed when evaluated against the
	draft model by type code
- every active live node with `cost_center_code` maps to a draft node type where
	`can_carry_cost_center = 1`
- there remains at least one active-capable type with `can_own_work = 1`
- there remains at least one active-capable type with `can_host_assets = 1`

Important implementation note:
- live nodes reference `org_node_types.id`
- those ids are version-scoped to the current active model
- to validate the draft model you must compare by `org_node_types.code`, not by id

2. `build_type_remap_plan(pool, draft_model_id) -> AppResult<Vec<NodeTypeRemap>>`
	 - map old active type IDs to new draft type IDs by stable `code`

3. `publish_model_with_remap(pool, draft_model_id, actor_id) -> AppResult<OrgPublishValidationResult>`
	 Transaction rules:
	 - run validation inside the transaction
	 - if `can_publish = false`, abort with `AppError::ValidationFailed`
	 - update all active `org_nodes.node_type_id` using the remap plan
	 - supersede the previous active model
	 - activate the draft model
	 - update touched nodes' `row_version = row_version + 1`

Pseudo-logic for remap:

```rust
for remap in remap_plan {
		sqlx::query!(
				"UPDATE org_nodes
				 SET node_type_id = ?, row_version = row_version + 1, updated_at = ?
				 WHERE node_type_id = ? AND deleted_at IS NULL",
				remap.new_type_id,
				now,
				remap.old_type_id,
		)
		.execute(&mut *tx)
		.await?;
}
```

Required tests:
- missing root type -> validation fails
- unreachable type -> validation fails
- missing code mapping for a live node type -> validation fails
- valid draft -> publish succeeds and live nodes reference new node-type IDs afterwards

ACCEPTANCE CRITERIA
- `cargo check` passes with 0 errors
- `cargo test` passes for validation and remap cases
- publish fails if any live node type code is missing in the draft model
- publish updates live nodes to the new node-type IDs in the same transaction
```

### Supervisor Verification - Sprint S1

**V1 - Missing type-code mapping blocks publish.**
Create an active model with a live node of type `WORKSHOP`. Create a draft model that
omits `WORKSHOP`. Validation must fail and `can_publish` must be `false`.

**V2 - Parent-child rule drift blocks publish.**
Create live nodes whose current arrangement is valid under the active model. In the draft
model, remove the rule that allows that arrangement. Validation must report a blocking
issue referencing the affected live node or type pair.

**V3 - Remap after publish.**
Publish a valid draft where type codes are preserved but labels differ. Query
`org_nodes.node_type_id` before and after publish. The IDs must change to the new draft
rows while the nodes remain intact.

---

## Sprint S2 - Dangerous-Action Permissions and Audit Trail

### AI Agent Prompt

```text
You are a senior Rust engineer. The validator exists. Your task is to harden the command
layer and add an append-only audit trail for org changes.

STEP 1 - CREATE src-tauri/migrations/m20260401_000009_org_change_events.rs

Create an append-only audit table:

```rust
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
		fn name(&self) -> &str {
				"m20260401_000009_org_change_events"
		}
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
		async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
				manager
						.create_table(
								Table::create()
										.table(Alias::new("org_change_events"))
										.if_not_exists()
										.col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
										.col(ColumnDef::new(Alias::new("entity_kind")).text().not_null())
										.col(ColumnDef::new(Alias::new("entity_id")).integer())
										.col(ColumnDef::new(Alias::new("change_type")).text().not_null())
										.col(ColumnDef::new(Alias::new("before_json")).text())
										.col(ColumnDef::new(Alias::new("after_json")).text())
										.col(ColumnDef::new(Alias::new("preview_summary_json")).text())
										.col(ColumnDef::new(Alias::new("changed_by_id")).integer())
										.col(ColumnDef::new(Alias::new("changed_at")).text().not_null())
										.col(ColumnDef::new(Alias::new("requires_step_up")).integer().not_null().default(0))
										.col(ColumnDef::new(Alias::new("apply_result")).text().not_null().default("applied"))
										.to_owned(),
						)
						.await?;
				Ok(())
		}

		async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
				manager
						.drop_table(Table::drop().table(Alias::new("org_change_events")).to_owned())
						.await?;
				Ok(())
		}
}
```

Register migration 009 after migration 008.

STEP 2 - CREATE src-tauri/src/org/audit.rs

Use these types and functions:

```rust
use crate::errors::AppResult;
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize)]
pub struct OrgAuditEventInput {
		pub entity_kind: String,
		pub entity_id: Option<i64>,
		pub change_type: String,
		pub before_json: Option<String>,
		pub after_json: Option<String>,
		pub preview_summary_json: Option<String>,
		pub changed_by_id: Option<i64>,
		pub requires_step_up: bool,
		pub apply_result: String,
}

pub async fn record_org_change(pool: &SqlitePool, input: OrgAuditEventInput) -> AppResult<()> {
		let now = chrono::Utc::now().to_rfc3339();
		sqlx::query!(
				r#"INSERT INTO org_change_events
					 (entity_kind, entity_id, change_type, before_json, after_json,
						preview_summary_json, changed_by_id, changed_at, requires_step_up, apply_result)
					 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
				input.entity_kind,
				input.entity_id,
				input.change_type,
				input.before_json,
				input.after_json,
				input.preview_summary_json,
				input.changed_by_id,
				now,
				if input.requires_step_up { 1 } else { 0 },
				input.apply_result,
		)
		.execute(pool)
		.await?;
		Ok(())
}
```

STEP 3 - PATCH src-tauri/src/commands/org.rs

Permission and step-up rules must become:

- `org.view`
	- list/read/snapshot/preview commands

- `org.manage`
	- create node
	- update node metadata
	- assign or end responsibilities
	- create or expire external bindings

- `org.admin` + `require_step_up!`
	- publish model with remap
	- move node
	- deactivate node
	- bulk structural imports if present

Also add:
- `validate_org_model_for_publish(model_id)`
- `publish_org_model(model_id)`
- `list_org_change_events(limit, entity_kind?, entity_id?)`

Audit requirements:
- successful create/update/move/deactivate/responsibility/binding/publish operations all
	call `record_org_change`
- validation failures for publish should also create an audit event with
	`apply_result = 'blocked'`
- audit rows are never updated or deleted

STEP 4 - Add tests

Required test cases:
- user with `org.manage` cannot publish a model
- `move_org_node` without step-up fails
- successful publish writes an `org_change_events` row
- blocked publish writes an `org_change_events` row with `apply_result = 'blocked'`

ACCEPTANCE CRITERIA
- `cargo check` passes with 0 errors
- migration 009 creates `org_change_events`
- dangerous actions require `org.admin` and step-up
- audit rows are written for both successful and blocked structural actions
```

### Supervisor Verification - Sprint S2

**V1 - Step-up enforced for move.**
Log in as a user with `org.admin` but without a fresh step-up session. Attempt
`move_org_node`. The operation must fail with an auth or step-up error. Perform step-up
and retry; it should then succeed.

**V2 - Publish audit row.**
Publish a valid draft model. Query `org_change_events` ordered by newest first. A row
with `change_type = 'publish_model'` and `apply_result = 'applied'` must exist.

**V3 - Blocked publish audit row.**
Attempt to publish an invalid draft model. The publish call must fail, and a blocked
audit row must still be recorded with a validation summary.

---

## Sprint S3 - Governance UI State and Verification Harness

### AI Agent Prompt

```text
You are a TypeScript engineer. The backend validation and audit capabilities exist.
Expose them to the frontend and wire them into the designer workspace.

STEP 1 - PATCH shared/ipc-types.ts

Add:

```typescript
export interface OrgValidationIssue {
	code: string;
	severity: string;
	message: string;
	related_id: number | null;
}

export interface OrgPublishValidationResult {
	model_id: number;
	can_publish: boolean;
	issue_count: number;
	blocking_count: number;
	issues: OrgValidationIssue[];
	remap_count: number;
}

export interface OrgChangeEvent {
	id: number;
	entity_kind: string;
	entity_id: number | null;
	change_type: string;
	before_json: string | null;
	after_json: string | null;
	preview_summary_json: string | null;
	changed_by_id: number | null;
	changed_at: string;
	requires_step_up: boolean;
	apply_result: string;
}
```

STEP 2 - CREATE src/services/org-governance-service.ts

Expose Zod-validated IPC wrappers:
- `validateOrgModelForPublish(modelId)`
- `publishOrgModel(modelId)`
- `listOrgChangeEvents(limit, entityKind?, entityId?)`

STEP 3 - CREATE src/stores/org-governance-store.ts

State shape:

```typescript
interface OrgGovernanceStoreState {
	publishValidation: OrgPublishValidationResult | null;
	validationLoading: boolean;
	auditEvents: OrgChangeEvent[];
	auditLoading: boolean;
	error: string | null;

	loadPublishValidation: (modelId: number) => Promise<void>;
	publishModel: (modelId: number) => Promise<void>;
	loadAuditEvents: (limit?: number, entityKind?: string, entityId?: number) => Promise<void>;
}
```

STEP 4 - PATCH OrganizationDesignerPage.tsx or related admin page

Add:
- publish-readiness banner showing `blocking_count` and the top validation issues
- disabled publish button when `can_publish = false`
- audit timeline tab or side panel showing recent org change events

UI behavior rules:
- publish button is only enabled when validation passes
- after publish succeeds, reload the designer snapshot and validation state
- blocked validation issues are displayed in a scannable list, not buried in toast text

STEP 5 - Add smoke tests

Minimum tests:
- validation result with blockers disables the publish button
- successful publish refreshes snapshot/governance state
- audit timeline renders rows from `listOrgChangeEvents`

ACCEPTANCE CRITERIA
- `pnpm run typecheck` passes with 0 errors
- validation banner shows blocking issues clearly
- publish button stays disabled while blockers exist
- audit timeline loads recent org change events without console errors
```

### Supervisor Verification - Sprint S3

**V1 - Publish-readiness banner.**
Open a draft model that is invalid for publish. The UI must show blocking issues and keep
the publish button disabled.

**V2 - Publish success refresh.**
Open a valid draft model, publish it, and confirm the page reloads the active snapshot
and clears prior validation blockers.

**V3 - Audit timeline.**
Trigger a node rename, a move, and a publish. The audit timeline must show all three
events in reverse chronological order.

---

*End of Phase 2 - Sub-phase 01 - File 04*
