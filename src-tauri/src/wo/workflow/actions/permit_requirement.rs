//! Permit / LOTO requirement on a work order — no status change.
//!
//! `requires_permit` drives the start gate (`permit::wo_gate`): a WO that needs
//! a permit cannot go in progress without an active, activation-ready permit.
//! The flag can be set or cleared only before execution starts (draft, planning,
//! ready); once work is under way it is frozen so the gate cannot be bypassed.
//!
//! A flag a plant permit rule set when the work order was converted from a request
//! (`permit::requirement_rules`, `work_orders.permit_rule_code`) is not cleared by the general
//! edit: [`clear_permit_rule_flag`] needs the permit-issue right (`ptw.issue`) and a reason of 10
//! characters or more, and records who, when and why on the work order (audited).

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
    if !input.requires_permit {
        assert_not_rule_flagged(&txn, input.wo_id).await?;
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

/// Stable codes of the rule flag.
pub const CODE_RULE_FLAG_NEEDS_CLEAR: &str = "WO_PERMIT_RULE_FLAG_NEEDS_CLEAR";
pub const CODE_RULE_FLAG_REASON: &str = "WO_PERMIT_RULE_CLEAR_REASON_REQUIRED";
pub const CODE_RULE_FLAG_ABSENT: &str = "WO_PERMIT_RULE_FLAG_ABSENT";
pub const CODE_RULE_FLAG_RIGHT: &str = "WO_PERMIT_RULE_CLEAR_RIGHT_REQUIRED";

/// Shortest reason accepted to clear a rule flag (the audit trail's "why").
pub const MIN_CLEAR_REASON_CHARS: usize = 10;

/// The work order's permit flag was set by a plant rule and has not been cleared: the general
/// edit may not clear it.
pub async fn assert_not_rule_flagged(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT permit_rule_code FROM work_orders \
              WHERE id = ? AND requires_permit = 1 AND permit_rule_code IS NOT NULL \
                AND permit_rule_cleared_at IS NULL",
            [wo_id.into()],
        ))
        .await?;
    if let Some(row) = row {
        let rule: String = row.try_get("", "permit_rule_code")?;
        return Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_NEEDS_CLEAR,
            format!(
                "The permit requirement was set by the plant rule '{rule}': clearing it needs the \
                 permit-issue right and a reason."
            ),
            crate::errors::issue_params(&[("rule", rule)]),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoPermitRuleClearInput {
    pub wo_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
}

/// Clear the permit flag a plant rule set at conversion. The person clearing it holds the
/// permit-issue right (`ptw.issue`) — checked here as well as by the command, so converting or
/// editing work orders is never enough. Allowed before execution starts, like any permit
/// requirement change; records who, when and why on the work order and in its action history.
pub async fn clear_permit_rule_flag(
    db: &DatabaseConnection,
    input: WoPermitRuleClearInput,
    actor_id: i64,
) -> AppResult<WorkOrder> {
    let reason = input.reason.trim().to_string();
    if reason.chars().count() < MIN_CLEAR_REASON_CHARS {
        return Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_REASON,
            format!("Give a reason of at least {MIN_CLEAR_REASON_CHARS} characters."),
            crate::errors::issue_params(&[("min", MIN_CLEAR_REASON_CHARS.to_string())]),
        ));
    }
    let txn = db.begin().await?;
    if !crate::settings::site::holds_tenant_right(&txn, actor_id, crate::rbac::permissions::PTW_ISSUE).await? {
        return Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_RIGHT,
            "Clearing a permit requirement set by a plant rule needs the permit-issue right.".to_string(),
            std::collections::HashMap::new(),
        ));
    }
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    if !permit_requirement_editable(&status) {
        return Err(AppError::ValidationFailed(vec![
            "L'exigence de permis / consignation ne peut plus être modifiée une fois l'exécution commencée."
                .to_string(),
        ]));
    }
    let flagged = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT permit_rule_code FROM work_orders \
              WHERE id = ? AND requires_permit = 1 AND permit_rule_code IS NOT NULL \
                AND permit_rule_cleared_at IS NULL",
            [input.wo_id.into()],
        ))
        .await?;
    let Some(flagged) = flagged else {
        return Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_ABSENT,
            "This work order carries no permit flag set by a plant rule.".to_string(),
            std::collections::HashMap::new(),
        ));
    };
    let rule: String = flagged.try_get("", "permit_rule_code")?;

    let now = now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                requires_permit = 0, \
                permit_rule_cleared_by_id = ?, \
                permit_rule_cleared_at = ?, \
                permit_rule_clear_reason = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                actor_id.into(),
                now.clone().into(),
                reason.clone().into(),
                now.into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let payload = serde_json::json!({ "requires_permit": false, "rule": rule, "reason": reason }).to_string();
    emit_action_event(
        &txn,
        input.wo_id,
        "permit_rule_flag_cleared",
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
