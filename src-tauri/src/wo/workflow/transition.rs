//! Apply status transitions + transition log.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::wo::time::now_utc_z;
use crate::wo::workflow::state_machine::{guard_wo_transition, WoStatus};

pub async fn resolve_status_id(txn: &impl ConnectionTrait, code: &str) -> AppResult<i64> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = ?",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("work_order_statuses missing row for code '{code}'")))?;
    row.try_get::<i64>("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("status id decode: {e}")))
}

pub async fn load_wo_status(txn: &impl ConnectionTrait, wo_id: i64) -> AppResult<(String, WoStatus, i64)> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code, wo.row_version \
             FROM work_orders wo \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    let code: String = row
        .try_get("", "status_code")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("status_code: {e}")))?;
    let row_version: i64 = row
        .try_get("", "row_version")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("row_version: {e}")))?;
    let status = WoStatus::try_from_str(&code)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored WO has invalid status: {e}")))?;
    Ok((code, status, row_version))
}

/// Writes a transition to the append-only log, after the WO row was updated (same transaction).
///
/// Entering `planning` records whether this WO needs planning approval, from the tenant setting
/// at this moment (`wo::lifecycle`: the lifecycle depends only on the WO's own facts, so a later
/// change of the setting only affects WOs that enter planning afterwards). The log row carries
/// the derived lifecycle state the WO is in after the transition.
pub async fn log_transition(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    from_status: &str,
    to_status: &str,
    action: &str,
    actor_id: i64,
    reason_code: Option<&str>,
    notes: Option<&str>,
    acted_at: &str,
) -> AppResult<()> {
    log_transition_with_remaining(
        txn,
        wo_id,
        from_status,
        to_status,
        action,
        actor_id,
        reason_code,
        notes,
        acted_at,
        None,
    )
    .await
}

/// [`log_transition`] of a resume or a reopening to in progress, with the estimated remaining time
/// the user entered for the permit gate (`permit::wo_gate`; `None`: the full expected duration).
/// The row is the record of who entered it, when, and the value (migration 242); it travels with
/// the synchronised log, so another PC judges the arrival on the same value.
pub async fn log_transition_with_remaining(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    from_status: &str,
    to_status: &str,
    action: &str,
    actor_id: i64,
    reason_code: Option<&str>,
    notes: Option<&str>,
    acted_at: &str,
    estimated_remaining_hours: Option<f64>,
) -> AppResult<()> {
    insert_transition(
        txn,
        wo_id,
        from_status,
        to_status,
        action,
        Some(actor_id),
        reason_code,
        notes,
        acted_at,
        estimated_remaining_hours,
    )
    .await
}

/// [`log_transition`] of a change the application makes by itself, no person acting (its
/// `actor_id` is empty): a running work order put on hold because its permit ended on another PC
/// (`wo::execution::hold_for_permit_ended_elsewhere`).
pub async fn log_system_transition(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    from_status: &str,
    to_status: &str,
    action: &str,
    reason_code: Option<&str>,
    notes: Option<&str>,
    acted_at: &str,
) -> AppResult<()> {
    insert_transition(
        txn,
        wo_id,
        from_status,
        to_status,
        action,
        None,
        reason_code,
        notes,
        acted_at,
        None,
    )
    .await
}

async fn insert_transition(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    from_status: &str,
    to_status: &str,
    action: &str,
    actor_id: Option<i64>,
    reason_code: Option<&str>,
    notes: Option<&str>,
    acted_at: &str,
    estimated_remaining_hours: Option<f64>,
) -> AppResult<()> {
    if to_status == WoStatus::Planning.as_str() {
        let required = crate::wo::workflow::readiness::load_approval_policy(txn).await?;
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET planning_approval_required = ? WHERE id = ?",
            [i64::from(required).into(), wo_id.into()],
        ))
        .await?;
    }
    let lifecycle = crate::wo::lifecycle::lifecycle_of(txn, wo_id).await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log \
         (wo_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at, lifecycle_state, \
          estimated_remaining_hours) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            wo_id.into(),
            from_status.into(),
            to_status.into(),
            action.into(),
            actor_id.into(),
            reason_code
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            acted_at.into(),
            lifecycle.as_str().into(),
            estimated_remaining_hours.into(),
        ],
    ))
    .await?;
    Ok(())
}

pub fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .to_string(),
        ]));
    }
    Ok(())
}

/// Guard transition, resolve target status id, update WO, write transition log.
pub async fn apply_status_transition(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    expected_row_version: i64,
    actor_id: i64,
    to: WoStatus,
    action: &str,
    reason_code: Option<&str>,
    notes: Option<&str>,
    extra_sets: &str,
) -> AppResult<String> {
    let (from_code, current, _) = load_wo_status(txn, wo_id).await?;
    guard_wo_transition(&current, &to).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let to_id = resolve_status_id(txn, to.as_str()).await?;
    let now = now_utc_z();
    let extra = if extra_sets.is_empty() {
        String::new()
    } else {
        format!(", {extra_sets}")
    };

    let sql = format!(
        "UPDATE work_orders SET status_id = ?, row_version = row_version + 1, updated_at = ?{extra} \
         WHERE id = ? AND row_version = ?"
    );
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [
                to_id.into(),
                now.clone().into(),
                wo_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        txn,
        wo_id,
        &from_code,
        to.as_str(),
        action,
        actor_id,
        reason_code,
        notes,
        &now,
    )
    .await?;

    Ok(now)
}
