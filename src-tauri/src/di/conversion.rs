//! DI-to-WO conversion — stage 3 gate (PRD §6.4).
//!
//! Phase 2 - Sub-phase 04 - File 03 - Sprint S2.
//!
//! A DI in `approved_for_planning` can be converted into a work order shell.
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
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::domain::{
    guard_transition, map_intervention_request, DiStatus, InterventionRequest,
};

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
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// All columns from `intervention_requests` for SELECT reuse (aliased as `ir`).
const IR_COLS: &str = "\
    ir.id, ir.code, ir.asset_id, ir.sub_asset_ref, ir.org_node_id, \
    ir.status, ir.title, ir.description, ir.origin_type, ir.symptom_code_id, \
    ir.impact_level, ir.production_impact, ir.safety_flag, ir.environmental_flag, \
    ir.quality_flag, ir.reported_urgency, ir.validated_urgency, \
    ir.observed_at, ir.submitted_at, \
    ir.review_team_id, ir.reviewer_id, ir.screened_at, ir.approved_at, \
    ir.deferred_until, ir.declined_at, ir.closed_at, ir.archived_at, \
    ir.converted_to_wo_id, ir.converted_at, \
    ir.reviewer_note, ir.classification_code_id, \
    ir.is_recurrence_flag, ir.recurrence_di_id, \
    ir.source_inspection_anomaly_id, \
    ir.row_version, ir.submitter_id, ir.created_at, ir.updated_at";

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Check concurrency: `rows_affected == 1` after an UPDATE with `row_version` guard.
fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a été modifié par un autre utilisateur. \
             Veuillez recharger et réessayer."
                .into(),
        ]));
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// convert_di_to_work_order — single atomic transaction
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert a DI in `approved_for_planning` to a work order shell.
///
/// The entire operation executes inside a single transaction:
///   1. Load DI + guard state transition
///   2. Validate prerequisites (asset_id, classification_code_id)
///   3. Generate WO code and insert WO stub
///   4. Update DI status to `converted_to_work_order`
///   5. Insert state transition log
///   6. Insert review event
///
/// Step-up validation is handled in the command layer via `require_step_up!`.
pub async fn convert_di_to_work_order(
    db: &DatabaseConnection,
    input: WoConversionInput,
) -> AppResult<WoConversionResult> {
    let txn = db.begin().await?;

    // ── 1. Load DI and validate state transition ──────────────────────────
    let di_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {IR_COLS} FROM intervention_requests ir WHERE ir.id = ?"),
            [input.di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;

    let di = map_intervention_request(&di_row)?;
    let current_status = DiStatus::try_from_str(&di.status).map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Stored DI has invalid status: {e}"))
    })?;

    guard_transition(&current_status, &DiStatus::ConvertedToWorkOrder).map_err(|e| {
        AppError::ValidationFailed(vec![e])
    })?;

    // ── 2. Validate conversion prerequisites ──────────────────────────────
    let mut errors: Vec<String> = Vec::new();

    if di.asset_id == 0 {
        errors.push("Contexte d'actif requis pour la conversion.".into());
    }

    if di.classification_code_id.is_none() {
        errors.push("Classification requise pour la conversion.".into());
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    // ── 3. Create WO in work_orders table ───────────────────────────────
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Resolve draft status_id
    let draft_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = 'draft'".to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "work_order_statuses missing 'draft' row"
            ))
        })?;
    let draft_status_id: i64 = draft_row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("draft status_id decode: {e}")))?;

    // Resolve default type_id (corrective = 1)
    let type_id: i64 = 1;

    // Generate WO code (OT-NNNN)
    let max_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(CAST(SUBSTR(code, 4) AS INTEGER)), 0) AS max_seq \
             FROM work_orders WHERE code LIKE 'OT-%'"
                .to_string(),
        ))
        .await?;
    let next_seq: i64 = max_row
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "max_seq").ok())
        .unwrap_or(0)
        + 1;
    let wo_code = format!("OT-{next_seq:04}");

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_orders (\
            code, type_id, status_id, equipment_id, \
            source_di_id, entity_id, requester_id, \
            title, description, row_version, created_at, updated_at\
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            wo_code.clone().into(),
            type_id.into(),
            draft_status_id.into(),
            if di.asset_id == 0 { sea_orm::Value::from(None::<i64>) } else { di.asset_id.into() },
            input.di_id.into(),
            if di.org_node_id == 0 { sea_orm::Value::from(None::<i64>) } else { di.org_node_id.into() },
            input.actor_id.into(),
            di.title.clone().into(),
            di.description.clone().into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::ValidationFailed(vec!["Code OT en doublon. Veuillez réessayer.".into()])
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
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Failed to retrieve WO id after insert"))
        })?;

    let wo_id: i64 = wo_id_row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("WO id decode: {e}")))?;

    // Insert initial WO transition log
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log \
            (wo_id, from_status, to_status, action, actor_id, notes, acted_at) \
         VALUES (?, '__none__', 'draft', 'create_from_di', ?, ?, ?)",
        [
            wo_id.into(),
            input.actor_id.into(),
            input.conversion_notes.clone().map(sea_orm::Value::from).unwrap_or(sea_orm::Value::from(None::<String>)),
            now.clone().into(),
        ],
    ))
    .await?;

    // ── 5. Update DI status ───────────────────────────────────────────────
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                status = 'converted_to_work_order', \
                converted_to_wo_id = ?, \
                converted_at = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                wo_id.into(),
                now.clone().into(),
                now.clone().into(),
                input.di_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let from_str = current_status.as_str();
    let to_str = DiStatus::ConvertedToWorkOrder.as_str();

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

    // ── 7. Insert review event ────────────────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO di_review_events \
            (di_id, event_type, actor_id, acted_at, from_status, to_status, \
             reason_code, notes, sla_target_hours, sla_deadline, step_up_used) \
         VALUES (?, 'converted', ?, ?, ?, ?, NULL, ?, NULL, NULL, 1)",
        [
            input.di_id.into(),
            input.actor_id.into(),
            now.clone().into(),
            from_str.into(),
            to_str.into(),
            input
                .conversion_notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    // ── 8. Re-fetch updated DI and commit ─────────────────────────────────
    let updated_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("SELECT {IR_COLS} FROM intervention_requests ir WHERE ir.id = ?"),
            [input.di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;

    let updated_di = map_intervention_request(&updated_row)?;
    txn.commit().await?;

    Ok(WoConversionResult {
        di: updated_di,
        wo_id,
        wo_code,
    })
}
