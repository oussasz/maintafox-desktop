//! WO execution transition functions.
//!
//! Option B lifecycle. Schedule/assign are non-status saves; start/hold/complete
//! drive status. Readiness gate is `mark_wo_ready` in `workflow::actions`.

use crate::errors::{AppError, AppResult};
use crate::wo::queries;
use crate::wo::time::{now_utc_second, now_utc_z, stamp};
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
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
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    /// Old fixed shift code: refused; use `shift_class_version_id` (`wo::shifts`).
    pub shift: Option<String>,
    /// A shift window of the team's Classe horaire on the planned day; `None` keeps the current.
    #[serde(default)]
    pub shift_class_version_id: Option<i64>,
    /// Removes the shift.
    #[serde(default)]
    pub clear_shift: bool,
    pub expected_duration_hours: Option<f64>,
    /// People the job takes; `None` keeps the current one.
    #[serde(default)]
    pub planned_crew_size: Option<i64>,
    /// Removes the crew size.
    #[serde(default)]
    pub clear_crew_size: bool,
    /// Level of the shared DI / OT urgency scale (`emergency` … `low`).
    #[serde(default)]
    pub urgency_code: Option<String>,
    pub planned_downtime_hours: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoAssignInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub assigned_team_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub scheduled_at: Option<String>,
    pub override_reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoEmergencyReassignInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub assigned_team_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    pub override_reason: String,
    /// The new team's shift window; `None` keeps the current one (the new team must work it).
    #[serde(default)]
    pub shift_class_version_id: Option<i64>,
    /// Removes the shift.
    #[serde(default)]
    pub clear_shift: bool,
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
    /// Estimated remaining time (hours, > 0) for the permit gate instead of the full expected
    /// duration (`permit::wo_gate`); recorded on the resume's transition-log row, never written
    /// over the work order's planned or expected duration. Absent: the full expected duration.
    #[serde(default)]
    pub estimated_remaining_hours: Option<f64>,
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
    AppError::Internal(anyhow::anyhow!("WO execution row decode error for '{field}': {e}"))
}

/// Verify `rows_affected == 1`. Returns a concurrency conflict error on mismatch.
fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .to_string(),
        ]));
    }
    Ok(())
}

/// Load current WO status from the DB and parse it.
/// Returns `(current_status_code: String, parsed: WoStatus, current_row_version: i64)`.
async fn load_wo_status(txn: &impl ConnectionTrait, wo_id: i64) -> AppResult<(String, WoStatus, i64)> {
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("work_order_statuses missing row for code '{code}'")))?;
    row.try_get::<i64>("", "id").map_err(|e| decode_err("status id", e))
}

/// Write an entry to the append-only state transition log (the shared writer also records the
/// derived lifecycle state and the planning-approval fact).
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
    crate::wo::workflow::transition::log_transition(
        txn,
        wo_id,
        from_status,
        to_status,
        action,
        actor_id,
        reason_code,
        notes,
        acted_at,
    )
    .await
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
            shift_class_version_id: input.shift_class_version_id,
            clear_shift: input.clear_shift,
            expected_duration_hours: input.expected_duration_hours,
            planned_crew_size: input.planned_crew_size,
            clear_crew_size: input.clear_crew_size,
            urgency_code: input.urgency_code,
            planned_downtime_hours: input.planned_downtime_hours,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) assign_wo / emergency_reassign_wo — assignment save (no status change)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn assign_wo(db: &DatabaseConnection, input: WoAssignInput) -> AppResult<WorkOrder> {
    crate::wo::workflow::actions::save_assignment(
        db,
        crate::wo::workflow::actions::WoAssignSaveInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            assigned_team_id: input.assigned_team_id,
            primary_responsible_id: input.primary_responsible_id,
            scheduled_at: input.scheduled_at,
            override_reason: input.override_reason,
        },
    )
    .await
}

pub async fn emergency_reassign_wo(db: &DatabaseConnection, input: WoEmergencyReassignInput) -> AppResult<WorkOrder> {
    crate::wo::workflow::actions::emergency_reassign(
        db,
        crate::wo::workflow::actions::WoEmergencyReassignInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            assigned_team_id: input.assigned_team_id,
            primary_responsible_id: input.primary_responsible_id,
            override_reason: input.override_reason,
            shift_class_version_id: input.shift_class_version_id,
            clear_shift: input.clear_shift,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) start_wo — ready → in_progress
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn start_wo(db: &DatabaseConnection, input: WoStartInput) -> AppResult<WorkOrder> {
    // One clock read: the moment judged by the permit gate is the start recorded below.
    let at = now_utc_second();
    let txn = db.begin().await?;
    // Permit gate in the same transaction as the status change (no window in between).
    crate::permit::wo_gate::assert_in_progress_permit_gate_with(&txn, input.wo_id, None, at).await?;
    let linked_permit = crate::permit::queries::get_work_permit_linked_to_work_order(&txn, input.wo_id).await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Start).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::InProgress).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let in_progress_status_id = resolve_status_id(&txn, "in_progress").await?;
    let now = stamp(at);

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

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    if wo.requires_permit {
        linked_permit.ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "PTW gate passed but no linked permit for WO {}",
                input.wo_id
            ))
        })?;
    }

    txn.commit().await?;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) pause_wo — in_progress → on_hold (opens delay segment)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn pause_wo(db: &DatabaseConnection, input: WoPauseInput) -> AppResult<WorkOrder> {
    hold_wo(db, input, crate::wo::lifecycle::HoldKind::Pause).await
}

/// in_progress → on_hold, opening a delay segment of the given kind (a pause, or waiting for a
/// prerequisite: parts, permit, access…). The kind decides the PRD lifecycle state
/// (`paused` / `waiting_for_prerequisite`, `wo::lifecycle`).
async fn hold_wo(
    db: &DatabaseConnection,
    input: WoPauseInput,
    hold_kind: crate::wo::lifecycle::HoldKind,
) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    let (_, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Hold).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::OnHold).map_err(|e| AppError::ValidationFailed(vec![e]))?;

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
                "GATE_WO_DELAY_REASON_UNKNOWN: Code de délai introuvable (delay_reason_id={}).",
                input.delay_reason_id
            )]))
        }
    };

    let wo = hold_wo_in(&txn, &input, &reason_code, hold_kind).await?;
    txn.commit().await?;
    Ok(wo)
}

/// The fixed delay reason of a work order put on hold by the suspension of its permit (owner
/// decision D5). A value of the plant's `WORK.DELAY_REASONS` list (reference data, seeded by
/// `reference::system_catalog_integrity`, its label editable there); its code is fixed because the
/// backend writes it itself on the suspension, never chosen on screen.
pub const PERMIT_SUSPENDED_DELAY_REASON: &str = "permit_suspended";

/// Puts work order `wo_id`, running, on hold because its permit `permit_code` was suspended (owner
/// decision D5), in the caller's transaction (the suspension's): a delay segment of the fixed reason
/// [`PERMIT_SUSPENDED_DELAY_REASON`] waiting for a prerequisite, the status change and its history
/// row recorded as `actor_id`'s, the same writes as a hold made on screen. The reason is found by its
/// code whatever its active flag (a plant hiding it from the hold screen never blocks a suspension);
/// it is created first when the plant's list lacks it.
///
/// The hold is the suspending user's action (D5), whatever their own work-order rights: the history
/// row and the delay segment name them, and the segment's comment names the permit and the reason.
/// That comment is stored as written, in French ("Permis <code> suspendu : <motif>"), like any
/// comment typed on the hold screen; it is not translated on display.
pub(crate) async fn hold_for_permit_suspension(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    actor_id: i64,
    permit_code: &str,
    suspension_reason: &str,
) -> AppResult<WorkOrder> {
    let delay_reason_id = system_delay_reason_id(txn, PERMIT_SUSPENDED_DELAY_REASON).await?;
    let (_, _, row_version) = load_wo_status(txn, wo_id).await?;
    hold_wo_in(
        txn,
        &WoPauseInput {
            wo_id,
            actor_id,
            expected_row_version: row_version,
            delay_reason_id,
            comment: Some(format!("Permis {permit_code} suspendu : {suspension_reason}")),
        },
        PERMIT_SUSPENDED_DELAY_REASON,
        crate::wo::lifecycle::HoldKind::Prerequisite,
    )
    .await
}

/// The fixed delay reason of a running work order put on hold because its permit was closed,
/// handed back or cancelled on another PC (CMMS Advisor answer to AU-101, option B). A value of the
/// plant's `WORK.DELAY_REASONS` list (reference data, seeded by
/// `reference::system_catalog_integrity`, its label editable there); its code is fixed because the
/// backend writes it itself after applying the received permit, never chosen on screen.
pub const PERMIT_ENDED_DELAY_REASON: &str = "permit_ended";

/// Audit action: a running work order put on hold because its permit ended on another PC
/// ("permis terminé ailleurs"), recorded as the system.
pub const AUDIT_HELD_PERMIT_ENDED_ELSEWHERE: &str = "wo.held_permit_ended_elsewhere";

/// The id of the system delay reason `code` of `WORK.DELAY_REASONS`, made sure of first (created
/// when the plant's list lacks it), found whatever its active flag (a plant hiding it from the hold
/// screen never blocks the hold the backend writes).
async fn system_delay_reason_id(txn: &impl ConnectionTrait, code: &str) -> AppResult<i64> {
    crate::reference::system_catalog_integrity::ensure_system_delay_reason(txn, code).await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id AS id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
             WHERE rv.code = ? AND UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS' \
             ORDER BY (rs.status = 'published') DESC, rs.version_no DESC, rv.id LIMIT 1",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "WORK.DELAY_REASONS has no '{code}' value and none could be created"
            ))
        })?;
    row.try_get("", "id").map_err(|e| decode_err("delay reason id", e))
}

/// A permit of work order `wo_id` received from another PC as ended (`permit_status`: closed,
/// handed back, cancelled; or suspended, waiting for revalidation) while the work order runs here:
/// the work order is put on hold, in the caller's transaction — its own, opened after the received
/// permit row was applied and committed (`permit::ended_hold::after_sync_pass`), so a failure here
/// never refuses the permit row, its review conflict or its notices. CMMS Advisor answer to AU-101,
/// option B.
///
/// The same writes as a hold made on screen, recorded as the system (no person: the history row and
/// the delay segment carry no actor), with the fixed reason [`PERMIT_SUSPENDED_DELAY_REASON`] or
/// [`PERMIT_ENDED_DELAY_REASON`], waiting for a prerequisite; audited
/// ([`AUDIT_HELD_PERMIT_ENDED_ELSEWHERE`], "permis terminé ailleurs", with the permit's sync id).
/// Sent to the other PCs as any local change. Nothing is written when the work order is not running
/// here (already on hold, completed, closed): `false`.
///
/// Two PCs receiving the same end each hold the work order: the delay segment's identity is derived
/// from the fact (work order, permit, status received, last start), so both write the same segment,
/// and the crossing of the two holds is settled by sync without a person
/// (`permit::ended_hold::settle_crossed_hold_*`).
pub(crate) async fn hold_for_permit_ended_elsewhere(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    permit_sync_id: &str,
    permit_code: &str,
    permit_status: &str,
) -> AppResult<bool> {
    let (_, status, row_version) = load_wo_status(txn, wo_id).await?;
    if status != WoStatus::InProgress {
        return Ok(false);
    }
    let suspended = matches!(permit_status, "suspended" | "revalidation_required");
    let reason_code = if suspended {
        PERMIT_SUSPENDED_DELAY_REASON
    } else {
        PERMIT_ENDED_DELAY_REASON
    };
    let delay_reason_id = system_delay_reason_id(txn, reason_code).await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT w.entity_sync_id AS sync_id, \
                    COALESCE((SELECT t.acted_at FROM wo_state_transition_log t \
                              WHERE t.wo_id = w.id AND t.to_status = 'in_progress' \
                              ORDER BY t.acted_at DESC, t.id DESC LIMIT 1), w.actual_start) AS last_start \
             FROM work_orders w WHERE w.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let wo_sync: Option<String> = row.try_get("", "sync_id").map_err(|e| decode_err("wo sync id", e))?;
    let last_start: Option<String> = row.try_get("", "last_start").map_err(|e| decode_err("last start", e))?;
    let segment_sync_id = crate::sync::identity::derived_entity_sync_id(
        "work_order_delay_segments",
        &format!(
            "permit-ended-hold|{}|{permit_sync_id}|{permit_status}|{}",
            wo_sync.as_deref().unwrap_or_default(),
            last_start.as_deref().unwrap_or_default()
        ),
    );
    let comment = if suspended {
        format!("Permis {permit_code} suspendu sur un autre poste")
    } else {
        format!("Permis {permit_code} terminé sur un autre poste ({permit_status})")
    };
    hold_writes(
        txn,
        &HoldWrites {
            wo_id,
            expected_row_version: row_version,
            delay_reason_id,
            comment: Some(comment),
            actor_id: None,
            segment_sync_id: Some(segment_sync_id.clone()),
        },
        reason_code,
        crate::wo::lifecycle::HoldKind::Prerequisite,
    )
    .await?;
    crate::audit::writer::write_audit_event_in(
        txn,
        crate::audit::writer::AuditEventInput {
            action_code: AUDIT_HELD_PERMIT_ENDED_ELSEWHERE.to_string(),
            target_type: Some("work_order".to_string()),
            target_id: Some(wo_id.to_string()),
            actor_id: None,
            auth_context: "system".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "permis terminé ailleurs",
                "permit_sync_id": permit_sync_id,
                "permit_code": permit_code,
                "permit_status": permit_status,
                "delay_reason": reason_code,
                "delay_segment_sync_id": segment_sync_id,
            })),
        },
    )
    .await?;
    Ok(true)
}

/// The writes of a hold, the delay reason already resolved.
struct HoldWrites {
    wo_id: i64,
    expected_row_version: i64,
    delay_reason_id: i64,
    comment: Option<String>,
    /// Who holds; `None`: the system (a hold the application makes by itself).
    actor_id: Option<i64>,
    /// The delay segment's identity, when it is derived from the fact (`None`: a new one).
    segment_sync_id: Option<String>,
}

/// The writes of a hold (in_progress → on_hold) in the caller's transaction, the delay reason
/// already resolved (`reason_code`: its code, recorded on the history row).
async fn hold_wo_in(
    txn: &impl ConnectionTrait,
    input: &WoPauseInput,
    reason_code: &str,
    hold_kind: crate::wo::lifecycle::HoldKind,
) -> AppResult<WorkOrder> {
    hold_writes(
        txn,
        &HoldWrites {
            wo_id: input.wo_id,
            expected_row_version: input.expected_row_version,
            delay_reason_id: input.delay_reason_id,
            comment: input.comment.clone(),
            actor_id: Some(input.actor_id),
            segment_sync_id: None,
        },
        reason_code,
        hold_kind,
    )
    .await
}

async fn hold_writes(
    txn: &impl ConnectionTrait,
    input: &HoldWrites,
    reason_code: &str,
    hold_kind: crate::wo::lifecycle::HoldKind,
) -> AppResult<WorkOrder> {
    let (from_code, current_status, _rv) = load_wo_status(txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Hold).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::OnHold).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let on_hold_status_id = resolve_status_id(txn, "on_hold").await?;
    let now = now_utc_z();

    // Open delay segment
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_delay_segments \
         (work_order_id, started_at, ended_at, delay_reason_id, comment, entered_by_id, hold_kind, entity_sync_id) \
         VALUES (?, ?, NULL, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            now.clone().into(),
            input.delay_reason_id.into(),
            input.comment.clone().into(),
            input.actor_id.into(),
            hold_kind.as_str().into(),
            input.segment_sync_id.clone().into(),
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

    match input.actor_id {
        Some(actor_id) => {
            log_transition(
                txn,
                input.wo_id,
                &from_code,
                "on_hold",
                "hold",
                actor_id,
                Some(reason_code),
                input.comment.as_deref(),
                &now,
            )
            .await?;
        }
        None => {
            crate::wo::workflow::transition::log_system_transition(
                txn,
                input.wo_id,
                &from_code,
                "on_hold",
                "hold",
                Some(reason_code),
                input.comment.as_deref(),
                &now,
            )
            .await?;
        }
    }

    emit_action_event(
        txn,
        input.wo_id,
        "held",
        input.actor_id,
        Some(&from_code),
        Some("on_hold"),
        None,
    )
    .await?;

    queries::get_work_order(txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })
}

/// Audit action: the downtime of a mechanically complete work order ended because the removal of
/// its last fitted lock was verified (owner decision D4, CMMS Advisor condition 2).
pub const AUDIT_DOWNTIME_ENDED_BY_LOCK_REMOVAL: &str = "wo.downtime_ended_by_lock_removal";

/// After a lock removal was verified (here, `actor_id` the verifier; or received by sync and
/// applied, `actor_id` `None`: the system), in the caller's transaction: when work order `wo_id` is
/// mechanically complete ("Terminé") and no lock is fitted under its permits any more, its open
/// downtime segments end at `verified_at`, the moment of that verification — the equipment goes
/// back in service only now, never at "Terminé" while a lock is fitted. Audited, and shown in the
/// execution log. Nothing to do otherwise (work still running or on hold, another lock still
/// fitted, no open downtime). Returns the number of segments ended.
pub(crate) async fn end_downtime_after_lock_removal(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    verified_at: &str,
    actor_id: Option<i64>,
    isolation_sync_id: &str,
) -> AppResult<u64> {
    let (status_code, _, _) = load_wo_status(txn, wo_id).await?;
    if status_code != "completed" || crate::permit::wo_gate::permit_holds(txn, wo_id).await?.fitted_locks > 0 {
        return Ok(0);
    }
    let open = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, downtime_type FROM work_order_downtime_segments WHERE work_order_id = ? AND ended_at IS NULL",
            [wo_id.into()],
        ))
        .await?;
    if open.is_empty() {
        return Ok(0);
    }
    let ended = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_downtime_segments SET ended_at = ? WHERE work_order_id = ? AND ended_at IS NULL",
            [verified_at.into(), wo_id.into()],
        ))
        .await?
        .rows_affected();
    let mut segment_ids = Vec::with_capacity(open.len());
    for row in &open {
        let id: i64 = row.try_get("", "id").map_err(|e| decode_err("downtime id", e))?;
        let kind: Option<String> = row
            .try_get("", "downtime_type")
            .map_err(|e| decode_err("downtime_type", e))?;
        crate::wo::execution_log::emit_execution_event(
            txn,
            wo_id,
            "downtime_closed",
            "executionLog.downtimeClosed",
            serde_json::json!({ "downtime_type": kind, "ended_by": "lock_removal_verified" }),
            Some("downtime"),
            Some(id),
            actor_id,
        )
        .await?;
        segment_ids.push(id);
    }
    crate::audit::writer::write_audit_event_in(
        txn,
        crate::audit::writer::AuditEventInput {
            action_code: AUDIT_DOWNTIME_ENDED_BY_LOCK_REMOVAL.to_string(),
            target_type: Some("work_order".to_string()),
            target_id: Some(wo_id.to_string()),
            actor_id,
            auth_context: if actor_id.is_some() { "session" } else { "system" }.to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Downtime ended: removal of the last fitted lock verified",
                "ended_at": verified_at,
                "segment_ids": segment_ids,
                "isolation_sync_id": isolation_sync_id,
            })),
        },
    )
    .await?;
    Ok(ended)
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) resume_wo — on_hold → in_progress (closes delay segment, recomputes waiting hours)
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn resume_wo(db: &DatabaseConnection, input: WoResumeInput) -> AppResult<WorkOrder> {
    let remaining = crate::permit::wo_gate::checked_remaining_hours(input.estimated_remaining_hours)?;
    // One clock read: the moment judged by the permit gate is the resume recorded below.
    let at = now_utc_second();
    let txn = db.begin().await?;
    // Permit gate in the same transaction as the status change (no window in between).
    crate::permit::wo_gate::assert_in_progress_permit_gate_with(&txn, input.wo_id, remaining, at).await?;
    let linked_permit = crate::permit::queries::get_work_permit_linked_to_work_order(&txn, input.wo_id).await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Resume).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::InProgress).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let in_progress_status_id = resolve_status_id(&txn, "in_progress").await?;
    let now = stamp(at);

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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Waiting hours query returned no row")))?;
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

    // The remaining time entered (if any) is recorded on this row: who, when, the value.
    crate::wo::workflow::transition::log_transition_with_remaining(
        &txn,
        input.wo_id,
        &from_code,
        "in_progress",
        "resume",
        input.actor_id,
        None,
        None,
        &now,
        remaining,
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

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    if wo.requires_permit {
        linked_permit.ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "PTW gate passed but no linked permit for WO {}",
                input.wo_id
            ))
        })?;
    }

    txn.commit().await?;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) set_waiting_for_prerequisite / hold — in_progress → on_hold
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn set_waiting_for_prerequisite(db: &DatabaseConnection, input: WoHoldInput) -> AppResult<WorkOrder> {
    // Same On Hold phase as a pause; the segment records that the WO waits for a prerequisite.
    hold_wo(
        db,
        WoPauseInput {
            wo_id: input.wo_id,
            actor_id: input.actor_id,
            expected_row_version: input.expected_row_version,
            delay_reason_id: input.delay_reason_id,
            comment: input.comment,
        },
        crate::wo::lifecycle::HoldKind::Prerequisite,
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

pub(crate) async fn collect_completion_gates(
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Labor open-entry check returned no row")))?;
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

    // ── Tasks: every mandatory task closed with an accepted result (D3) ────
    let incomplete_descs = crate::wo::tasks::open_mandatory_tasks(conn, wo_id).await?;
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
                    AND COALESCE(origin, 'planned') = 'planned' \
                    AND consumption_status <> 'cancelled') AS planned_count, \
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
    } else {
        !(planned_count == 0 && used_count == 0 && parts_confirmed == 0)
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Downtime gate check returned no row")))?;
    let open_dt: i64 = open_dt_row
        .try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("open_dt cnt", e))?;
    // A lock still fitted under the work order's permits keeps the equipment down past "Terminé"
    // (owner decision D4, CMMS Advisor condition 2): its downtime stays open and ends when the
    // removal of the last fitted lock is verified (`end_downtime_after_lock_removal`). Without a
    // fitted lock, open downtime must be ended first, as before.
    let downtime_held_by_locks =
        open_dt > 0 && crate::permit::wo_gate::permit_holds(conn, wo_id).await?.fitted_locks > 0;
    gates.push(WoCompletionGate {
        code: "OPEN_DOWNTIME".into(),
        passed: open_dt == 0 || downtime_held_by_locks,
        required: true,
        detail: if downtime_held_by_locks {
            Some(format!(
                "{open_dt} open downtime segments, ended when the removal of the last fitted lock is verified"
            ))
        } else if open_dt > 0 {
            Some(format!("{open_dt} open downtime segments"))
        } else {
            None
        },
    });

    // ── RAMS (corrective natures only, EN 13306) ──────────────────────────
    let wo_type_row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wot.semantic_tag AS nature_code, wo.root_cause_summary \
             FROM work_orders wo \
             LEFT JOIN reference_values wot ON wot.id = wo.type_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let nature_code: Option<String> = wo_type_row
        .try_get("", "nature_code")
        .map_err(|e| decode_err("nature_code", e))?;
    let rams_required = nature_code
        .as_deref()
        .and_then(crate::wo::types::MaintenanceNature::parse)
        .is_some_and(|n| n.is_corrective());

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
                let symptom_id: Option<i64> = r.try_get("", "symptom_id").map_err(|e| decode_err("symptom_id", e))?;
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
        root_cause_ok = !root_cause.as_deref().map(str::trim).unwrap_or_default().is_empty();
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
pub async fn evaluate_completion_gates(db: &DatabaseConnection, wo_id: i64) -> AppResult<Vec<WoCompletionGate>> {
    collect_completion_gates(db, wo_id).await
}

pub async fn complete_wo_mechanically(db: &DatabaseConnection, input: WoMechCompleteInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    assert_action_allowed(&current_status, WoAction::Complete).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::Completed).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let gates = collect_completion_gates(&txn, input.wo_id).await?;
    let blocking: Vec<String> = gates
        .iter()
        .filter(|g| g.required && !g.passed)
        .map(|g| match g.code.as_str() {
            "OPEN_LABOR" => {
                "GATE_COMPLETE_OPEN_LABOR: Les entrées de main-d'œuvre ouvertes doivent être clôturées avant la complétion. (Open labor entries must be closed first.)".into()
            }
            "TASKS" => format!(
                "GATE_COMPLETE_TASKS_INCOMPLETE: Tâches obligatoires non terminées{}. (Mandatory tasks incomplete.)",
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
                    "GATE_COMPLETE_PARTS_UNDISPOSED: Toutes les pièces planifiées doivent être marquées Utilisée ou Non utilisée avant complétion. \
(All planned parts must be marked Used or Not used before completion.)".into()
                } else {
                    "GATE_COMPLETE_PARTS_UNCONFIRMED: Réels des pièces non confirmés. Ajoutez une pièce consommée ou marquez « aucune pièce utilisée ». \
(Parts actuals not confirmed. Add a consumed part or mark none used.)".into()
                }
            }
            "OPEN_DOWNTIME" => {
                "GATE_COMPLETE_OPEN_DOWNTIME: Les segments de temps d'arrêt ouverts doivent être clôturés avant la complétion. (Open downtime segments must be closed before completion.)".into()
            }
            "SYMPTOM" => {
                "GATE_COMPLETE_SYMPTOM_REQUIRED: Symptôme requis avant complétion mécanique (RAMS data quality).".into()
            }
            "FAILURE_MODE" => {
                "GATE_COMPLETE_FAILURE_MODE_REQUIRED: Mode de défaillance requis avant complétion mécanique (RAMS data quality).".into()
            }
            "ROOT_CAUSE" => {
                "GATE_COMPLETE_ROOT_CAUSE_REQUIRED: Résumé de cause racine requis avant complétion mécanique.".into()
            }
            other => format!("GATE_COMPLETE_GATE_FAILED:gate={other}: Completion gate failed: {other}"),
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Labor sum query returned no row")))?;
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
                input
                    .actual_end
                    .clone()
                    .map(|v| v.into())
                    .unwrap_or(sea_orm::Value::String(None)),
                input
                    .actual_duration_hours
                    .map(|v| v.into())
                    .unwrap_or(sea_orm::Value::Double(None)),
                input
                    .conclusion
                    .clone()
                    .map(|v| v.into())
                    .unwrap_or(sea_orm::Value::String(None)),
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

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    // Its schedule commitments are closed with it (`planning::lifecycle`).
    crate::planning::lifecycle::close_commitments_of_work_order(&txn, input.wo_id).await?;
    txn.commit().await?;
    Ok(wo)
}
