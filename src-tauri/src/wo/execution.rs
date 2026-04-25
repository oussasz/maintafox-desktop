//! WO execution transition functions.
//!
//! Phase 2 - Sub-phase 05 - File 02 - Sprint S1.
//!
//! All state transitions are enforced through `guard_wo_transition` before any write.
//! All mutation functions run inside a sea-orm transaction for atomicity.
//!
//! Functions:
//!   plan_wo                     — draft/awaiting_approval → planned
//!   assign_wo                   — ready_to_schedule → assigned
//!   start_wo                    — assigned/waiting_for_prerequisite → in_progress
//!   pause_wo                    — in_progress → paused (opens delay segment)
//!   resume_wo                   — paused → in_progress (closes delay segment)
//!   set_waiting_for_prerequisite— assigned → waiting_for_prerequisite (opens delay segment)
//!   complete_wo_mechanically    — in_progress → mechanically_complete (quality-gated)

use crate::errors::{AppError, AppResult};
use crate::wo::queries;
use chrono::Utc;
use uuid::Uuid;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use serde::Deserialize;

use super::domain::{guard_wo_transition, WoStatus, WorkOrder};

// ═══════════════════════════════════════════════════════════════════════════════
// Input structs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
pub struct WoPlanInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub planner_id: i64,
    pub planned_start: String,
    pub planned_end: String,
    pub shift: Option<String>,
    pub expected_duration_hours: Option<f64>,
    pub urgency_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoAssignInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub assigned_group_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub scheduled_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoStartInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoPauseInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub delay_reason_id: i64,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoResumeInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoHoldInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub delay_reason_id: i64,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoMechCompleteInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub actual_end: Option<String>,
    pub actual_duration_hours: Option<f64>,
    pub conclusion: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WO execution row decode error for '{field}': {e}"
    ))
}

/// Verify `rows_affected == 1`. Returns a concurrency conflict error on mismatch.
fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .to_string(),
        ]));
    }
    Ok(())
}

/// Load current WO status from the DB and parse it.
/// Returns `(current_status_code: String, parsed: WoStatus, current_row_version: i64)`.
async fn load_wo_status(
    txn: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<(String, WoStatus, i64)> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code, wo.row_version \
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

    let code: String = row
        .try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;
    let row_version: i64 = row
        .try_get::<i64>("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    let status = WoStatus::try_from_str(&code)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored WO has invalid status: {e}")))?;

    Ok((code, status, row_version))
}

/// Resolve `status_id` for a given status code from `work_order_statuses`.
async fn resolve_status_id(txn: &impl ConnectionTrait, code: &str) -> AppResult<i64> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = ?",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "work_order_statuses missing row for code '{code}'"
            ))
        })?;
    row.try_get::<i64>("", "id")
        .map_err(|e| decode_err("status id", e))
}

/// Write an entry to the append-only state transition log.
async fn log_transition(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    from_status: &str,
    to_status: &str,
    action: &str,
    actor_id: i64,
    reason_code: Option<&str>,
    notes: Option<&str>,
    acted_at: &str,
) -> AppResult<()> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log \
         (wo_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            wo_id.into(),
            from_status.into(),
            to_status.into(),
            action.into(),
            actor_id.into(),
            reason_code
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            acted_at.into(),
        ],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) plan_wo — draft / awaiting_approval → planned
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn plan_wo(db: &DatabaseConnection, input: WoPlanInput) -> AppResult<WorkOrder> {
    // Validate timestamps before opening transaction
    let start = chrono::DateTime::parse_from_rfc3339(&input.planned_start)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(&input.planned_start, "%Y-%m-%dT%H:%M:%SZ")
                .map(|dt| dt.and_utc().fixed_offset())
        })
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "planned_start n'est pas une date ISO valide : '{}'",
                input.planned_start
            )])
        })?;

    let end = chrono::DateTime::parse_from_rfc3339(&input.planned_end)
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(&input.planned_end, "%Y-%m-%dT%H:%M:%SZ")
                .map(|dt| dt.and_utc().fixed_offset())
        })
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "planned_end n'est pas une date ISO valide : '{}'",
                input.planned_end
            )])
        })?;

    if end < start {
        return Err(AppError::ValidationFailed(vec![
            "planned_end doit être >= planned_start.".to_string(),
        ]));
    }

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;

    // Guard: draft OR awaiting_approval → planned
    guard_wo_transition(&current_status, &WoStatus::Planned)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let planned_status_id = resolve_status_id(&txn, "planned").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                planner_id = ?, \
                planned_start = ?, \
                planned_end = ?, \
                shift = COALESCE(?, shift), \
                urgency_id = COALESCE(?, urgency_id), \
                expected_duration_hours = COALESCE(?, expected_duration_hours), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                planned_status_id.into(),
                input.planner_id.into(),
                input.planned_start.clone().into(),
                input.planned_end.clone().into(),
                input
                    .shift
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                input
                    .urgency_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .expected_duration_hours
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "planned",
        "plan",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    txn.commit().await?;

    queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) assign_wo — ready_to_schedule → assigned
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn assign_wo(db: &DatabaseConnection, input: WoAssignInput) -> AppResult<WorkOrder> {
    if input.assigned_group_id.is_none() && input.primary_responsible_id.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Au moins un champ parmi assigned_group_id ou primary_responsible_id est obligatoire."
                .to_string(),
        ]));
    }

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;

    guard_wo_transition(&current_status, &WoStatus::Assigned)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let assigned_status_id = resolve_status_id(&txn, "assigned").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let assignee_notes = input
        .primary_responsible_id
        .map(|id| format!("primary_responsible_id={id}"));

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                assigned_group_id = COALESCE(?, assigned_group_id), \
                primary_responsible_id = COALESCE(?, primary_responsible_id), \
                scheduled_at = COALESCE(?, scheduled_at), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                assigned_status_id.into(),
                input
                    .assigned_group_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .primary_responsible_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .scheduled_at
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "assigned",
        "assign",
        input.actor_id,
        None,
        assignee_notes.as_deref(),
        &now,
    )
    .await?;

    txn.commit().await?;

    queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) start_wo — assigned / waiting_for_prerequisite → in_progress
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn start_wo(db: &DatabaseConnection, input: WoStartInput) -> AppResult<WorkOrder> {
    crate::permit::wo_gate::assert_in_progress_permit_gate(db, input.wo_id).await?;

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;

    guard_wo_transition(&current_status, &WoStatus::InProgress)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let in_progress_status_id = resolve_status_id(&txn, "in_progress").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Close any open delay segment (may exist when coming from waiting_for_prerequisite)
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_delay_segments SET ended_at = ? \
         WHERE work_order_id = ? AND ended_at IS NULL",
        [now.clone().into(), input.wo_id.into()],
    ))
    .await?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                actual_start = COALESCE(actual_start, ?), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                in_progress_status_id.into(),
                now.clone().into(),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "in_progress",
        "start",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    txn.commit().await?;

    let wo = queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    if wo.requires_permit {
        let permit = crate::permit::queries::get_work_permit_linked_to_work_order(db, input.wo_id)
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "PTW gate passed but no linked permit for WO {}",
                    input.wo_id
                ))
            })?;
        crate::permit::wo_gate::stage_wo_in_progress_sync_pair(
            db,
            &wo,
            &permit,
            &Uuid::new_v4().to_string(),
        )
        .await?;
    }

    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) pause_wo — in_progress → paused (opens delay segment)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn pause_wo(db: &DatabaseConnection, input: WoPauseInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;

    guard_wo_transition(&current_status, &WoStatus::Paused)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // Validate delay_reason_id resolves
    let reason_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM delay_reason_codes WHERE id = ?",
            [input.delay_reason_id.into()],
        ))
        .await?;
    let reason_code = match &reason_row {
        Some(r) => r
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("delay_reason code", e))?,
        None => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Code de délai introuvable (delay_reason_id={}).",
                input.delay_reason_id
            )]))
        }
    };

    let paused_status_id = resolve_status_id(&txn, "paused").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Open delay segment
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_delay_segments \
         (work_order_id, started_at, ended_at, delay_reason_id, comment, entered_by_id) \
         VALUES (?, ?, NULL, ?, ?, ?)",
        [
            input.wo_id.into(),
            now.clone().into(),
            input.delay_reason_id.into(),
            input
                .comment
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input.actor_id.into(),
        ],
    ))
    .await?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                paused_status_id.into(),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "paused",
        "pause",
        input.actor_id,
        Some(&reason_code),
        input.comment.as_deref(),
        &now,
    )
    .await?;

    txn.commit().await?;

    queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) resume_wo — paused → in_progress (closes delay segment, recomputes waiting hours)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn resume_wo(db: &DatabaseConnection, input: WoResumeInput) -> AppResult<WorkOrder> {
    crate::permit::wo_gate::assert_in_progress_permit_gate(db, input.wo_id).await?;

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;

    guard_wo_transition(&current_status, &WoStatus::InProgress)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let in_progress_status_id = resolve_status_id(&txn, "in_progress").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Close all open delay segments
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_delay_segments SET ended_at = ? \
         WHERE work_order_id = ? AND ended_at IS NULL",
        [now.clone().into(), input.wo_id.into()],
    ))
    .await?;

    // Recompute total_waiting_hours
    let waiting_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(ROUND((JULIANDAY(COALESCE(ended_at, strftime('%Y-%m-%dT%H:%M:%SZ','now'))) \
              - JULIANDAY(started_at)) * 24, 2)), 0) AS total_waiting \
             FROM work_order_delay_segments \
             WHERE work_order_id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Waiting hours query returned no row"))
        })?;
    let total_waiting: f64 = waiting_row
        .try_get::<f64>("", "total_waiting")
        .map_err(|e| decode_err("total_waiting", e))?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                total_waiting_hours = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                in_progress_status_id.into(),
                total_waiting.into(),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "in_progress",
        "resume",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    txn.commit().await?;

    let wo = queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    if wo.requires_permit {
        let permit = crate::permit::queries::get_work_permit_linked_to_work_order(db, input.wo_id)
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "PTW gate passed but no linked permit for WO {}",
                    input.wo_id
                ))
            })?;
        crate::permit::wo_gate::stage_wo_in_progress_sync_pair(
            db,
            &wo,
            &permit,
            &Uuid::new_v4().to_string(),
        )
        .await?;
    }

    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) set_waiting_for_prerequisite — assigned → waiting_for_prerequisite
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn set_waiting_for_prerequisite(
    db: &DatabaseConnection,
    input: WoHoldInput,
) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;

    guard_wo_transition(&current_status, &WoStatus::WaitingForPrerequisite)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // Validate delay_reason_id resolves
    let reason_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM delay_reason_codes WHERE id = ?",
            [input.delay_reason_id.into()],
        ))
        .await?;
    let reason_code = match &reason_row {
        Some(r) => r
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("delay_reason code", e))?,
        None => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Code de délai introuvable (delay_reason_id={}).",
                input.delay_reason_id
            )]))
        }
    };

    let hold_status_id = resolve_status_id(&txn, "waiting_for_prerequisite").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Open delay segment (same pattern as pause)
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_delay_segments \
         (work_order_id, started_at, ended_at, delay_reason_id, comment, entered_by_id) \
         VALUES (?, ?, NULL, ?, ?, ?)",
        [
            input.wo_id.into(),
            now.clone().into(),
            input.delay_reason_id.into(),
            input
                .comment
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input.actor_id.into(),
        ],
    ))
    .await?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                hold_status_id.into(),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "waiting_for_prerequisite",
        "hold",
        input.actor_id,
        Some(&reason_code),
        input.comment.as_deref(),
        &now,
    )
    .await?;

    txn.commit().await?;

    queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) complete_wo_mechanically — in_progress → mechanically_complete
// ═══════════════════════════════════════════════════════════════════════════════

/// Pre-flight result accumulating all blocking conditions.
#[derive(Debug, Default)]
struct PreflightResult {
    errors: Vec<String>,
}

impl PreflightResult {
    fn add(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }
    fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub async fn complete_wo_mechanically(
    db: &DatabaseConnection,
    input: WoMechCompleteInput,
) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;

    guard_wo_transition(&current_status, &WoStatus::MechanicallyComplete)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let mut preflight = PreflightResult::default();

    // ── Check 1: No open labor entries ───────────────────────────────────
    let open_labor_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt \
             FROM work_order_interveners \
             WHERE work_order_id = ? \
               AND ended_at IS NULL \
               AND (hours_worked IS NULL OR hours_worked <= 0)",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Labor open-entry check returned no row"))
        })?;
    let open_labor: i64 = open_labor_row
        .try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("open_labor cnt", e))?;
    if open_labor > 0 {
        preflight.add("Les entrées de main-d'œuvre ouvertes doivent être clôturées avant la complétion. (Open labor entries must be closed first.)");
    }

    // ── Check 2: All mandatory tasks completed ────────────────────────────
    let incomplete_tasks = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT task_description \
             FROM work_order_tasks \
             WHERE work_order_id = ? AND is_mandatory = 1 AND is_completed = 0 \
             ORDER BY sequence_order ASC",
            [input.wo_id.into()],
        ))
        .await?;
    if !incomplete_tasks.is_empty() {
        let descs: Vec<String> = incomplete_tasks
            .iter()
            .filter_map(|r| r.try_get::<String>("", "task_description").ok())
            .collect();
        preflight.add(format!(
            "Tâches obligatoires non terminées : {}. (Mandatory tasks incomplete: {}.)",
            descs.join(", "),
            descs.join(", ")
        ));
    }

    // ── Check 3: Parts actuals confirmed ─────────────────────────────────
    let parts_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                (SELECT COUNT(*) FROM work_order_parts WHERE work_order_id = ? AND quantity_used > 0) AS used_count, \
                (SELECT parts_actuals_confirmed FROM work_orders WHERE id = ?) AS confirmed",
            [input.wo_id.into(), input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Parts gate check returned no row"))
        })?;
    let used_count: i64 = parts_row
        .try_get::<i64>("", "used_count")
        .map_err(|e| decode_err("used_count", e))?;
    let parts_confirmed: i64 = parts_row
        .try_get::<i64>("", "confirmed")
        .map_err(|e| decode_err("parts_actuals_confirmed", e))?;
    if used_count == 0 && parts_confirmed == 0 {
        preflight.add("Réels des pièces non confirmés. Entrez les pièces consommées ou marquez 'aucune pièce utilisée'. (Parts actuals not confirmed. Enter consumed parts or mark none used.)");
    }

    // ── Check 4: No open downtime segments ───────────────────────────────
    let open_dt_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt \
             FROM work_order_downtime_segments \
             WHERE work_order_id = ? AND ended_at IS NULL",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Downtime gate check returned no row"))
        })?;
    let open_dt: i64 = open_dt_row
        .try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("open_dt cnt", e))?;
    if open_dt > 0 {
        preflight.add("Les segments de temps d'arrêt ouverts doivent être clôturés avant la complétion. (Open downtime segments must be closed before completion.)");
    }

    // ── Return all preflight errors at once ───────────────────────────────
    if !preflight.is_ok() {
        txn.rollback().await?;
        return Err(AppError::ValidationFailed(preflight.errors));
    }

    // ── Post-check: recompute active_labor_hours ──────────────────────────
    let labor_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(COALESCE(hours_worked, 0.0)), 0.0) AS total_labor \
             FROM work_order_interveners \
             WHERE work_order_id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Labor sum query returned no row"))
        })?;
    let total_labor: f64 = labor_row
        .try_get::<f64>("", "total_labor")
        .map_err(|e| decode_err("total_labor", e))?;

    let mech_complete_status_id = resolve_status_id(&txn, "mechanically_complete").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                mechanically_completed_at = ?, \
                active_labor_hours = ?, \
                actual_end = COALESCE(?, actual_end), \
                actual_duration_hours = COALESCE(?, actual_duration_hours), \
                conclusion = COALESCE(?, conclusion), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                mech_complete_status_id.into(),
                now.clone().into(),
                total_labor.into(),
                input.actual_end.clone().map(|v| v.into()).unwrap_or(sea_orm::Value::String(None)),
                input.actual_duration_hours.map(|v| v.into()).unwrap_or(sea_orm::Value::Double(None)),
                input.conclusion.clone().map(|v| v.into()).unwrap_or(sea_orm::Value::String(None)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "mechanically_complete",
        "complete_mechanically",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    txn.commit().await?;

    queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })
}
