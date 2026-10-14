//! Permit / LOTO requirement on a work order — no status change.
//!
//! `requires_permit` drives the start gate (`permit::wo_gate`): a WO that needs
//! a permit cannot go in progress without an active, activation-ready permit.
//! The flag can be set or cleared only before execution starts (draft, planning,
//! ready); once work is under way it is frozen so the gate cannot be bypassed.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::queries;
use crate::wo::time::now_utc_z;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::WoStatus;
use crate::wo::workflow::transition::{check_concurrency, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoPermitRequirementInput {
    pub wo_id: i64,
    pub expected_row_version: i64,
    pub requires_permit: bool,
}

/// Statuses in which the permit requirement may still change.
pub fn permit_requirement_editable(status: &WoStatus) -> bool {
    matches!(status, WoStatus::Draft | WoStatus::Planning | WoStatus::Ready)
}

pub async fn set_permit_requirement(
    db: &DatabaseConnection,
    input: WoPermitRequirementInput,
    actor_id: i64,
) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    if !permit_requirement_editable(&status) {
        return Err(AppError::ValidationFailed(vec![
            "L'exigence de permis / consignation ne peut plus être modifiée une fois l'exécution commencée."
                .to_string(),
        ]));
    }

    let now = now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                requires_permit = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                i64::from(input.requires_permit).into(),
                now.into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let payload = serde_json::json!({ "requires_permit": input.requires_permit }).to_string();
    emit_action_event(
        &txn,
        input.wo_id,
        "permit_requirement_changed",
        Some(actor_id),
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
    txn.commit().await?;
    Ok(wo)
}
