//! Submit: Draft → Planning.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::wo::queries;
use crate::wo::domain::WorkOrder;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction, WoStatus};
use crate::wo::workflow::transition::{apply_status_transition, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoSubmitInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
}

pub async fn submit_wo(db: &DatabaseConnection, input: WoSubmitInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&status, WoAction::Submit)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // Minimum identification for Submit
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT title, type_id, equipment_id, entity_id FROM work_orders WHERE id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    let mut errors = Vec::new();
    let title: String = row.try_get("", "title").unwrap_or_default();
    if title.trim().is_empty() {
        errors.push("Titre obligatoire pour soumettre l'OT.".into());
    }
    let type_id: i64 = row.try_get("", "type_id").unwrap_or(0);
    if type_id <= 0 {
        errors.push("Type d'OT obligatoire pour soumettre.".into());
    }
    let equipment_id: Option<i64> = row.try_get("", "equipment_id").ok().flatten();
    let entity_id: Option<i64> = row.try_get("", "entity_id").ok().flatten();
    if equipment_id.is_none() && entity_id.is_none() {
        errors.push("Équipement ou entité obligatoire pour soumettre.".into());
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    apply_status_transition(
        &txn,
        input.wo_id,
        input.expected_row_version,
        input.actor_id,
        WoStatus::Planning,
        WoAction::Submit.as_str(),
        None,
        None,
        "",
    )
    .await?;

    emit_action_event(
        &txn,
        input.wo_id,
        "submitted",
        Some(input.actor_id),
        Some(&from_code),
        Some(WoStatus::Planning.as_str()),
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
