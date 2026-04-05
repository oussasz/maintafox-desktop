//! Relationship rules service.
//!
//! A relationship rule declares that a node of `parent_type` may contain
//! children of `child_type`, with optional min/max cardinality constraints.
//!
//! Rules are scoped to a structure model and can only be added / removed while
//! the model is in `draft` status.
//!
//! The `list_rules` function returns denormalized rows that include the parent
//! and child type labels so the UI can render them directly without a second
//! round-trip.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgRelationshipRule {
    pub id: i32,
    pub structure_model_id: i32,
    pub parent_type_id: i32,
    pub child_type_id: i32,
    pub min_children: Option<i32>,
    pub max_children: Option<i32>,
    pub created_at: String,
    // Denormalized labels (set by list_rules, empty for other returns)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_type_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_type_label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRelationshipRulePayload {
    pub structure_model_id: i32,
    pub parent_type_id: i32,
    pub child_type_id: i32,
    pub min_children: Option<i32>,
    pub max_children: Option<i32>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_type_relationship_rules row decode failed for column '{column}': {e}"
    ))
}

const SELECT_COLS: &str = r"
    r.id, r.structure_model_id, r.parent_type_id, r.child_type_id,
    r.min_children, r.max_children, r.created_at
";

fn map_rule_basic(row: QueryResult) -> AppResult<OrgRelationshipRule> {
    Ok(OrgRelationshipRule {
        id: row.try_get::<i32>("", "id").map_err(|e| decode_err("id", e))?,
        structure_model_id: row
            .try_get::<i32>("", "structure_model_id")
            .map_err(|e| decode_err("structure_model_id", e))?,
        parent_type_id: row
            .try_get::<i32>("", "parent_type_id")
            .map_err(|e| decode_err("parent_type_id", e))?,
        child_type_id: row
            .try_get::<i32>("", "child_type_id")
            .map_err(|e| decode_err("child_type_id", e))?,
        min_children: row
            .try_get::<Option<i32>>("", "min_children")
            .map_err(|e| decode_err("min_children", e))?,
        max_children: row
            .try_get::<Option<i32>>("", "max_children")
            .map_err(|e| decode_err("max_children", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        parent_type_label: None,
        child_type_label: None,
    })
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Return all relationship rules for a structure model, with parent/child labels.
pub async fn list_rules(db: &DatabaseConnection, structure_model_id: i32) -> AppResult<Vec<OrgRelationshipRule>> {
    let sql = format!(
        "SELECT {SELECT_COLS},
                pt.label AS parent_type_label,
                ct.label AS child_type_label
         FROM org_type_relationship_rules r
         JOIN org_node_types pt ON pt.id = r.parent_type_id
         JOIN org_node_types ct ON ct.id = r.child_type_id
         WHERE r.structure_model_id = ?
         ORDER BY pt.label ASC, ct.label ASC"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [structure_model_id.into()],
        ))
        .await?;

    rows.into_iter()
        .map(|row| {
            let parent_label: Option<String> = row.try_get("", "parent_type_label").ok();
            let child_label: Option<String> = row.try_get("", "child_type_label").ok();
            let mut rule = map_rule_basic(row)?;
            rule.parent_type_label = parent_label;
            rule.child_type_label = child_label;
            Ok(rule)
        })
        .collect()
}

/// Check whether a parent–child type combination is allowed.
pub async fn is_allowed(
    db: &DatabaseConnection,
    structure_model_id: i32,
    parent_type_id: i32,
    child_type_id: i32,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_type_relationship_rules \
             WHERE structure_model_id = ? AND parent_type_id = ? AND child_type_id = ?",
            [structure_model_id.into(), parent_type_id.into(), child_type_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i32 = row.try_get("", "cnt").unwrap_or(0);
    Ok(cnt > 0)
}

/// Create a relationship rule in a draft structure model.
pub async fn create_rule(
    db: &DatabaseConnection,
    payload: CreateRelationshipRulePayload,
) -> AppResult<OrgRelationshipRule> {
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
            "rules can only be added to draft structure models".to_string(),
        ]));
    }

    // Validate no duplicate rule
    let existing = is_allowed(
        db,
        payload.structure_model_id,
        payload.parent_type_id,
        payload.child_type_id,
    )
    .await?;
    if existing {
        return Err(AppError::ValidationFailed(vec![
            "this parent–child relationship rule already exists".to_string(),
        ]));
    }

    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_type_relationship_rules
          (structure_model_id, parent_type_id, child_type_id,
           min_children, max_children, created_at)
          VALUES (?, ?, ?, ?, ?, ?)",
        [
            payload.structure_model_id.into(),
            payload.parent_type_id.into(),
            payload.child_type_id.into(),
            payload.min_children.into(),
            payload.max_children.into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve the inserted rule via last_insert_rowid
    let id_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id",
            [],
        ))
        .await?
        .expect("last_insert_rowid always returns a row");
    let new_id: i32 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

    tracing::info!(
        rule_id = new_id,
        parent_type_id = payload.parent_type_id,
        child_type_id = payload.child_type_id,
        model_id = payload.structure_model_id,
        "org relationship rule created"
    );

    get_rule_by_id(db, new_id).await
}

/// Return a single rule by its id.
pub async fn get_rule_by_id(db: &DatabaseConnection, id: i32) -> AppResult<OrgRelationshipRule> {
    let sql = format!("SELECT {SELECT_COLS} FROM org_type_relationship_rules r WHERE r.id = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_type_relationship_rule".to_string(),
            id: id.to_string(),
        })?;
    map_rule_basic(row)
}

/// Delete a relationship rule. Only allowed on draft models.
pub async fn delete_rule(db: &DatabaseConnection, rule_id: i32) -> AppResult<()> {
    let rule = get_rule_by_id(db, rule_id).await?;

    // Verify the model is still draft
    let model_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM org_structure_models WHERE id = ?",
            [rule.structure_model_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_structure_model".to_string(),
            id: rule.structure_model_id.to_string(),
        })?;
    let model_status: String = model_row.try_get("", "status").map_err(|e| decode_err("status", e))?;

    if model_status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "rules can only be deleted from draft structure models".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM org_type_relationship_rules WHERE id = ?",
        [rule_id.into()],
    ))
    .await?;

    tracing::info!(rule_id = rule_id, "org relationship rule deleted");
    Ok(())
}
