//! Approve planning — records approval for approval_required readiness rule.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::queries;
use crate::wo::time::now_utc_z;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
use crate::wo::workflow::transition::{check_concurrency, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoApprovePlanningInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
}

pub async fn approve_planning(db: &DatabaseConnection, input: WoApprovePlanningInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&status, WoAction::ApprovePlanning).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let now = now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                planning_approved_at = ?, \
                planning_approved_by_id = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                input.actor_id.into(),
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
        "planning_approved",
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
