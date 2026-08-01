//! Return to Planning from Ready (plan-substance demotion).

use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::queries;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction, WoStatus};
use crate::wo::workflow::transition::{apply_status_transition, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoReturnToPlanningInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub reason: Option<String>,
}

pub async fn return_to_planning(db: &DatabaseConnection, input: WoReturnToPlanningInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&status, WoAction::ReturnToPlanning).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    apply_status_transition(
        &txn,
        input.wo_id,
        input.expected_row_version,
        input.actor_id,
        WoStatus::Planning,
        WoAction::ReturnToPlanning.as_str(),
        None,
        input.reason.as_deref(),
        "",
    )
    .await?;

    emit_action_event(
        &txn,
        input.wo_id,
        "returned_to_planning",
        Some(input.actor_id),
        Some(&from_code),
        Some(WoStatus::Planning.as_str()),
        input.reason.as_deref(),
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
