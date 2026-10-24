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
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::domain::{
    guard_transition, map_intervention_request, DiStatus, InterventionRequest, IR_COLS, IR_JOINS, IR_JOIN_COLS,
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
    pub classification_code_id: Option<i64>,
    pub reviewer_note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiReturnInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub reviewer_note: String,
    /// Reason from the plant's `DI.RETURN_REASON` list (required).
    #[serde(default)]
    pub reason_code: Option<String>,
}

/// The requester answers the reviewer's question and sends the request back to review.
#[derive(Debug, Clone, Deserialize)]
pub struct DiResubmitInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    /// The requester's answer to the reviewer (required).
    pub response_note: String,
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
pub struct DiCloseInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub disposition_code: String,
    pub notes: Option<String>,
    pub related_di_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiCancelOwnInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
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

#[derive(Debug, Clone, Deserialize)]
pub struct DiCloseNonExecutableInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiArchiveInput {
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
    /// Display name (else username) of the actor; `None` for system events.
    pub actor_display_name: Option<String>,
    pub acted_at: String,
    pub from_status: String,
    pub to_status: String,
    pub reason_code: Option<String>,
    pub notes: Option<String>,
    pub sla_target_hours: Option<i64>,
    pub sla_deadline: Option<String>,
    pub sla_resolution_target_hours: Option<i64>,
    pub sla_resolution_deadline: Option<String>,
    pub step_up_used: bool,
}

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
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        di_id: row.try_get::<i64>("", "di_id").map_err(|e| decode_err("di_id", e))?,
        event_type: row
            .try_get::<String>("", "event_type")
            .map_err(|e| decode_err("event_type", e))?,
        actor_id: row
            .try_get::<Option<i64>>("", "actor_id")
            .map_err(|e| decode_err("actor_id", e))?,
        actor_display_name: row
            .try_get::<Option<String>>("", "actor_display_name")
            .map_err(|e| decode_err("actor_display_name", e))?,
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
        sla_resolution_target_hours: row
            .try_get::<Option<i64>>("", "sla_resolution_target_hours")
            .ok()
            .flatten(),
        sla_resolution_deadline: row
            .try_get::<Option<String>>("", "sla_resolution_deadline")
            .ok()
            .flatten(),
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
async fn load_di_with_status(txn: &impl ConnectionTrait, di_id: i64) -> AppResult<(InterventionRequest, DiStatus)> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {IR_COLS}, {IR_JOIN_COLS} \
                 FROM intervention_requests ir \
                 {IR_JOINS} \
                 WHERE ir.id = ?"
            ),
            [di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    let di = map_intervention_request(&row)?;
    let status = DiStatus::try_from_str(&di.status)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored DI has invalid status: {e}")))?;

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
    sla_resolution_target_hours: Option<i64>,
    sla_resolution_deadline: Option<&str>,
    step_up_used: bool,
) -> AppResult<()> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_review_events \
            (di_id, event_type, actor_id, acted_at, from_status, to_status, \
             reason_code, notes, sla_target_hours, sla_deadline, \
             sla_resolution_target_hours, sla_resolution_deadline, step_up_used) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            sla_resolution_target_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            sla_resolution_deadline
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
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }
    Ok(())
}

/// Re-fetch the updated DI after a successful write.
async fn refetch_di(txn: &impl ConnectionTrait, di_id: i64) -> AppResult<InterventionRequest> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {IR_COLS}, {IR_JOIN_COLS} \
                 FROM intervention_requests ir \
                 {IR_JOINS} \
                 WHERE ir.id = ?"
            ),
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

pub async fn screen_di(db: &DatabaseConnection, input: DiScreenInput) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;

    // 1. Load and validate both transitions upfront
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    guard_transition(&current_status, &DiStatus::AwaitingApproval).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // 2. Validate urgency against the shared DI / OT scale (stored normalised)
    let validated_urgency = crate::reference::urgency_scale::validate_urgency_code(&input.validated_urgency)?;

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
                "GATE_DI_CLASSIFICATION_NOT_FOUND:id={0}: Code de classification introuvable (classification_code_id={0}).",
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
                classification_code_id = COALESCE(?, classification_code_id), \
                reviewer_note = COALESCE(?, reviewer_note), \
                reviewer_id = ?, \
                screened_at = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                validated_urgency.as_str().into(),
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
    let to_str = DiStatus::AwaitingApproval.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "screened",
        input.actor_id,
        &now,
        from_str,
        to_str,
        None,
        input.reviewer_note.as_deref(),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "screen",
        input.actor_id,
        &now,
        None,
        input.reviewer_note.as_deref(),
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
            "GATE_DI_REVIEWER_NOTE_REQUIRED: La note du réviseur est obligatoire pour retourner une DI.".into(),
        ]));
    }
    let reason = super::reference_catalog::validate_di_decision_reason(
        db,
        super::reference_catalog::DOMAIN_RETURN_REASON,
        input.reason_code.as_deref().unwrap_or(""),
    )
    .await?;

    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    guard_transition(&current_status, &DiStatus::ReturnedForClarification)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

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
        Some(&reason),
        Some(&input.reviewer_note),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
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
        Some(&reason),
        Some(&input.reviewer_note),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;
    super::notifications::notify_returned(db, &updated).await;
    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B2) resubmit_returned_di — ReturnedForClarification → InReview (by the requester)
// ═══════════════════════════════════════════════════════════════════════════════

/// The requester answers the reviewer's question and the request goes back to the review queue.
/// Only the requester may do it; the answer is kept on the request's timeline (review event and
/// transition log), next to the reviewer's question.
pub async fn resubmit_returned_di(db: &DatabaseConnection, input: DiResubmitInput) -> AppResult<InterventionRequest> {
    let answer = input.response_note.trim().to_string();
    if answer.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_DI_RESPONSE_REQUIRED: La réponse au réviseur est obligatoire pour renvoyer la demande.".into(),
        ]));
    }

    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;

    if di.submitter_id != input.actor_id {
        return Err(AppError::ValidationFailed(vec![
            "GATE_DI_RESUBMIT_NOT_SUBMITTER: Seul le demandeur peut répondre et renvoyer sa demande.".into(),
        ]));
    }
    if current_status != DiStatus::ReturnedForClarification {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_DI_RESUBMIT_STATUS_INVALID: Seule une demande retournée pour information peut être renvoyée. Statut actuel : '{}'.",
            current_status.as_str()
        )]));
    }
    guard_transition(&current_status, &DiStatus::InReview).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'in_review', \
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
    let to_str = DiStatus::InReview.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "resubmitted",
        input.actor_id,
        &now,
        from_str,
        to_str,
        None,
        Some(&answer),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "re_submitted",
        input.actor_id,
        &now,
        None,
        Some(&answer),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;
    super::notifications::notify_entered_in_review(db, &updated).await;
    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) close_di — InReview|AwaitingApproval|Approved → Closed (+ disposition)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn close_di(db: &DatabaseConnection, input: DiCloseInput) -> AppResult<InterventionRequest> {
    super::disposition::validate_close_disposition(
        db,
        &input.disposition_code,
        input.notes.as_deref(),
        input.related_di_id,
        input.di_id,
    )
    .await?;

    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    match current_status {
        DiStatus::InReview | DiStatus::AwaitingApproval | DiStatus::Approved => {}
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_DI_CLOSE_STATUS_INVALID: La fermeture n'est possible qu'au statut 'in_review', 'awaiting_approval' ou 'approved'. \
                 Statut actuel : '{}'.",
                other.as_str()
            )]));
        }
    }

    guard_transition(&current_status, &DiStatus::Closed).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let disposition = input.disposition_code.trim().to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'closed', \
                disposition_code = ?, \
                disposition_notes = ?, \
                related_di_id = ?, \
                closed_at = ?, \
                closed_by_id = ?, \
                reviewer_id = ?, \
                reviewer_note = COALESCE(?, reviewer_note), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                disposition.clone().into(),
                input
                    .notes
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                input
                    .related_di_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                now.clone().into(),
                input.actor_id.into(),
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
    let to_str = DiStatus::Closed.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "closed",
        input.actor_id,
        &now,
        from_str,
        to_str,
        Some(&disposition),
        input.notes.as_deref(),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
        false,
    )
    .await?;

    // Persist related_di_id on event when present
    if let Some(related_id) = input.related_di_id {
        let _ = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE di_review_events SET related_di_id = ? \
                 WHERE id = (SELECT MAX(id) FROM di_review_events WHERE di_id = ?)",
                [related_id.into(), input.di_id.into()],
            ))
            .await;
    }

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "close",
        input.actor_id,
        &now,
        Some(&disposition),
        input.notes.as_deref(),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;
    super::notifications::notify_closed(db, &updated).await;
    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C2) cancel_own_di — Submitted|ReturnedForClarification → Closed
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn cancel_own_di(db: &DatabaseConnection, input: DiCancelOwnInput) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;

    if di.submitter_id != input.actor_id {
        return Err(AppError::ValidationFailed(vec![
            "GATE_DI_CANCEL_OWN_NOT_SUBMITTER: Seul le demandeur peut annuler sa propre DI.".into(),
        ]));
    }

    match current_status {
        DiStatus::Submitted | DiStatus::ReturnedForClarification => {}
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_DI_CANCEL_OWN_STATUS_INVALID: L'annulation demandeur n'est possible qu'aux statuts 'submitted' ou \
                 'returned_for_clarification'. Statut actuel : '{}'.",
                other.as_str()
            )]));
        }
    }

    guard_transition(&current_status, &DiStatus::Closed).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let disposition = super::disposition::DISPOSITION_CANCELLED_BY_REQUESTER.to_string();
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'closed', \
                disposition_code = ?, \
                disposition_notes = ?, \
                closed_at = ?, \
                closed_by_id = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                disposition.clone().into(),
                input
                    .notes
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                now.clone().into(),
                input.actor_id.into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::Closed.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "closed",
        input.actor_id,
        &now,
        from_str,
        to_str,
        Some(&disposition),
        input.notes.as_deref(),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "cancel_own",
        input.actor_id,
        &now,
        Some(&disposition),
        input.notes.as_deref(),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;
    super::notifications::notify_closed(db, &updated).await;
    Ok(updated)
}

// Compat: reject_di → close with rejected_invalid (or duplicate if reason says so)
pub async fn reject_di(db: &DatabaseConnection, input: DiRejectInput) -> AppResult<InterventionRequest> {
    let reason = input.reason_code.trim().to_lowercase();
    let disposition = if reason.contains("duplicate") || reason == "doublon" {
        super::disposition::DISPOSITION_DUPLICATE.to_string()
    } else {
        super::disposition::DISPOSITION_REJECTED_INVALID.to_string()
    };
    close_di(
        db,
        DiCloseInput {
            di_id: input.di_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            disposition_code: disposition,
            notes: Some(input.reason_code).filter(|s| !s.trim().is_empty()).or(input.notes),
            related_di_id: None,
        },
    )
    .await
}

// Compat: close_di_as_non_executable → close with no_work_required
pub async fn close_di_as_non_executable(
    db: &DatabaseConnection,
    input: DiCloseNonExecutableInput,
) -> AppResult<InterventionRequest> {
    close_di(
        db,
        DiCloseInput {
            di_id: input.di_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            disposition_code: super::disposition::DISPOSITION_NO_WORK_REQUIRED.to_string(),
            notes: input.notes,
            related_di_id: None,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) approve_di — AwaitingApproval → Approved (step-up at IPC)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn approve_di(db: &DatabaseConnection, input: DiApproveInput) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    guard_transition(&current_status, &DiStatus::Approved).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'approved', \
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
    let to_str = DiStatus::Approved.as_str();

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
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
        true,
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
    super::notifications::notify_approved(db, &updated).await;
    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) defer_di — InReview|AwaitingApproval|Approved → Deferred
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn defer_di(db: &DatabaseConnection, input: DiDeferInput) -> AppResult<InterventionRequest> {
    if input.reason_code.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_DI_DEFER_REASON_REQUIRED: Le motif de report est obligatoire.".into(),
        ]));
    }
    let reason = super::reference_catalog::validate_di_decision_reason(
        db,
        super::reference_catalog::DOMAIN_DEFERRAL_REASON,
        &input.reason_code,
    )
    .await?;

    // deferred_until must be a calendar date strictly after today (UTC).
    let until = input.deferred_until.trim();
    if until.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_DI_DEFER_DATE_REQUIRED: La date de report (deferred_until) est obligatoire.".into(),
        ]));
    }
    let today = Utc::now().date_naive();
    let date_part = until.get(..10).unwrap_or(until);
    let until_date = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").map_err(|_| {
        AppError::ValidationFailed(vec![
            "GATE_DI_DEFER_DATE_INVALID: Format de date de report invalide (attendu YYYY-MM-DD).".into(),
        ])
    })?;
    if until_date <= today {
        return Err(AppError::ValidationFailed(vec![
            "GATE_DI_DEFER_DATE_NOT_FUTURE: La date de report doit être une date future (strictement après aujourd'hui).".into(),
        ]));
    }

    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    match current_status {
        DiStatus::InReview | DiStatus::AwaitingApproval | DiStatus::Approved => {}
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_DI_DEFER_STATUS_INVALID: Le report n'est possible qu'au statut 'in_review', 'awaiting_approval' ou 'approved'. \
                 Statut actuel : '{}'.",
                other.as_str()
            )]));
        }
    }

    guard_transition(&current_status, &DiStatus::Deferred).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let from_saved = current_status.as_str().to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'deferred', \
                deferred_from_status = ?, \
                deferred_until = ?, \
                reviewer_note = COALESCE(?, reviewer_note), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                from_saved.clone().into(),
                input.deferred_until.clone().into(),
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
    let to_str = DiStatus::Deferred.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "deferred",
        input.actor_id,
        &now,
        from_str,
        to_str,
        Some(&reason),
        input.notes.as_deref(),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
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
        Some(&reason),
        input.notes.as_deref(),
    )
    .await?;

    let updated = refetch_di(&txn, input.di_id).await?;
    txn.commit().await?;
    super::notifications::notify_deferred(db, &updated).await;
    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) reactivate_deferred_di — Deferred → deferred_from_status
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn reactivate_deferred_di(
    db: &DatabaseConnection,
    input: DiReactivateInput,
) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    if current_status != DiStatus::Deferred {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_DI_REACTIVATE_STATUS_INVALID: Seule une DI reportée peut être réactivée. Statut actuel : '{}'.",
            current_status.as_str()
        )]));
    }

    let target = di
        .deferred_from_status
        .as_deref()
        .and_then(|s| DiStatus::try_from_str(s).ok())
        .unwrap_or(DiStatus::AwaitingApproval);

    guard_transition(&current_status, &target).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let target_str = target.as_str().to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = ?, \
                deferred_until = NULL, \
                deferred_from_status = NULL, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                target_str.clone().into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "reactivated",
        input.actor_id,
        &now,
        from_str,
        &target_str,
        None,
        input.notes.as_deref(),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        &target_str,
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
// H) archive_di — Closed only: set archived_at (no status change)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn archive_di(db: &DatabaseConnection, input: DiArchiveInput) -> AppResult<InterventionRequest> {
    let txn = db.begin().await?;
    let (di, current_status) = load_di_with_status(&txn, input.di_id).await?;
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);

    if current_status != DiStatus::Closed {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_DI_ARCHIVE_STATUS_INVALID: Seules les DI clôturées peuvent être archivées. Statut actuel : '{}'.",
            current_status.as_str()
        )]));
    }
    if di.archived_at.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_DI_ALREADY_ARCHIVED: Cette DI est déjà archivée.".into(),
        ]));
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                archived_at = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = current_status.as_str();

    insert_review_event(
        &txn,
        input.di_id,
        "archived",
        input.actor_id,
        &now,
        from_str,
        to_str,
        None,
        input.notes.as_deref(),
        snap.response_target_hours,
        snap.response_deadline.as_deref(),
        snap.resolution_target_hours,
        snap.resolution_deadline.as_deref(),
        false,
    )
    .await?;

    insert_transition_log(
        &txn,
        input.di_id,
        from_str,
        to_str,
        "archive",
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

pub async fn get_review_events(db: &DatabaseConnection, di_id: i64) -> AppResult<Vec<DiReviewEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, e.di_id, e.event_type, e.actor_id, \
                    COALESCE(ua.display_name, ua.username) AS actor_display_name, \
                    e.acted_at, e.from_status, e.to_status, \
                    e.reason_code, e.notes, e.sla_target_hours, e.sla_deadline, \
                    e.sla_resolution_target_hours, e.sla_resolution_deadline, e.step_up_used \
             FROM di_review_events e \
             LEFT JOIN user_accounts ua ON ua.id = e.actor_id \
             WHERE e.di_id = ? \
             ORDER BY e.acted_at ASC, e.id ASC",
            [di_id.into()],
        ))
        .await?;

    rows.iter().map(map_review_event).collect()
}
