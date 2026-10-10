//! Assignment / emergency reassign (Planning + Ready) — no status change.
//!
//! Product UI uses `emergency_reassign` (reason required, skips Planning gates).
//! Quiet assign without reason is no longer allowed.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;
use serde_json::json;

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::queries;
use crate::wo::time::now_utc_z;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction, WoStatus};
use crate::wo::workflow::transition::{check_concurrency, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoAssignSaveInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub assigned_team_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub scheduled_at: Option<String>,
    /// Required for any non-Planning assignment write.
    pub override_reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoEmergencyReassignInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub assigned_team_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub override_reason: String,
}

struct LatestCommitment {
    id: i64,
    frozen_at: Option<String>,
    assigned_team_id: i64,
    assigned_personnel_id: Option<i64>,
}

async fn load_latest_commitment(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<LatestCommitment>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, frozen_at, assigned_team_id, assigned_personnel_id
             FROM schedule_commitments
             WHERE source_type = 'work_order' AND source_id = ?
             ORDER BY id DESC
             LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(LatestCommitment {
        id: row
            .try_get("", "id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("schedule_commitments.id: {e}")))?,
        frozen_at: row.try_get("", "frozen_at").ok().flatten(),
        assigned_team_id: row
            .try_get("", "assigned_team_id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("schedule_commitments.assigned_team_id: {e}")))?,
        assigned_personnel_id: row.try_get("", "assigned_personnel_id").ok().flatten(),
    }))
}

async fn resolve_user_personnel_id(db: &impl ConnectionTrait, user_id: Option<i64>) -> AppResult<Option<i64>> {
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id
             FROM user_accounts
             WHERE id = ?
               AND deleted_at IS NULL
             LIMIT 1",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<Option<i64>>("", "personnel_id").ok().flatten()))
}

async fn append_emergency_schedule_log(
    db: &impl ConnectionTrait,
    commitment_id: i64,
    actor_id: i64,
    old_team: i64,
    old_personnel: Option<i64>,
    new_team: i64,
    new_personnel: Option<i64>,
    reason: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO schedule_change_log
            (commitment_id, action_type, actor_id, field_changed, old_value, new_value,
             reason_code, reason_note, reason, details_json)
         VALUES (?, 'emergency_reassign', ?, 'assignment', ?, ?, 'emergency_reassign', ?, ?, ?)",
        [
            commitment_id.into(),
            actor_id.into(),
            format!("team={old_team};personnel={old_personnel:?}").into(),
            format!("team={new_team};personnel={new_personnel:?}").into(),
            reason.to_string().into(),
            reason.to_string().into(),
            json!({
                "origin": "emergency_reassign",
                "override_reason": reason,
            })
            .to_string()
            .into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Product path: emergency reassign (skips capacity / lock / double-booking gates).
pub async fn emergency_reassign(db: &DatabaseConnection, input: WoEmergencyReassignInput) -> AppResult<WorkOrder> {
    let reason = input.override_reason.trim().to_string();
    if reason.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "override_reason is required for emergency reassign.".to_string(),
        ]));
    }
    if input.assigned_team_id.is_none() && input.primary_responsible_id.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "At least one of assigned_team_id or primary_responsible_id is required.".to_string(),
        ]));
    }
    if let Some(team_id) = input.assigned_team_id {
        crate::repository::team_repository::assert_active_crew_exists(db, team_id).await?;
    }

    let commitment = load_latest_commitment(db, input.wo_id).await?;
    if let Some(ref cmt) = commitment {
        if cmt.frozen_at.is_some() {
            return Err(AppError::ValidationFailed(vec![
                "Schedule is frozen for this work order. Use Planning break-in — emergency reassign is blocked."
                    .to_string(),
            ]));
        }
    }

    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;

    let action = match status {
        WoStatus::Planning => WoAction::EditAssignment,
        WoStatus::Ready => WoAction::Reassign,
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Emergency reassign is only allowed in status planning or ready (current: {}).",
                status.as_str()
            )]));
        }
    };
    assert_action_allowed(&status, action).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                assigned_team_id = COALESCE(?, assigned_team_id), \
                primary_responsible_id = COALESCE(?, primary_responsible_id), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input
                    .assigned_team_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .primary_responsible_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    if let Some(cmt) = commitment {
        let next_team = input.assigned_team_id.unwrap_or(cmt.assigned_team_id);
        let next_personnel = if input.primary_responsible_id.is_some() {
            resolve_user_personnel_id(&txn, input.primary_responsible_id).await?
        } else {
            cmt.assigned_personnel_id
        };
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE schedule_commitments SET
                assigned_team_id = ?,
                assigned_personnel_id = ?,
                row_version = row_version + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [next_team.into(), next_personnel.into(), cmt.id.into()],
        ))
        .await?;
        append_emergency_schedule_log(
            &txn,
            cmt.id,
            input.actor_id,
            cmt.assigned_team_id,
            cmt.assigned_personnel_id,
            next_team,
            next_personnel,
            &reason,
        )
        .await?;
    }

    let payload = json!({
        "origin": "emergency_reassign",
        "override_reason": reason,
        "assigned_team_id": input.assigned_team_id,
        "primary_responsible_id": input.primary_responsible_id,
        "skips_planning_gates": true,
    })
    .to_string();

    emit_action_event(
        &txn,
        input.wo_id,
        "emergency_reassigned",
        Some(input.actor_id),
        Some(&from_code),
        Some(&from_code),
        Some(&payload),
    )
    .await?;

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    crate::wo::sync_stage::stage_work_order_by_id(&txn, wo.id).await?;
    txn.commit().await?;
    Ok(wo)
}

/// Legacy entry used by tests / seed — requires override_reason; delegates to emergency_reassign.
pub async fn save_assignment(db: &DatabaseConnection, input: WoAssignSaveInput) -> AppResult<WorkOrder> {
    emergency_reassign(
        db,
        WoEmergencyReassignInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            assigned_team_id: input.assigned_team_id,
            primary_responsible_id: input.primary_responsible_id,
            override_reason: input.override_reason,
        },
    )
    .await
}
