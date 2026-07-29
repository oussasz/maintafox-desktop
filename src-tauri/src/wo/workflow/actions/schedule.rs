//! Schedule save (Planning + Ready) — no status change.

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
    pub planned_start: String,
    pub planned_end: String,
    pub shift: Option<String>,
    pub expected_duration_hours: Option<f64>,
    pub urgency_id: Option<i64>,
    pub planned_downtime_hours: Option<f64>,
}

pub async fn save_schedule(
    db: &DatabaseConnection,
    input: WoScheduleSaveInput,
) -> AppResult<WorkOrder> {
    let start = parse_utc_timestamp(&input.planned_start, "planned_start")?;
    let end = parse_utc_timestamp(&input.planned_end, "planned_end")?;
    if end < start {
        return Err(AppError::ValidationFailed(vec![
            "planned_end doit être >= planned_start.".to_string(),
        ]));
    }

    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;

    let action = match status {
        WoStatus::Planning => WoAction::EditSchedule,
        WoStatus::Ready => WoAction::Reschedule,
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Le planning ne peut être modifié qu'en statut planning ou ready (actuel: {}).",
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
                planner_id = COALESCE(?, planner_id), \
                planned_start = ?, \
                planned_end = ?, \
                shift = COALESCE(?, shift), \
                urgency_id = COALESCE(?, urgency_id), \
                expected_duration_hours = COALESCE(?, expected_duration_hours), \
                planned_downtime_hours = COALESCE(?, planned_downtime_hours), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input
                    .planner_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
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
                input
                    .planned_downtime_hours
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    emit_action_event(
        &txn,
        input.wo_id,
        "schedule_changed",
        Some(input.actor_id),
        Some(&from_code),
        Some(&from_code),
        None,
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
