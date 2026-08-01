//! Assignment save (Planning + Ready) — no status change.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;

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
    pub assigned_group_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub scheduled_at: Option<String>,
}

pub async fn save_assignment(
    db: &DatabaseConnection,
    input: WoAssignSaveInput,
) -> AppResult<WorkOrder> {
    if input.assigned_group_id.is_none() && input.primary_responsible_id.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Au moins un champ parmi assigned_group_id ou primary_responsible_id est obligatoire."
                .to_string(),
        ]));
    }

    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;

    let action = match status {
        WoStatus::Planning => WoAction::EditAssignment,
        WoStatus::Ready => WoAction::Reassign,
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "L'affectation n'est autorisée qu'en statut planning ou ready (actuel: {}).",
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
                assigned_group_id = COALESCE(?, assigned_group_id), \
                primary_responsible_id = COALESCE(?, primary_responsible_id), \
                scheduled_at = COALESCE(?, scheduled_at), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                input
                    .assigned_group_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .primary_responsible_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .scheduled_at
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let event = if matches!(status, WoStatus::Ready) {
        "reassigned"
    } else {
        "assigned"
    };
    emit_action_event(
        &txn,
        input.wo_id,
        event,
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
