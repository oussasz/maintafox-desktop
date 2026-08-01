//! Structure model service.
//!
//! A structure model is the schema definition of the tenant's organizational
//! hierarchy: which node types exist, how they may relate, and what capability
//! flags they carry.
//!
//! Lifecycle:
//!   `create()`       â†’ status = "draft"
//!   `publish()`      â†’ status = "active" (previous active â†’ "superseded")
//!   `archive()`      â†’ status = "archived" (only for drafts or superseded)
//!
//! The "active" model is a singleton â€” only one model is active at a time.
//! The publish step validates that existing nodes conform to the new rules
//! before committing the transition (validation logic is in F04).

use std::collections::HashMap;

use crate::errors::{issue_params, org_validation_failed_issues, AppError, AppResult, AppValidationIssue};
use crate::org::fail::{fail, fail_params};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, QueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// â”€â”€â”€ Types â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgStructureModel {
    pub id: i32,
    pub sync_id: String,
    pub version_number: i32,
    /// "draft" | "active" | "superseded" | "archived"
    pub status: String,
    pub description: Option<String>,
    pub activated_at: Option<String>,
    pub activated_by_id: Option<i32>,
    pub superseded_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateStructureModelPayload {
    pub description: Option<String>,
}

// â”€â”€â”€ Row mapping â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_structure_models row decode failed for column '{column}': {e}"
    ))
}

fn map_model(row: QueryResult) -> AppResult<OrgStructureModel> {
    Ok(OrgStructureModel {
        id: row.try_get::<i32>("", "id").map_err(|e| decode_err("id", e))?,
        sync_id: row
            .try_get::<String>("", "sync_id")
            .map_err(|e| decode_err("sync_id", e))?,
        version_number: row
            .try_get::<i32>("", "version_number")
            .map_err(|e| decode_err("version_number", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        description: row
            .try_get::<Option<String>>("", "description")
            .map_err(|e| decode_err("description", e))?,
        activated_at: row
            .try_get::<Option<String>>("", "activated_at")
            .map_err(|e| decode_err("activated_at", e))?,
        activated_by_id: row
            .try_get::<Option<i32>>("", "activated_by_id")
            .map_err(|e| decode_err("activated_by_id", e))?,
        superseded_at: row
            .try_get::<Option<String>>("", "superseded_at")
            .map_err(|e| decode_err("superseded_at", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

// â”€â”€â”€ SQL constants â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const SELECT_COLS: &str = r"
    id, sync_id, version_number, status, description,
    activated_at, activated_by_id, superseded_at,
    created_at, updated_at
";

// â”€â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Return all structure models ordered by version number descending.
pub async fn list_models(db: &DatabaseConnection) -> AppResult<Vec<OrgStructureModel>> {
    let sql = format!("SELECT {SELECT_COLS} FROM org_structure_models ORDER BY version_number DESC");
    let rows = db.query_all(Statement::from_string(DbBackend::Sqlite, sql)).await?;
    rows.into_iter().map(map_model).collect()
}

/// Return the currently active structure model, or None if none has been activated.
pub async fn get_active_model(db: &DatabaseConnection) -> AppResult<Option<OrgStructureModel>> {
    let sql = format!("SELECT {SELECT_COLS} FROM org_structure_models WHERE status = 'active' LIMIT 1");
    let row = db.query_one(Statement::from_string(DbBackend::Sqlite, sql)).await?;
    row.map(map_model).transpose()
}

/// Return a specific model by id.
pub async fn get_model_by_id(db: &DatabaseConnection, id: i32) -> AppResult<OrgStructureModel> {
    let sql = format!("SELECT {SELECT_COLS} FROM org_structure_models WHERE id = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_structure_model".to_string(),
            id: id.to_string(),
        })?;
    map_model(row)
}

/// Create a new structure model in draft status.
/// The version number is set to max(existing) + 1.
///
/// **Bootstrap only:** the IPC layer must reject this when a published (active) model
/// already exists so tenants start a new version via [`fork_draft_from_published`].
pub async fn create_model(
    db: &DatabaseConnection,
    payload: CreateStructureModelPayload,
    created_by_id: i32,
) -> AppResult<OrgStructureModel> {
    create_model_in_conn(db, &payload, created_by_id).await
}

/// Same as [`create_model`] but against any `ConnectionTrait` (used inside transactions).
pub async fn create_model_in_conn(
    db: &impl ConnectionTrait,
    payload: &CreateStructureModelPayload,
    created_by_id: i32,
) -> AppResult<OrgStructureModel> {
    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    let max_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(version_number), 0) AS max_ver FROM org_structure_models".to_string(),
        ))
        .await?;
    let max_version: i32 = max_row
        .and_then(|r| r.try_get::<i64>("", "max_ver").ok().and_then(|v| i32::try_from(v).ok()))
        .unwrap_or(0);
    let next_version = max_version + 1;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_structure_models
          (sync_id, version_number, status, description, created_at, updated_at)
          VALUES (?, ?, 'draft', ?, ?, ?)",
        [
            sync_id.clone().into(),
            next_version.into(),
            payload.description.clone().into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    let sql = format!("SELECT {SELECT_COLS} FROM org_structure_models WHERE sync_id = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [sync_id.into()]))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("structure model created but not found after insert")))?;
    let model = map_model(row)?;

    tracing::info!(
        model_id = model.id,
        version = next_version,
        actor = created_by_id,
        "org structure model created (draft)"
    );

    Ok(model)
}

/// Returns `true` if at least one draft structure model already exists.
async fn has_any_draft(db: &impl ConnectionTrait) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM org_structure_models WHERE status = 'draft'".to_string(),
        ))
        .await?
        .expect("COUNT");
    let c: i32 = row.try_get::<i64>("", "c").map_err(|e| decode_err("c", e))? as i32;
    Ok(c > 0)
}

/// Start a new draft from the current **published (active)** structure model: copies all
/// node types and relationship rules (same codes) so that publish-time remap
/// (see `validation::build_type_remap_plan`) applies cleanly.
///
/// Fails if there is no active model, or if a draft already exists, or (defensively) if
/// the active model has no node types (the published org cannot be described without a schema).
pub async fn fork_draft_from_published(
    db: &DatabaseConnection,
    payload: &CreateStructureModelPayload,
    created_by_id: i32,
) -> AppResult<OrgStructureModel> {
    let Some(active) = get_active_model(db).await? else {
        return Err(fail(
            "ORG_NO_ACTIVE_MODEL",
            "No published structure model exists — create an initial draft when bootstrapping the first structure.",
        ));
    };

    if has_any_draft(db).await? {
        return Err(fail(
            "ORG_DRAFT_EXISTS",
            "A draft structure model already exists — publish, archive, or abandon it first.",
        ));
    }

    let type_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, label, icon_key, color, depth_hint, \
                    can_host_assets, can_own_work, can_carry_cost_center, can_aggregate_kpis, can_receive_permits, \
                    is_root_type, is_active \
             FROM org_node_types \
             WHERE structure_model_id = ? \
             ORDER BY id ASC",
            [active.id.into()],
        ))
        .await?;

    if type_rows.is_empty() {
        return Err(fail(
            "ORG_FORK_NO_NODE_TYPES",
            "The published model has no node types — add types to the active model before forking, or create an empty first draft instead.",
        ));
    }

    // Heal NULL-scoped live nodes onto the active model before cloning.
    crate::org::model_scope::heal_null_structure_model_ids(db, active.id as i64).await?;
    crate::org::model_scope::assert_no_null_structure_model_ids(db).await?;

    let txn: DatabaseTransaction = db.begin().await?;

    if has_any_draft(&txn).await? {
        return Err(fail(
            "ORG_DRAFT_EXISTS",
            "A draft structure model already exists — publish, archive, or abandon it first.",
        ));
    }

    let draft = create_model_in_conn(&txn, payload, created_by_id).await?;
    let draft_id = draft.id;
    let now = Utc::now().to_rfc3339();

    let mut old_to_new: HashMap<i32, i32> = HashMap::new();

    for row in &type_rows {
        let old_id: i32 = row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?
            .try_into()
            .map_err(|_| AppError::Internal(anyhow::anyhow!("org_node_types id does not fit i32")))?;
        let code: String = row.try_get::<String>("", "code").map_err(|e| decode_err("code", e))?;
        let label: String = row.try_get::<String>("", "label").map_err(|e| decode_err("label", e))?;
        let icon_key: Option<String> = row
            .try_get::<Option<String>>("", "icon_key")
            .map_err(|e| decode_err("icon_key", e))?;
        let color: Option<String> = row
            .try_get::<Option<String>>("", "color")
            .map_err(|e| decode_err("color", e))?;
        let depth_hint: Option<i32> = row
            .try_get::<Option<i64>>("", "depth_hint")
            .map_err(|e| decode_err("depth_hint", e))?
            .map(|d| d as i32);
        let can_host_assets: i64 = row
            .try_get::<i64>("", "can_host_assets")
            .map_err(|e| decode_err("can_host_assets", e))?;
        let can_own_work: i64 = row
            .try_get::<i64>("", "can_own_work")
            .map_err(|e| decode_err("can_own_work", e))?;
        let can_carry_cost_center: i64 = row
            .try_get::<i64>("", "can_carry_cost_center")
            .map_err(|e| decode_err("can_carry_cost_center", e))?;
        let can_aggregate_kpis: i64 = row
            .try_get::<i64>("", "can_aggregate_kpis")
            .map_err(|e| decode_err("can_aggregate_kpis", e))?;
        let can_receive_permits: i64 = row
            .try_get::<i64>("", "can_receive_permits")
            .map_err(|e| decode_err("can_receive_permits", e))?;
        let is_root_type: i64 = row
            .try_get::<i64>("", "is_root_type")
            .map_err(|e| decode_err("is_root_type", e))?;
        let is_active: i64 = row
            .try_get::<i64>("", "is_active")
            .map_err(|e| decode_err("is_active", e))?;

        let sync_id = Uuid::new_v4().to_string();
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO org_node_types
            (sync_id, structure_model_id, code, label, icon_key, color, depth_hint,
             can_host_assets, can_own_work, can_carry_cost_center, can_aggregate_kpis, can_receive_permits,
             is_root_type, is_active, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                sync_id.clone().into(),
                draft_id.into(),
                code.into(),
                label.into(),
                icon_key.into(),
                color.into(),
                depth_hint.into(),
                (can_host_assets as i32).into(),
                (can_own_work as i32).into(),
                (can_carry_cost_center as i32).into(),
                (can_aggregate_kpis as i32).into(),
                (can_receive_permits as i32).into(),
                (is_root_type as i32).into(),
                (is_active as i32).into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;

        let new_id_row = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS new_id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("node type insert did not return row id")))?;
        let new_id: i32 = new_id_row
            .try_get::<i64>("", "new_id")
            .map_err(|e| decode_err("new_id", e))?
            .try_into()
            .map_err(|_| AppError::Internal(anyhow::anyhow!("new node type id does not fit i32")))?;
        old_to_new.insert(old_id, new_id);
    }

    let rules = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT parent_type_id, child_type_id, min_children, max_children \
             FROM org_type_relationship_rules WHERE structure_model_id = ?",
            [active.id.into()],
        ))
        .await?;

    for rule in &rules {
        let p_old: i32 = rule
            .try_get::<i64>("", "parent_type_id")
            .map_err(|e| decode_err("parent_type_id", e))? as i32;
        let c_old: i32 = rule
            .try_get::<i64>("", "child_type_id")
            .map_err(|e| decode_err("child_type_id", e))? as i32;
        let p_new = *old_to_new.get(&p_old).ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "parent_type_id {p_old} missing from fork map â€” data integrity"
            ))
        })?;
        let c_new = *old_to_new.get(&c_old).ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "child_type_id {c_old} missing from fork map â€” data integrity"
            ))
        })?;
        let min_children: Option<i32> = rule
            .try_get::<Option<i64>>("", "min_children")
            .map_err(|e| decode_err("min_children", e))?
            .map(|v| v as i32);
        let max_children: Option<i32> = rule
            .try_get::<Option<i64>>("", "max_children")
            .map_err(|e| decode_err("max_children", e))?
            .map(|v| v as i32);

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO org_type_relationship_rules
              (structure_model_id, parent_type_id, child_type_id, min_children, max_children, created_at)
              VALUES (?, ?, ?, ?, ?, ?)",
            [
                draft_id.into(),
                (p_new as i64).into(),
                (c_new as i64).into(),
                min_children.into(),
                max_children.into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }

    // â”€â”€ Clone nodes from the active model into the draft â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    //
    // Load non-deleted active nodes depth-first (depth ASC, id ASC) so that
    // parents are always inserted before their children, letting us remap
    // parent_id immediately during the loop.

    let active_node_rows = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, node_type_id, parent_id, depth, \
                    description, cost_center_code, external_reference, \
                    status, effective_from, effective_to, erp_reference, notes \
             FROM org_nodes \
             WHERE structure_model_id = ? AND deleted_at IS NULL \
             ORDER BY depth ASC, id ASC",
            [(active.id as i64).into()],
        ))
        .await?;

    // old_node_id â†’ new_node_id (used to remap parent_id for children)
    let mut old_to_new_node: HashMap<i64, i64> = HashMap::new();
    // new_node_id â†’ computed ancestor_path (used to build child paths)
    let mut new_node_paths: HashMap<i64, String> = HashMap::new();

    let node_now = Utc::now().to_rfc3339();

    for node_row in &active_node_rows {
        let old_id: i64 = node_row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?;
        let code: String = node_row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?;
        let name: String = node_row
            .try_get::<String>("", "name")
            .map_err(|e| decode_err("name", e))?;
        let old_type_id: i32 = node_row
            .try_get::<i64>("", "node_type_id")
            .map_err(|e| decode_err("node_type_id", e))? as i32;
        let new_type_id: i32 = *old_to_new.get(&old_type_id).ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "node type {old_type_id} missing from fork type map â€” data integrity"
            ))
        })?;
        let old_parent_id: Option<i64> = node_row
            .try_get::<Option<i64>>("", "parent_id")
            .map_err(|e| decode_err("parent_id", e))?;
        let new_parent_id: Option<i64> = match old_parent_id {
            Some(p) => Some(*old_to_new_node.get(&p).ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "parent node {p} missing from fork node map â€” depth ordering violated"
                ))
            })?),
            None => None,
        };
        let depth: i64 = node_row
            .try_get::<i64>("", "depth")
            .map_err(|e| decode_err("depth", e))?;
        let description: Option<String> = node_row
            .try_get::<Option<String>>("", "description")
            .map_err(|e| decode_err("description", e))?;
        let cost_center_code: Option<String> = node_row
            .try_get::<Option<String>>("", "cost_center_code")
            .map_err(|e| decode_err("cost_center_code", e))?;
        let external_reference: Option<String> = node_row
            .try_get::<Option<String>>("", "external_reference")
            .map_err(|e| decode_err("external_reference", e))?;
        let status: String = node_row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?;
        let effective_from: Option<String> = node_row
            .try_get::<Option<String>>("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?;
        let effective_to: Option<String> = node_row
            .try_get::<Option<String>>("", "effective_to")
            .map_err(|e| decode_err("effective_to", e))?;
        let erp_reference: Option<String> = node_row
            .try_get::<Option<String>>("", "erp_reference")
            .map_err(|e| decode_err("erp_reference", e))?;
        let notes: Option<String> = node_row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?;

        let node_sync_id = Uuid::new_v4().to_string();

        // Insert clone with temporary ancestor_path = '/'
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO org_nodes
              (sync_id, code, name, node_type_id, parent_id,
               ancestor_path, depth, description, cost_center_code,
               external_reference, status, effective_from, effective_to,
               erp_reference, notes, created_at, updated_at, row_version,
               structure_model_id, origin_node_id)
              VALUES (?, ?, ?, ?, ?, '/', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
            [
                node_sync_id.clone().into(),
                code.into(),
                name.into(),
                (new_type_id as i64).into(),
                new_parent_id.into(),
                depth.into(),
                description.into(),
                cost_center_code.into(),
                external_reference.into(),
                status.into(),
                effective_from.into(),
                effective_to.into(),
                erp_reference.into(),
                notes.into(),
                node_now.clone().into(),
                node_now.clone().into(),
                (draft_id as i64).into(),
                old_id.into(), // origin_node_id
            ],
        ))
        .await?;

        let new_id_row = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS new_id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("org_nodes clone did not return row id")))?;
        let new_node_id: i64 = new_id_row
            .try_get::<i64>("", "new_id")
            .map_err(|e| decode_err("new_id", e))?;

        // Compute correct ancestor_path now that we know the new id
        let ancestor_path = match new_parent_id {
            Some(new_pid) => {
                let parent_path = new_node_paths.get(&new_pid).ok_or_else(|| {
                    AppError::Internal(anyhow::anyhow!(
                        "parent path for new node {new_pid} not found â€” ordering bug"
                    ))
                })?;
                format!("{parent_path}{new_node_id}/")
            }
            None => format!("/{new_node_id}/"),
        };

        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET ancestor_path = ? WHERE id = ?",
            [ancestor_path.clone().into(), new_node_id.into()],
        ))
        .await?;

        old_to_new_node.insert(old_id, new_node_id);
        new_node_paths.insert(new_node_id, ancestor_path);
    }

    // â”€â”€ Clone responsibilities for each node â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    for (old_node_id, new_node_id) in &old_to_new_node {
        let resp_rows = txn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT responsibility_type, person_id, team_id, valid_from, valid_to \
                 FROM org_node_responsibilities WHERE node_id = ?",
                [(*old_node_id).into()],
            ))
            .await?;

        for resp in &resp_rows {
            let responsibility_type: String = resp
                .try_get::<String>("", "responsibility_type")
                .map_err(|e| decode_err("responsibility_type", e))?;
            let person_id: Option<i64> = resp
                .try_get::<Option<i64>>("", "person_id")
                .map_err(|e| decode_err("person_id", e))?;
            let team_id: Option<i64> = resp
                .try_get::<Option<i64>>("", "team_id")
                .map_err(|e| decode_err("team_id", e))?;
            let valid_from: Option<String> = resp
                .try_get::<Option<String>>("", "valid_from")
                .map_err(|e| decode_err("valid_from", e))?;
            let valid_to: Option<String> = resp
                .try_get::<Option<String>>("", "valid_to")
                .map_err(|e| decode_err("valid_to", e))?;

            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT INTO org_node_responsibilities
                  (node_id, responsibility_type, person_id, team_id, valid_from, valid_to,
                   created_at, updated_at)
                  VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    (*new_node_id).into(),
                    responsibility_type.into(),
                    person_id.into(),
                    team_id.into(),
                    valid_from.into(),
                    valid_to.into(),
                    node_now.clone().into(),
                    node_now.clone().into(),
                ],
            ))
            .await?;
        }
    }

    // â”€â”€ Clone entity bindings for each node â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    for (old_node_id, new_node_id) in &old_to_new_node {
        let binding_rows = txn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT binding_type, external_system, external_id, is_primary, valid_from, valid_to \
                 FROM org_entity_bindings WHERE node_id = ?",
                [(*old_node_id).into()],
            ))
            .await?;

        for binding in &binding_rows {
            let binding_type: String = binding
                .try_get::<String>("", "binding_type")
                .map_err(|e| decode_err("binding_type", e))?;
            let external_system: String = binding
                .try_get::<String>("", "external_system")
                .map_err(|e| decode_err("external_system", e))?;
            let external_id: String = binding
                .try_get::<String>("", "external_id")
                .map_err(|e| decode_err("external_id", e))?;
            let is_primary: i64 = binding
                .try_get::<i64>("", "is_primary")
                .map_err(|e| decode_err("is_primary", e))?;
            let valid_from: Option<String> = binding
                .try_get::<Option<String>>("", "valid_from")
                .map_err(|e| decode_err("valid_from", e))?;
            let valid_to: Option<String> = binding
                .try_get::<Option<String>>("", "valid_to")
                .map_err(|e| decode_err("valid_to", e))?;

            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT INTO org_entity_bindings
                  (node_id, binding_type, external_system, external_id, is_primary,
                   valid_from, valid_to, created_at)
                  VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    (*new_node_id).into(),
                    binding_type.into(),
                    external_system.into(),
                    external_id.into(),
                    is_primary.into(),
                    valid_from.into(),
                    valid_to.into(),
                    node_now.clone().into(),
                ],
            ))
            .await?;
        }
    }

    // Postcondition: every live active node must have a draft clone with origin_node_id.
    let active_live_count: i64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM org_nodes \
             WHERE structure_model_id = ? AND deleted_at IS NULL",
            [(active.id as i64).into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count query returned no row")))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    let draft_origin_count: i64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM org_nodes \
             WHERE structure_model_id = ? AND origin_node_id IS NOT NULL AND deleted_at IS NULL",
            [(draft_id as i64).into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count query returned no row")))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    if active_live_count != draft_origin_count {
        return Err(fail_params(
            "ORG_FORK_INCOMPLETE",
            format!(
                "Fork incomplete: active live nodes={active_live_count}, draft clones with origin={draft_origin_count}."
            ),
            &[
                ("activeLiveCount", active_live_count.to_string()),
                ("draftCloneCount", draft_origin_count.to_string()),
            ],
        ));
    }

    txn.commit().await?;
    get_model_by_id(db, draft_id).await
}

/// Result of repairing missing draft lineage clones for an existing draft.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgDraftLineageReconcileResult {
    pub draft_model_id: i64,
    pub cloned_count: i64,
}

/// Clone any active live nodes that are missing a draft `origin_node_id` clone.
///
/// Used to repair drafts forked before NULL-scoped nodes were healed. Does not
/// partial-corrupt: if a required draft type is missing by code, fails before inserts.
pub async fn reconcile_org_draft_lineage(
    db: &DatabaseConnection,
    draft_model_id: i64,
) -> AppResult<OrgDraftLineageReconcileResult> {
    let draft = get_model_by_id(db, draft_model_id as i32).await?;
    if draft.status != "draft" {
        return Err(fail_params(
            "ORG_PUBLISH_NOT_DRAFT",
            format!(
                "This model is '{}', not a draft — only draft models can be reconciled.",
                draft.status
            ),
            &[("status", draft.status.clone())],
        ));
    }

    let Some(active) = get_active_model(db).await? else {
        return Err(fail(
            "ORG_NO_ACTIVE_MODEL",
            "No active structure model — nothing to reconcile into the draft.",
        ));
    };

    crate::org::model_scope::heal_null_structure_model_ids(db, active.id as i64).await?;
    crate::org::model_scope::assert_no_null_structure_model_ids(db).await?;

    // Active type id â†’ draft type id (matched by code)
    let type_map_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.id AS old_id, d.id AS new_id \
             FROM org_node_types a \
             INNER JOIN org_node_types d \
               ON d.code = a.code AND d.structure_model_id = ? \
             WHERE a.structure_model_id = ?",
            [(draft_model_id).into(), (active.id as i64).into()],
        ))
        .await?;
    let mut old_to_new_type: HashMap<i32, i32> = HashMap::new();
    for row in &type_map_rows {
        let old_id: i32 = row.try_get::<i64>("", "old_id").map_err(|e| decode_err("old_id", e))? as i32;
        let new_id: i32 = row.try_get::<i64>("", "new_id").map_err(|e| decode_err("new_id", e))? as i32;
        old_to_new_type.insert(old_id, new_id);
    }

    let missing_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT a.id, a.code, a.name, a.node_type_id, a.parent_id, a.depth, \
                    a.description, a.cost_center_code, a.external_reference, \
                    a.status, a.effective_from, a.effective_to, a.erp_reference, a.notes, \
                    t.code AS type_code \
             FROM org_nodes a \
             INNER JOIN org_node_types t ON t.id = a.node_type_id \
             WHERE a.structure_model_id = ? AND a.deleted_at IS NULL \
               AND NOT EXISTS ( \
                 SELECT 1 FROM org_nodes d \
                 WHERE d.structure_model_id = ? \
                   AND d.origin_node_id = a.id \
                   AND d.deleted_at IS NULL \
               ) \
             ORDER BY a.depth ASC, a.id ASC",
            [(active.id as i64).into(), draft_model_id.into()],
        ))
        .await?;

    if missing_rows.is_empty() {
        return Ok(OrgDraftLineageReconcileResult {
            draft_model_id,
            cloned_count: 0,
        });
    }

    let mut type_issues: Vec<AppValidationIssue> = Vec::new();
    for row in &missing_rows {
        let old_type_id: i32 = row
            .try_get::<i64>("", "node_type_id")
            .map_err(|e| decode_err("node_type_id", e))? as i32;
        if !old_to_new_type.contains_key(&old_type_id) {
            let type_code: String = row
                .try_get::<String>("", "type_code")
                .map_err(|e| decode_err("type_code", e))?;
            let node_name: String = row.try_get::<String>("", "name").map_err(|e| decode_err("name", e))?;
            type_issues.push(AppValidationIssue::error(
                "ORG_RECONCILE_MISSING_NODE_TYPE",
                format!(
                    "The draft model is missing node type '{type_code}' required to clone active node '{node_name}'."
                ),
                issue_params(&[("typeCode", type_code), ("nodeName", node_name)]),
            ));
        }
    }
    if !type_issues.is_empty() {
        return Err(org_validation_failed_issues(type_issues));
    }

    // Seed remap with clones already present in the draft (for parent remapping).
    let existing_clones = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, origin_node_id FROM org_nodes \
             WHERE structure_model_id = ? AND origin_node_id IS NOT NULL AND deleted_at IS NULL",
            [draft_model_id.into()],
        ))
        .await?;
    let mut old_to_new_node: HashMap<i64, i64> = HashMap::new();
    let mut new_node_paths: HashMap<i64, String> = HashMap::new();
    for row in &existing_clones {
        let new_id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let old_id: i64 = row
            .try_get("", "origin_node_id")
            .map_err(|e| decode_err("origin_node_id", e))?;
        old_to_new_node.insert(old_id, new_id);
    }
    // Load ancestor paths for existing draft clones (needed if they become parents).
    if !old_to_new_node.is_empty() {
        let path_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, ancestor_path FROM org_nodes \
                 WHERE structure_model_id = ? AND deleted_at IS NULL",
                [draft_model_id.into()],
            ))
            .await?;
        for row in &path_rows {
            let id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
            let path: String = row
                .try_get("", "ancestor_path")
                .map_err(|e| decode_err("ancestor_path", e))?;
            new_node_paths.insert(id, path);
        }
    }

    let txn: DatabaseTransaction = db.begin().await?;
    let node_now = Utc::now().to_rfc3339();
    let mut cloned_count: i64 = 0;

    for node_row in &missing_rows {
        let old_id: i64 = node_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let code: String = node_row.try_get("", "code").map_err(|e| decode_err("code", e))?;
        let name: String = node_row.try_get("", "name").map_err(|e| decode_err("name", e))?;
        let old_type_id: i32 = node_row
            .try_get::<i64>("", "node_type_id")
            .map_err(|e| decode_err("node_type_id", e))? as i32;
        let new_type_id = *old_to_new_type.get(&old_type_id).expect("type pre-checked");
        let old_parent_id: Option<i64> = node_row
            .try_get("", "parent_id")
            .map_err(|e| decode_err("parent_id", e))?;
        let new_parent_id: Option<i64> = match old_parent_id {
            Some(p) => Some(*old_to_new_node.get(&p).ok_or_else(|| {
                fail(
                    "ORG_RECONCILE_PARENT_NOT_CLONED",
                    "Cannot clone this node because its parent has no draft clone yet — repair parents first.",
                )
            })?),
            None => None,
        };
        let depth: i64 = node_row.try_get("", "depth").map_err(|e| decode_err("depth", e))?;
        let description: Option<String> = node_row
            .try_get("", "description")
            .map_err(|e| decode_err("description", e))?;
        let cost_center_code: Option<String> = node_row
            .try_get("", "cost_center_code")
            .map_err(|e| decode_err("cost_center_code", e))?;
        let external_reference: Option<String> = node_row
            .try_get("", "external_reference")
            .map_err(|e| decode_err("external_reference", e))?;
        let status: String = node_row.try_get("", "status").map_err(|e| decode_err("status", e))?;
        let effective_from: Option<String> = node_row
            .try_get("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?;
        let effective_to: Option<String> = node_row
            .try_get("", "effective_to")
            .map_err(|e| decode_err("effective_to", e))?;
        let erp_reference: Option<String> = node_row
            .try_get("", "erp_reference")
            .map_err(|e| decode_err("erp_reference", e))?;
        let notes: Option<String> = node_row.try_get("", "notes").map_err(|e| decode_err("notes", e))?;

        // Re-attach lineage when a same-code draft node already exists (broken
        // origin_node_id) instead of inserting a duplicate code.
        let existing_by_code = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, ancestor_path FROM org_nodes \
                 WHERE structure_model_id = ? AND code = ? AND deleted_at IS NULL \
                 LIMIT 1",
                [draft_model_id.into(), code.clone().into()],
            ))
            .await?;
        if let Some(existing) = existing_by_code {
            let existing_id: i64 = existing.try_get("", "id").map_err(|e| decode_err("id", e))?;
            let path: String = existing
                .try_get("", "ancestor_path")
                .map_err(|e| decode_err("ancestor_path", e))?;
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE org_nodes SET origin_node_id = ?, updated_at = ? WHERE id = ?",
                [old_id.into(), node_now.clone().into(), existing_id.into()],
            ))
            .await?;
            old_to_new_node.insert(old_id, existing_id);
            new_node_paths.insert(existing_id, path);
            cloned_count += 1;
            continue;
        }

        let node_sync_id = Uuid::new_v4().to_string();
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO org_nodes
              (sync_id, code, name, node_type_id, parent_id,
               ancestor_path, depth, description, cost_center_code,
               external_reference, status, effective_from, effective_to,
               erp_reference, notes, created_at, updated_at, row_version,
               structure_model_id, origin_node_id)
              VALUES (?, ?, ?, ?, ?, '/', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
            [
                node_sync_id.into(),
                code.into(),
                name.into(),
                (new_type_id as i64).into(),
                new_parent_id.into(),
                depth.into(),
                description.into(),
                cost_center_code.into(),
                external_reference.into(),
                status.into(),
                effective_from.into(),
                effective_to.into(),
                erp_reference.into(),
                notes.into(),
                node_now.clone().into(),
                node_now.clone().into(),
                draft_model_id.into(),
                old_id.into(),
            ],
        ))
        .await?;

        let new_id_row = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS new_id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("org_nodes reconcile clone did not return row id")))?;
        let new_node_id: i64 = new_id_row.try_get("", "new_id").map_err(|e| decode_err("new_id", e))?;

        let ancestor_path = match new_parent_id {
            Some(new_pid) => {
                let parent_path = new_node_paths.get(&new_pid).ok_or_else(|| {
                    AppError::Internal(anyhow::anyhow!(
                        "parent path for new node {new_pid} not found during reconcile"
                    ))
                })?;
                format!("{parent_path}{new_node_id}/")
            }
            None => format!("/{new_node_id}/"),
        };
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET ancestor_path = ? WHERE id = ?",
            [ancestor_path.clone().into(), new_node_id.into()],
        ))
        .await?;

        // Responsibilities
        let resp_rows = txn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT responsibility_type, person_id, team_id, valid_from, valid_to \
                 FROM org_node_responsibilities WHERE node_id = ?",
                [old_id.into()],
            ))
            .await?;
        for resp in &resp_rows {
            let responsibility_type: String = resp
                .try_get("", "responsibility_type")
                .map_err(|e| decode_err("responsibility_type", e))?;
            let person_id: Option<i64> = resp.try_get("", "person_id").map_err(|e| decode_err("person_id", e))?;
            let team_id: Option<i64> = resp.try_get("", "team_id").map_err(|e| decode_err("team_id", e))?;
            let valid_from: Option<String> = resp
                .try_get("", "valid_from")
                .map_err(|e| decode_err("valid_from", e))?;
            let valid_to: Option<String> = resp.try_get("", "valid_to").map_err(|e| decode_err("valid_to", e))?;
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT INTO org_node_responsibilities
                  (node_id, responsibility_type, person_id, team_id, valid_from, valid_to,
                   created_at, updated_at)
                  VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    new_node_id.into(),
                    responsibility_type.into(),
                    person_id.into(),
                    team_id.into(),
                    valid_from.into(),
                    valid_to.into(),
                    node_now.clone().into(),
                    node_now.clone().into(),
                ],
            ))
            .await?;
        }

        // Entity bindings
        let binding_rows = txn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT binding_type, external_system, external_id, is_primary, valid_from, valid_to \
                 FROM org_entity_bindings WHERE node_id = ?",
                [old_id.into()],
            ))
            .await?;
        for binding in &binding_rows {
            let binding_type: String = binding
                .try_get("", "binding_type")
                .map_err(|e| decode_err("binding_type", e))?;
            let external_system: String = binding
                .try_get("", "external_system")
                .map_err(|e| decode_err("external_system", e))?;
            let external_id: String = binding
                .try_get("", "external_id")
                .map_err(|e| decode_err("external_id", e))?;
            let is_primary: i64 = binding
                .try_get("", "is_primary")
                .map_err(|e| decode_err("is_primary", e))?;
            let valid_from: Option<String> = binding
                .try_get("", "valid_from")
                .map_err(|e| decode_err("valid_from", e))?;
            let valid_to: Option<String> = binding.try_get("", "valid_to").map_err(|e| decode_err("valid_to", e))?;
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r"INSERT INTO org_entity_bindings
                  (node_id, binding_type, external_system, external_id, is_primary,
                   valid_from, valid_to, created_at)
                  VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    new_node_id.into(),
                    binding_type.into(),
                    external_system.into(),
                    external_id.into(),
                    is_primary.into(),
                    valid_from.into(),
                    valid_to.into(),
                    node_now.clone().into(),
                ],
            ))
            .await?;
        }

        old_to_new_node.insert(old_id, new_node_id);
        new_node_paths.insert(new_node_id, ancestor_path);
        cloned_count += 1;
    }

    txn.commit().await?;
    Ok(OrgDraftLineageReconcileResult {
        draft_model_id,
        cloned_count,
    })
}

/// Publish a draft model as the new active model.
///
/// The previously active model is moved to "superseded".
/// Validation must be performed by the caller before calling this function â€”
/// this function does not re-validate node conformance.
pub async fn publish_model(
    db: &DatabaseConnection,
    model_id: i32,
    activated_by_id: i32,
) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(db, model_id).await?;
    if model.status != "draft" {
        return Err(fail_params(
            "ORG_PUBLISH_NOT_DRAFT",
            format!(
                "This model is '{}', not a draft — only draft models can be published.",
                model.status
            ),
            &[("status", model.status.clone())],
        ));
    }

    let now = Utc::now().to_rfc3339();

    // Supersede the current active model (if any)
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"UPDATE org_structure_models
          SET status = 'superseded', superseded_at = ?, updated_at = ?
          WHERE status = 'active'",
        [now.clone().into(), now.clone().into()],
    ))
    .await?;

    // Activate the target model
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"UPDATE org_structure_models
          SET status = 'active', activated_at = ?, activated_by_id = ?, updated_at = ?
          WHERE id = ?",
        [
            now.into(),
            activated_by_id.into(),
            Utc::now().to_rfc3339().into(),
            model_id.into(),
        ],
    ))
    .await?;

    tracing::info!(
        model_id = model_id,
        actor = activated_by_id,
        "org structure model published (active)"
    );

    get_model_by_id(db, model_id).await
}

/// Archive a draft or superseded model.
/// Active models cannot be archived â€” publish a new model first.
///
/// For **draft** models, all draft-scoped data is hard-deleted before archiving
/// (in FK-safe order): entity bindings â†’ responsibilities â†’ nodes â†’ relationship
/// rules â†’ node types â†’ model status update.
///
/// For **superseded** models, only the status is updated (historical data is kept).
pub async fn archive_model(db: &DatabaseConnection, model_id: i32) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(db, model_id).await?;
    if model.status == "active" {
        return Err(fail(
            "ORG_ARCHIVE_ACTIVE",
            "Cannot archive the active model — publish a new model first.",
        ));
    }
    if model.status == "archived" {
        return Err(fail("ORG_ALREADY_ARCHIVED", "This model is already archived."));
    }

    let now = Utc::now().to_rfc3339();

    if model.status == "draft" {
        // For draft models, hard-delete all draft-scoped artifacts then archive.
        let txn = db.begin().await?;

        // Collect all draft node ids (include soft-deleted â€” full cleanup)
        let node_id_rows = txn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes WHERE structure_model_id = ?",
                [model_id.into()],
            ))
            .await?;

        // Hard-delete entity bindings and responsibilities referencing draft nodes
        for row in &node_id_rows {
            let nid: i64 = row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?;

            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM org_entity_bindings WHERE node_id = ?",
                [nid.into()],
            ))
            .await?;

            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM org_node_responsibilities WHERE node_id = ?",
                [nid.into()],
            ))
            .await?;
        }

        // Nullify self-referential parent_id links before deleting nodes to
        // avoid FK violations when SQLite enforces referential integrity.
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET parent_id = NULL WHERE structure_model_id = ?",
            [model_id.into()],
        ))
        .await?;

        // Hard-delete all draft nodes
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM org_nodes WHERE structure_model_id = ?",
            [model_id.into()],
        ))
        .await?;

        // Hard-delete relationship rules for this model
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM org_type_relationship_rules WHERE structure_model_id = ?",
            [model_id.into()],
        ))
        .await?;

        // Hard-delete node types for this model
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM org_node_types WHERE structure_model_id = ?",
            [model_id.into()],
        ))
        .await?;

        // Set model archived
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_structure_models SET status = 'archived', updated_at = ? WHERE id = ?",
            [now.into(), model_id.into()],
        ))
        .await?;

        txn.commit().await?;
    } else {
        // Superseded â€” only update status; historical data is preserved.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_structure_models SET status = 'archived', updated_at = ? WHERE id = ?",
            [now.into(), model_id.into()],
        ))
        .await?;
    }

    tracing::info!(model_id = model_id, "org structure model archived");
    get_model_by_id(db, model_id).await
}

/// Update a draft model's description.
/// Only draft models can be edited.
pub async fn update_model_description(
    db: &DatabaseConnection,
    model_id: i32,
    description: Option<String>,
) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(db, model_id).await?;
    if model.status != "draft" {
        return Err(fail("ORG_EDIT_REQUIRES_DRAFT", "Only draft models can be edited."));
    }

    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_structure_models SET description = ?, updated_at = ? WHERE id = ?",
        [description.into(), now.into(), model_id.into()],
    ))
    .await?;

    get_model_by_id(db, model_id).await
}
