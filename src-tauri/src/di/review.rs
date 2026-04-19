//! DI review and triage workflow functions.
//!
//! Phase 2 – Sub-phase 04 – File 02 – Sprint S1.
//!
//! Each function enforces:
//!   1. State machine guard via `guard_transition`
//!   2. Business validation (required fields, FK resolution, date checks)
//!   3. Atomic DI update + `di_review_events` write inside a single transaction
//!   4. Optimistic concurrency via `row_version`
//!
//! Architecture rules:
//!   - Transition commands are separate from CRUD commands
//!   - `di_review_events` is append-only — no update/delete
//!   - `screened_at` and `approved_at` are written once, never overwritten
//!   - `screen_di` auto-advances PendingReview → Screened → AwaitingApproval atomically

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::domain::{
    guard_transition, map_intervention_request, DiStatus, DiUrgency, InterventionRequest,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Input structs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
pub struct DiScreenInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub validated_urgency: String,
    pub review_team_id: Option<i64>,
    pub classification_code_id: Option<i64>,
    pub reviewer_note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiReturnInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub reviewer_note: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiRejectInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub reason_code: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiApproveInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiDeferInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub deferred_until: String,
    pub reason_code: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiReactivateInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub notes: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// DiReviewEvent — row from di_review_events
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiReviewEvent {
    pub id: i64,
    pub di_id: i64,
    pub event_type: String,
    pub actor_id: Option<i64>,
    pub acted_at: String,
    pub from_status: String,
    pub to_status: String,
    pub reason_code: Option<String>,
    pub notes: Option<String>,
    pub sla_target_hours: Option<i64>,
    pub sla_deadline: Option<String>,
    pub step_up_used: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// All columns from `intervention_requests` for SELECT reuse (aliased as `ir`).
const IR_COLS: &str = "\
    ir.id, ir.code, ir.asset_id, ir.sub_asset_ref, ir.org_node_id, \
    ir.status, ir.title, ir.description, ir.origin_type, ir.symptom_code_id, \
    ir.impact_level, ir.production_impact, ir.safety_flag, ir.environmental_flag, \
    ir.quality_flag, ir.reported_urgency, ir.validated_urgency, \
    ir.observed_at, ir.submitted_at, \
    ir.review_team_id, ir.reviewer_id, ir.screened_at, ir.approved_at, \
    ir.deferred_until, ir.declined_at, ir.closed_at, ir.archived_at, \
    ir.converted_to_wo_id, ir.converted_at, \
    ir.reviewer_note, ir.classification_code_id, \
    ir.is_recurrence_flag, ir.recurrence_di_id, \
    ir.source_inspection_anomaly_id, \
    ir.row_version, ir.submitter_id, ir.created_at, ir.updated_at";

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "di_review_events row decode failed for column '{column}': {e}"
    ))
}

fn map_review_event(row: &QueryResult) -> AppResult<DiReviewEvent> {
    Ok(DiReviewEvent {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        di_id: row
            .try_get::<i64>("", "di_id")
            .map_err(|e| decode_err("di_id", e))?,
        event_type: row
            .try_get::<String>("", "event_type")
            .map_err(|e| decode_err("event_type", e))?,
        actor_id: row
            .try_get::<Option<i64>>("", "actor_id")
            .map_err(|e| decode_err("actor_id", e))?,
        acted_at: row
            .try_get::<String>("", "acted_at")
            .map_err(|e| decode_err("acted_at", e))?,
        from_status: row
            .try_get::<String>("", "from_status")
            .map_err(|e| decode_err("from_status", e))?,
        to_status: row
            .try_get::<String>("", "to_status")
            .map_err(|e| decode_err("to_status", e))?,
        reason_code: row
            .try_get::<Option<String>>("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        sla_target_hours: row
            .try_get::<Option<i64>>("", "sla_target_hours")
            .map_err(|e| decode_err("sla_target_hours", e))?,
        sla_deadline: row
            .try_get::<Option<String>>("", "sla_deadline")
            .map_err(|e| decode_err("sla_deadline", e))?,
        step_up_used: row
            .try_get::<i64>("", "step_up_used")
            .map_err(|e| decode_err("step_up_used", e))?
            != 0,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Load a DI and parse its current status. Returns `(InterventionRequest, DiStatus)`.
async fn load_di_with_status(
    txn: &impl ConnectionTrait,
    di_id: i64,
) -> AppResult<(InterventionRequest, DiStatus)> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {IR_COLS} FROM intervention_requests ir WHERE ir.id = ?"),
            [di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    let di = map_intervention_request(&row)?;
    let status = DiStatus::try_from_str(&di.status).map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Stored DI has invalid status: {e}"))
    })?;

    Ok((di, status))
}

/// Insert a row into `di_review_events` inside an active transaction.
async fn insert_review_event(
    txn: &impl ConnectionTrait,
    di_id: i64,
    event_type: &str,
    actor_id: i64,
    acted_at: &str,
    from_status: &str,
    to_status: &str,
    reason_code: Option<&str>,
    notes: Option<&str>,
    sla_target_hours: Option<i64>,
    sla_deadline: Option<&str>,
    step_up_used: bool,
) -> AppResult<()> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_review_events \
            (di_id, event_type, actor_id, acted_at, from_status, to_status, \
             reason_code, notes, sla_target_hours, sla_deadline, step_up_used) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            di_id.into(),
            event_type.into(),
            actor_id.into(),
            acted_at.into(),
            from_status.into(),
            to_status.into(),
            reason_code
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            sla_target_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            sla_deadline
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            i64::from(step_up_used).into(),
        ],
    ))
    .await?;

    Ok(())
}

/// Also write to the legacy `di_state_transition_log` for backward compatibility
/// with File 01's audit trail.
async fn insert_transition_log(
    txn: &impl ConnectionTrait,
    di_id: i64,
    from_status: &str,
    to_status: &str,
    action: &str,
    actor_id: i64,
    acted_at: &str,
    reason_code: Option<&str>,
    notes: Option<&str>,
) -> AppResult<()> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_state_transition_log \
            (di_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            di_id.into(),
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

/// Check concurrency: `rows_affected == 1` after an UPDATE with `row_version` guard.
fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }
    Ok(())
}

/// Re-fetch the updated DI after a successful write.
async fn refetch_di(
    txn: &impl ConnectionTrait,
    di_id: i64,
) -> AppResult<InterventionRequest> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {IR_COLS} FROM intervention_requests ir WHERE ir.id = ?"),
            [di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    map_intervention_request(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) screen_di — PendingReview → Screened → AwaitingApproval (atomic)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn screen_di(
    db: &DatabaseConnection,
    input: DiScreenInput,
) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;

    // 1. Load and validate both transitions upfront
    let (_di, current_status) = load_di_with_status(&txn, input.di_id).await?;

    guard_transition(&current_status, &DiStatus::Screened).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;
    guard_transition(&DiStatus::Screened, &DiStatus::AwaitingApproval).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    // 2. Validate urgency
    DiUrgency::try_from_str(&input.validated_urgency).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    // 3. Validate classification_code_id resolves in reference_values (if provided)
    if let Some(cid) = input.classification_code_id {
        let ref_exists = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM reference_values WHERE id = ?",
                [cid.into()],
            ))
            .await?;
        if ref_exists.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Code de classification introuvable (classification_code_id={}).",
                cid
            )]));
        }
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // 4. UPDATE — final persisted state is 'awaiting_approval' (auto-advanced)
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'awaiting_approval', \
                validated_urgency = ?, \
                review_team_id = COALESCE(?, review_team_id), \
                classification_code_id = COALESCE(?, classification_code_id), \
                reviewer_note = COALESCE(?, reviewer_note), \
                reviewer_id = ?, \
                screened_at = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input.validated_urgency.clone().into(),
                input
                    .review_team_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .classification_code_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .reviewer_note
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                input.actor_id.into(),
                now.clone().into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();

    // 5. Event row 1: screened
    insert_review_event(
        &txn,
        input.di_id,
        "screened",
        input.actor_id,
        &now,
        from_str,
        DiStatus::Screened.as_str(),
        None,
        input.reviewer_note.as_deref(),
        None,
        None,
        false,
    )
    .await?;

    // 6. Event row 2: advanced_to_approval
    insert_review_event(
        &txn,
        input.di_id,
        "advanced_to_approval",
        input.actor_id,
        &now,
        DiStatus::Screened.as_str(),
        DiStatus::AwaitingApproval.as_str(),
        None,
        None,
        None,
        None,
        false,
    )
    .await?;

    // 7. Legacy transition log entries
    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        DiStatus::Screened.as_str(),
        "screen",
        input.actor_id,
        &now,
        None,
        input.reviewer_note.as_deref(),
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        DiStatus::Screened.as_str(),
        DiStatus::AwaitingApproval.as_str(),
        "advance_to_approval",
        input.actor_id,
        &now,
        None,
        None,
    )
    .await?;

    // 8. Re-fetch and commit
    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;

    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) return_di_for_clarification — PendingReview → ReturnedForClarification
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn return_di_for_clarification(
    db: &DatabaseConnection,
    input: DiReturnInput,
) -> AppResult<InterventionRequest> {
    // Validate required field
    if input.reviewer_note.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "La note du réviseur est obligatoire pour retourner une DI.".into(),
        ]));
    }

    let txn = db.begin().await?;
    let (_, current_status) = load_di_with_status(&txn, input.di_id).await?;

    guard_transition(&current_status, &DiStatus::ReturnedForClarification).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'returned_for_clarification', \
                reviewer_note = ?, \
                reviewer_id = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input.reviewer_note.clone().into(),
                input.actor_id.into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::ReturnedForClarification.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "returned_for_clarification",
        input.actor_id,
        &now,
        from_str,
        to_str,
        None,
        Some(&input.reviewer_note),
        None,
        None,
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "return_for_clarification",
        input.actor_id,
        &now,
        None,
        Some(&input.reviewer_note),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;

    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) reject_di — PendingReview|Screened|AwaitingApproval → Rejected
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn reject_di(
    db: &DatabaseConnection,
    input: DiRejectInput,
) -> AppResult<InterventionRequest> {
    // Validate required field
    if input.reason_code.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le code de motif est obligatoire pour rejeter une DI.".into(),
        ]));
    }

    let txn = db.begin().await?;
    let (_, current_status) = load_di_with_status(&txn, input.di_id).await?;

    guard_transition(&current_status, &DiStatus::Rejected).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'rejected', \
                declined_at = ?, \
                reviewer_id = ?, \
                reviewer_note = COALESCE(?, reviewer_note), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                input.actor_id.into(),
                input
                    .notes
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::Rejected.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "rejected",
        input.actor_id,
        &now,
        from_str,
        to_str,
        Some(&input.reason_code),
        input.notes.as_deref(),
        None,
        None,
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "reject",
        input.actor_id,
        &now,
        Some(&input.reason_code),
        input.notes.as_deref(),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;

    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) approve_di_for_planning — AwaitingApproval → ApprovedForPlanning
//    Step-up is enforced at the IPC command layer (require_step_up!).
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn approve_di_for_planning(
    db: &DatabaseConnection,
    input: DiApproveInput,
) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;
    let (_, current_status) = load_di_with_status(&txn, input.di_id).await?;

    guard_transition(&current_status, &DiStatus::ApprovedForPlanning).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'approved_for_planning', \
                approved_at = ?, \
                reviewer_note = COALESCE(?, reviewer_note), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                input
                    .notes
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::ApprovedForPlanning.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "approved",
        input.actor_id,
        &now,
        from_str,
        to_str,
        None,
        input.notes.as_deref(),
        None,
        None,
        true, // step_up_used — enforced at IPC layer
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "approve",
        input.actor_id,
        &now,
        None,
        input.notes.as_deref(),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;

    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) defer_di — ApprovedForPlanning|AwaitingApproval → Deferred
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn defer_di(
    db: &DatabaseConnection,
    input: DiDeferInput,
) -> AppResult<InterventionRequest> {
    // Validate required field
    if input.reason_code.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le code de motif est obligatoire pour reporter une DI.".into(),
        ]));
    }

    // Validate deferred_until is a future date
    let deferred_date = chrono::NaiveDate::parse_from_str(&input.deferred_until, "%Y-%m-%d")
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "Format de date invalide pour deferred_until : '{}'. Format attendu : YYYY-MM-DD.",
                input.deferred_until
            )])
        })?;

    let today = Utc::now().date_naive();
    if deferred_date <= today {
        return Err(AppError::ValidationFailed(vec![
            "La date de report (deferred_until) doit être dans le futur.".into(),
        ]));
    }

    let txn = db.begin().await?;
    let (_, current_status) = load_di_with_status(&txn, input.di_id).await?;

    guard_transition(&current_status, &DiStatus::Deferred).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'deferred', \
                deferred_until = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input.deferred_until.clone().into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::Deferred.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "deferred",
        input.actor_id,
        &now,
        from_str,
        to_str,
        Some(&input.reason_code),
        input.notes.as_deref(),
        None,
        None,
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "defer",
        input.actor_id,
        &now,
        Some(&input.reason_code),
        input.notes.as_deref(),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;

    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) reactivate_deferred_di — Deferred → AwaitingApproval
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn reactivate_deferred_di(
    db: &DatabaseConnection,
    input: DiReactivateInput,
) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;
    let (_, current_status) = load_di_with_status(&txn, input.di_id).await?;

    guard_transition(&current_status, &DiStatus::AwaitingApproval).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'awaiting_approval', \
                deferred_until = NULL, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::AwaitingApproval.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "reactivated",
        input.actor_id,
        &now,
        from_str,
        to_str,
        None,
        input.notes.as_deref(),
        None,
        None,
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "reactivate",
        input.actor_id,
        &now,
        None,
        input.notes.as_deref(),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;

    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) get_review_events — read-only query
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn get_review_events(
    db: &DatabaseConnection,
    di_id: i64,
) -> AppResult<Vec<DiReviewEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, di_id, event_type, actor_id, acted_at, from_status, to_status, \
                    reason_code, notes, sla_target_hours, sla_deadline, step_up_used \
             FROM di_review_events \
             WHERE di_id = ? \
             ORDER BY acted_at ASC, id ASC",
            [di_id.into()],
        ))
        .await?;

    rows.iter().map(map_review_event).collect()
}
