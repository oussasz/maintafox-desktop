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
use crate::wo::workflow::state_machine::{assert_action_allowed, status_i18n_key, WoAction, WoStatus};
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
    /// The new team's shift window (`wo::shifts`), when the team changes; `None` keeps the
    /// current one, which the new team must then work on the planned day.
    #[serde(default)]
    pub shift_class_version_id: Option<i64>,
    /// Removes the shift.
    #[serde(default)]
    pub clear_shift: bool,
}

struct LatestCommitment {
    id: i64,
    frozen_at: Option<String>,
    status: String,
    committed_start: String,
    committed_end: String,
    assigned_team_id: i64,
    assigned_personnel_id: Option<i64>,
}

async fn load_latest_commitment(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<LatestCommitment>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, frozen_at, status, committed_start, committed_end, assigned_team_id, assigned_personnel_id
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
        status: row
            .try_get("", "status")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("schedule_commitments.status: {e}")))?,
        committed_start: row
            .try_get("", "committed_start")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("schedule_commitments.committed_start: {e}")))?,
        committed_end: row
            .try_get("", "committed_end")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("schedule_commitments.committed_end: {e}")))?,
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

/// Product path: emergency reassign (skips capacity / lock / double-booking gates). Never blocked
/// by the shift: a shift the new team does not work is cleared, and the work order records that it
/// must be chosen again.
pub async fn emergency_reassign(db: &DatabaseConnection, input: WoEmergencyReassignInput) -> AppResult<WorkOrder> {
    reassign(db, input, true).await
}

async fn reassign(db: &DatabaseConnection, input: WoEmergencyReassignInput, emergency: bool) -> AppResult<WorkOrder> {
    let reason = input.override_reason.trim().to_string();
    if reason.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_REASSIGN_REASON_REQUIRED: override_reason is required for emergency reassign.".to_string(),
        ]));
    }
    if input.assigned_team_id.is_none() && input.primary_responsible_id.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_WO_REASSIGN_TARGET_REQUIRED: At least one of assigned_team_id or primary_responsible_id is required."
                .to_string(),
        ]));
    }
    if let Some(team_id) = input.assigned_team_id {
        crate::repository::team_repository::assert_active_crew_exists(db, team_id).await?;
    }

    let txn = db.begin().await?;
    // Read inside the transaction: the freeze checked is the one the write below lands on.
    let commitment = load_latest_commitment(&txn, input.wo_id).await?;
    if let Some(ref cmt) = commitment {
        if cmt.frozen_at.is_some() {
            return Err(AppError::ValidationFailed(vec![
                "GATE_WO_REASSIGN_SCHEDULE_FROZEN: Schedule is frozen for this work order. Use Planning break-in — emergency reassign is blocked."
                    .to_string(),
            ]));
        }
        // A new crew moves the job onto that crew's site schedule: entering a week frozen there
        // (or due to freeze) is a Planning break-in, never a reassignment. An emergency skips
        // capacity and double-booking, not the frozen week's four-eyes. Same verdict as a PC
        // receiving the row (`planning::sync_check`): a crew change of a committed slot is a move,
        // an assignee change alone is not, and a released or cancelled slot enters no week.
        if let Some(next_team) = input.assigned_team_id {
            if next_team != cmt.assigned_team_id && cmt.status == crate::planning::lifecycle::STATUS_COMMITTED {
                if let Some(week) = crate::planning::weekly_schedule::frozen_week_for_slot(
                    &txn,
                    next_team,
                    &cmt.committed_start,
                    &cmt.committed_end,
                    chrono::Utc::now(),
                )
                .await?
                {
                    return Err(crate::errors::org_validation_failed(
                        "PLANNING_WEEK_FROZEN",
                        format!(
                            "The week of {} is frozen: move work into it with a break-in, with its reason.",
                            week.week_start
                        ),
                        std::collections::HashMap::from([("week_start".to_string(), week.week_start.clone())]),
                    ));
                }
            }
        }
    }

    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;

    let action = match status {
        WoStatus::Planning => WoAction::EditAssignment,
        WoStatus::Ready => WoAction::Reassign,
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_WO_REASSIGN_STATUS:status={0}: Emergency reassign is only allowed in status planning or ready (current: {1}).",
                status_i18n_key(status.as_str()),
                status.as_str()
            )]));
        }
    };
    assert_action_allowed(&status, action).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // The shift must stay one the (new) team works on the planned day. A normal assignment is
    // refused otherwise; an emergency is not blocked: the shift is cleared, never silently.
    use crate::wo::shifts::{self, ShiftChange};
    let decided = shifts::decide_shift(
        &txn,
        input.wo_id,
        input.assigned_team_id,
        None,
        input.shift_class_version_id,
        input.clear_shift,
    )
    .await;
    let (shift_change, shift_cleared) = match decided {
        Ok(change) => (change, None),
        Err(e)
            if emergency
                && input.shift_class_version_id.is_none()
                && [
                    shifts::GATE_WO_SHIFT_TEAM_CHANGED,
                    shifts::GATE_WO_SHIFT_NOT_OFFERED,
                    shifts::GATE_WO_SHIFT_LEGACY_REPLAN,
                ]
                .iter()
                .any(|code| shifts::is_refusal(&e, code)) =>
        {
            (
                ShiftChange::Clear,
                Some(shifts::current_shift_note(&txn, input.wo_id).await?),
            )
        }
        Err(e) => return Err(e),
    };

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
    crate::wo::shifts::apply_shift_change(&txn, input.wo_id, &shift_change).await?;

    // The new responsible must hold what the work order requires (or have a supervisor
    // override recorded for it — `record_qualification_override`).
    // A login without a personnel card cannot be checked: refused when anything is required.
    if let Some(uid) = input.primary_responsible_id {
        crate::qualification::gate::enforce_user_on_work_order(&txn, uid, input.wo_id).await?;
    }

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
        "origin": if emergency { "emergency_reassign" } else { "assign" },
        "override_reason": reason,
        "assigned_team_id": input.assigned_team_id,
        "primary_responsible_id": input.primary_responsible_id,
        "skips_planning_gates": true,
        "shift_cleared": shift_cleared,
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
    if let Some(previous) = shift_cleared {
        // The work order's trail says the shift must be chosen again for the new team.
        let note = json!({
            "reason": "team_changed_in_emergency",
            "shift_to_rechoose": true,
            "previous_shift": previous,
            "assigned_team_id": input.assigned_team_id,
        })
        .to_string();
        emit_action_event(
            &txn,
            input.wo_id,
            "shift_cleared",
            Some(input.actor_id),
            Some(&from_code),
            Some(&from_code),
            Some(&note),
        )
        .await?;
    }

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    txn.commit().await?;
    Ok(wo)
}

/// Assignment (`assign_wo`) — requires override_reason; the same write as an emergency reassign,
/// except that a shift the new team does not work refuses it (the planner chooses one of the new
/// team's shifts, or none).
pub async fn save_assignment(db: &DatabaseConnection, input: WoAssignSaveInput) -> AppResult<WorkOrder> {
    reassign(
        db,
        WoEmergencyReassignInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            assigned_team_id: input.assigned_team_id,
            primary_responsible_id: input.primary_responsible_id,
            override_reason: input.override_reason,
            shift_class_version_id: None,
            clear_shift: false,
        },
        false,
    )
    .await
}
