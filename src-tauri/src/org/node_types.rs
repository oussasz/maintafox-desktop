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
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNodeType {
    pub id: i32,
    pub sync_id: String,
    pub structure_model_id: i32,
    pub code: String,
    pub label: String,
    pub icon_key: Option<String>,
    pub color: Option<String>,
    pub depth_hint: Option<i32>,
    pub can_host_assets: bool,
    pub can_own_work: bool,
    pub can_carry_cost_center: bool,
    pub can_aggregate_kpis: bool,
    pub can_receive_permits: bool,
    pub is_root_type: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Deserialize)]
pub struct CreateNodeTypePayload {
    pub structure_model_id: i32,
    pub code: String,
    pub label: String,
    pub icon_key: Option<String>,
    pub color: Option<String>,
    pub depth_hint: Option<i32>,
    pub can_host_assets: bool,
    pub can_own_work: bool,
    pub can_carry_cost_center: bool,
    pub can_aggregate_kpis: bool,
    pub can_receive_permits: bool,
    pub is_root_type: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNodeTypePayload {
    pub id: i32,
    pub label: Option<String>,
    pub icon_key: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub depth_hint: Option<Option<i32>>,
    pub can_host_assets: Option<bool>,
    pub can_own_work: Option<bool>,
    pub can_carry_cost_center: Option<bool>,
    pub can_aggregate_kpis: Option<bool>,
    pub can_receive_permits: Option<bool>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn bool_to_i32(b: bool) -> i32 {
    i32::from(b)
}

const fn i32_to_bool(n: i32) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_node_types row decode failed for column '{column}': {e}"
    ))
}

// ─── Row mapping ──────────────────────────────────────────────────────────────

const SELECT_COLS: &str = r"
    id, sync_id, structure_model_id, code, label, icon_key, color, depth_hint,
    can_host_assets, can_own_work, can_carry_cost_center,
    can_aggregate_kpis, can_receive_permits, is_root_type,
    is_active, created_at, updated_at
";

fn map_node_type(row: QueryResult) -> AppResult<OrgNodeType> {
    Ok(OrgNodeType {
        id: row.try_get::<i32>("", "id").map_err(|e| decode_err("id", e))?,
        sync_id: row
            .try_get::<String>("", "sync_id")
            .map_err(|e| decode_err("sync_id", e))?,
        structure_model_id: row
            .try_get::<i32>("", "structure_model_id")
            .map_err(|e| decode_err("structure_model_id", e))?,
        code: row.try_get::<String>("", "code").map_err(|e| decode_err("code", e))?,
        label: row.try_get::<String>("", "label").map_err(|e| decode_err("label", e))?,
        icon_key: row
            .try_get::<Option<String>>("", "icon_key")
            .map_err(|e| decode_err("icon_key", e))?,
        color: row
            .try_get::<Option<String>>("", "color")
            .map_err(|e| decode_err("color", e))?,
        depth_hint: row
            .try_get::<Option<i32>>("", "depth_hint")
            .map_err(|e| decode_err("depth_hint", e))?,
        can_host_assets: i32_to_bool(
            row.try_get::<i32>("", "can_host_assets")
                .map_err(|e| decode_err("can_host_assets", e))?,
        ),
        can_own_work: i32_to_bool(
            row.try_get::<i32>("", "can_own_work")
                .map_err(|e| decode_err("can_own_work", e))?,
        ),
        can_carry_cost_center: i32_to_bool(
            row.try_get::<i32>("", "can_carry_cost_center")
                .map_err(|e| decode_err("can_carry_cost_center", e))?,
        ),
        can_aggregate_kpis: i32_to_bool(
            row.try_get::<i32>("", "can_aggregate_kpis")
                .map_err(|e| decode_err("can_aggregate_kpis", e))?,
        ),
        can_receive_permits: i32_to_bool(
            row.try_get::<i32>("", "can_receive_permits")
                .map_err(|e| decode_err("can_receive_permits", e))?,
        ),
        is_root_type: i32_to_bool(
            row.try_get::<i32>("", "is_root_type")
                .map_err(|e| decode_err("is_root_type", e))?,
        ),
        is_active: i32_to_bool(
            row.try_get::<i32>("", "is_active")
                .map_err(|e| decode_err("is_active", e))?,
        ),
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Return all node types for a given structure model.
pub async fn list_node_types(db: &DatabaseConnection, structure_model_id: i32) -> AppResult<Vec<OrgNodeType>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM org_node_types \
         WHERE structure_model_id = ? \
         ORDER BY depth_hint ASC, label ASC"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [structure_model_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_node_type).collect()
}

/// Return a single node type by id.
pub async fn get_node_type_by_id(db: &DatabaseConnection, id: i32) -> AppResult<OrgNodeType> {
    let sql = format!("SELECT {SELECT_COLS} FROM org_node_types WHERE id = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node_type".to_string(),
            id: id.to_string(),
        })?;
    map_node_type(row)
}

/// Create a node type for a (draft) structure model.
/// Only draft models can have node types added to them.
pub async fn create_node_type(db: &DatabaseConnection, payload: CreateNodeTypePayload) -> AppResult<OrgNodeType> {
    // Verify the target model is in draft status
    let model_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM org_structure_models WHERE id = ?",
            [payload.structure_model_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_structure_model".to_string(),
            id: payload.structure_model_id.to_string(),
        })?;
    let model_status: String = model_row.try_get("", "status").map_err(|e| decode_err("status", e))?;

    if model_status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "node types can only be added to draft structure models".to_string(),
        ]));
    }

    // Validate code uniqueness within this model
    let count_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_types WHERE structure_model_id = ? AND code = ?",
            [payload.structure_model_id.into(), payload.code.clone().into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let existing: i32 = count_row.try_get("", "cnt").unwrap_or(0);
    if existing > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "node type code '{}' already exists in this model",
            payload.code
        )]));
    }

    // If this is declared as root type, ensure no other root type exists in this model
    if payload.is_root_type {
        let root_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM org_node_types \
                 WHERE structure_model_id = ? AND is_root_type = 1",
                [payload.structure_model_id.into()],
            ))
            .await?
            .expect("COUNT always returns a row");
        let root_count: i32 = root_row.try_get("", "cnt").unwrap_or(0);
        if root_count > 0 {
            return Err(AppError::ValidationFailed(vec![
                "a root node type already exists in this model — only one root type is allowed".to_string(),
            ]));
        }
    }

    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_node_types
          (sync_id, structure_model_id, code, label, icon_key, color, depth_hint,
           can_host_assets, can_own_work, can_carry_cost_center,
           can_aggregate_kpis, can_receive_permits, is_root_type,
           is_active, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            sync_id.clone().into(),
            payload.structure_model_id.into(),
            payload.code.clone().into(),
            payload.label.into(),
            payload.icon_key.into(),
            payload.color.into(),
            payload.depth_hint.into(),
            bool_to_i32(payload.can_host_assets).into(),
            bool_to_i32(payload.can_own_work).into(),
            bool_to_i32(payload.can_carry_cost_center).into(),
            bool_to_i32(payload.can_aggregate_kpis).into(),
            bool_to_i32(payload.can_receive_permits).into(),
            bool_to_i32(payload.is_root_type).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve via sync_id
    let sql = format!("SELECT {SELECT_COLS} FROM org_node_types WHERE sync_id = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [sync_id.into()]))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("node type created but not found after insert")))?;
    let node_type = map_node_type(row)?;

    tracing::info!(
        node_type_id = node_type.id,
        code = %payload.code,
        model_id = payload.structure_model_id,
        "org node type created"
    );

    Ok(node_type)
}

/// Deactivate a node type. Cannot deactivate if `org_nodes` of this type exist.
pub async fn deactivate_node_type(db: &DatabaseConnection, id: i32) -> AppResult<OrgNodeType> {
    // Verify the node type exists first
    let _existing = get_node_type_by_id(db, id).await?;

    let count_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes WHERE node_type_id = ? AND deleted_at IS NULL",
            [id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let node_count: i32 = count_row.try_get("", "cnt").unwrap_or(0);

    if node_count > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "{node_count} node(s) of this type exist — cannot deactivate"
        )]));
    }

    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_node_types SET is_active = 0, updated_at = ? WHERE id = ?",
        [now.into(), id.into()],
    ))
    .await?;

    tracing::info!(node_type_id = id, "org node type deactivated");
    get_node_type_by_id(db, id).await
}

/// Update mutable fields of a node type. Only fields with `Some` values are updated.
pub async fn update_node_type(
    db: &DatabaseConnection,
    payload: UpdateNodeTypePayload,
) -> AppResult<OrgNodeType> {
    let _existing = get_node_type_by_id(db, payload.id).await?;

    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref label) = payload.label {
        sets.push("label = ?".to_string());
        values.push(label.clone().into());
    }
    if let Some(ref icon_key) = payload.icon_key {
        sets.push("icon_key = ?".to_string());
        values.push(icon_key.clone().into());
    }
    if let Some(ref color) = payload.color {
        sets.push("color = ?".to_string());
        values.push(color.clone().into());
    }
    if let Some(ref depth_hint) = payload.depth_hint {
        sets.push("depth_hint = ?".to_string());
        values.push((*depth_hint).into());
    }
    if let Some(v) = payload.can_host_assets {
        sets.push("can_host_assets = ?".to_string());
        values.push(bool_to_i32(v).into());
    }
    if let Some(v) = payload.can_own_work {
        sets.push("can_own_work = ?".to_string());
        values.push(bool_to_i32(v).into());
    }
    if let Some(v) = payload.can_carry_cost_center {
        sets.push("can_carry_cost_center = ?".to_string());
        values.push(bool_to_i32(v).into());
    }
    if let Some(v) = payload.can_aggregate_kpis {
        sets.push("can_aggregate_kpis = ?".to_string());
        values.push(bool_to_i32(v).into());
    }
    if let Some(v) = payload.can_receive_permits {
        sets.push("can_receive_permits = ?".to_string());
        values.push(bool_to_i32(v).into());
    }

    if sets.is_empty() {
        return get_node_type_by_id(db, payload.id).await;
    }

    let now = Utc::now().to_rfc3339();
    sets.push("updated_at = ?".to_string());
    values.push(now.into());
    values.push(payload.id.into());

    let sql = format!(
        "UPDATE org_node_types SET {} WHERE id = ?",
        sets.join(", ")
    );
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;

    tracing::info!(node_type_id = payload.id, "org node type updated");
    get_node_type_by_id(db, payload.id).await
}

/// Count how many live nodes reference this type (for delete-blocking).
pub async fn count_nodes_using_type(db: &DatabaseConnection, node_type_id: i32) -> AppResult<i32> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes WHERE node_type_id = ? AND deleted_at IS NULL",
            [node_type_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    Ok(row.try_get::<i32>("", "cnt").unwrap_or(0))
}
