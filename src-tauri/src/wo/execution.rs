//! WO execution transition functions.
//!
//! Option B lifecycle. Schedule/assign are non-status saves; start/hold/complete
//! drive status. Readiness gate is `mark_wo_ready` in `workflow::actions`.

use crate::errors::{AppError, AppResult};
use crate::wo::queries;
use crate::wo::time::now_utc_z;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
use uuid::Uuid;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::domain::{guard_wo_transition, WoStatus, WorkOrder};

// ═══════════════════════════════════════════════════════════════════════════════
// Input structs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
pub struct WoPlanInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub planner_id: i64,
    pub planned_start: String,
    pub planned_end: String,
    pub shift: Option<String>,
    pub expected_duration_hours: Option<f64>,
    pub urgency_id: Option<i64>,
    pub planned_downtime_hours: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoAssignInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub assigned_group_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub scheduled_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoStartInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoPauseInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub delay_reason_id: i64,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoResumeInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoHoldInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub delay_reason_id: i64,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoMechCompleteInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub actual_end: Option<String>,
    pub actual_duration_hours: Option<f64>,
    pub conclusion: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WO execution row decode error for '{field}': {e}"
    ))
}

/// Verify `rows_affected == 1`. Returns a concurrency conflict error on mismatch.
fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .to_string(),
        ]));
    }
    Ok(())
}

/// Load current WO status from the DB and parse it.
/// Returns `(current_status_code: String, parsed: WoStatus, current_row_version: i64)`.
async fn load_wo_status(
    txn: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<(String, WoStatus, i64)> {
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
        .try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;
    let row_version: i64 = row
        .try_get::<i64>("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    let status = WoStatus::try_from_str(&code)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored WO has invalid status: {e}")))?;

    Ok((code, status, row_version))
}

/// Resolve `status_id` for a given status code from `work_order_statuses`.
async fn resolve_status_id(txn: &impl ConnectionTrait, code: &str) -> AppResult<i64> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = ?",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "work_order_statuses missing row for code '{code}'"
            ))
        })?;
    row.try_get::<i64>("", "id")
        .map_err(|e| decode_err("status id", e))
}

/// Write an entry to the append-only state transition log.
async fn log_transition(
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
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log \
         (wo_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
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
        ],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) plan_wo — schedule save (no status change; Planning / Ready)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn plan_wo(db: &DatabaseConnection, input: WoPlanInput) -> AppResult<WorkOrder> {
    crate::wo::workflow::actions::save_schedule(
        db,
        crate::wo::workflow::actions::WoScheduleSaveInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            planner_id: Some(input.planner_id),
            planned_start: input.planned_start,
            planned_end: input.planned_end,
            shift: input.shift,
            expected_duration_hours: input.expected_duration_hours,
            urgency_id: input.urgency_id,
            planned_downtime_hours: input.planned_downtime_hours,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) assign_wo — assignment save (no status change; Planning / Ready)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn assign_wo(db: &DatabaseConnection, input: WoAssignInput) -> AppResult<WorkOrder> {
    crate::wo::workflow::actions::save_assignment(
        db,
        crate::wo::workflow::actions::WoAssignSaveInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            assigned_group_id: input.assigned_group_id,
            primary_responsible_id: input.primary_responsible_id,
            scheduled_at: input.scheduled_at,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) start_wo — ready → in_progress
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn start_wo(db: &DatabaseConnection, input: WoStartInput) -> AppResult<WorkOrder> {
    crate::permit::wo_gate::assert_in_progress_permit_gate(db, input.wo_id).await?;

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Start)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::InProgress)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let in_progress_status_id = resolve_status_id(&txn, "in_progress").await?;
    let now = now_utc_z();

    // Close any open delay segment
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_delay_segments SET ended_at = ? \
         WHERE work_order_id = ? AND ended_at IS NULL",
        [now.clone().into(), input.wo_id.into()],
    ))
    .await?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                actual_start = COALESCE(actual_start, ?), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                in_progress_status_id.into(),
                now.clone().into(),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "in_progress",
        "start",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    emit_action_event(
        &txn,
        input.wo_id,
        "started",
        Some(input.actor_id),
        Some(&from_code),
        Some("in_progress"),
        None,
    )
    .await?;

    txn.commit().await?;

    let wo = queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    if wo.requires_permit {
        let permit = crate::permit::queries::get_work_permit_linked_to_work_order(db, input.wo_id)
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "PTW gate passed but no linked permit for WO {}",
                    input.wo_id
                ))
            })?;
        crate::permit::wo_gate::stage_wo_in_progress_sync_pair(
            db,
            &wo,
            &permit,
            &Uuid::new_v4().to_string(),
        )
        .await?;
    }

    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) pause_wo — in_progress → on_hold (opens delay segment)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn pause_wo(db: &DatabaseConnection, input: WoPauseInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Hold)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::OnHold)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // Validate delay_reason_id resolves to WORK.DELAY_REASONS reference value
    let reason_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code AS code \
             FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rv.id = ? \
               AND rv.is_active = 1 \
               AND rs.status = 'published' \
               AND UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.DELAY_REASONS'))",
            [input.delay_reason_id.into()],
        ))
        .await?;
    let reason_code = match &reason_row {
        Some(r) => r
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("delay_reason code", e))?,
        None => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Code de délai introuvable (delay_reason_id={}).",
                input.delay_reason_id
            )]))
        }
    };

    let on_hold_status_id = resolve_status_id(&txn, "on_hold").await?;
    let now = now_utc_z();

    // Open delay segment
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_delay_segments \
         (work_order_id, started_at, ended_at, delay_reason_id, comment, entered_by_id) \
         VALUES (?, ?, NULL, ?, ?, ?)",
        [
            input.wo_id.into(),
            now.clone().into(),
            input.delay_reason_id.into(),
            input
                .comment
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input.actor_id.into(),
        ],
    ))
    .await?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                on_hold_status_id.into(),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "on_hold",
        "hold",
        input.actor_id,
        Some(&reason_code),
        input.comment.as_deref(),
        &now,
    )
    .await?;

    emit_action_event(
        &txn,
        input.wo_id,
        "held",
        Some(input.actor_id),
        Some(&from_code),
        Some("on_hold"),
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

// ═══════════════════════════════════════════════════════════════════════════════
// E) resume_wo — on_hold → in_progress (closes delay segment, recomputes waiting hours)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn resume_wo(db: &DatabaseConnection, input: WoResumeInput) -> AppResult<WorkOrder> {
    crate::permit::wo_gate::assert_in_progress_permit_gate(db, input.wo_id).await?;

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Resume)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::InProgress)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let in_progress_status_id = resolve_status_id(&txn, "in_progress").await?;
    let now = now_utc_z();

    // Close all open delay segments
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_delay_segments SET ended_at = ? \
         WHERE work_order_id = ? AND ended_at IS NULL",
        [now.clone().into(), input.wo_id.into()],
    ))
    .await?;

    // Recompute total_waiting_hours
    let waiting_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(ROUND((JULIANDAY(COALESCE(ended_at, strftime('%Y-%m-%dT%H:%M:%SZ','now'))) \
              - JULIANDAY(started_at)) * 24, 2)), 0) AS total_waiting \
             FROM work_order_delay_segments \
             WHERE work_order_id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Waiting hours query returned no row"))
        })?;
    let total_waiting: f64 = waiting_row
        .try_get::<f64>("", "total_waiting")
        .map_err(|e| decode_err("total_waiting", e))?;

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                total_waiting_hours = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                in_progress_status_id.into(),
                total_waiting.into(),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "in_progress",
        "resume",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    emit_action_event(
        &txn,
        input.wo_id,
        "resumed",
        Some(input.actor_id),
        Some(&from_code),
        Some("in_progress"),
        None,
    )
    .await?;

    txn.commit().await?;

    let wo = queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    if wo.requires_permit {
        let permit = crate::permit::queries::get_work_permit_linked_to_work_order(db, input.wo_id)
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "PTW gate passed but no linked permit for WO {}",
                    input.wo_id
                ))
            })?;
        crate::permit::wo_gate::stage_wo_in_progress_sync_pair(
            db,
            &wo,
            &permit,
            &Uuid::new_v4().to_string(),
        )
        .await?;
    }

    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) set_waiting_for_prerequisite / hold — in_progress → on_hold
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn set_waiting_for_prerequisite(
    db: &DatabaseConnection,
    input: WoHoldInput,
) -> AppResult<WorkOrder> {
    // Alias of pause/hold under Option B (single On Hold status).
    pause_wo(
        db,
        WoPauseInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            delay_reason_id: input.delay_reason_id,
            comment: input.comment,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) complete_wo_mechanically — in_progress → mechanically_complete
// ═══════════════════════════════════════════════════════════════════════════════

/// Structured completion gate for technician checklist UX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoCompletionGate {
    pub code: String,
    pub passed: bool,
    /// When false, gate is omitted from blocking (e.g. RAMS not required for PM).
    pub required: bool,
    pub detail: Option<String>,
}

async fn collect_completion_gates(
    conn: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<Vec<WoCompletionGate>> {
    let mut gates = Vec::new();

    // ── Labor: no open entries ────────────────────────────────────────────
    let open_labor_row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt \
             FROM work_order_interveners \
             WHERE work_order_id = ? \
               AND ended_at IS NULL \
               AND (hours_worked IS NULL OR hours_worked <= 0)",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Labor open-entry check returned no row"))
        })?;
    let open_labor: i64 = open_labor_row
        .try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("open_labor cnt", e))?;
    gates.push(WoCompletionGate {
        code: "OPEN_LABOR".into(),
        passed: open_labor == 0,
        required: true,
        detail: if open_labor > 0 {
            Some(format!("{open_labor} open labor entries"))
        } else {
            None
        },
    });

    // ── Tasks: mandatory completed ────────────────────────────────────────
    let incomplete_tasks = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT task_description \
             FROM work_order_tasks \
             WHERE work_order_id = ? AND is_mandatory = 1 AND is_completed = 0 \
             ORDER BY sequence_order ASC",
            [wo_id.into()],
        ))
        .await?;
    let incomplete_descs: Vec<String> = incomplete_tasks
        .iter()
        .filter_map(|r| r.try_get::<String>("", "task_description").ok())
        .collect();
    gates.push(WoCompletionGate {
        code: "TASKS".into(),
        passed: incomplete_descs.is_empty(),
        required: true,
        detail: if incomplete_descs.is_empty() {
            None
        } else {
            Some(incomplete_descs.join(", "))
        },
    });

    // ── Parts disposition ─────────────────────────────────────────────────
    let parts_gate = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                (SELECT COUNT(*) FROM work_order_parts \
                  WHERE work_order_id = ? \
                    AND COALESCE(origin, 'planned') = 'planned' \
                    AND COALESCE(consumption_status, 'pending') = 'pending' \
                    AND COALESCE(quantity_used, 0) <= 0) AS pending_planned, \
                (SELECT COUNT(*) FROM work_order_parts \
                  WHERE work_order_id = ? \
                    AND COALESCE(origin, 'planned') = 'planned') AS planned_count, \
                (SELECT COUNT(*) FROM work_order_parts \
                  WHERE work_order_id = ? \
                    AND (COALESCE(consumption_status, '') = 'used' \
                         OR COALESCE(quantity_used, 0) > 0)) AS used_count, \
                (SELECT parts_actuals_confirmed FROM work_orders WHERE id = ?) AS confirmed",
            [wo_id.into(), wo_id.into(), wo_id.into(), wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Parts gate check returned no row")))?;
    let pending_planned: i64 = parts_gate
        .try_get::<i64>("", "pending_planned")
        .map_err(|e| decode_err("pending_planned", e))?;
    let planned_count: i64 = parts_gate
        .try_get::<i64>("", "planned_count")
        .map_err(|e| decode_err("planned_count", e))?;
    let used_count: i64 = parts_gate
        .try_get::<i64>("", "used_count")
        .map_err(|e| decode_err("used_count", e))?;
    let parts_confirmed: i64 = parts_gate
        .try_get::<i64>("", "confirmed")
        .map_err(|e| decode_err("parts_actuals_confirmed", e))?;
    let parts_ok = if pending_planned > 0 {
        false
    } else if planned_count == 0 && used_count == 0 && parts_confirmed == 0 {
        false
    } else {
        true
    };
    gates.push(WoCompletionGate {
        code: "PARTS".into(),
        passed: parts_ok,
        required: true,
        detail: if pending_planned > 0 {
            Some(format!("{pending_planned} planned lines pending disposition"))
        } else if !parts_ok {
            Some("parts actuals not confirmed".into())
        } else {
            None
        },
    });

    // ── Downtime open segments ────────────────────────────────────────────
    let open_dt_row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt \
             FROM work_order_downtime_segments \
             WHERE work_order_id = ? AND ended_at IS NULL",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Downtime gate check returned no row"))
        })?;
    let open_dt: i64 = open_dt_row
        .try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("open_dt cnt", e))?;
    gates.push(WoCompletionGate {
        code: "OPEN_DOWNTIME".into(),
        passed: open_dt == 0,
        required: true,
        detail: if open_dt > 0 {
            Some(format!("{open_dt} open downtime segments"))
        } else {
            None
        },
    });

    // ── RAMS (corrective / emergency only) ────────────────────────────────
    let wo_type_row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wot.code AS type_code, wo.root_cause_summary \
             FROM work_orders wo \
             JOIN work_order_types wot ON wot.id = wo.type_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let type_code: String = wo_type_row
        .try_get("", "type_code")
        .map_err(|e| decode_err("type_code", e))?;
    let rams_required = matches!(type_code.as_str(), "corrective" | "emergency");

    let mut symptom_ok = true;
    let mut failure_mode_ok = true;
    let mut root_cause_ok = true;
    if rams_required {
        let fd_row = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT symptom_id, failure_mode_id, cause_not_determined \
                 FROM work_order_failure_details WHERE work_order_id = ?",
                [wo_id.into()],
            ))
            .await?;
        match fd_row {
            Some(r) => {
                let symptom_id: Option<i64> = r
                    .try_get("", "symptom_id")
                    .map_err(|e| decode_err("symptom_id", e))?;
                let failure_mode_id: Option<i64> = r
                    .try_get("", "failure_mode_id")
                    .map_err(|e| decode_err("failure_mode_id", e))?;
                let cnd: bool = r
                    .try_get::<i64>("", "cause_not_determined")
                    .map_err(|e| decode_err("cause_not_determined", e))?
                    != 0;
                symptom_ok = symptom_id.is_some();
                failure_mode_ok = failure_mode_id.is_some() || cnd;
            }
            None => {
                symptom_ok = false;
                failure_mode_ok = false;
            }
        }
        let root_cause: Option<String> = wo_type_row
            .try_get("", "root_cause_summary")
            .map_err(|e| decode_err("root_cause_summary", e))?;
        root_cause_ok = !root_cause
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty();
    }

    gates.push(WoCompletionGate {
        code: "FAILURE_MODE".into(),
        passed: failure_mode_ok,
        required: rams_required,
        detail: None,
    });
    gates.push(WoCompletionGate {
        code: "SYMPTOM".into(),
        passed: symptom_ok,
        required: rams_required,
        detail: None,
    });
    gates.push(WoCompletionGate {
        code: "ROOT_CAUSE".into(),
        passed: root_cause_ok,
        required: rams_required,
        detail: None,
    });

    Ok(gates)
}

/// Public evaluate API for Completion / Closeout progress checklists.
pub async fn evaluate_completion_gates(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<Vec<WoCompletionGate>> {
    collect_completion_gates(db, wo_id).await
}

pub async fn complete_wo_mechanically(
    db: &DatabaseConnection,
    input: WoMechCompleteInput,
) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Complete)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::Completed)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let gates = collect_completion_gates(&txn, input.wo_id).await?;
    let blocking: Vec<String> = gates
        .iter()
        .filter(|g| g.required && !g.passed)
        .map(|g| match g.code.as_str() {
            "OPEN_LABOR" => {
                "Les entrées de main-d'œuvre ouvertes doivent être clôturées avant la complétion. (Open labor entries must be closed first.)".into()
            }
            "TASKS" => format!(
                "Tâches obligatoires non terminées{}. (Mandatory tasks incomplete.)",
                g.detail
                    .as_ref()
                    .map(|d| format!(" : {d}"))
                    .unwrap_or_default()
            ),
            "PARTS" => {
                if g.detail
                    .as_deref()
                    .unwrap_or("")
                    .contains("pending disposition")
                {
                    "Toutes les pièces planifiées doivent être marquées Utilisée ou Non utilisée avant complétion. \
(All planned parts must be marked Used or Not used before completion.)".into()
                } else {
                    "Réels des pièces non confirmés. Ajoutez une pièce consommée ou marquez « aucune pièce utilisée ». \
(Parts actuals not confirmed. Add a consumed part or mark none used.)".into()
                }
            }
            "OPEN_DOWNTIME" => {
                "Les segments de temps d'arrêt ouverts doivent être clôturés avant la complétion. (Open downtime segments must be closed before completion.)".into()
            }
            "SYMPTOM" => {
                "Symptôme requis avant complétion mécanique (RAMS data quality).".into()
            }
            "FAILURE_MODE" => {
                "Mode de défaillance requis avant complétion mécanique (RAMS data quality).".into()
            }
            "ROOT_CAUSE" => {
                "Résumé de cause racine requis avant complétion mécanique.".into()
            }
            other => format!("Completion gate failed: {other}"),
        })
        .collect();

    if !blocking.is_empty() {
        txn.rollback().await?;
        return Err(AppError::ValidationFailed(blocking));
    }

    // ── Post-check: recompute active_labor_hours ──────────────────────────
    let labor_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(COALESCE(hours_worked, 0.0)), 0.0) AS total_labor \
             FROM work_order_interveners \
             WHERE work_order_id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Labor sum query returned no row"))
        })?;
    let total_labor: f64 = labor_row
        .try_get::<f64>("", "total_labor")
        .map_err(|e| decode_err("total_labor", e))?;

    let completed_status_id = resolve_status_id(&txn, "completed").await?;
    let now = now_utc_z();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                mechanically_completed_at = ?, \
                active_labor_hours = ?, \
                actual_end = COALESCE(?, actual_end), \
                actual_duration_hours = COALESCE(?, actual_duration_hours), \
                conclusion = COALESCE(?, conclusion), \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                completed_status_id.into(),
                now.clone().into(),
                total_labor.into(),
                input.actual_end.clone().map(|v| v.into()).unwrap_or(sea_orm::Value::String(None)),
                input.actual_duration_hours.map(|v| v.into()).unwrap_or(sea_orm::Value::Double(None)),
                input.conclusion.clone().map(|v| v.into()).unwrap_or(sea_orm::Value::String(None)),
                now.clone().into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    log_transition(
        &txn,
        input.wo_id,
        &from_code,
        "completed",
        "complete",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    emit_action_event(
        &txn,
        input.wo_id,
        "completed",
        Some(input.actor_id),
        Some(&from_code),
        Some("completed"),
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
