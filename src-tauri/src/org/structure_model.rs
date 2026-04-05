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

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
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
pub async fn create_model(
    db: &DatabaseConnection,
    payload: CreateStructureModelPayload,
    created_by_id: i32,
) -> AppResult<OrgStructureModel> {
    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    // Calculate next version number
    let max_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(version_number), 0) AS max_ver FROM org_structure_models".to_string(),
        ))
        .await?;
    let max_version: i32 = max_row
        .map_or(0, |r| r.try_get::<i32>("", "max_ver").unwrap_or(0));
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

    // Retrieve the inserted row via sync_id (stable across DB backends)
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
