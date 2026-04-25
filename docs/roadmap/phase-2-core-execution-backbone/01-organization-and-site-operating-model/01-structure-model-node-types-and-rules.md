# Phase 2 · Sub-phase 01 · File 01
# Structure Model, Node Types, and Rules

## Context and Purpose

Phase 1 delivered the full engineering foundation: database migrations, authentication,
RBAC, settings, and diagnostics. Phase 2 builds the core execution backbone — the data
and workflow layer that Phase 3 planning, Phase 4 integrations, and Phase 5 reliability
modules all depend on.

Sub-phase 01 activates the Organization & Site Operating Model (PRD §6.2). The database
tables for this module (`org_structure_models`, `org_node_types`, `org_type_relationship_rules`,
`org_nodes`, `org_node_responsibilities`, `org_entity_bindings`) were created in Phase 1
migration 004. This sub-phase builds the Rust service layer and frontend UI on top of
those tables.

This first file delivers the **configuration layer**: the structure model lifecycle,
node type definitions, and the relationship rules that govern which node types may
contain which others. This is the schema of the operating model — the tenant configures
it before they begin populating actual org nodes.

## Why the Org Model Is Central to Everything

Every significant entity in Maintafox — a work order, a DI, an asset, a permit, a cost
center, a planning scope — must be anchored to a location or organizational context.
That anchor is an `org_node`. The org model controls what kinds of nodes exist, how they
nest, and what governance rules apply (can this node host assets? own work? aggregate KPIs?).

Getting the org model right at the start of Phase 2 prevents every downstream module
from having to work around an underspecified organizational backbone. PRD §6.2 is
explicit: "Maintafox treats this module as the operating backbone for routing, ownership,
planning scope, KPI aggregation, and structural analytics."

## Architecture Rules Applied

- **Structure model versioning.** A structure model is the schema of the org design
  (the node types and rules), not the nodes themselves. When an admin changes node type
  definitions or relationship rules, a new model version is drafted, validated, and
  activated. The previously active model is superseded but not deleted — historical
  records retain a reference to the model version in effect when they were created.
- **Node types are tenant-defined.** There is no hardcoded "Site → Plant → Workshop"
  hierarchy. The tenant configures the vocabulary (names, codes, icons, depth hints).
- **Capability flags are product-fixed semantics.** The names of capability flags
  (`can_host_assets`, `can_own_work`, `can_carry_cost_center`, `can_aggregate_kpis`,
  `can_receive_permits`) are product-defined because downstream modules query them
  directly. The tenant decides which node types carry which flags.
- **Relationship rules prevent invalid structures.** An admin cannot create an org node
  whose parent is an incompatible type according to the `org_type_relationship_rules`
  for the active model.
- **Permission gate:** all structure model and node-type configuration requires
  `org.admin`. Reading the structure requires `org.view`.
- **Draft-first safety.** A new structure model begins in `draft` status. It can only
  be published (set to `active`) if all existing nodes in the database still conform to
  the new rules, or if there are no nodes yet. This is the `validate_before_publish`
  contract (detailed in F04).

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/org/mod.rs` | Org module root, public API re-exports |
| `src-tauri/src/org/structure_model.rs` | Service: CRUD for `org_structure_models` |
| `src-tauri/src/org/node_types.rs` | Service: CRUD for `org_node_types` + capability flag management |
| `src-tauri/src/org/relationship_rules.rs` | Service: CRUD for `org_type_relationship_rules` |
| `src-tauri/src/commands/org.rs` | IPC: structure model, node type, and relationship rule commands |
| `src-tauri/src/lib.rs` (patch) | Register org module + IPC commands |
| `shared/ipc-types.ts` (patch) | `OrgStructureModel`, `OrgNodeType`, `OrgRelationshipRule` types |
| `src/services/org-service.ts` | Frontend IPC wrappers (structure model section) |
| `src/stores/org-store.ts` | Zustand store: active model, node types, relationship rules |

## Prerequisites

- Phase 1 migration 004: all 6 org tables present in the database
- SP04-F03: `require_permission!` macro and `org.*` permission seeds available
- SP06-F01: settings service in place (not directly needed here but establishes the
  service layer pattern Phase 2 follows)

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Structure Model Service and Lifecycle | `org/structure_model.rs`, model CRUD and lifecycle |
| S2 | Node Types and Relationship Rules Services | `org/node_types.rs`, `org/relationship_rules.rs`, capability flag management |
| S3 | IPC Commands, Frontend Service and Store | `commands/org.rs`, `org-service.ts`, `org-store.ts` |

---

## Sprint S1 — Structure Model Service and Lifecycle

### AI Agent Prompt

```
You are a senior Rust engineer. The org database tables exist from Phase 1 migration 004.
Your task is to write the org module root and the structure model service.

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/org/mod.rs
────────────────────────────────────────────────────────────────────
```rust
//! Organization & Site Operating Model module.
//!
//! This module provides the Rust service layer for PRD §6.2.
//! The database tables (org_structure_models, org_node_types,
//! org_type_relationship_rules, org_nodes, org_node_responsibilities,
//! org_entity_bindings) were created in Phase 1 migration 004.
//!
//! Sub-module layout:
//!   structure_model — lifecycle of the versioned structure schema
//!   node_types      — tenant-defined node type vocabulary
//!   relationship_rules — allowed parent-child type pairings
//!
//! Sub-phase 01 (this file + F02) covers the configuration layer.
//! Sub-phase 01 F02 covers node management and responsibility bindings.

pub mod node_types;
pub mod relationship_rules;
pub mod structure_model;

// Re-export most-used types at module root for clean import in command handlers.
pub use node_types::{OrgNodeType, CreateNodeTypePayload, UpdateNodeTypePayload};
pub use relationship_rules::{OrgRelationshipRule, CreateRelationshipRulePayload};
pub use structure_model::{OrgStructureModel, CreateStructureModelPayload};
```

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/org/structure_model.rs
────────────────────────────────────────────────────────────────────
```rust
//! Structure model service.
//!
//! A structure model is the schema definition of the tenant's organizational
//! hierarchy: which node types exist, how they may relate, and what capability
//! flags they carry.
//!
//! Lifecycle:
//!   create()       → status = "draft"
//!   publish()      → status = "active" (previous active → "superseded")
//!   archive()      → status = "archived" (only for drafts or superseded)
//!
//! The "active" model is a singleton — only one model is active at a time.
//! The publish step validates that existing nodes conform to the new rules
//! before committing the transition (validation logic is in F04).

use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgStructureModel {
    pub id: i64,
    pub sync_id: String,
    pub version_number: i64,
    /// "draft" | "active" | "superseded" | "archived"
    pub status: String,
    pub description: Option<String>,
    pub activated_at: Option<String>,
    pub activated_by_id: Option<i64>,
    pub superseded_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateStructureModelPayload {
    pub description: Option<String>,
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Return all structure models ordered by version number descending.
pub async fn list_models(pool: &SqlitePool) -> AppResult<Vec<OrgStructureModel>> {
    let rows = sqlx::query_as!(
        OrgStructureModel,
        r#"SELECT id, sync_id, version_number, status, description,
                  activated_at, activated_by_id, superseded_at,
                  created_at, updated_at
           FROM org_structure_models
           ORDER BY version_number DESC"#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Return the currently active structure model, or None if none has been activated.
pub async fn get_active_model(pool: &SqlitePool) -> AppResult<Option<OrgStructureModel>> {
    let row = sqlx::query_as!(
        OrgStructureModel,
        r#"SELECT id, sync_id, version_number, status, description,
                  activated_at, activated_by_id, superseded_at,
                  created_at, updated_at
           FROM org_structure_models
           WHERE status = 'active'
           LIMIT 1"#
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Return a specific model by id.
pub async fn get_model_by_id(pool: &SqlitePool, id: i64) -> AppResult<OrgStructureModel> {
    let row = sqlx::query_as!(
        OrgStructureModel,
        r#"SELECT id, sync_id, version_number, status, description,
                  activated_at, activated_by_id, superseded_at,
                  created_at, updated_at
           FROM org_structure_models
           WHERE id = ?"#,
        id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound {
        entity: "org_structure_model".to_string(),
        id: id.to_string(),
    })?;
    Ok(row)
}

/// Create a new structure model in draft status.
/// The version number is set to max(existing) + 1.
pub async fn create_model(
    pool: &SqlitePool,
    payload: CreateStructureModelPayload,
    created_by_id: i64,
) -> AppResult<OrgStructureModel> {
    let now = chrono::Utc::now().naive_utc().to_string();
    let sync_id = Uuid::new_v4().to_string();

    // Calculate next version number
    let max_version: i64 =
        sqlx::query_scalar!("SELECT COALESCE(MAX(version_number), 0) FROM org_structure_models")
            .fetch_one(pool)
            .await?
            .unwrap_or(0);
    let next_version = max_version + 1;

    let id = sqlx::query_scalar!(
        r#"INSERT INTO org_structure_models
           (sync_id, version_number, status, description, created_at, updated_at)
           VALUES (?, ?, 'draft', ?, ?, ?)
           RETURNING id"#,
        sync_id,
        next_version,
        payload.description,
        now,
        now
    )
    .fetch_one(pool)
    .await?;

    tracing::info!(
        model_id = id,
        version = next_version,
        actor = created_by_id,
        "org structure model created (draft)"
    );
    get_model_by_id(pool, id).await
}

/// Publish a draft model as the new active model.
/// The previously active model is moved to "superseded".
/// Validation must be performed by the caller before calling this function —
/// this function does not re-validate.
pub async fn publish_model(
    pool: &SqlitePool,
    model_id: i64,
    activated_by_id: i64,
) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(pool, model_id).await?;
    if model.status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            format!("model {} is '{}', not 'draft' — only draft models can be published", model_id, model.status),
        ]));
    }

    let now = chrono::Utc::now().naive_utc().to_string();

    // Supersede the current active model (if any)
    sqlx::query!(
        "UPDATE org_structure_models SET status = 'superseded', superseded_at = ?, updated_at = ?
         WHERE status = 'active'",
        now,
        now
    )
    .execute(pool)
    .await?;

    // Activate the target model
    sqlx::query!(
        "UPDATE org_structure_models
         SET status = 'active', activated_at = ?, activated_by_id = ?, updated_at = ?
         WHERE id = ?",
        now,
        activated_by_id,
        now,
        model_id
    )
    .execute(pool)
    .await?;

    tracing::info!(
        model_id = model_id,
        actor = activated_by_id,
        "org structure model published (active)"
    );
    get_model_by_id(pool, model_id).await
}

/// Archive a draft or superseded model.
/// Active models cannot be archived — publish a new model first.
pub async fn archive_model(pool: &SqlitePool, model_id: i64) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(pool, model_id).await?;
    if model.status == "active" {
        return Err(AppError::ValidationFailed(vec![
            "cannot archive the active model — publish a new model first".to_string(),
        ]));
    }

    let now = chrono::Utc::now().naive_utc().to_string();
    sqlx::query!(
        "UPDATE org_structure_models SET status = 'archived', updated_at = ? WHERE id = ?",
        now,
        model_id
    )
    .execute(pool)
    .await?;

    tracing::info!(model_id = model_id, "org structure model archived");
    get_model_by_id(pool, model_id).await
}

/// Update a draft model's description.
/// Only draft models can be edited.
pub async fn update_model_description(
    pool: &SqlitePool,
    model_id: i64,
    description: Option<String>,
) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(pool, model_id).await?;
    if model.status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            format!("model {} is not a draft — only draft models can be edited", model_id),
        ]));
    }

    let now = chrono::Utc::now().naive_utc().to_string();
    sqlx::query!(
        "UPDATE org_structure_models SET description = ?, updated_at = ? WHERE id = ?",
        description,
        now,
        model_id
    )
    .execute(pool)
    .await?;

    get_model_by_id(pool, model_id).await
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- `create_model` inserts a row with `status = "draft"` and auto-incremented version
- `publish_model` transitions the previous active model to "superseded" atomically
- Attempting to archive the active model returns `AppError::ValidationFailed`
- `get_active_model` returns None when no model has been published yet
```

---

### Supervisor Verification — Sprint S1

**V1 — Model creation and versioning.**
Write a Rust integration test or use the REPL test approach: call `create_model` twice.
The second model must have `version_number = 2`. Both must have `status = "draft"`.

**V2 — Publish transitions correctly.**
Call `publish_model` on model 1. Confirm: model 1 status = "active", model 2 status
still = "draft". Then publish model 2. Confirm: model 1 status = "superseded",
model 2 status = "active". The transition must be atomic — verify no intermediate state
leaves both models as "active" simultaneously.

**V3 — Archive guard.**
Attempt `archive_model` on the active model. Confirm it returns `AppError::ValidationFailed`
with the message about needing to publish a new model first.

---

## Sprint S2 — Node Types and Relationship Rules Services

### AI Agent Prompt

```
You are a senior Rust engineer. The structure model service is in place. Write the node
type service and the relationship rules service.

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/org/node_types.rs
────────────────────────────────────────────────────────────────────
```rust
//! Node type service.
//!
//! Node types are the tenant's organizational vocabulary:
//! e.g. "Site", "Plant", "Zone", "Unit", "Workshop", "Zone de production".
//!
//! Each node type carries capability flags that govern what records may be
//! attached to nodes of that type. Capability flags are product-defined semantics
//! that downstream modules (work orders, equipment registry, permits, etc.) query
//! directly — so their names are fixed but values are tenant-configured.
//!
//! Node types belong to a structure model. Only node types belonging to the
//! active structure model are used for validation at runtime.

use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNodeType {
    pub id: i64,
    pub sync_id: String,
    pub structure_model_id: i64,
    pub code: String,
    pub label: String,
    pub icon_key: Option<String>,
    pub depth_hint: Option<i64>,
    /// 1 = nodes of this type can host equipment assets
    pub can_host_assets: bool,
    /// 1 = work orders and DIs can be scoped to nodes of this type
    pub can_own_work: bool,
    /// 1 = nodes of this type can carry a cost center code
    pub can_carry_cost_center: bool,
    /// 1 = KPI aggregation rolls up through nodes of this type
    pub can_aggregate_kpis: bool,
    /// 1 = work permits can be issued at nodes of this type
    pub can_receive_permits: bool,
    /// 1 = this type is the root of the hierarchy (only one per model)
    pub is_root_type: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateNodeTypePayload {
    pub structure_model_id: i64,
    pub code: String,
    pub label: String,
    pub icon_key: Option<String>,
    pub depth_hint: Option<i64>,
    pub can_host_assets: bool,
    pub can_own_work: bool,
    pub can_carry_cost_center: bool,
    pub can_aggregate_kpis: bool,
    pub can_receive_permits: bool,
    pub is_root_type: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNodeTypePayload {
    pub label: Option<String>,
    pub icon_key: Option<Option<String>>,
    pub depth_hint: Option<Option<i64>>,
    pub can_host_assets: Option<bool>,
    pub can_own_work: Option<bool>,
    pub can_carry_cost_center: Option<bool>,
    pub can_aggregate_kpis: Option<bool>,
    pub can_receive_permits: Option<bool>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn bool_to_i64(b: bool) -> i64 {
    if b { 1 } else { 0 }
}

fn i64_to_bool(n: i64) -> bool {
    n != 0
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Return all node types for a given structure model.
pub async fn list_node_types(
    pool: &SqlitePool,
    structure_model_id: i64,
) -> AppResult<Vec<OrgNodeType>> {
    let rows = sqlx::query!(
        r#"SELECT id, sync_id, structure_model_id, code, label, icon_key, depth_hint,
                  can_host_assets, can_own_work, can_carry_cost_center,
                  can_aggregate_kpis, can_receive_permits, is_root_type,
                  is_active, created_at, updated_at
           FROM org_node_types
           WHERE structure_model_id = ?
           ORDER BY depth_hint ASC, label ASC"#,
        structure_model_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| OrgNodeType {
            id: r.id,
            sync_id: r.sync_id,
            structure_model_id: r.structure_model_id,
            code: r.code,
            label: r.label,
            icon_key: r.icon_key,
            depth_hint: r.depth_hint,
            can_host_assets: i64_to_bool(r.can_host_assets),
            can_own_work: i64_to_bool(r.can_own_work),
            can_carry_cost_center: i64_to_bool(r.can_carry_cost_center),
            can_aggregate_kpis: i64_to_bool(r.can_aggregate_kpis),
            can_receive_permits: i64_to_bool(r.can_receive_permits),
            is_root_type: i64_to_bool(r.is_root_type),
            is_active: i64_to_bool(r.is_active),
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Return a single node type by id.
pub async fn get_node_type_by_id(pool: &SqlitePool, id: i64) -> AppResult<OrgNodeType> {
    let r = sqlx::query!(
        r#"SELECT id, sync_id, structure_model_id, code, label, icon_key, depth_hint,
                  can_host_assets, can_own_work, can_carry_cost_center,
                  can_aggregate_kpis, can_receive_permits, is_root_type,
                  is_active, created_at, updated_at
           FROM org_node_types WHERE id = ?"#,
        id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound {
        entity: "org_node_type".to_string(),
        id: id.to_string(),
    })?;

    Ok(OrgNodeType {
        id: r.id,
        sync_id: r.sync_id,
        structure_model_id: r.structure_model_id,
        code: r.code,
        label: r.label,
        icon_key: r.icon_key,
        depth_hint: r.depth_hint,
        can_host_assets: i64_to_bool(r.can_host_assets),
        can_own_work: i64_to_bool(r.can_own_work),
        can_carry_cost_center: i64_to_bool(r.can_carry_cost_center),
        can_aggregate_kpis: i64_to_bool(r.can_aggregate_kpis),
        can_receive_permits: i64_to_bool(r.can_receive_permits),
        is_root_type: i64_to_bool(r.is_root_type),
        is_active: i64_to_bool(r.is_active),
        created_at: r.created_at,
        updated_at: r.updated_at,
    })
}

/// Create a node type for a (draft) structure model.
/// Only draft models can have node types added to them.
pub async fn create_node_type(
    pool: &SqlitePool,
    payload: CreateNodeTypePayload,
) -> AppResult<OrgNodeType> {
    // Verify the target model is in draft status
    let model_status: String = sqlx::query_scalar!(
        "SELECT status FROM org_structure_models WHERE id = ?",
        payload.structure_model_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound {
        entity: "org_structure_model".to_string(),
        id: payload.structure_model_id.to_string(),
    })?;

    if model_status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "node types can only be added to draft structure models".to_string(),
        ]));
    }

    // Validate code uniqueness within this model
    let existing: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM org_node_types WHERE structure_model_id = ? AND code = ?",
        payload.structure_model_id,
        payload.code
    )
    .fetch_one(pool)
    .await?;
    if existing > 0 {
        return Err(AppError::ValidationFailed(vec![
            format!("node type code '{}' already exists in this model", payload.code),
        ]));
    }

    // If this is declared as root type, ensure no other root type exists
    if payload.is_root_type {
        let root_exists: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM org_node_types WHERE structure_model_id = ? AND is_root_type = 1",
            payload.structure_model_id
        )
        .fetch_one(pool)
        .await?;
        if root_exists > 0 {
            return Err(AppError::ValidationFailed(vec![
                "a root node type already exists in this model — only one root type is allowed".to_string(),
            ]));
        }
    }

    let now = chrono::Utc::now().naive_utc().to_string();
    let sync_id = Uuid::new_v4().to_string();
    let can_host = bool_to_i64(payload.can_host_assets);
    let can_work = bool_to_i64(payload.can_own_work);
    let can_cost = bool_to_i64(payload.can_carry_cost_center);
    let can_kpi = bool_to_i64(payload.can_aggregate_kpis);
    let can_permit = bool_to_i64(payload.can_receive_permits);
    let is_root = bool_to_i64(payload.is_root_type);

    let id = sqlx::query_scalar!(
        r#"INSERT INTO org_node_types
           (sync_id, structure_model_id, code, label, icon_key, depth_hint,
            can_host_assets, can_own_work, can_carry_cost_center,
            can_aggregate_kpis, can_receive_permits, is_root_type,
            is_active, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
           RETURNING id"#,
        sync_id,
        payload.structure_model_id,
        payload.code,
        payload.label,
        payload.icon_key,
        payload.depth_hint,
        can_host,
        can_work,
        can_cost,
        can_kpi,
        can_permit,
        is_root,
        now,
        now
    )
    .fetch_one(pool)
    .await?;

    tracing::info!(
        node_type_id = id,
        code = %payload.code,
        model_id = payload.structure_model_id,
        "org node type created"
    );
    get_node_type_by_id(pool, id).await
}

/// Deactivate a node type. Cannot deactivate if org_nodes of this type exist.
pub async fn deactivate_node_type(pool: &SqlitePool, id: i64) -> AppResult<OrgNodeType> {
    let node_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM org_nodes WHERE node_type_id = ? AND deleted_at IS NULL",
        id
    )
    .fetch_one(pool)
    .await?;

    if node_count > 0 {
        return Err(AppError::ValidationFailed(vec![
            format!("{} node(s) of this type exist — cannot deactivate", node_count),
        ]));
    }

    let now = chrono::Utc::now().naive_utc().to_string();
    sqlx::query!(
        "UPDATE org_node_types SET is_active = 0, updated_at = ? WHERE id = ?",
        now,
        id
    )
    .execute(pool)
    .await?;

    get_node_type_by_id(pool, id).await
}
```

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/org/relationship_rules.rs
────────────────────────────────────────────────────────────────────
```rust
//! Relationship rules service.
//!
//! Defines which node types may be children of which other node types.
//! These rules are enforced at node-creation time and at structure-model
//! publish time.
//!
//! Example: "Site" can contain "Plant"; "Plant" can contain "Workshop"
//! and "Zone". Cross-type hierarchies that are not covered by an explicit
//! rule are forbidden.

use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgRelationshipRule {
    pub id: i64,
    pub structure_model_id: i64,
    pub parent_type_id: i64,
    /// Denormalized labels for display in the UI
    pub parent_type_label: Option<String>,
    pub child_type_id: i64,
    pub child_type_label: Option<String>,
    pub min_children: Option<i64>,
    pub max_children: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRelationshipRulePayload {
    pub structure_model_id: i64,
    pub parent_type_id: i64,
    pub child_type_id: i64,
    pub min_children: Option<i64>,
    pub max_children: Option<i64>,
}

// ─── Service functions ────────────────────────────────────────────────────────

/// List all relationship rules for a structure model.
pub async fn list_rules(
    pool: &SqlitePool,
    structure_model_id: i64,
) -> AppResult<Vec<OrgRelationshipRule>> {
    let rows = sqlx::query!(
        r#"SELECT r.id, r.structure_model_id,
                  r.parent_type_id, pt.label AS "parent_type_label?: String",
                  r.child_type_id, ct.label AS "child_type_label?: String",
                  r.min_children, r.max_children, r.created_at
           FROM org_type_relationship_rules r
           LEFT JOIN org_node_types pt ON pt.id = r.parent_type_id
           LEFT JOIN org_node_types ct ON ct.id = r.child_type_id
           WHERE r.structure_model_id = ?
           ORDER BY pt.label ASC, ct.label ASC"#,
        structure_model_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| OrgRelationshipRule {
            id: r.id,
            structure_model_id: r.structure_model_id,
            parent_type_id: r.parent_type_id,
            parent_type_label: r.parent_type_label,
            child_type_id: r.child_type_id,
            child_type_label: r.child_type_label,
            min_children: r.min_children,
            max_children: r.max_children,
            created_at: r.created_at,
        })
        .collect())
}

/// Check whether a specific parent-child type combination is allowed
/// under the active structure model.
pub async fn is_allowed(
    pool: &SqlitePool,
    parent_type_id: i64,
    child_type_id: i64,
) -> AppResult<bool> {
    // Get the active model id
    let active_model_id: Option<i64> = sqlx::query_scalar!(
        "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1"
    )
    .fetch_optional(pool)
    .await?;

    let Some(model_id) = active_model_id else {
        // No active model — only root nodes (no parent) are allowed
        return Ok(false);
    };

    let count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM org_type_relationship_rules
         WHERE structure_model_id = ? AND parent_type_id = ? AND child_type_id = ?",
        model_id,
        parent_type_id,
        child_type_id
    )
    .fetch_one(pool)
    .await?;

    Ok(count > 0)
}

/// Create a relationship rule in a draft structure model.
pub async fn create_rule(
    pool: &SqlitePool,
    payload: CreateRelationshipRulePayload,
) -> AppResult<OrgRelationshipRule> {
    // Verify model is a draft
    let model_status: String = sqlx::query_scalar!(
        "SELECT status FROM org_structure_models WHERE id = ?",
        payload.structure_model_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound {
        entity: "org_structure_model".to_string(),
        id: payload.structure_model_id.to_string(),
    })?;

    if model_status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "relationship rules can only be added to draft structure models".to_string(),
        ]));
    }

    // Prevent duplicate rules
    let existing: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM org_type_relationship_rules
         WHERE structure_model_id = ? AND parent_type_id = ? AND child_type_id = ?",
        payload.structure_model_id,
        payload.parent_type_id,
        payload.child_type_id
    )
    .fetch_one(pool)
    .await?;

    if existing > 0 {
        return Err(AppError::ValidationFailed(vec![
            "this parent-child type rule already exists in the model".to_string(),
        ]));
    }

    let now = chrono::Utc::now().naive_utc().to_string();
    let id = sqlx::query_scalar!(
        r#"INSERT INTO org_type_relationship_rules
           (structure_model_id, parent_type_id, child_type_id, min_children, max_children, created_at)
           VALUES (?, ?, ?, ?, ?, ?)
           RETURNING id"#,
        payload.structure_model_id,
        payload.parent_type_id,
        payload.child_type_id,
        payload.min_children,
        payload.max_children,
        now
    )
    .fetch_one(pool)
    .await?;

    // Return via list function to get denormalized labels
    let rules = list_rules(pool, payload.structure_model_id).await?;
    rules
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| AppError::Internal("rule created but not found after insert".to_string()))
}

/// Delete a relationship rule from a draft model.
pub async fn delete_rule(pool: &SqlitePool, rule_id: i64) -> AppResult<()> {
    let model_id: Option<i64> = sqlx::query_scalar!(
        "SELECT structure_model_id FROM org_type_relationship_rules WHERE id = ?",
        rule_id
    )
    .fetch_optional(pool)
    .await?;

    let Some(model_id) = model_id else {
        return Err(AppError::NotFound {
            entity: "org_relationship_rule".to_string(),
            id: rule_id.to_string(),
        });
    };

    // Verify draft
    let status: String = sqlx::query_scalar!(
        "SELECT status FROM org_structure_models WHERE id = ?",
        model_id
    )
    .fetch_one(pool)
    .await?;

    if status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "relationship rules can only be deleted from draft models".to_string(),
        ]));
    }

    sqlx::query!("DELETE FROM org_type_relationship_rules WHERE id = ?", rule_id)
        .execute(pool)
        .await?;

    Ok(())
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- Creating two node types with the same code in the same model returns `ValidationFailed`
- Creating a second root type in the same model returns `ValidationFailed`
- `is_allowed` returns false when no active model exists
- Creating a duplicate relationship rule returns `ValidationFailed`
- Deactivating a node type that has active nodes returns `ValidationFailed`
```

---

### Supervisor Verification — Sprint S2

**V1 — Root type uniqueness.**
Create a node type with `is_root_type = true`. Then attempt to create another with
`is_root_type = true` in the same model. Confirm a `ValidationFailed` error.

**V2 — Relationship rule guard on published model.**
Publish the draft model. Then attempt to add a relationship rule to it. Confirm
the call returns `ValidationFailed` with "draft" in the message.

**V3 — Capability flag round-trip.**
Create a node type with all capability flags set to `true`. Read it back and confirm
all five bool fields are `true`. Create another with all `false` and confirm all `false`.

---

## Sprint S3 — IPC Commands, Frontend Service and Store

### AI Agent Prompt

```
You are a TypeScript and React engineer working with a Rust backend. The structure model,
node type, and relationship rule services are in place. Write the IPC commands, frontend
service wrappers, and Zustand store for the org configuration layer.

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/commands/org.rs
────────────────────────────────────────────────────────────────────
```rust
//! Org module IPC commands.
//!
//! Permission gates:
//!   org.view   — read structure models, node types, rules
//!   org.manage — create/update nodes and responsibility bindings (F02)
//!   org.admin  — create/publish structure models, node types, rules

use crate::{
    auth::AuthState,
    errors::AppResult,
    org::{
        node_types::{self, CreateNodeTypePayload},
        relationship_rules::{self, CreateRelationshipRulePayload},
        structure_model::{self, CreateStructureModelPayload, OrgStructureModel},
        OrgNodeType, OrgRelationshipRule,
    },
};
use tauri::State;

// ─── Structure model commands ─────────────────────────────────────────────────

#[tauri::command]
pub async fn list_org_structure_models(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
) -> AppResult<Vec<OrgStructureModel>> {
    let _user = require_session!(state);
    require_permission!(_user, "org.view");
    structure_model::list_models(&pool).await
}

#[tauri::command]
pub async fn get_active_org_structure_model(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
) -> AppResult<Option<OrgStructureModel>> {
    let _user = require_session!(state);
    require_permission!(_user, "org.view");
    structure_model::get_active_model(&pool).await
}

#[tauri::command]
pub async fn create_org_structure_model(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    payload: CreateStructureModelPayload,
) -> AppResult<OrgStructureModel> {
    let user = require_session!(state);
    require_permission!(user, "org.admin");
    structure_model::create_model(&pool, payload, user.user_id).await
}

#[tauri::command]
pub async fn publish_org_structure_model(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    model_id: i64,
) -> AppResult<OrgStructureModel> {
    let user = require_session!(state);
    require_permission!(user, "org.admin");
    require_step_up!(state);
    // Validation runs in F04 — this command assumes validation was called first.
    structure_model::publish_model(&pool, model_id, user.user_id).await
}

// ─── Node type commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_org_node_types(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    structure_model_id: i64,
) -> AppResult<Vec<OrgNodeType>> {
    let _user = require_session!(state);
    require_permission!(_user, "org.view");
    node_types::list_node_types(&pool, structure_model_id).await
}

#[tauri::command]
pub async fn create_org_node_type(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    payload: CreateNodeTypePayload,
) -> AppResult<OrgNodeType> {
    let user = require_session!(state);
    require_permission!(user, "org.admin");
    node_types::create_node_type(&pool, payload).await
}

#[tauri::command]
pub async fn deactivate_org_node_type(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    node_type_id: i64,
) -> AppResult<OrgNodeType> {
    let user = require_session!(state);
    require_permission!(user, "org.admin");
    node_types::deactivate_node_type(&pool, node_type_id).await
}

// ─── Relationship rule commands ───────────────────────────────────────────────

#[tauri::command]
pub async fn list_org_relationship_rules(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    structure_model_id: i64,
) -> AppResult<Vec<OrgRelationshipRule>> {
    let _user = require_session!(state);
    require_permission!(_user, "org.view");
    relationship_rules::list_rules(&pool, structure_model_id).await
}

#[tauri::command]
pub async fn create_org_relationship_rule(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    payload: CreateRelationshipRulePayload,
) -> AppResult<OrgRelationshipRule> {
    let user = require_session!(state);
    require_permission!(user, "org.admin");
    relationship_rules::create_rule(&pool, payload).await
}

#[tauri::command]
pub async fn delete_org_relationship_rule(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    rule_id: i64,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(user, "org.admin");
    relationship_rules::delete_rule(&pool, rule_id).await
}
```

────────────────────────────────────────────────────────────────────
PATCH src-tauri/src/commands/mod.rs and lib.rs
────────────────────────────────────────────────────────────────────
Add `pub mod org;` to `commands/mod.rs`.
Add `pub mod org;` to `src-tauri/src/lib.rs`.

Add all org commands to the invoke_handler:
```rust
commands::org::list_org_structure_models,
commands::org::get_active_org_structure_model,
commands::org::create_org_structure_model,
commands::org::publish_org_structure_model,
commands::org::list_org_node_types,
commands::org::create_org_node_type,
commands::org::deactivate_org_node_type,
commands::org::list_org_relationship_rules,
commands::org::create_org_relationship_rule,
commands::org::delete_org_relationship_rule,
```

────────────────────────────────────────────────────────────────────
PATCH shared/ipc-types.ts — org configuration types
────────────────────────────────────────────────────────────────────
```typescript
// Add to shared/ipc-types.ts

export interface OrgStructureModel {
  id: number;
  sync_id: string;
  version_number: number;
  /** "draft" | "active" | "superseded" | "archived" */
  status: string;
  description: string | null;
  activated_at: string | null;
  activated_by_id: number | null;
  superseded_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface OrgNodeType {
  id: number;
  sync_id: string;
  structure_model_id: number;
  code: string;
  label: string;
  icon_key: string | null;
  depth_hint: number | null;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  is_root_type: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface OrgRelationshipRule {
  id: number;
  structure_model_id: number;
  parent_type_id: number;
  parent_type_label: string | null;
  child_type_id: number;
  child_type_label: string | null;
  min_children: number | null;
  max_children: number | null;
  created_at: string;
}

export interface CreateOrgNodeTypePayload {
  structure_model_id: number;
  code: string;
  label: string;
  icon_key?: string;
  depth_hint?: number;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  is_root_type: boolean;
}
```

────────────────────────────────────────────────────────────────────
CREATE src/services/org-service.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * org-service.ts
 *
 * IPC wrappers for the Organization module commands.
 * RULE: All invoke() calls for org commands are isolated here.
 *
 * This file covers the configuration layer (structure models, node types,
 * relationship rules). Node CRUD and responsibility bindings are in
 * org-node-service.ts (SP01-F02).
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  OrgStructureModel,
  OrgNodeType,
  OrgRelationshipRule,
  CreateOrgNodeTypePayload,
} from "@shared/ipc-types";

// ─── Structure models ─────────────────────────────────────────────────────────

export function listOrgStructureModels(): Promise<OrgStructureModel[]> {
  return invoke<OrgStructureModel[]>("list_org_structure_models");
}

export function getActiveOrgStructureModel(): Promise<OrgStructureModel | null> {
  return invoke<OrgStructureModel | null>("get_active_org_structure_model");
}

export function createOrgStructureModel(description?: string): Promise<OrgStructureModel> {
  return invoke<OrgStructureModel>("create_org_structure_model", {
    payload: { description: description ?? null },
  });
}

export function publishOrgStructureModel(modelId: number): Promise<OrgStructureModel> {
  return invoke<OrgStructureModel>("publish_org_structure_model", { model_id: modelId });
}

// ─── Node types ───────────────────────────────────────────────────────────────

export function listOrgNodeTypes(structureModelId: number): Promise<OrgNodeType[]> {
  return invoke<OrgNodeType[]>("list_org_node_types", {
    structure_model_id: structureModelId,
  });
}

export function createOrgNodeType(payload: CreateOrgNodeTypePayload): Promise<OrgNodeType> {
  return invoke<OrgNodeType>("create_org_node_type", { payload });
}

export function deactivateOrgNodeType(nodeTypeId: number): Promise<OrgNodeType> {
  return invoke<OrgNodeType>("deactivate_org_node_type", { node_type_id: nodeTypeId });
}

// ─── Relationship rules ───────────────────────────────────────────────────────

export function listOrgRelationshipRules(
  structureModelId: number
): Promise<OrgRelationshipRule[]> {
  return invoke<OrgRelationshipRule[]>("list_org_relationship_rules", {
    structure_model_id: structureModelId,
  });
}

export function createOrgRelationshipRule(
  structureModelId: number,
  parentTypeId: number,
  childTypeId: number,
  minChildren?: number,
  maxChildren?: number
): Promise<OrgRelationshipRule> {
  return invoke<OrgRelationshipRule>("create_org_relationship_rule", {
    payload: {
      structure_model_id: structureModelId,
      parent_type_id: parentTypeId,
      child_type_id: childTypeId,
      min_children: minChildren ?? null,
      max_children: maxChildren ?? null,
    },
  });
}

export function deleteOrgRelationshipRule(ruleId: number): Promise<void> {
  return invoke<void>("delete_org_relationship_rule", { rule_id: ruleId });
}
```

────────────────────────────────────────────────────────────────────
CREATE src/stores/org-store.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * org-store.ts
 *
 * Zustand store for org configuration state.
 * Caches the active structure model and its node types for use throughout
 * the UI. Node-level state (the actual org tree) is in a separate store
 * added in SP01-F02.
 */

import { create } from "zustand";
import {
  getActiveOrgStructureModel,
  listOrgNodeTypes,
  listOrgRelationshipRules,
} from "../services/org-service";
import type { OrgNodeType, OrgRelationshipRule, OrgStructureModel } from "@shared/ipc-types";

interface OrgConfigState {
  activeModel: OrgStructureModel | null;
  nodeTypes: OrgNodeType[];
  relationshipRules: OrgRelationshipRule[];
  loading: boolean;
  error: string | null;

  /** Load the active model and its node types + rules into the store. */
  loadActiveModelConfig: () => Promise<void>;
  /** Replace the active model after a publish operation. */
  setActiveModel: (model: OrgStructureModel) => void;
}

export const useOrgStore = create<OrgConfigState>((set) => ({
  activeModel: null,
  nodeTypes: [],
  relationshipRules: [],
  loading: false,
  error: null,

  loadActiveModelConfig: async () => {
    set({ loading: true, error: null });
    try {
      const model = await getActiveOrgStructureModel();
      if (model) {
        const [types, rules] = await Promise.all([
          listOrgNodeTypes(model.id),
          listOrgRelationshipRules(model.id),
        ]);
        set({ activeModel: model, nodeTypes: types, relationshipRules: rules });
      } else {
        set({ activeModel: null, nodeTypes: [], relationshipRules: [] });
      }
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) });
    } finally {
      set({ loading: false });
    }
  },

  setActiveModel: (model) => set({ activeModel: model }),
}));
```

────────────────────────────────────────────────────────────────────
SEED org.view / org.manage / org.admin permissions
────────────────────────────────────────────────────────────────────
Open `src-tauri/src/db/seeder.rs` (Phase 1 SP04-F03). Add the org permissions to the
permissions vec in `seed_permissions_and_roles()`:

```rust
// Org module permissions
("org.view",   "View org structure and nodes",         "org",  false, false),
("org.manage", "Create and edit org nodes",            "org",  false, false),
("org.admin",  "Manage org structure model and types", "org",  true,  true), // dangerous + step-up
```

The `org.admin` permission is dangerous (requires explicit grant) and requires step-up
because publishing a structural model can affect referential integrity across all modules.

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- `pnpm run typecheck` passes with 0 errors
- `list_org_structure_models` IPC returns `[]` on a fresh database
- After `create_org_structure_model`, calling `list_org_node_types(id)` returns `[]`
- `org.view`, `org.manage`, `org.admin` exist in the `permissions` table after startup
- A user without `org.view` permission gets `AppError::Permission` calling any read IPC
```

---

### Supervisor Verification — Sprint S3

**V1 — IPC registration.**
Open DevTools on a running application. Run:
```javascript
await window.__TAURI__.invoke('list_org_structure_models')
```
Expected: `[]` (empty array on a fresh database). If rejected, the command is not
registered in the invoke handler.

**V2 — Permission enforcement.**
Log in as a user without `org.view`. Run `list_org_structure_models`. Expected: rejection
with a permission error. Then log in as admin (who has `org.view`). Confirm the same
call succeeds.

**V3 — Store loads cleanly.**
With no active model, log in as admin and confirm `useOrgStore.getState().activeModel`
is `null` after calling `loadActiveModelConfig()`. After creating and publishing a model,
call `loadActiveModelConfig()` again and confirm `activeModel` is populated.

---

*End of Phase 2 · Sub-phase 01 · File 01*
