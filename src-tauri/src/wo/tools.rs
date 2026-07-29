//! WO tools — planned vs actually used (Phase 2).

use crate::errors::{AppError, AppResult};
use crate::wo::execution_log::{emit_execution_event, part_label_json};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoTool {
    pub id: i64,
    pub work_order_id: i64,
    pub origin: String,
    pub tool_code: Option<String>,
    pub tool_label: String,
    pub usage_status: String,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddToolInput {
    pub wo_id: i64,
    pub tool_label: String,
    pub tool_code: Option<String>,
    pub origin: Option<String>,
    pub notes: Option<String>,
}

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("WoTool decode error for '{field}': {e}"))
}

fn map_tool(row: &sea_orm::QueryResult) -> AppResult<WoTool> {
    Ok(WoTool {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        origin: row
            .try_get("", "origin")
            .map_err(|e| decode_err("origin", e))?,
        tool_code: row
            .try_get("", "tool_code")
            .map_err(|e| decode_err("tool_code", e))?,
        tool_label: row
            .try_get("", "tool_label")
            .map_err(|e| decode_err("tool_label", e))?,
        usage_status: row
            .try_get("", "usage_status")
            .map_err(|e| decode_err("usage_status", e))?,
        notes: row
            .try_get("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

async fn load_wo_status_code(db: &DatabaseConnection, wo_id: i64) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code \
             FROM work_orders wo \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    row.try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))
}

pub async fn list_tools(db: &DatabaseConnection, wo_id: i64) -> AppResult<Vec<WoTool>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, origin, tool_code, tool_label, usage_status, notes, created_at \
               FROM work_order_tools WHERE work_order_id = ? ORDER BY id ASC",
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_tool).collect()
}

pub async fn add_tool(db: &DatabaseConnection, input: AddToolInput) -> AppResult<WoTool> {
    let status = load_wo_status_code(db, input.wo_id).await?;
    if matches!(status.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![
            "Impossible d'ajouter un outil sur un OT clôturé/annulé.".into(),
        ]));
    }
    let label = input.tool_label.trim();
    if label.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le libellé de l'outil est obligatoire.".into(),
        ]));
    }
    let origin = if matches!(status.as_str(), "in_progress" | "on_hold") {
        input
            .origin
            .unwrap_or_else(|| "execution_added".into())
    } else {
        input.origin.unwrap_or_else(|| "planned".into())
    };
    if !matches!(origin.as_str(), "planned" | "execution_added") {
        return Err(AppError::ValidationFailed(vec![
            "origin invalide (planned|execution_added).".into(),
        ]));
    }
    let usage = if origin == "execution_added" {
        "used"
    } else {
        "planned"
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_tools \
         (work_order_id, origin, tool_code, tool_label, usage_status, notes) \
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            origin.into(),
            input
                .tool_code
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            label.to_string().into(),
            usage.into(),
            input
                .notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, origin, tool_code, tool_label, usage_status, notes, created_at \
               FROM work_order_tools WHERE rowid = last_insert_rowid()",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read tool")))?;
    let tool = map_tool(&row)?;
    let _ = emit_execution_event(
        db,
        tool.work_order_id,
        "tool_added",
        "executionLog.toolAdded",
        part_label_json(&tool.tool_label),
        Some("tool"),
        Some(tool.id),
        None,
    )
    .await;
    Ok(tool)
}

pub async fn mark_tool_used(db: &DatabaseConnection, tool_id: i64) -> AppResult<WoTool> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_tools SET usage_status = 'used' WHERE id = ?",
        [tool_id.into()],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, origin, tool_code, tool_label, usage_status, notes, created_at \
               FROM work_order_tools WHERE id = ?",
            [tool_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTool".into(),
            id: tool_id.to_string(),
        })?;
    let tool = map_tool(&row)?;
    let _ = emit_execution_event(
        db,
        tool.work_order_id,
        "tool_used",
        "executionLog.toolUsed",
        part_label_json(&tool.tool_label),
        Some("tool"),
        Some(tool.id),
        None,
    )
    .await;
    Ok(tool)
}

pub async fn mark_tool_not_used(db: &DatabaseConnection, tool_id: i64) -> AppResult<WoTool> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_tools SET usage_status = 'not_used' WHERE id = ?",
        [tool_id.into()],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, origin, tool_code, tool_label, usage_status, notes, created_at \
               FROM work_order_tools WHERE id = ?",
            [tool_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTool".into(),
            id: tool_id.to_string(),
        })?;
    let tool = map_tool(&row)?;
    let _ = emit_execution_event(
        db,
        tool.work_order_id,
        "tool_not_used",
        "executionLog.toolNotUsed",
        part_label_json(&tool.tool_label),
        Some("tool"),
        Some(tool.id),
        None,
    )
    .await;
    Ok(tool)
}
