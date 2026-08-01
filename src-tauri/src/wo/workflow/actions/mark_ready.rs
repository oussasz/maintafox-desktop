//! Mark Ready: Planning → Ready (all blocking readiness rules Pass).

use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::queries;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::readiness::assert_ready_to_mark;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction, WoStatus};
use crate::wo::workflow::transition::{apply_status_transition, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoMarkReadyInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
}

pub async fn mark_wo_ready(db: &DatabaseConnection, input: WoMarkReadyInput) -> AppResult<WorkOrder> {
    assert_ready_to_mark(db, input.wo_id).await?;

    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&status, WoAction::MarkReady).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    apply_status_transition(
        &txn,
        input.wo_id,
        input.expected_row_version,
        input.actor_id,
        WoStatus::Ready,
        WoAction::MarkReady.as_str(),
        None,
        None,
        "",
    )
    .await?;

    emit_action_event(
        &txn,
        input.wo_id,
        "marked_ready",
        Some(input.actor_id),
        Some(&from_code),
        Some(WoStatus::Ready.as_str()),
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
