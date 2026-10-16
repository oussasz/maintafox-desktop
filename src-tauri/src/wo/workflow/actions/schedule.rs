//! Schedule / job-prep save (Planning + Ready) — no status change.
//!
//! When a Planning commitment exists, planned_start/planned_end are SSOT there
//! and must not be overwritten from the WO Plan tab.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::queries;
use crate::wo::time::{now_utc_z, parse_utc_timestamp};
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction, WoStatus};
use crate::wo::workflow::transition::{check_concurrency, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoScheduleSaveInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub planner_id: Option<i64>,
    /// Optional; rejected when they would overwrite a committed slot.
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    /// Old fixed shift code: refused (`domain::validate_wo_shift`); use `shift_class_version_id`.
    pub shift: Option<String>,
    /// A shift window of the team's Classe horaire on the planned day (`wo::shifts`); `None`
    /// keeps the current one.
    #[serde(default)]
    pub shift_class_version_id: Option<i64>,
    /// Removes the shift (no window).
    #[serde(default)]
    pub clear_shift: bool,
    pub expected_duration_hours: Option<f64>,
    /// People the job takes; `None` keeps the current one.
    #[serde(default)]
    pub planned_crew_size: Option<i64>,
    /// Removes the crew size (the job is then not estimated for planning).
    #[serde(default)]
    pub clear_crew_size: bool,
    /// Level of the shared DI / OT urgency scale (`emergency` … `low`); `None` keeps the current one.
    pub urgency_code: Option<String>,
    pub planned_downtime_hours: Option<f64>,
}

async fn has_schedule_commitment(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS present
             FROM schedule_commitments
             WHERE source_type = 'work_order' AND source_id = ?
             LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    Ok(row.is_some())
}

pub async fn save_schedule(db: &DatabaseConnection, input: WoScheduleSaveInput) -> AppResult<WorkOrder> {
    crate::wo::domain::validate_wo_shift(input.shift.as_deref())?;
    let committed = has_schedule_commitment(db, input.wo_id).await?;

    let start_raw = input.planned_start.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let end_raw = input.planned_end.as_deref().map(str::trim).filter(|s| !s.is_empty());

    if committed && (start_raw.is_some() || end_raw.is_some()) {
        // Allow only if both match current WO dates (idempotent job-prep save).
        let current = queries::get_work_order(db, input.wo_id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: input.wo_id.to_string(),
            })?;
        let same_start = start_raw
            .map(|s| current.planned_start.as_deref() == Some(s))
            .unwrap_or(true);
        let same_end = end_raw
            .map(|s| current.planned_end.as_deref() == Some(s))
            .unwrap_or(true);
        if !same_start || !same_end {
            return Err(AppError::ValidationFailed(vec![
                "Committed schedule dates are owned by Planning. Open in Planning to reschedule — WO Plan cannot overwrite them."
                    .to_string(),
            ]));
        }
    }

    if let (Some(start_s), Some(end_s)) = (start_raw, end_raw) {
        let start = parse_utc_timestamp(start_s, "planned_start")?;
        let end = parse_utc_timestamp(end_s, "planned_end")?;
        if end < start {
            return Err(AppError::ValidationFailed(vec![
                "planned_end must be >= planned_start.".to_string(),
            ]));
        }
    } else if start_raw.is_some() ^ end_raw.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Provide both planned_start and planned_end, or neither.".to_string(),
        ]));
    }

    crate::planning::load::validate_crew_size(input.planned_crew_size)?;

    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    let urgency_id = match input.urgency_code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(code) => Some(crate::reference::urgency_scale::urgency_value_id(&txn, code).await?),
        None => None,
    };

    let action = match status {
        WoStatus::Planning => WoAction::EditSchedule,
        WoStatus::Ready => WoAction::Reschedule,
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Job prep schedule fields can only be saved in status planning or ready (current: {}).",
                status.as_str()
            )]));
        }
    };
    assert_action_allowed(&status, action).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = now_utc_z();
    // When committed (or no dates sent), preserve existing planned_start/end.
    let update_dates = !committed && start_raw.is_some() && end_raw.is_some();

    // The shift: a window of the team's Classe horaire on the planned day, or none. Decided on
    // the work order as it stands (before the new dates are written).
    let shift_change = crate::wo::shifts::decide_shift(
        &txn,
        input.wo_id,
        None,
        if update_dates { start_raw } else { None },
        input.shift_class_version_id,
        input.clear_shift,
    )
    .await?;

    let result = if update_dates {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                planner_id = COALESCE(?, planner_id), \
                planned_start = ?, \
                planned_end = ?, \
                shift = COALESCE(?, shift), \
                urgency_id = COALESCE(?, urgency_id), \
                expected_duration_hours = COALESCE(?, expected_duration_hours), \
                planned_crew_size = CASE WHEN ? = 1 THEN NULL ELSE COALESCE(?, planned_crew_size) END, \
                planned_downtime_hours = COALESCE(?, planned_downtime_hours), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input
                    .planner_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                start_raw.unwrap().to_string().into(),
                end_raw.unwrap().to_string().into(),
                input
                    .shift
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                urgency_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .expected_duration_hours
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                i64::from(input.clear_crew_size).into(),
                input
                    .planned_crew_size
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .planned_downtime_hours
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?
    } else {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                planner_id = COALESCE(?, planner_id), \
                shift = COALESCE(?, shift), \
                urgency_id = COALESCE(?, urgency_id), \
                expected_duration_hours = COALESCE(?, expected_duration_hours), \
                planned_crew_size = CASE WHEN ? = 1 THEN NULL ELSE COALESCE(?, planned_crew_size) END, \
                planned_downtime_hours = COALESCE(?, planned_downtime_hours), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input
                    .planner_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .shift
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                urgency_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .expected_duration_hours
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                i64::from(input.clear_crew_size).into(),
                input
                    .planned_crew_size
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .planned_downtime_hours
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?
    };
    check_concurrency(result.rows_affected())?;
    crate::wo::shifts::apply_shift_change(&txn, input.wo_id, &shift_change).await?;

    emit_action_event(
        &txn,
        input.wo_id,
        if update_dates {
            "schedule_changed"
        } else {
            "job_prep_saved"
        },
        Some(input.actor_id),
        Some(&from_code),
        Some(&from_code),
        None,
    )
    .await?;

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    txn.commit().await?;
    Ok(wo)
}
