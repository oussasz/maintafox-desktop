//! WO task checklist — ordered mandatory/optional execution tasks.
//!
//! Phase 2 - Sub-phase 05 - File 02 - Sprint S1.
//!
//! `work_order_tasks` provides a per-WO checklist with sequence order, mandatory flag,
//! and result codes (ok / nok / na / deferred / cancelled).
//!
//! Business rules:
//!   - Tasks may be added when WO is in [draft, planning, ready] (planned) or in
//!     [in_progress, on_hold] (added during execution).
//!   - A task result is recorded only while the WO is `in_progress` or `on_hold` (AU-15), once:
//!     a recorded result is changed only by reopening the task first. Recording during a hold
//!     never resumes the WO; each record keeps the time it was entered.
//!   - A mandatory task closes only with OK or N/A, or with NOK when a finding note or a
//!     follow-up work order is recorded on it; cancelled and deferred are refused on it (CMMS
//!     Advisor decision D3). Optional tasks take every result.
//!   - A mandatory task not closed that way (including a legacy row closed cancelled, deferred or
//!     NOK without evidence, or one received from a PC that has not upgraded) is open
//!     ([`MANDATORY_TASK_OPEN_SQL`]): it blocks the WO's completion and close.
//!   - `reopen_task` ("Rouvrir la tâche") only while the WO is `in_progress` or `on_hold`, with a
//!     comment; the previous result stays in the WO's history (execution timeline and audit).

use crate::errors::{AppError, AppResult};
use crate::wo::execution_log::{emit_execution_event, part_label_json};
use crate::wo::time::now_utc_z;
use crate::wo::workflow::state_machine::status_i18n_key;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
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
    /// `planned` | `execution_added` | `generated` (reserved for PM)
    #[serde(default = "default_task_origin")]
    pub origin: String,
    /// What a NOK result found (migration 256).
    #[serde(default)]
    pub finding_note: Option<String>,
    /// The work order opened to deal with a NOK result (migration 256).
    #[serde(default)]
    pub follow_up_work_order_id: Option<i64>,
}

/// A task result and its evidence (`complete_task_with`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompleteTaskInput {
    pub result_code: String,
    pub notes: Option<String>,
    pub finding_note: Option<String>,
    pub follow_up_work_order_id: Option<i64>,
}

/// Stable codes of the task refusals (texts in `errors:gateError`).
pub mod task_codes {
    pub const RESULT_INVALID: &str = "GATE_TASK_RESULT_INVALID";
    pub const WRONG_STATUS: &str = "GATE_TASK_WRONG_STATUS";
    pub const ALREADY_COMPLETED: &str = "GATE_TASK_ALREADY_COMPLETED";
    pub const MANDATORY_RESULT_REFUSED: &str = "GATE_TASK_MANDATORY_RESULT_REFUSED";
    pub const NOK_NEEDS_EVIDENCE: &str = "GATE_TASK_NOK_NEEDS_EVIDENCE";
    pub const FOLLOW_UP_INVALID: &str = "GATE_TASK_FOLLOW_UP_INVALID";
    pub const NOT_COMPLETED: &str = "GATE_TASK_NOT_COMPLETED";
    pub const REOPEN_COMMENT_REQUIRED: &str = "GATE_TASK_REOPEN_COMMENT_REQUIRED";
}

/// WO statuses in which task results are recorded or reopened (AU-15, CMMS Advisor O3).
pub(crate) const TASK_RECORDING_STATUSES: &[&str] = &["in_progress", "on_hold"];

/// A mandatory task that is not closed with an accepted result (D3): not completed, or completed
/// with anything but OK / N/A, or NOK without a finding note or follow-up work order. On the
/// columns of `work_order_tasks`, without alias.
pub(crate) const MANDATORY_TASK_OPEN_SQL: &str = "is_mandatory = 1 AND NOT (is_completed = 1 AND \
     (result_code IN ('ok', 'na') OR (result_code = 'nok' AND (follow_up_work_order_id IS NOT NULL \
     OR length(trim(COALESCE(finding_note, ''))) > 0))))";

/// Descriptions of the open mandatory tasks of a WO ([`MANDATORY_TASK_OPEN_SQL`]), in order.
pub(crate) async fn open_mandatory_tasks(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<String>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT task_description FROM work_order_tasks \
                 WHERE work_order_id = ? AND {MANDATORY_TASK_OPEN_SQL} ORDER BY sequence_order ASC, id ASC"
            ),
            [wo_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            r.try_get::<String>("", "task_description")
                .map_err(|e| decode_err("task_description", e))
        })
        .collect()
}

fn refusal(code: &str, params: &str, text: String) -> AppError {
    let head = if params.is_empty() {
        code.to_string()
    } else {
        format!("{code}:{params}")
    };
    AppError::ValidationFailed(vec![format!("{head}: {text}")])
}

/// Refused unless the WO is in progress or on hold.
fn require_recording_status(status_code: &str) -> AppResult<()> {
    if TASK_RECORDING_STATUSES.contains(&status_code) {
        return Ok(());
    }
    Err(refusal(
        task_codes::WRONG_STATUS,
        &format!("status={status_code}"),
        format!(
            "Le résultat d'une tâche n'est saisi ou rouvert que pendant l'exécution (en cours ou en attente), \
             pas au statut '{status_code}'. (Task results are recorded only while the work order is in progress or on hold.)"
        ),
    ))
}

fn trimmed(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn default_task_origin() -> String {
    "planned".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddTaskInput {
    pub wo_id: i64,
    pub task_description: String,
    pub sequence_order: i64,
    pub is_mandatory: bool,
    pub estimated_minutes: Option<i64>,
    pub origin: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("WoTask row decode error for '{field}': {e}"))
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
        origin: row
            .try_get::<Option<String>>("", "origin")
            .ok()
            .flatten()
            .unwrap_or_else(|| "planned".into()),
        finding_note: row
            .try_get::<Option<String>>("", "finding_note")
            .map_err(|e| decode_err("finding_note", e))?,
        follow_up_work_order_id: row
            .try_get::<Option<i64>>("", "follow_up_work_order_id")
            .map_err(|e| decode_err("follow_up_work_order_id", e))?,
    })
}

/// Load the WO status code.
async fn load_wo_status_code(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<String> {
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
const VALID_RESULT_CODES: &[&str] = &["ok", "nok", "na", "deferred", "cancelled"];

const TASK_COLS: &str = "id, work_order_id, task_description, sequence_order, \
    estimated_minutes, is_mandatory, is_completed, completed_by_id, completed_at, \
    result_code, notes, origin, finding_note, follow_up_work_order_id";

// ═══════════════════════════════════════════════════════════════════════════════
// A) add_task
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn add_task(db: &DatabaseConnection, input: AddTaskInput) -> AppResult<WoTask> {
    let txn = db.begin().await?;
    let status_code = load_wo_status_code(&txn, input.wo_id).await?;

    let origin = if let Some(ref o) = input.origin {
        o.clone()
    } else if matches!(status_code.as_str(), "in_progress" | "on_hold") {
        "execution_added".to_string()
    } else {
        "planned".to_string()
    };

    // `generated` reserved for PM — reject client writes for now.
    if origin == "generated" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_TASK_ORIGIN_RESERVED: origin 'generated' est réservé à la génération PM.".into(),
        ]));
    }
    if !matches!(origin.as_str(), "planned" | "execution_added") {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_ORIGIN_INVALID: origin invalide (planned|execution_added).".into(),
        ]));
    }

    match (status_code.as_str(), origin.as_str()) {
        ("draft" | "planning" | "ready", "planned") => {}
        ("in_progress" | "on_hold", "execution_added") => {}
        _ => {
            let status_key = status_i18n_key(&status_code);
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_WO_TASK_ADD_STATUS:status={status_key}: Impossible d'ajouter une tâche ({origin}) au statut '{status_code}'."
            )]));
        }
    }

    if input.task_description.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_TASK_DESCRIPTION_REQUIRED: La description de la tâche est obligatoire.".to_string(),
        ]));
    }

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_tasks \
         (work_order_id, task_description, sequence_order, is_mandatory, estimated_minutes, origin, entity_sync_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            input.task_description.clone().into(),
            input.sequence_order.into(),
            i64::from(input.is_mandatory).into(),
            input
                .estimated_minutes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            origin.clone().into(),
            crate::sync::identity::new_entity_sync_id().into(),
        ],
    ))
    .await?;

    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {TASK_COLS} FROM work_order_tasks WHERE rowid = last_insert_rowid()"),
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read task after insert")))?;
    let task = map_task(&row)?;
    let _ = emit_execution_event(
        &txn,
        task.work_order_id,
        "task_added",
        "executionLog.taskAdded",
        part_label_json(&task.task_description),
        Some("task"),
        Some(task.id),
        None,
    )
    .await;
    txn.commit().await?;
    Ok(task)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) complete_task
// ═══════════════════════════════════════════════════════════════════════════════

/// The task fields a result is judged on.
struct TaskState {
    work_order_id: i64,
    task_description: String,
    is_mandatory: bool,
    is_completed: bool,
}

async fn load_task_state(conn: &impl ConnectionTrait, task_id: i64) -> AppResult<TaskState> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT work_order_id, task_description, is_mandatory, is_completed FROM work_order_tasks WHERE id = ?",
            [task_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        })?;
    Ok(TaskState {
        work_order_id: row
            .try_get("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        task_description: row
            .try_get("", "task_description")
            .map_err(|e| decode_err("task_description", e))?,
        is_mandatory: row
            .try_get::<i64>("", "is_mandatory")
            .map_err(|e| decode_err("is_mandatory", e))?
            != 0,
        is_completed: row
            .try_get::<i64>("", "is_completed")
            .map_err(|e| decode_err("is_completed", e))?
            != 0,
    })
}

/// The follow-up of a NOK result is another work order, held here and not cancelled.
async fn check_follow_up(conn: &impl ConnectionTrait, task_wo_id: i64, follow_up_id: i64) -> AppResult<()> {
    let invalid = |why: &str| {
        Err(refusal(
            task_codes::FOLLOW_UP_INVALID,
            &format!("work_order_id={follow_up_id}"),
            format!(
                "L'OT de suivi doit être un autre OT existant, non annulé ({why}). \
                 (The follow-up must be another existing, not cancelled work order.)"
            ),
        ))
    };
    if follow_up_id == task_wo_id {
        return invalid("même OT");
    }
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code FROM work_orders wo \
             JOIN work_order_statuses wos ON wos.id = wo.status_id WHERE wo.id = ?",
            [follow_up_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return invalid("introuvable");
    };
    let status: String = row
        .try_get("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;
    if status == "cancelled" {
        return invalid("annulé");
    }
    Ok(())
}

/// Record a task result without finding evidence (see [`complete_task_with`]).
pub async fn complete_task(
    db: &DatabaseConnection,
    task_id: i64,
    actor_id: i64,
    result_code: String,
    notes: Option<String>,
) -> AppResult<WoTask> {
    complete_task_with(
        db,
        task_id,
        actor_id,
        CompleteTaskInput {
            result_code,
            notes,
            finding_note: None,
            follow_up_work_order_id: None,
        },
    )
    .await
}

/// Record a task result, with what a NOK found (finding note, follow-up work order). See the
/// module documentation for the rules; every check and the write are in one transaction.
pub async fn complete_task_with(
    db: &DatabaseConnection,
    task_id: i64,
    actor_id: i64,
    input: CompleteTaskInput,
) -> AppResult<WoTask> {
    let result_code = input.result_code.trim().to_string();
    if !VALID_RESULT_CODES.contains(&result_code.as_str()) {
        return Err(refusal(
            task_codes::RESULT_INVALID,
            "",
            format!("result_code invalide : '{result_code}'. Valeurs autorisées : ok, nok, na, deferred, cancelled."),
        ));
    }
    let finding_note = trimmed(input.finding_note);
    let follow_up = input.follow_up_work_order_id;

    let now = now_utc_z();
    let txn = db.begin().await?;

    let state = load_task_state(&txn, task_id).await?;
    let status_code = load_wo_status_code(&txn, state.work_order_id).await?;
    require_recording_status(&status_code)?;
    if state.is_completed {
        return Err(refusal(
            task_codes::ALREADY_COMPLETED,
            "",
            "Cette tâche a déjà un résultat : rouvrez-la d'abord pour en saisir un autre. \
             (The task already has a result: reopen it first.)"
                .into(),
        ));
    }
    if let Some(id) = follow_up {
        check_follow_up(&txn, state.work_order_id, id).await?;
    }
    if state.is_mandatory {
        if matches!(result_code.as_str(), "cancelled" | "deferred") {
            return Err(refusal(
                task_codes::MANDATORY_RESULT_REFUSED,
                &format!("result={result_code}"),
                "Une tâche obligatoire se clôt avec OK, N/A, ou NOK accompagné d'un constat ou d'un OT de suivi ; \
                 annulée et reportée sont refusées. (A mandatory task cannot be cancelled or deferred.)"
                    .into(),
            ));
        }
        if result_code == "nok" && finding_note.is_none() && follow_up.is_none() {
            return Err(refusal(
                task_codes::NOK_NEEDS_EVIDENCE,
                "",
                "Une tâche obligatoire NOK exige le constat ou l'OT de suivi. \
                 (A mandatory NOK task needs a finding note or a follow-up work order.)"
                    .into(),
            ));
        }
    }

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_tasks SET \
            is_completed = 1, \
            completed_by_id = ?, \
            completed_at = ?, \
            result_code = ?, \
            notes = COALESCE(?, notes), \
            finding_note = ?, \
            follow_up_work_order_id = ? \
         WHERE id = ? AND is_completed = 0",
        [
            actor_id.into(),
            now.into(),
            result_code.clone().into(),
            input
                .notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            finding_note.into(),
            follow_up.into(),
            task_id.into(),
        ],
    ))
    .await?;

    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {TASK_COLS} FROM work_order_tasks WHERE id = ?"),
            [task_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        })?;
    let task = map_task(&row)?;
    let cancelled = result_code == "cancelled";
    let _ = emit_execution_event(
        &txn,
        task.work_order_id,
        if cancelled { "task_cancelled" } else { "task_completed" },
        if cancelled {
            "executionLog.taskCancelled"
        } else {
            "executionLog.taskCompleted"
        },
        part_label_json(&state.task_description),
        Some("task"),
        Some(task.id),
        Some(actor_id),
    )
    .await;
    txn.commit().await?;
    Ok(task)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) reopen_task — "Rouvrir la tâche"
// ═══════════════════════════════════════════════════════════════════════════════

/// Reopen a task that has a result, so another one can be recorded (CMMS Advisor O2): only while
/// the WO is in progress or on hold, with a comment. The previous result (code, notes, finding,
/// follow-up, who and when) is written to the WO's execution timeline and its audit trail in the
/// same transaction, so it stays visible after the task is recorded again.
pub async fn reopen_task(db: &DatabaseConnection, task_id: i64, actor_id: i64, comment: String) -> AppResult<WoTask> {
    let comment = comment.trim().to_string();
    if comment.is_empty() {
        return Err(refusal(
            task_codes::REOPEN_COMMENT_REQUIRED,
            "",
            "Indiquez pourquoi la tâche est rouverte. (A comment is required to reopen a task.)".into(),
        ));
    }
    let txn = db.begin().await?;
    let state = load_task_state(&txn, task_id).await?;
    let status_code = load_wo_status_code(&txn, state.work_order_id).await?;
    require_recording_status(&status_code)?;
    if !state.is_completed {
        return Err(refusal(
            task_codes::NOT_COMPLETED,
            "",
            "Cette tâche n'a pas de résultat à rouvrir. (The task has no result to reopen.)".into(),
        ));
    }

    // The previous result, as the history keeps it (names and codes, never local ids).
    let previous = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.result_code, t.notes, t.finding_note, t.completed_at, \
                    COALESCE(u.display_name, u.username) AS completed_by, f.code AS follow_up_code \
             FROM work_order_tasks t \
             LEFT JOIN user_accounts u ON u.id = t.completed_by_id \
             LEFT JOIN work_orders f ON f.id = t.follow_up_work_order_id \
             WHERE t.id = ?",
            [task_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        })?;
    let text = |c: &str| -> AppResult<Option<String>> {
        previous.try_get::<Option<String>>("", c).map_err(|e| decode_err(c, e))
    };
    let details = serde_json::json!({
        "label": state.task_description,
        "previous_result": text("result_code")?,
        "previous_notes": text("notes")?,
        "previous_finding_note": text("finding_note")?,
        "previous_follow_up": text("follow_up_code")?,
        "previous_completed_by": text("completed_by")?,
        "previous_completed_at": text("completed_at")?,
        "comment": comment,
    });

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_tasks SET \
            is_completed = 0, \
            completed_by_id = NULL, \
            completed_at = NULL, \
            result_code = NULL, \
            finding_note = NULL, \
            follow_up_work_order_id = NULL \
         WHERE id = ?",
        [task_id.into()],
    ))
    .await?;

    emit_execution_event(
        &txn,
        state.work_order_id,
        "task_reopened",
        "executionLog.taskReopened",
        details.clone(),
        Some("task"),
        Some(task_id),
        Some(actor_id),
    )
    .await?;
    crate::wo::audit::insert_wo_change_event(
        &txn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(state.work_order_id),
            action: "task_reopened".into(),
            actor_id: Some(actor_id),
            summary: Some(format!("Tâche rouverte : {}", state.task_description)),
            details_json: Some(details.to_string()),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;

    let updated = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {TASK_COLS} FROM work_order_tasks WHERE id = ?"),
            [task_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoTask".into(),
            id: task_id.to_string(),
        })?;
    let task = map_task(&updated)?;
    txn.commit().await?;
    Ok(task)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) list_tasks
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_tasks(db: &DatabaseConnection, wo_id: i64) -> AppResult<Vec<WoTask>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {TASK_COLS} FROM work_order_tasks \
                 WHERE work_order_id = ? ORDER BY sequence_order ASC, id ASC"
            ),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_task).collect()
}
