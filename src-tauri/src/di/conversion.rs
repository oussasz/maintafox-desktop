//! DI-to-WO conversion — stage 3 gate (PRD §6.4).
//!
//! Phase 2 - Sub-phase 04 - File 03 - Sprint S2.
//!
//! A DI in `approved` can be converted into a work order shell.
//! After conversion the DI is locked as an immutable origin record.
//!
//! Architecture rules:
//!   - The entire conversion is a single sqlx transaction: DI update, WO stub
//!     insert, state transition log, and review event. Partial states impossible.
//!   - The WO shell is intentionally minimal (traceability only). SP05 fills
//!     all planning, execution, and cost fields.
//!   - Step-up reauthentication is required (validated in the command layer
//!     via `require_step_up!`).

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::domain::{
    guard_transition, map_intervention_request, DiStatus, InterventionRequest, IR_COLS, IR_JOINS, IR_JOIN_COLS,
};
use crate::wo::domain::generate_wo_code;
use crate::wo::time::now_utc_z;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Input for converting a DI into a work order.
#[derive(Debug, Clone, Deserialize)]
pub struct WoConversionInput {
    pub di_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub conversion_notes: Option<String>,
}

/// Result of a successful DI-to-WO conversion.
#[derive(Debug, Clone, Serialize)]
pub struct WoConversionResult {
    pub di: InterventionRequest,
    pub wo_id: i64,
    pub wo_code: String,
    /// Whether the new work order requires a permit, and why (the plant's permit rules,
    /// `permit::requirement_rules`, and the request's safety flag).
    pub permit_flag: crate::permit::requirement_rules::PermitFlagOutcome,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Check concurrency: `rows_affected == 1` after an UPDATE with `row_version` guard.
fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }
    Ok(())
}

/// Nature of the OT a request becomes (EN 13306): the nature its request type names (semantic
/// tag of the `DI.REQUEST_TYPE` value: preventive, inspection, improvement…), else corrective —
/// *immediate* when the carried urgency is the top level of the shared scale (emergency),
/// *deferred* otherwise.
pub(crate) async fn conversion_nature<C: ConnectionTrait>(
    conn: &C,
    request_type: &str,
    urgency: crate::reference::urgency_scale::UrgencyCode,
) -> AppResult<crate::wo::types::MaintenanceNature> {
    use crate::wo::types::MaintenanceNature;
    Ok(
        match crate::wo::types::nature_for_request_type(conn, request_type).await? {
            Some(nature) => nature,
            None if urgency == crate::reference::urgency_scale::UrgencyCode::Emergency => {
                MaintenanceNature::CorrectiveImmediate
            }
            None => MaintenanceNature::CorrectiveDeferred,
        },
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// convert_di_to_work_order — single atomic transaction
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert a DI in `approved` to a work order shell.
///
/// The entire operation executes inside a single transaction:
///   1. Load DI + guard state transition
///   2. Validate prerequisites (equipment_id, classification_code_id)
///   3. Generate WO code and insert WO stub
///   4. Update DI status to `closed` with disposition `converted_to_wo`
///   5. Insert state transition log
///   6. Insert review event
///
/// Step-up validation is handled in the command layer via `require_step_up!`.
pub async fn convert_di_to_work_order(
    db: &DatabaseConnection,
    input: WoConversionInput,
) -> AppResult<WoConversionResult> {
    crate::wo::statuses::ensure_work_order_statuses_if_needed(db).await?;

    let txn = db.begin().await?;

    // ── 1. Load DI and validate state transition ──────────────────────────
    let di_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {IR_COLS}, {IR_JOIN_COLS} \
                 FROM intervention_requests ir \
                 {IR_JOINS} \
                 WHERE ir.id = ?"
            ),
            [input.di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;

    let di = map_intervention_request(&di_row)?;
    let current_status = DiStatus::try_from_str(&di.status)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored DI has invalid status: {e}")))?;

    if current_status != DiStatus::Approved {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_DI_CONVERT_STATUS_INVALID: La conversion n'est possible qu'au statut 'approved'. Statut actuel : '{}'.",
            current_status.as_str()
        )]));
    }

    guard_transition(&current_status, &DiStatus::Closed).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // ── 2. Validate conversion prerequisites ──────────────────────────────
    let mut errors: Vec<String> = Vec::new();

    if di.equipment_id == 0 {
        errors.push("GATE_DI_CONVERT_ASSET_REQUIRED: Contexte d'actif requis pour la conversion.".into());
    }
    if di.org_node_id == 0 {
        errors.push("GATE_DI_CONVERT_ORG_NODE_REQUIRED: Contexte d'entité requis pour la conversion.".into());
    }

    if di.classification_code_id.is_none() {
        errors.push("GATE_DI_CONVERT_CLASSIFICATION_REQUIRED: Classification requise pour la conversion.".into());
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    // ── 3. Create WO in work_orders table ───────────────────────────────
    let now = now_utc_z();

    // Guard duplicate conversion: keep only one non-terminal WO per DI.
    let duplicate_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id, wo.code \
                 FROM work_orders wo \
                 JOIN work_order_statuses wos ON wos.id = wo.status_id \
                 WHERE wo.source_di_id = ? AND wos.code NOT IN {} \
                 ORDER BY wo.id DESC LIMIT 1",
                crate::wo::backlog::CLOSED_STATUS_SQL
            ),
            [input.di_id.into()],
        ))
        .await?;
    if let Some(row) = duplicate_row {
        let existing_id: i64 = row.try_get("", "id").unwrap_or_default();
        let existing_code: String = row.try_get("", "code").unwrap_or_else(|_| "N/A".to_string());
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_DI_WO_ACTIVE_EXISTS:id={existing_id},code={existing_code}: Une OT active existe déjà pour cette DI (id={existing_id}, code={existing_code})."
        )]));
    }

    // Resolve planning status_id (DI already submitted intake → land in planning)
    let planning_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = 'planning'".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("work_order_statuses missing 'planning' row")))?;
    let planning_status_id: i64 = planning_row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("planning status_id decode: {e}")))?;

    // Single SSOT allocator (same sequence as manual / PM / inspection create).
    let wo_code = generate_wo_code(&txn).await?;

    // Carry the triage context onto the WO: the urgency the reviewer validated (falling back to
    // what the requester reported) on the shared DI / OT scale, the OT type of the matching
    // maintenance nature, and the safety flag, which makes the permit / LOTO gate mandatory
    // before execution can start.
    let carried_urgency = crate::reference::urgency_scale::validate_urgency_code(
        di.validated_urgency
            .as_deref()
            .filter(|code| !code.trim().is_empty())
            .unwrap_or(di.reported_urgency.as_str()),
    )?;
    let urgency_id = crate::reference::urgency_scale::urgency_value_id(&txn, carried_urgency.as_str()).await?;
    let nature = conversion_nature(&txn, &di.request_type, carried_urgency).await?;
    let type_id = crate::wo::types::default_type_for_nature(&txn, nature).await?.id;
    // Permit: the plant's rules (equipment class / work type → permit type), and the request's
    // safety flag as before. No rule configured → nothing flagged by a rule, and the outcome says so.
    let permit_flag = crate::permit::requirement_rules::evaluate(
        &txn,
        (di.equipment_id != 0).then_some(di.equipment_id),
        type_id,
        di.safety_flag,
    )
    .await?;
    let requires_permit: i64 = i64::from(permit_flag.requires_permit);

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_orders (\
            code, type_id, status_id, equipment_id, \
            source_di_id, entity_id, requester_id, \
            title, description, urgency_id, requires_permit, \
            permit_rule_code, permit_rule_permit_type, permit_from_safety_di, \
            entity_sync_id, row_version, created_at, updated_at\
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            wo_code.clone().into(),
            type_id.into(),
            planning_status_id.into(),
            if di.equipment_id == 0 {
                sea_orm::Value::from(None::<i64>)
            } else {
                di.equipment_id.into()
            },
            input.di_id.into(),
            if di.org_node_id == 0 {
                sea_orm::Value::from(None::<i64>)
            } else {
                di.org_node_id.into()
            },
            input.actor_id.into(),
            di.title.clone().into(),
            di.description.clone().into(),
            urgency_id.into(),
            requires_permit.into(),
            permit_flag.rule_code.clone().into(),
            permit_flag.permit_type_code.clone().into(),
            // The request's safety flag is the origin of the requirement: cleared only with the
            // permit-issue right at the work order's site and a reason (migration 255).
            i64::from(di.safety_flag).into(),
            crate::sync::identity::new_entity_sync_id().into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec![
                "GATE_DI_WO_CODE_DUPLICATE: Code OT en doublon. Veuillez réessayer.".into(),
            ])
        } else {
            AppError::Database(e)
        }
    })?;

    // Get the inserted WO id
    let wo_id_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_orders WHERE code = ?",
            [wo_code.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to retrieve WO id after insert")))?;

    let wo_id: i64 = wo_id_row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("WO id decode: {e}")))?;

    // Requests and the close-out share one symptom list (`WORK.SYMPTOMS`): the observed symptom
    // carries over as is, and from the close-out into the RAMS failure event.
    let wo_symptom_id = di.symptom_code_id;

    // Carry-over DI technical context into WO closeout context for RAMS.
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_failure_details \
         (work_order_id, symptom_id, failure_mode_id, failure_cause_id, failure_effect_id, \
          is_temporary_repair, is_permanent_repair, cause_not_determined, notes) \
         VALUES (?, ?, NULL, NULL, NULL, 0, 0, 0, ?)",
        [
            wo_id.into(),
            wo_symptom_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            Some(format!(
                "Auto-propagated from DI #{}, classification_code_id={}",
                di.id,
                di.classification_code_id.unwrap_or_default()
            ))
            .map(sea_orm::Value::from)
            .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    // Carry-over DI attachments by linking existing file paths to the new WO.
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO work_order_attachments \
         (work_order_id, file_name, relative_path, mime_type, size_bytes, uploaded_by_id, uploaded_at, notes) \
         SELECT ?, da.file_name, da.relative_path, da.mime_type, da.size_bytes, da.uploaded_by_id, da.uploaded_at, \
                COALESCE(da.notes, 'Linked from DI conversion') \
         FROM di_attachments da WHERE da.di_id = ?",
        [wo_id.into(), input.di_id.into()],
    ))
    .await?;

    // Insert initial WO transition log
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log \
            (wo_id, from_status, to_status, action, actor_id, notes, acted_at) \
         VALUES (?, '__none__', 'planning', 'create_from_di', ?, ?, ?)",
        [
            wo_id.into(),
            input.actor_id.into(),
            input
                .conversion_notes
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            now.clone().into(),
        ],
    ))
    .await?;

    // Rules that do not read, or name codes not held here: in the work order's history and audit
    // trail too (the conversion response shows them).
    crate::permit::requirement_rules::record_rule_problems(&txn, wo_id, Some(input.actor_id), &permit_flag).await?;

    // ── 5. Update DI status ───────────────────────────────────────────────
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'closed', \
                disposition_code = 'converted_to_wo', \
                converted_to_wo_id = ?, \
                converted_at = ?, \
                closed_at = ?, \
                closed_by_id = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                wo_id.into(),
                now.clone().into(),
                now.clone().into(),
                input.actor_id.into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::Closed.as_str();

    // ── 6. Insert state transition log ────────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_state_transition_log \
            (di_id, from_status, to_status, action, actor_id, reason_code, notes, acted_at) \
         VALUES (?, ?, ?, 'convert', ?, NULL, ?, ?)",
        [
            input.di_id.into(),
            from_str.into(),
            to_str.into(),
            input.actor_id.into(),
            input
                .conversion_notes
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            now.clone().into(),
        ],
    ))
    .await?;

    // ── 7. Insert review event (SLA snapshot from frozen DI columns) ──────
    let di = super::sla::freeze_sla_on_di(&txn, &di).await?;
    let snap = super::sla::snapshot_sla_for_review_event(&di);
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_review_events \
            (di_id, event_type, actor_id, acted_at, from_status, to_status, \
             reason_code, notes, sla_target_hours, sla_deadline, \
             sla_resolution_target_hours, sla_resolution_deadline, step_up_used) \
         VALUES (?, 'converted', ?, ?, ?, ?, 'converted_to_wo', ?, ?, ?, ?, ?, 1)",
        [
            input.di_id.into(),
            input.actor_id.into(),
            now.clone().into(),
            from_str.into(),
            to_str.into(),
            input
                .conversion_notes
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            snap.response_target_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            snap.response_deadline
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            snap.resolution_target_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            snap.resolution_deadline
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    // ── 8. Re-fetch updated DI and commit ─────────────────────────────────
    let updated_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {IR_COLS}, {IR_JOIN_COLS} \
                 FROM intervention_requests ir \
                 {IR_JOINS} \
                 WHERE ir.id = ?"
            ),
            [input.di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;

    let updated_di = map_intervention_request(&updated_row)?;

    // ── 9. Stage the new WO and the closed DI in the same transaction ─────

    txn.commit().await?;

    super::notifications::notify_converted(db, &updated_di).await;
    super::notifications::notify_closed(db, &updated_di).await;

    Ok(WoConversionResult {
        di: updated_di,
        wo_id,
        wo_code,
        permit_flag,
    })
}
