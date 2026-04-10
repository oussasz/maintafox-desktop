//! WO task checklist — ordered mandatory/optional execution tasks.
//!
//! Phase 2 - Sub-phase 05 - File 02 - Sprint S1.
//!
//! `work_order_tasks` provides a per-WO checklist with sequence order, mandatory flag,
//! and result codes (ok / nok / na / deferred). Mandatory incomplete tasks block
//! `complete_wo_mechanically`.
//!
//! Business rules:
//!   - Tasks may be added when WO is in [draft, planned, ready_to_schedule, assigned].
//!   - `complete_task` sets `is_completed = 1` with a result code.
//!   - `reopen_task` is not allowed after `mechanically_complete`.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoTask {
    pub id: i64,
    pub work_order_id: i64,
    pub task_description: String,
    pub sequence_order: i64,
    pub estimated_minutes: Option<i64>,
    pub is_mandatory: bool,
    pub is_completed: bool,
    pub completed_by_id: Option<i64>,
    pub completed_at: Option<String>,
    pub result_code: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddTaskInput {
    pub wo_id: i64,
    pub task_description: String,
    pub sequence_order: i64,
    pub is_mandatory: bool,
    pub estimated_minutes: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WoTask row decode error for '{field}': {e}"
    ))
}

fn map_task(row: &sea_orm::QueryResult) -> AppResult<WoTask> {
    Ok(WoTask {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        task_description: row
            .try_get::<String>("", "task_description")
            .map_err(|e| decode_err("task_description", e))?,
        sequence_order: row
            .try_get::<i64>("", "sequence_order")
            .map_err(|e| decode_err("sequence_order", e))?,
        estimated_minutes: row
            .try_get::<Option<i64>>("", "estimated_minutes")
            .map_err(|e| decode_err("estimated_minutes", e))?,
        is_mandatory: row
            .try_get::<i64>("", "is_mandatory")
            .map_err(|e| decode_err("is_mandatory", e))?
            != 0,
        is_completed: row
            .try_get::<i64>("", "is_completed")
            .map_err(|e| decode_err("is_completed", e))?
            != 0,
        completed_by_id: row
            .try_get::<Option<i64>>("", "completed_by_id")
            .map_err(|e| decode_err("completed_by_id", e))?,
        completed_at: row
            .try_get::<Option<String>>("", "completed_at")
            .map_err(|e| decode_err("completed_at", e))?,
        result_code: row
            .try_get::<Option<String>>("", "result_code")
            .map_err(|e| decode_err("result_code", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
    })
}

/// Load the WO status code.
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

/// Valid result codes for task completion.
const VALID_RESULT_CODES: &[&str] = &["ok", "nok", "na", "deferred"];

const TASK_COLS: &str = "id, work_order_id, task_description, sequence_order, \
    estimated_minutes, is_mandatory, is_completed, completed_by_id, completed_at, \
    result_code, notes";

// ═══════════════════════════════════════════════════════════════════════════════
// A) add_task
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn add_task(db: &DatabaseConnection, input: AddTaskInput) -> AppResult<WoTask> {
    let status_code = load_wo_status_code(db, input.wo_id).await?;
    if !matches!(
        status_code.as_str(),
        "draft" | "planned" | "ready_to_schedule" | "assigned"
    ) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Les tâches ne peuvent être ajoutées qu'aux statuts draft/planned/ready_to_schedule/assigned. \
             Statut actuel : '{status_code}'."
        )]));
    }

    if input.task_description.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "La description de la tâche est obligatoire.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_tasks \
         (work_order_id, task_description, sequence_order, is_mandatory, estimated_minutes) \
         VALUES (?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            input.task_description.into(),
            input.sequence_order.into(),
            i64::from(input.is_mandatory).into(),
            input
                .estimated_minutes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {TASK_COLS} FROM work_order_tasks WHERE rowid = last_insert_rowid()"),
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Failed to re-read task after insert"))
        })?;
    map_task(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) complete_task
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn complete_task(
    db: &DatabaseConnection,
    task_id: i64,
    actor_id: i64,
    result_code: String,
    notes: Option<String>,
) -> AppResult<WoTask> {
    if !VALID_RESULT_CODES.contains(&result_code.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "result_code invalide : '{}'. Valeurs autorisées : ok, nok, na, deferred.",
            result_code
        )]));
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_tasks SET \
                is_completed = 1, \
                completed_by_id = ?, \
                completed_at = ?, \
                result_code = ?, \
                notes = COALESCE(?, notes) \
             WHERE id = ?",
            [
                actor_id.into(),
                now.into(),
                result_code.into(),
                notes
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                task_id.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        });
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {TASK_COLS} FROM work_order_tasks WHERE id = ?"),
            [task_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        })?;
    map_task(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) reopen_task
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn reopen_task(
    db: &DatabaseConnection,
    task_id: i64,
    _actor_id: i64,
) -> AppResult<WoTask> {
    // Load the task to get wo_id
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT work_order_id FROM work_order_tasks WHERE id = ?",
            [task_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        })?;
    let wo_id: i64 = row
        .try_get::<i64>("", "work_order_id")
        .map_err(|e| decode_err("work_order_id", e))?;

    let status_code = load_wo_status_code(db, wo_id).await?;
    // Blocked once mechanically complete or later
    if matches!(
        status_code.as_str(),
        "mechanically_complete" | "technically_verified" | "closed" | "cancelled"
    ) {
        return Err(AppError::ValidationFailed(vec![format!(
            "La réouverture d'une tâche n'est pas autorisée au statut '{status_code}'."
        )]));
    }

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_tasks SET \
                is_completed = 0, \
                completed_by_id = NULL, \
                completed_at = NULL, \
                result_code = NULL \
             WHERE id = ?",
            [task_id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        });
    }

    let updated = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {TASK_COLS} FROM work_order_tasks WHERE id = ?"),
            [task_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        })?;
    map_task(&updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) list_tasks
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_tasks(db: &DatabaseConnection, wo_id: i64) -> AppResult<Vec<WoTask>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {TASK_COLS} FROM work_order_tasks \
                 WHERE work_order_id = ? ORDER BY sequence_order ASC, id ASC"
            ),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_task).collect()
}
