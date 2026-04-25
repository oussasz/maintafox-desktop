//! Structure model service.
//!
//! A structure model is the schema definition of the tenant's organizational
//! hierarchy: which node types exist, how they may relate, and what capability
//! flags they carry.
//!
//! Lifecycle:
//!   `create()`       → status = "draft"
//!   `publish()`      → status = "active" (previous active → "superseded")
//!   `archive()`      → status = "archived" (only for drafts or superseded)
//!
//! The "active" model is a singleton — only one model is active at a time.
//! The publish step validates that existing nodes conform to the new rules
//! before committing the transition (validation logic is in F04).

use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

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

// ─── Row mapping ──────────────────────────────────────────────────────────────

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

// ─── SQL constants ────────────────────────────────────────────────────────────

const SELECT_COLS: &str = r"
    id, sync_id, version_number, status, description,
    activated_at, activated_by_id, superseded_at,
    created_at, updated_at
";

// ─── Service functions ────────────────────────────────────────────────────────

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
        return Err(AppError::ValidationFailed(vec![format!(
            "no published (active) structure model exists — use create_model when bootstrapping the first structure draft"
        )]));
    };

    if has_any_draft(db).await? {
        return Err(AppError::ValidationFailed(vec!["a draft structure model already exists — publish, archive, or abandon it first".to_string()]));
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
        return Err(AppError::ValidationFailed(vec!["the published model has no node types — add types to the active model before forking, or use an empty first draft (bootstrap) instead".to_string()]));
    }

    let txn: DatabaseTransaction = db.begin().await?;

    if has_any_draft(&txn).await? {
        return Err(AppError::ValidationFailed(vec!["a draft structure model already exists — publish, archive, or abandon it first".to_string()]));
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
            .map_err(|_| {
                AppError::Internal(anyhow::anyhow!("org_node_types id does not fit i32"))
            })?;
        let code: String = row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?;
        let label: String = row
            .try_get::<String>("", "label")
            .map_err(|e| decode_err("label", e))?;
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
        txn
            .execute(Statement::from_sql_and_values(
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
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("node type insert did not return row id"))
            })?;
        let new_id: i32 = new_id_row
            .try_get::<i64>("", "new_id")
            .map_err(|e| decode_err("new_id", e))?
            .try_into()
            .map_err(|_| {
                AppError::Internal(anyhow::anyhow!("new node type id does not fit i32"))
            })?;
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
            .map_err(|e| decode_err("parent_type_id", e))?
            as i32;
        let c_old: i32 = rule
            .try_get::<i64>("", "child_type_id")
            .map_err(|e| decode_err("child_type_id", e))?
            as i32;
        let p_new = *old_to_new.get(&p_old).ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "parent_type_id {p_old} missing from fork map — data integrity"
            ))
        })?;
        let c_new = *old_to_new.get(&c_old).ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "child_type_id {c_old} missing from fork map — data integrity"
            ))
        })?;
        let min_c: Option<i32> = rule
            .try_get::<Option<i64>>("", "min_children")
            .map_err(|e| decode_err("min_children", e))?
            .map(|v| v as i32);
        let max_c: Option<i32> = rule
            .try_get::<Option<i64>>("", "max_children")
            .map_err(|e| decode_err("max_children", e))?
            .map(|v| v as i32);
        let rule_now = Utc::now().to_rfc3339();
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO org_type_relationship_rules
         (structure_model_id, parent_type_id, child_type_id, min_children, max_children, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
            [
                draft_id.into(),
                p_new.into(),
                c_new.into(),
                min_c.into(),
                max_c.into(),
                rule_now.into(),
            ],
        ))
        .await?;
    }

    txn.commit().await?;
    get_model_by_id(db, draft_id).await
}

/// Publish a draft model as the new active model.
///
/// The previously active model is moved to "superseded".
/// Validation must be performed by the caller before calling this function —
/// this function does not re-validate node conformance.
pub async fn publish_model(
    db: &DatabaseConnection,
    model_id: i32,
    activated_by_id: i32,
) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(db, model_id).await?;
    if model.status != "draft" {
        return Err(AppError::ValidationFailed(vec![format!(
            "model {} is '{}', not 'draft' — only draft models can be published",
            model_id, model.status
        )]));
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
/// Active models cannot be archived — publish a new model first.
pub async fn archive_model(db: &DatabaseConnection, model_id: i32) -> AppResult<OrgStructureModel> {
    let model = get_model_by_id(db, model_id).await?;
    if model.status == "active" {
        return Err(AppError::ValidationFailed(vec![
            "cannot archive the active model — publish a new model first".to_string(),
        ]));
    }
    if model.status == "archived" {
        return Err(AppError::ValidationFailed(
            vec!["model is already archived".to_string()],
        ));
    }

    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_structure_models SET status = 'archived', updated_at = ? WHERE id = ?",
        [now.into(), model_id.into()],
    ))
    .await?;

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
        return Err(AppError::ValidationFailed(vec![format!(
            "model {} is not a draft — only draft models can be edited",
            model_id
        )]));
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
