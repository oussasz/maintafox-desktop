//! Planning commitment → Work Order write-back (SSOT for committed slots).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use crate::errors::{AppError, AppResult};
use crate::wo::audit::{self, WoAuditInput};
use crate::wo::time::now_utc_z;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction, WoStatus};
use crate::wo::workflow::transition::{check_concurrency, load_wo_status};

/// Result of a successful in-transaction WO sync (for post-commit audit).
#[derive(Debug, Clone, Copy)]
pub struct WoSyncOutcome {
    pub wo_id: i64,
}

/// Write committed slot/crew onto the linked WO and mirror candidate window fields.
///
/// Call only when `source_type == "work_order"`, after the commitment row is written,
/// on the open planning transaction.
pub async fn sync_work_order_from_commitment(
    tx: &impl ConnectionTrait,
    actor_id: i64,
    schedule_candidate_id: i64,
    wo_id: i64,
    committed_start: &str,
    committed_end: &str,
    assigned_team_id: i64,
    assigned_personnel_id: Option<i64>,
) -> AppResult<WoSyncOutcome> {
    let (from_code, status, row_version) = load_wo_status(tx, wo_id).await?;

    let (schedule_action, assign_action, assign_event) = match status {
        WoStatus::Planning => (WoAction::EditSchedule, WoAction::EditAssignment, "assigned"),
        WoStatus::Ready => (WoAction::Reschedule, WoAction::Reassign, "reassigned"),
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Planning commitment cannot sync Work Order #{wo_id}: status must be planning or ready (actuel: {}).",
                status.as_str()
            )]));
        }
    };
    assert_action_allowed(&status, schedule_action).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    assert_action_allowed(&status, assign_action).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let primary_responsible_id = resolve_personnel_user_id(tx, assigned_personnel_id).await?;
    let now = now_utc_z();

    let result = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET
                planned_start = ?,
                planned_end = ?,
                assigned_team_id = ?,
                primary_responsible_id = ?,
                scheduled_at = ?,
                planner_id = COALESCE(planner_id, ?),
                row_version = row_version + 1,
                updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                committed_start.to_string().into(),
                committed_end.to_string().into(),
                assigned_team_id.into(),
                primary_responsible_id.into(),
                now.clone().into(),
                actor_id.into(),
                now.clone().into(),
                wo_id.into(),
                row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let payload = json!({
        "origin": "planning_commitment",
        "committed_start": committed_start,
        "committed_end": committed_end,
        "assigned_team_id": assigned_team_id,
        "assigned_personnel_id": assigned_personnel_id,
        "primary_responsible_id": primary_responsible_id,
    })
    .to_string();

    emit_action_event(
        tx,
        wo_id,
        "schedule_changed",
        Some(actor_id),
        Some(&from_code),
        Some(&from_code),
        Some(&payload),
    )
    .await?;
    emit_action_event(
        tx,
        wo_id,
        assign_event,
        Some(actor_id),
        Some(&from_code),
        Some(&from_code),
        Some(&payload),
    )
    .await?;

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_candidates
         SET window_start = ?,
             window_end = ?,
             assigned_team_id = ?,
             assigned_personnel_id = ?,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [
            committed_start.to_string().into(),
            committed_end.to_string().into(),
            assigned_team_id.into(),
            assigned_personnel_id.into(),
            schedule_candidate_id.into(),
        ],
    ))
    .await?;

    // Staged on the planning transaction so the WO outbox entry commits with the commitment.

    Ok(WoSyncOutcome { wo_id })
}

/// Fire-and-log WO change events after the planning transaction commits.
pub async fn record_wo_sync_audit(db: &DatabaseConnection, actor_id: i64, wo_id: i64, summary_suffix: &str) {
    let details = json!({ "origin": "planning_commitment", "detail": summary_suffix }).to_string();
    audit::record_wo_change_event(
        db,
        WoAuditInput {
            wo_id: Some(wo_id),
            action: "planned".to_string(),
            actor_id: Some(actor_id),
            summary: Some(format!("Synced from planning commitment ({summary_suffix})")),
            details_json: Some(details.clone()),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await;
    audit::record_wo_change_event(
        db,
        WoAuditInput {
            wo_id: Some(wo_id),
            action: "assigned".to_string(),
            actor_id: Some(actor_id),
            summary: Some(format!("Synced from planning commitment ({summary_suffix})")),
            details_json: Some(details),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await;
}

async fn resolve_personnel_user_id(tx: &impl ConnectionTrait, personnel_id: Option<i64>) -> AppResult<Option<i64>> {
    let Some(personnel_id) = personnel_id else {
        return Ok(None);
    };
    let row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id
             FROM user_accounts
             WHERE personnel_id = ?
               AND deleted_at IS NULL
             ORDER BY id
             LIMIT 1",
            [personnel_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i64>("", "id").ok()))
}
