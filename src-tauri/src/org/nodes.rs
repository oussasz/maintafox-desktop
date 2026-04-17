//! Org node lifecycle service.
//!
//! Org nodes are the operational instances of the tenant's configured hierarchy.
//! Each node has a type from the active structure model, a position in the tree
//! (tracked via `ancestor_path` and `depth`), and optimistic-concurrency control
//! via `row_version`.
//!
//! Lifecycle operations:
//!   `create_org_node`           → insert with computed path/depth
//!   `update_org_node_metadata`  → metadata-only, no parent change
//!   `move_org_node`             → re-parent with subtree path rewrite
//!   `deactivate_org_node`       → status = "inactive", effective_to = now
//!
//! All mutating operations run inside a SQL transaction.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

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

// ─── Helpers ──────────────────────────────────────────────────────────────────

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_nodes row decode failed for column '{column}': {e}"
    ))
}

// ─── Row mapping ──────────────────────────────────────────────────────────────

const NODE_SELECT_COLS: &str = r"
    n.id, n.sync_id, n.code, n.name, n.node_type_id,
    n.parent_id, n.ancestor_path, n.depth,
    n.description, n.cost_center_code, n.external_reference,
    n.status, n.effective_from, n.effective_to,
    n.erp_reference, n.notes,
    n.created_at, n.updated_at, n.deleted_at,
    n.row_version, n.origin_machine_id, n.last_synced_checkpoint
";

fn map_node(row: &QueryResult) -> AppResult<OrgNode> {
    Ok(OrgNode {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        sync_id: row
            .try_get::<String>("", "sync_id")
            .map_err(|e| decode_err("sync_id", e))?,
        code: row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?,
        name: row
            .try_get::<String>("", "name")
            .map_err(|e| decode_err("name", e))?,
        node_type_id: row
            .try_get::<i64>("", "node_type_id")
            .map_err(|e| decode_err("node_type_id", e))?,
        parent_id: row
            .try_get::<Option<i64>>("", "parent_id")
            .map_err(|e| decode_err("parent_id", e))?,
        ancestor_path: row
            .try_get::<String>("", "ancestor_path")
            .map_err(|e| decode_err("ancestor_path", e))?,
        depth: row
            .try_get::<i64>("", "depth")
            .map_err(|e| decode_err("depth", e))?,
        description: row
            .try_get::<Option<String>>("", "description")
            .map_err(|e| decode_err("description", e))?,
        cost_center_code: row
            .try_get::<Option<String>>("", "cost_center_code")
            .map_err(|e| decode_err("cost_center_code", e))?,
        external_reference: row
            .try_get::<Option<String>>("", "external_reference")
            .map_err(|e| decode_err("external_reference", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        effective_from: row
            .try_get::<Option<String>>("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?,
        effective_to: row
            .try_get::<Option<String>>("", "effective_to")
            .map_err(|e| decode_err("effective_to", e))?,
        erp_reference: row
            .try_get::<Option<String>>("", "erp_reference")
            .map_err(|e| decode_err("erp_reference", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
        deleted_at: row
            .try_get::<Option<String>>("", "deleted_at")
            .map_err(|e| decode_err("deleted_at", e))?,
        row_version: row
            .try_get::<i64>("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        origin_machine_id: row
            .try_get::<Option<String>>("", "origin_machine_id")
            .map_err(|e| decode_err("origin_machine_id", e))?,
        last_synced_checkpoint: row
            .try_get::<Option<String>>("", "last_synced_checkpoint")
            .map_err(|e| decode_err("last_synced_checkpoint", e))?,
    })
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Return the id of the currently active structure model.
async fn get_active_model_id(db: &impl ConnectionTrait) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1".to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "no active org structure model exists".to_string(),
            ])
        })?;
    row.try_get::<i64>("", "id")
        .map_err(|e| decode_err("id", e))
}

/// Validate a parent–child node type pair against the active model's rules.
async fn assert_parent_child_allowed(
    db: &impl ConnectionTrait,
    model_id: i64,
    parent_type_id: i64,
    child_type_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_type_relationship_rules \
             WHERE structure_model_id = ? AND parent_type_id = ? AND child_type_id = ?",
            [model_id.into(), parent_type_id.into(), child_type_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt == 0 {
        return Err(AppError::ValidationFailed(vec![
            "parent-child node type combination is not allowed by the active model".to_string(),
        ]));
    }
    Ok(())
}

/// Fetch a node by id within a transaction context.
async fn fetch_node(db: &impl ConnectionTrait, node_id: i64) -> AppResult<OrgNode> {
    let sql = format!(
        "SELECT {NODE_SELECT_COLS} FROM org_nodes n WHERE n.id = ? AND n.deleted_at IS NULL"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [node_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".to_string(),
            id: node_id.to_string(),
        })?;
    map_node(&row)
}

/// Fetch node type flags for a node type within the active model.
async fn fetch_node_type_flags(
    db: &impl ConnectionTrait,
    model_id: i64,
    node_type_id: i64,
) -> AppResult<NodeTypeFlags> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_root_type, can_carry_cost_center \
             FROM org_node_types \
             WHERE id = ? AND structure_model_id = ? AND is_active = 1",
            [node_type_id.into(), model_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "node type {node_type_id} does not belong to the active structure model or is inactive"
            )])
        })?;
    Ok(NodeTypeFlags {
        is_root_type: i64_to_bool(
            row.try_get::<i64>("", "is_root_type")
                .map_err(|e| decode_err("is_root_type", e))?,
        ),
        can_carry_cost_center: i64_to_bool(
            row.try_get::<i64>("", "can_carry_cost_center")
                .map_err(|e| decode_err("can_carry_cost_center", e))?,
        ),
    })
}

struct NodeTypeFlags {
    is_root_type: bool,
    can_carry_cost_center: bool,
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Return the full org tree with denormalized node-type info and child counts.
/// Only returns non-deleted nodes. Sorted by `ancestor_path ASC, name ASC`.
pub async fn list_active_org_tree(db: &DatabaseConnection) -> AppResult<Vec<OrgTreeRow>> {
    let sql = format!(
        "SELECT {NODE_SELECT_COLS},
                t.code  AS node_type_code,
                t.label AS node_type_label,
                t.can_host_assets,
                t.can_own_work,
                t.can_carry_cost_center,
                t.can_aggregate_kpis,
                t.can_receive_permits,
                (SELECT COUNT(*) FROM org_nodes c
                 WHERE c.parent_id = n.id AND c.deleted_at IS NULL) AS child_count
         FROM org_nodes n
         JOIN org_node_types t ON t.id = n.node_type_id
         WHERE n.deleted_at IS NULL
         ORDER BY n.ancestor_path ASC, n.name ASC"
    );
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;

    rows.iter()
        .map(|row| {
            let node = map_node(row)?;
            Ok(OrgTreeRow {
                node,
                node_type_code: row
                    .try_get::<String>("", "node_type_code")
                    .map_err(|e| decode_err("node_type_code", e))?,
                node_type_label: row
                    .try_get::<String>("", "node_type_label")
                    .map_err(|e| decode_err("node_type_label", e))?,
                can_host_assets: i64_to_bool(
                    row.try_get::<i64>("", "can_host_assets")
                        .map_err(|e| decode_err("can_host_assets", e))?,
                ),
                can_own_work: i64_to_bool(
                    row.try_get::<i64>("", "can_own_work")
                        .map_err(|e| decode_err("can_own_work", e))?,
                ),
                can_carry_cost_center: i64_to_bool(
                    row.try_get::<i64>("", "can_carry_cost_center")
                        .map_err(|e| decode_err("can_carry_cost_center", e))?,
                ),
                can_aggregate_kpis: i64_to_bool(
                    row.try_get::<i64>("", "can_aggregate_kpis")
                        .map_err(|e| decode_err("can_aggregate_kpis", e))?,
                ),
                can_receive_permits: i64_to_bool(
                    row.try_get::<i64>("", "can_receive_permits")
                        .map_err(|e| decode_err("can_receive_permits", e))?,
                ),
                child_count: row
                    .try_get::<i64>("", "child_count")
                    .map_err(|e| decode_err("child_count", e))?,
            })
        })
        .collect()
}

/// Return a single org node by id (not deleted).
pub async fn get_org_node_by_id(db: &DatabaseConnection, node_id: i64) -> AppResult<OrgNode> {
    fetch_node(db, node_id).await
}

/// Create an org node. Runs inside a transaction.
///
/// Validation:
/// - `code` non-empty, trimmed, unique across active nodes
/// - `node_type_id` must belong to the active structure model
/// - root nodes: type must be `is_root_type`, depth = 0, path = `/{id}/`
/// - child nodes: parent must exist, parent-child pair must be allowed
/// - `cost_center_code` requires `can_carry_cost_center` on the node type
pub async fn create_org_node(
    db: &DatabaseConnection,
    payload: CreateOrgNodePayload,
    _created_by_id: i32,
) -> AppResult<OrgNode> {
    let code = payload.code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "node code must not be empty".to_string(),
        ]));
    }

    let txn = db.begin().await?;

    // Check code uniqueness across non-deleted nodes
    let dup_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes WHERE code = ? AND deleted_at IS NULL",
            [code.clone().into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let dup_count: i64 = dup_row.try_get("", "cnt").unwrap_or(0);
    if dup_count > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "node code '{code}' already exists"
        )]));
    }

    // Resolve active model and validate node type
    let model_id = get_active_model_id(&txn).await?;
    let flags = fetch_node_type_flags(&txn, model_id, payload.node_type_id).await?;

    // Validate cost_center_code against capability flag
    if payload.cost_center_code.is_some() && !flags.can_carry_cost_center {
        return Err(AppError::ValidationFailed(vec![
            "this node type cannot carry a cost center code".to_string(),
        ]));
    }

    // Compute depth and ancestor_path based on parent
    let (depth, parent_path, parent_id_val): (i64, String, Option<i64>) =
        if let Some(pid) = payload.parent_id {
            // Child node
            if flags.is_root_type {
                return Err(AppError::ValidationFailed(vec![
                    "a root node type cannot be created as a child node".to_string(),
                ]));
            }
            let parent = fetch_node(&txn, pid).await?;
            // Validate parent-child type pair
            assert_parent_child_allowed(&txn, model_id, parent.node_type_id, payload.node_type_id)
                .await?;
            (parent.depth + 1, parent.ancestor_path.clone(), Some(pid))
        } else {
            // Root node — type must be is_root_type
            if !flags.is_root_type {
                return Err(AppError::ValidationFailed(vec![
                    "only root node types can be created without a parent".to_string(),
                ]));
            }
            (0, String::new(), None)
        };

    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    // Insert with a temporary ancestor_path; we'll update once we know the id
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_nodes
          (sync_id, code, name, node_type_id, parent_id,
           ancestor_path, depth, description, cost_center_code,
           external_reference, status, effective_from, effective_to,
           erp_reference, notes, created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, ?, '/', ?, ?, ?, ?, 'active', ?, NULL, ?, ?, ?, ?, 1)",
        [
            sync_id.clone().into(),
            code.into(),
            payload.name.into(),
            payload.node_type_id.into(),
            parent_id_val.into(),
            depth.into(),
            payload.description.into(),
            payload.cost_center_code.into(),
            payload.external_reference.into(),
            payload.effective_from.into(),
            payload.erp_reference.into(),
            payload.notes.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve the inserted id via sync_id
    let id_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE sync_id = ?",
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("node created but not found after insert"))
        })?;
    let node_id: i64 = id_row
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    // Compute final ancestor_path: root = /{id}/, child = {parent_path}{id}/
    let ancestor_path = if parent_id_val.is_some() {
        format!("{parent_path}{node_id}/")
    } else {
        format!("/{node_id}/")
    };

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_nodes SET ancestor_path = ? WHERE id = ?",
        [ancestor_path.into(), node_id.into()],
    ))
    .await?;

    let node = fetch_node(&txn, node_id).await?;
    txn.commit().await?;

    tracing::info!(node_id, "org node created");

    Ok(node)
}

/// Update metadata fields on an org node. Does not change parent or tree position.
/// Requires matching `expected_row_version` for optimistic concurrency.
pub async fn update_org_node_metadata(
    db: &DatabaseConnection,
    payload: UpdateOrgNodeMetadataPayload,
) -> AppResult<OrgNode> {
    let node = fetch_node(db, payload.node_id).await?;
    if node.row_version != payload.expected_row_version {
        return Err(AppError::ValidationFailed(vec![format!(
            "row version mismatch: expected {}, actual {}",
            payload.expected_row_version, node.row_version
        )]));
    }

    // If cost_center_code is being set, verify the node type allows it
    if let Some(Some(_)) = &payload.cost_center_code {
        let model_id = get_active_model_id(db).await?;
        let flags = fetch_node_type_flags(db, model_id, node.node_type_id).await?;
        if !flags.can_carry_cost_center {
            return Err(AppError::ValidationFailed(vec![
                "this node type cannot carry a cost center code".to_string(),
            ]));
        }
    }

    // Build dynamic SET clause
    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref name) = payload.name {
        sets.push("name = ?".to_string());
        vals.push(name.clone().into());
    }
    if let Some(ref desc) = payload.description {
        sets.push("description = ?".to_string());
        vals.push(desc.clone().into());
    }
    if let Some(ref cc) = payload.cost_center_code {
        sets.push("cost_center_code = ?".to_string());
        vals.push(cc.clone().into());
    }
    if let Some(ref ext) = payload.external_reference {
        sets.push("external_reference = ?".to_string());
        vals.push(ext.clone().into());
    }
    if let Some(ref erp) = payload.erp_reference {
        sets.push("erp_reference = ?".to_string());
        vals.push(erp.clone().into());
    }
    if let Some(ref n) = payload.notes {
        sets.push("notes = ?".to_string());
        vals.push(n.clone().into());
    }
    if let Some(ref st) = payload.status {
        sets.push("status = ?".to_string());
        vals.push(st.clone().into());
    }

    if sets.is_empty() {
        return Ok(node);
    }

    let now = Utc::now().to_rfc3339();
    sets.push("updated_at = ?".to_string());
    vals.push(now.into());
    sets.push("row_version = row_version + 1".to_string());

    // WHERE clause values
    vals.push(payload.node_id.into());
    vals.push(payload.expected_row_version.into());

    let sql = format!(
        "UPDATE org_nodes SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );

    let result = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "concurrent modification detected — row version mismatch".to_string(),
        ]));
    }

    fetch_node(db, payload.node_id).await
}

/// Move an org node to a new parent. Runs inside a transaction.
///
/// Validation:
/// - Reject stale `row_version`
/// - Reject moving a node under itself or any descendant
/// - Reject root-to-child transitions when type is `is_root_type`
/// - Validate new parent-child pair against active model rules
/// - Recompute `ancestor_path` and `depth` for the moved node and all descendants
pub async fn move_org_node(
    db: &DatabaseConnection,
    payload: MoveOrgNodePayload,
    _moved_by_id: i32,
) -> AppResult<OrgNode> {
    let txn = db.begin().await?;

    let node = fetch_node(&txn, payload.node_id).await?;
    if node.row_version != payload.expected_row_version {
        return Err(AppError::ValidationFailed(vec![format!(
            "row version mismatch: expected {}, actual {}",
            payload.expected_row_version, node.row_version
        )]));
    }

    let model_id = get_active_model_id(&txn).await?;
    let flags = fetch_node_type_flags(&txn, model_id, node.node_type_id).await?;

    // Compute new parent context
    let (new_depth, new_parent_path, new_parent_id_val): (i64, String, Option<i64>) =
        if let Some(new_pid) = payload.new_parent_id {
            // Reject root-type → child transition
            if flags.is_root_type {
                return Err(AppError::ValidationFailed(vec![
                    "a root node type cannot be moved under a parent".to_string(),
                ]));
            }

            // Reject moving under self
            if new_pid == payload.node_id {
                return Err(AppError::ValidationFailed(vec![
                    "cannot move a node under itself".to_string(),
                ]));
            }

            let new_parent = fetch_node(&txn, new_pid).await?;

            // Reject moving under a descendant — the new parent's ancestor_path
            // must not contain this node's id segment
            let self_segment = format!("/{}/", payload.node_id);
            if new_parent.ancestor_path.contains(&self_segment) {
                return Err(AppError::ValidationFailed(vec![
                    "cannot move a node under one of its own descendants".to_string(),
                ]));
            }

            // Validate parent-child type pair
            assert_parent_child_allowed(
                &txn,
                model_id,
                new_parent.node_type_id,
                node.node_type_id,
            )
            .await?;

            (
                new_parent.depth + 1,
                new_parent.ancestor_path.clone(),
                Some(new_pid),
            )
        } else {
            // Moving to root
            if !flags.is_root_type {
                return Err(AppError::ValidationFailed(vec![
                    "only root node types can be moved to the root level".to_string(),
                ]));
            }
            (0, String::new(), None)
        };

    let old_path = node.ancestor_path.clone();
    let new_ancestor_path = if new_parent_id_val.is_some() {
        format!("{new_parent_path}{}/", payload.node_id)
    } else {
        format!("/{}/", payload.node_id)
    };
    let depth_delta = new_depth - node.depth;
    let now = Utc::now().to_rfc3339();

    // Update the moved node itself
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_nodes \
         SET parent_id = ?, ancestor_path = ?, depth = ?, \
             updated_at = ?, row_version = row_version + 1 \
         WHERE id = ?",
        [
            new_parent_id_val.into(),
            new_ancestor_path.clone().into(),
            new_depth.into(),
            now.clone().into(),
            payload.node_id.into(),
        ],
    ))
    .await?;

    // Update all descendants: rewrite ancestor_path prefix and adjust depth.
    // Descendants are identified by ancestor_path LIKE '{old_path}%' AND id != self.
    let descendants = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, ancestor_path, depth FROM org_nodes \
             WHERE ancestor_path LIKE ? AND id != ? AND deleted_at IS NULL",
            [format!("{old_path}%").into(), payload.node_id.into()],
        ))
        .await?;

    for desc_row in &descendants {
        let desc_id: i64 = desc_row
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
        let desc_path: String = desc_row
            .try_get("", "ancestor_path")
            .map_err(|e| decode_err("ancestor_path", e))?;
        let desc_depth: i64 = desc_row
            .try_get("", "depth")
            .map_err(|e| decode_err("depth", e))?;

        let updated_path = desc_path.replacen(&old_path, &new_ancestor_path, 1);
        let updated_depth = desc_depth + depth_delta;

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes \
             SET ancestor_path = ?, depth = ?, updated_at = ?, row_version = row_version + 1 \
             WHERE id = ?",
            [
                updated_path.into(),
                updated_depth.into(),
                now.clone().into(),
                desc_id.into(),
            ],
        ))
        .await?;
    }

    let moved_node = fetch_node(&txn, payload.node_id).await?;
    txn.commit().await?;

    tracing::info!(node_id = payload.node_id, "org node moved");

    Ok(moved_node)
}

/// Deactivate an org node. Runs inside a transaction.
///
/// Validation:
/// - Reject stale `row_version`
/// - Reject if active descendants exist
/// - Reject if active responsibility assignments exist (`valid_to IS NULL`)
/// - Sets `status = 'inactive'`, `effective_to = now`, increments `row_version`
pub async fn deactivate_org_node(
    db: &DatabaseConnection,
    node_id: i64,
    expected_row_version: i64,
    _deactivated_by_id: i32,
) -> AppResult<OrgNode> {
    let txn = db.begin().await?;

    let node = fetch_node(&txn, node_id).await?;
    if node.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![format!(
            "row version mismatch: expected {expected_row_version}, actual {}",
            node.row_version
        )]));
    }

    // Reject if active descendants exist
    let child_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes \
             WHERE parent_id = ? AND status = 'active' AND deleted_at IS NULL",
            [node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let active_children: i64 = child_row.try_get("", "cnt").unwrap_or(0);
    if active_children > 0 {
        return Err(AppError::ValidationFailed(vec![
            "cannot deactivate a node that has active child nodes".to_string(),
        ]));
    }

    // Reject if active responsibility assignments exist
    let resp_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_responsibilities \
             WHERE node_id = ? AND valid_to IS NULL",
            [node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let active_responsibilities: i64 = resp_row.try_get("", "cnt").unwrap_or(0);
    if active_responsibilities > 0 {
        return Err(AppError::ValidationFailed(vec![
            "cannot deactivate a node with active responsibility assignments — end them first"
                .to_string(),
        ]));
    }

    let now = Utc::now().to_rfc3339();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes \
             SET status = 'inactive', effective_to = ?, updated_at = ?, \
                 row_version = row_version + 1 \
             WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                now.into(),
                node_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "concurrent modification detected — row version mismatch".to_string(),
        ]));
    }

    let deactivated = fetch_node(&txn, node_id).await?;
    txn.commit().await?;

    tracing::info!(node_id, "org node deactivated");

    Ok(deactivated)
}
