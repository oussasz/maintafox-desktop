//! WO close-out — failure detail capture, technical verification, closure quality gate,
//! and reopen logic.
//!
//! Phase 2 - Sub-phase 05 - File 03 - Sprint S1.
//!
//! Functions:
//!   save_failure_detail     — upsert structured failure taxonomy (symptom/mode/cause/effect)
//!   save_verification       — record verification + transition to technically_verified
//!   close_wo                — quality-gated closure with cost roll-up
//!   reopen_wo               — supervised reopen within configurable recurrence window
//!   get_failure_details     — list failure detail rows for a WO
//!   get_verifications       — list verification rows for a WO

use crate::activity::emitter;
use crate::audit;
use crate::errors::{AppError, AppResult};
use crate::inventory::queries as inventory_queries;
use crate::reliability::queries as reliability_queries;
use crate::wo::queries;
use crate::wo::sync_stage;
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::domain::{guard_wo_transition, WoStatus, WorkOrder};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Row from `work_order_failure_details`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoFailureDetail {
    pub id: i64,
    pub work_order_id: i64,
    pub symptom_id: Option<i64>,
    pub failure_mode_id: Option<i64>,
    pub failure_cause_id: Option<i64>,
    pub failure_effect_id: Option<i64>,
    pub is_temporary_repair: bool,
    pub is_permanent_repair: bool,
    pub cause_not_determined: bool,
    pub notes: Option<String>,
}

/// Row from `work_order_verifications`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoVerification {
    pub id: i64,
    pub work_order_id: i64,
    pub verified_by_id: i64,
    pub verified_at: String,
    pub result: String,
    pub return_to_service_confirmed: bool,
    pub recurrence_risk_level: Option<String>,
    pub notes: Option<String>,
}

/// Input for upserting a failure detail record.
#[derive(Debug, Clone, Deserialize)]
pub struct SaveFailureDetailInput {
    pub wo_id: i64,
    pub symptom_id: Option<i64>,
    pub failure_mode_id: Option<i64>,
    pub failure_cause_id: Option<i64>,
    pub failure_effect_id: Option<i64>,
    pub is_temporary_repair: bool,
    pub is_permanent_repair: bool,
    pub cause_not_determined: bool,
    pub notes: Option<String>,
}

/// Input for recording a technical verification.
#[derive(Debug, Clone, Deserialize)]
pub struct SaveVerificationInput {
    pub wo_id: i64,
    pub verified_by_id: i64,
    pub result: String,
    pub return_to_service_confirmed: bool,
    pub recurrence_risk_level: Option<String>,
    pub notes: Option<String>,
    pub expected_row_version: i64,
}

/// Input for the closure quality gate.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WoCloseInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    #[serde(default)]
    pub no_downtime_attestation: Option<bool>,
    #[serde(default)]
    pub no_downtime_attestation_reason: Option<String>,
}

/// Input for reopening a recently closed WO.
#[derive(Debug, Clone, Deserialize)]
pub struct WoReopenInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
}

/// Input for updating root cause analysis fields on the WO.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateWoRcaInput {
    pub wo_id: i64,
    pub root_cause_summary: Option<String>,
    pub corrective_action_summary: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

const VALID_VERIFICATION_RESULTS: &[&str] = &["pass", "fail", "monitor"];

const VALID_RECURRENCE_LEVELS: &[&str] = &["none", "low", "medium", "high"];

/// WO types that require failure coding and root cause before closure.
const FAILURE_REQUIRED_TYPE_CODES: &[&str] = &["corrective", "emergency"];

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WO closeout row decode failed for column '{column}': {e}"
    ))
}

fn map_failure_detail(row: &sea_orm::QueryResult) -> AppResult<WoFailureDetail> {
    Ok(WoFailureDetail {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        symptom_id: row
            .try_get::<Option<i64>>("", "symptom_id")
            .map_err(|e| decode_err("symptom_id", e))?,
        failure_mode_id: row
            .try_get::<Option<i64>>("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?,
        failure_cause_id: row
            .try_get::<Option<i64>>("", "failure_cause_id")
            .map_err(|e| decode_err("failure_cause_id", e))?,
        failure_effect_id: row
            .try_get::<Option<i64>>("", "failure_effect_id")
            .map_err(|e| decode_err("failure_effect_id", e))?,
        is_temporary_repair: row
            .try_get::<i64>("", "is_temporary_repair")
            .map_err(|e| decode_err("is_temporary_repair", e))?
            != 0,
        is_permanent_repair: row
            .try_get::<i64>("", "is_permanent_repair")
            .map_err(|e| decode_err("is_permanent_repair", e))?
            != 0,
        cause_not_determined: row
            .try_get::<i64>("", "cause_not_determined")
            .map_err(|e| decode_err("cause_not_determined", e))?
            != 0,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
    })
}

fn map_verification(row: &sea_orm::QueryResult) -> AppResult<WoVerification> {
    Ok(WoVerification {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        verified_by_id: row
            .try_get::<i64>("", "verified_by_id")
            .map_err(|e| decode_err("verified_by_id", e))?,
        verified_at: row
            .try_get::<String>("", "verified_at")
            .map_err(|e| decode_err("verified_at", e))?,
        result: row
            .try_get::<String>("", "result")
            .map_err(|e| decode_err("result", e))?,
        return_to_service_confirmed: row
            .try_get::<i64>("", "return_to_service_confirmed")
            .map_err(|e| decode_err("return_to_service_confirmed", e))?
            != 0,
        recurrence_risk_level: row
            .try_get::<Option<String>>("", "recurrence_risk_level")
            .map_err(|e| decode_err("recurrence_risk_level", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify `rows_affected == 1`. Returns a concurrency conflict error on mismatch.
fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Conflit de version : cet enregistrement a ete modifie par un autre utilisateur. \
             Veuillez recharger et reessayer."
                .to_string(),
        ]));
    }
    Ok(())
}

/// Load current WO status from the DB and parse it.
/// Returns `(status_code, parsed_status, row_version)`.
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

/// Resolve `status_id` for a given status code.
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

/// Load the WO type code (e.g. "corrective", "preventive").
async fn load_wo_type_code(txn: &impl ConnectionTrait, wo_id: i64) -> AppResult<String> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wot.code AS type_code \
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
    row.try_get::<String>("", "type_code")
        .map_err(|e| decode_err("type_code", e))
}

struct CloseoutPolicyRow {
    require_downtime_if_production_impact: bool,
    allow_close_with_cause_not_determined: bool,
    allow_close_with_cause_mode_only: bool,
    require_verification_return_to_service: bool,
    notes_min_length_when_cnd: i64,
}

async fn load_closeout_policy(
    txn: &impl ConnectionTrait,
    policy_id: i64,
) -> AppResult<CloseoutPolicyRow> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, require_downtime_if_production_impact, \
             allow_close_with_cause_not_determined, allow_close_with_cause_mode_only, \
             require_verification_return_to_service, notes_min_length_when_cnd \
             FROM closeout_validation_policies WHERE id = ?",
            [policy_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "closeout_validation_policies missing id {policy_id}"
            ))
        })?;

    Ok(CloseoutPolicyRow {
        require_downtime_if_production_impact: row
            .try_get::<i64>("", "require_downtime_if_production_impact")
            .map_err(|e| decode_err("require_downtime_if_production_impact", e))?
            != 0,
        allow_close_with_cause_not_determined: row
            .try_get::<i64>("", "allow_close_with_cause_not_determined")
            .map_err(|e| decode_err("allow_close_with_cause_not_determined", e))?
            != 0,
        allow_close_with_cause_mode_only: row
            .try_get::<i64>("", "allow_close_with_cause_mode_only")
            .map_err(|e| decode_err("allow_close_with_cause_mode_only", e))?
            != 0,
        require_verification_return_to_service: row
            .try_get::<i64>("", "require_verification_return_to_service")
            .map_err(|e| decode_err("require_verification_return_to_service", e))?
            != 0,
        notes_min_length_when_cnd: row
            .try_get::<i64>("", "notes_min_length_when_cnd")
            .map_err(|e| decode_err("notes_min_length_when_cnd", e))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) save_failure_detail — upsert failure taxonomy
// ═══════════════════════════════════════════════════════════════════════════════

/// Upsert a failure detail record for a WO (one row per WO).
/// WO must not be in closed or cancelled state.
pub async fn save_failure_detail(
    db: &DatabaseConnection,
    input: SaveFailureDetailInput,
) -> AppResult<WoFailureDetail> {
    // ── Validate: cannot be both temporary AND permanent ──────────────────
    if input.is_temporary_repair && input.is_permanent_repair {
        return Err(AppError::ValidationFailed(vec![
            "L'action ne peut pas etre a la fois temporaire et permanente. \
             (Action cannot be both temporary and permanent.)"
                .to_string(),
        ]));
    }

    // ── Guard: WO not closed/cancelled ────────────────────────────────────
    let (status_code, _status, _rv) = load_wo_status(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de modifier les details de defaillance pour un OT en statut '{status_code}'."
        )]));
    }

    let temp = if input.is_temporary_repair { 1i64 } else { 0 };
    let perm = if input.is_permanent_repair { 1i64 } else { 0 };
    let cnd = if input.cause_not_determined { 1i64 } else { 0 };

    // ── Check for existing row (upsert) ───────────────────────────────────
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_failure_details WHERE work_order_id = ?",
            [input.wo_id.into()],
        ))
        .await?;

    if let Some(existing_row) = existing {
        let existing_id: i64 = existing_row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_failure_details SET \
                symptom_id = ?, failure_mode_id = ?, failure_cause_id = ?, \
                failure_effect_id = ?, is_temporary_repair = ?, is_permanent_repair = ?, \
                cause_not_determined = ?, notes = ? \
             WHERE id = ?",
            [
                input
                    .symptom_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .failure_mode_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .failure_cause_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .failure_effect_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                temp.into(),
                perm.into(),
                cnd.into(),
                input
                    .notes
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                existing_id.into(),
            ],
        ))
        .await?;
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_order_failure_details \
                (work_order_id, symptom_id, failure_mode_id, failure_cause_id, \
                 failure_effect_id, is_temporary_repair, is_permanent_repair, \
                 cause_not_determined, notes) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                input.wo_id.into(),
                input
                    .symptom_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .failure_mode_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .failure_cause_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                input
                    .failure_effect_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
                temp.into(),
                perm.into(),
                cnd.into(),
                input
                    .notes
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
            ],
        ))
        .await?;
    }

    // ── Re-fetch ──────────────────────────────────────────────────────────
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM work_order_failure_details WHERE work_order_id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Failed to re-fetch failure detail after upsert"
            ))
        })?;

    map_failure_detail(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) save_verification — mechanically_complete → technically_verified
// ═══════════════════════════════════════════════════════════════════════════════

/// Record a technical verification and transition the WO to technically_verified.
///
/// Enforces:
///   - WO must be in `mechanically_complete`
///   - `result` must be one of: pass, fail, monitor
///   - `verified_by_id` must differ from `primary_responsible_id` (no self-verification)
///   - Only `pass` with `return_to_service_confirmed` triggers the status transition
pub async fn save_verification(
    db: &DatabaseConnection,
    input: SaveVerificationInput,
) -> AppResult<(WoVerification, WorkOrder)> {
    // ── Validate result ───────────────────────────────────────────────────
    if !VALID_VERIFICATION_RESULTS.contains(&input.result.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Resultat de verification invalide : '{}'. Valeurs autorisees : pass, fail, monitor.",
            input.result
        )]));
    }

    // ── Validate recurrence risk level if provided ────────────────────────
    if let Some(ref level) = input.recurrence_risk_level {
        if !VALID_RECURRENCE_LEVELS.contains(&level.as_str()) {
            return Err(AppError::ValidationFailed(vec![format!(
                "Niveau de risque de recurrence invalide : '{}'. Valeurs autorisees : none, low, medium, high.",
                level
            )]));
        }
    }

    let txn = db.begin().await?;

    // ── Guard: WO must be in mechanically_complete ────────────────────────
    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    guard_wo_transition(&current_status, &WoStatus::TechnicallyVerified)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // ── No self-verification ──────────────────────────────────────────────
    let responsible_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT primary_responsible_id FROM work_orders WHERE id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    let primary_responsible_id: Option<i64> = responsible_row
        .try_get::<Option<i64>>("", "primary_responsible_id")
        .map_err(|e| decode_err("primary_responsible_id", e))?;

    if let Some(resp_id) = primary_responsible_id {
        if resp_id == input.verified_by_id {
            return Err(AppError::ValidationFailed(vec![
                "L'auto-verification n'est pas autorisee : le verificateur doit etre different \
                 du responsable principal. (Self-verification is not allowed.)"
                    .to_string(),
            ]));
        }
    }

    // ── Insert verification record ────────────────────────────────────────
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_verifications \
            (work_order_id, verified_by_id, verified_at, result, \
             return_to_service_confirmed, recurrence_risk_level, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            input.verified_by_id.into(),
            now.clone().into(),
            input.result.clone().into(),
            if input.return_to_service_confirmed { 1i64 } else { 0 }.into(),
            input
                .recurrence_risk_level
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input
                .notes
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    // ── Transition to technically_verified ─────────────────────────────────
    // Only pass + return_to_service_confirmed triggers the transition.
    // fail/monitor records the verification but leaves WO in mechanically_complete.
    if input.result == "pass" && input.return_to_service_confirmed {
        let tv_status_id = resolve_status_id(&txn, "technically_verified").await?;

        // Update recurrence_risk_level on the WO if provided
        let result = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET \
                    status_id = ?, \
                    technically_verified_at = ?, \
                    recurrence_risk_level = COALESCE(?, recurrence_risk_level), \
                    row_version = row_version + 1, \
                    updated_at = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    tv_status_id.into(),
                    now.clone().into(),
                    input
                        .recurrence_risk_level
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

        log_transition(
            &txn,
            input.wo_id,
            &from_code,
            "technically_verified",
            "verify",
            input.verified_by_id,
            None,
            input.notes.as_deref(),
            &now,
        )
        .await?;
    }

    txn.commit().await?;

    // ── Re-fetch verification and WO ──────────────────────────────────────
    let ver_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM work_order_verifications \
             WHERE work_order_id = ? ORDER BY id DESC LIMIT 1",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Failed to re-fetch verification after insert"
            ))
        })?;
    let verification = map_verification(&ver_row)?;

    let wo = queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    Ok((verification, wo))
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) close_wo — quality-gated closure with cost roll-up
// ═══════════════════════════════════════════════════════════════════════════════

/// Pre-flight result accumulating all blocking conditions.
#[derive(Debug, Default)]
struct PreflightResult {
    errors: Vec<String>,
}

impl PreflightResult {
    fn add(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }
    fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Close a WO after passing the mandatory quality gate.
///
/// All blocking conditions are collected and returned together so the
/// user sees every issue at once, not one-at-a-time.
pub async fn close_wo(db: &DatabaseConnection, input: WoCloseInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;

    // ── Guard: technically_verified → closed ──────────────────────────────
    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    guard_wo_transition(&current_status, &WoStatus::Closed)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // ── Load WO type code for type-dependent checks ───────────────────────
    let type_code = load_wo_type_code(&txn, input.wo_id).await?;
    let is_failure_required = FAILURE_REQUIRED_TYPE_CODES.contains(&type_code.as_str());

    let wo_scope = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT closeout_validation_profile_id, production_impact_id \
             FROM work_orders WHERE id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    let profile_id: i64 = wo_scope
        .try_get::<Option<i64>>("", "closeout_validation_profile_id")
        .map_err(|e| decode_err("closeout_validation_profile_id", e))?
        .unwrap_or(1);
    let production_impact_id: Option<i64> = wo_scope
        .try_get::<Option<i64>>("", "production_impact_id")
        .map_err(|e| decode_err("production_impact_id", e))?;

    let policy = load_closeout_policy(&txn, profile_id).await?;

    // ── Quality gate — collect ALL blocking errors ────────────────────────
    let mut preflight = PreflightResult::default();

    // (a) Labor actuals required
    let labor_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                COALESCE(wo.active_labor_hours, 0) AS alh, \
                (SELECT COUNT(*) FROM work_order_interveners WHERE work_order_id = wo.id) AS labor_cnt \
             FROM work_orders wo WHERE wo.id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    let alh: f64 = labor_row
        .try_get::<f64>("", "alh")
        .map_err(|e| decode_err("alh", e))?;
    let labor_cnt: i64 = labor_row
        .try_get::<i64>("", "labor_cnt")
        .map_err(|e| decode_err("labor_cnt", e))?;
    if alh <= 0.0 && labor_cnt == 0 {
        preflight.add("Heures de main-d'oeuvre requises. (Labor actuals required.)");
    }

    // (b) Parts actuals required
    let parts_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                wo.parts_actuals_confirmed AS pac, \
                (SELECT COUNT(*) FROM work_order_parts WHERE work_order_id = wo.id AND quantity_used > 0) AS parts_cnt \
             FROM work_orders wo WHERE wo.id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    let pac: i64 = parts_row
        .try_get::<i64>("", "pac")
        .map_err(|e| decode_err("pac", e))?;
    let parts_cnt: i64 = parts_row
        .try_get::<i64>("", "parts_cnt")
        .map_err(|e| decode_err("parts_cnt", e))?;
    if pac == 0 && parts_cnt == 0 {
        preflight.add("Consommation de pieces requise. (Parts actuals required.)");
    }

    // (c) Failure coding + RCA for corrective/emergency (ISO 14224 close-out)
    if is_failure_required {
        let fd_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT failure_mode_id, failure_cause_id, cause_not_determined, notes \
                 FROM work_order_failure_details WHERE work_order_id = ?",
                [input.wo_id.into()],
            ))
            .await?;

        if let Some(r) = fd_row {
            let failure_mode_id: Option<i64> = r
                .try_get::<Option<i64>>("", "failure_mode_id")
                .map_err(|e| decode_err("failure_mode_id", e))?;
            let failure_cause_id: Option<i64> = r
                .try_get::<Option<i64>>("", "failure_cause_id")
                .map_err(|e| decode_err("failure_cause_id", e))?;
            let cause_not_determined: bool = r
                .try_get::<i64>("", "cause_not_determined")
                .map_err(|e| decode_err("cause_not_determined", e))?
                != 0;
            let fd_notes: Option<String> = r
                .try_get::<Option<String>>("", "notes")
                .map_err(|e| decode_err("notes", e))?;

            if cause_not_determined {
                if !policy.allow_close_with_cause_not_determined {
                    preflight.add(
                        "La politique n'autorise pas la cloture avec cause non determinee. \
                         (Policy disallows close with cause not determined.)",
                    );
                }
                let nmin = policy.notes_min_length_when_cnd.max(1) as usize;
                if fd_notes.as_deref().map_or(true, |s| s.trim().len() < nmin) {
                    preflight.add(format!(
                        "Notes obligatoires (cause non determinee), minimum {nmin} caracteres. \
                         (Notes required when cause not determined.)"
                    ));
                }
            } else {
                if failure_mode_id.is_none() {
                    preflight.add(
                        "Mode de defaillance obligatoire (ISO 14224 — failure mode). \
                         (Failure mode is required.)",
                    );
                }
                if failure_cause_id.is_none() && !policy.allow_close_with_cause_mode_only {
                    preflight.add(
                        "Cause de defaillance obligatoire (ISO 14224 — failure cause), \
                         sauf politique mode-seul. (Failure cause is required.)",
                    );
                }
            }
        } else {
            preflight.add(
                "Codification de defaillance requise pour un OT correctif/urgence. \
                 (Failure coding required for corrective/emergency work.)",
            );
        }

        // (d) Root cause summary
        let rcs_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT root_cause_summary FROM work_orders WHERE id = ?",
                [input.wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: input.wo_id.to_string(),
            })?;
        let rcs: Option<String> = rcs_row
            .try_get::<Option<String>>("", "root_cause_summary")
            .map_err(|e| decode_err("root_cause_summary", e))?;
        if rcs.as_deref().map_or(true, |s| s.trim().is_empty()) {
            preflight.add(
                "Resume de cause racine requis. (Root cause summary required.)",
            );
        }
    }

    // (c2) Downtime or attestation when production impact is recorded
    if policy.require_downtime_if_production_impact && production_impact_id.is_some() {
        let dt_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM work_order_downtime_segments \
                 WHERE work_order_id = ?",
                [input.wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("downtime count")))?;
        let dt_cnt: i64 = dt_row
            .try_get::<i64>("", "cnt")
            .map_err(|e| decode_err("dt_cnt", e))?;
        let attest = input.no_downtime_attestation == Some(true);
        let reason_ok = input
            .no_downtime_attestation_reason
            .as_deref()
            .map(|s| s.trim().len() >= 8)
            .unwrap_or(false);
        if dt_cnt == 0 && !(attest && reason_ok) {
            preflight.add(
                "Temps d'arret production : enregistrer un segment d'arret ou attester \
                 explicitement l'absence d'arret avec justification. \
                 (Downtime segment or explicit no-downtime attestation required.)",
            );
        }
    }

    // (e) Technical verification
    if is_failure_required && policy.require_verification_return_to_service {
        let rts_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM work_order_verifications \
                 WHERE work_order_id = ? AND result = 'pass' AND return_to_service_confirmed = 1",
                [input.wo_id.into()],
            ))
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("Verification RTS count returned no row"))
            })?;
        let rts_cnt: i64 = rts_row
            .try_get::<i64>("", "cnt")
            .map_err(|e| decode_err("rts_cnt", e))?;
        if rts_cnt == 0 {
            preflight.add(
                "Verification avec retour en service confirme requis. \
                 (Verification with return-to-service confirmed is required.)",
            );
        }
    } else {
        let ver_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt \
                 FROM work_order_verifications \
                 WHERE work_order_id = ? AND result IN ('pass', 'monitor')",
                [input.wo_id.into()],
            ))
            .await?
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("Verification count returned no row"))
            })?;
        let ver_cnt: i64 = ver_row
            .try_get::<i64>("", "cnt")
            .map_err(|e| decode_err("ver_cnt", e))?;
        if ver_cnt == 0 {
            preflight.add(
                "Verification technique requise. (Technical verification required.)",
            );
        }
    }

    // ── Return all blocking errors at once ────────────────────────────────
    if !preflight.is_ok() {
        return Err(AppError::ValidationFailed(preflight.errors));
    }

    // Release any remaining WO/PM reservation envelopes during closeout.
    // This keeps inventory state aligned with closed execution context.
    let reservation_rows = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT reservation_id
             FROM work_order_parts
             WHERE work_order_id = ? AND reservation_id IS NOT NULL",
            [input.wo_id.into()],
        ))
        .await?;
    for row in reservation_rows {
        let reservation_id: i64 = row
            .try_get("", "reservation_id")
            .map_err(|e| decode_err("reservation_id", e))?;
        inventory_queries::release_stock_reservation_with_connection(
            &txn,
            reservation_id,
            Some("WO closeout reservation release"),
        )
        .await?;
    }

    // ── Compute final costs ───────────────────────────────────────────────
    let cost_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                COALESCE((SELECT SUM(hours_worked * COALESCE(hourly_rate, 0.0)) \
                          FROM work_order_interveners WHERE work_order_id = ?), 0.0) AS labor_cost, \
                COALESCE((SELECT SUM(quantity_used * COALESCE(unit_cost, 0.0)) \
                          FROM work_order_parts WHERE work_order_id = ?), 0.0) AS parts_cost, \
                COALESCE(wo.service_cost_input, 0.0) AS service_cost, \
                wo.actual_start \
             FROM work_orders wo WHERE wo.id = ?",
            [
                input.wo_id.into(),
                input.wo_id.into(),
                input.wo_id.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    let labor_cost: f64 = cost_row
        .try_get::<f64>("", "labor_cost")
        .map_err(|e| decode_err("labor_cost", e))?;
    let parts_cost: f64 = cost_row
        .try_get::<f64>("", "parts_cost")
        .map_err(|e| decode_err("parts_cost", e))?;
    let service_cost: f64 = cost_row
        .try_get::<f64>("", "service_cost")
        .map_err(|e| decode_err("service_cost", e))?;
    let total_cost = labor_cost + parts_cost + service_cost;

    let actual_start: Option<String> = cost_row
        .try_get::<Option<String>>("", "actual_start")
        .map_err(|e| decode_err("actual_start", e))?;

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Compute actual_duration_hours from actual_start to now
    let actual_duration_hours: Option<f64> = if actual_start.is_some() {
        let dur_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT ROUND((JULIANDAY(?) - JULIANDAY(actual_start)) * 24, 2) AS dur \
                 FROM work_orders WHERE id = ? AND actual_start IS NOT NULL",
                [now.clone().into(), input.wo_id.into()],
            ))
            .await?;
        dur_row.and_then(|r| r.try_get::<Option<f64>>("", "dur").ok().flatten())
    } else {
        None
    };

    // ── Final UPDATE ──────────────────────────────────────────────────────
    let closed_status_id = resolve_status_id(&txn, "closed").await?;

    let no_dt = if input.no_downtime_attestation == Some(true) {
        1i64
    } else {
        0i64
    };
    let no_dt_reason = input.no_downtime_attestation_reason.clone();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                closed_at = ?, \
                labor_cost = ?, \
                parts_cost = ?, \
                service_cost = ?, \
                total_cost = ?, \
                actual_duration_hours = COALESCE(?, actual_duration_hours), \
                closeout_validation_passed = 1, \
                closeout_validation_profile_id = ?, \
                no_downtime_attestation = ?, \
                no_downtime_attestation_reason = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                closed_status_id.into(),
                now.clone().into(),
                labor_cost.into(),
                parts_cost.into(),
                service_cost.into(),
                total_cost.into(),
                actual_duration_hours
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<f64>)),
                profile_id.into(),
                no_dt.into(),
                no_dt_reason
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
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
        "closed",
        "close",
        input.actor_id,
        None,
        None,
        &now,
    )
    .await?;

    txn.commit().await?;

    if let Ok(Some(wo)) = queries::get_work_order(db, input.wo_id).await {
        let status_code = wo.status_code.as_deref().unwrap_or("closed");
        let type_code = wo.type_code.as_deref().unwrap_or("corrective");
        if let Err(e) = sync_stage::stage_work_order_sync(
            db,
            &wo,
            status_code,
            type_code,
            wo.closed_at.as_deref(),
            wo.closeout_validation_profile_id.or(Some(profile_id)),
            wo.closeout_validation_passed,
        )
        .await
        {
            tracing::warn!(target: "maintafox", "stage_work_order_sync after close: {e}");
        }
    }

    if let Err(e) =
        reliability_queries::ingest_failure_event_from_closed_wo(db, input.wo_id, input.actor_id).await
    {
        tracing::warn!(target: "maintafox", "ingest_failure_event_from_closed_wo: {e}");
    }

    let wo_id_str = input.wo_id.to_string();
    let _ = emitter::emit_wo_event(
        db,
        input.wo_id,
        "wo.closed",
        Some(input.actor_id),
        None,
        None,
    )
    .await;

    let actor_i32 = i32::try_from(input.actor_id).unwrap_or(0);
    audit::emit(
        db,
        audit::AuditEvent {
            event_type: "wo.closed",
            actor_id: Some(actor_i32),
            entity_type: Some("work_order"),
            entity_id: Some(wo_id_str.as_str()),
            summary: "Work order closed",
            ..Default::default()
        },
    )
    .await;

    queries::get_work_order(db, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) reopen_wo — closed → technically_verified within recurrence window
// ═══════════════════════════════════════════════════════════════════════════════

/// Reopen a recently-closed WO, reverting it to `technically_verified`.
///
/// Guards:
///   - WO must be in `closed` status
///   - `closed_at` must be within the configurable recurrence window (default 7 days)
///   - All original close-out evidence is preserved
pub async fn reopen_wo(db: &DatabaseConnection, input: WoReopenInput) -> AppResult<WorkOrder> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Une raison de reouverture est obligatoire. (Reopen reason is required.)".to_string(),
        ]));
    }

    let txn = db.begin().await?;

    // ── Guard: must be closed ─────────────────────────────────────────────
    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    if current_status != WoStatus::Closed {
        return Err(AppError::ValidationFailed(vec![format!(
            "Seuls les OT clotures peuvent etre reouverts. Statut actuel : '{from_code}'."
        )]));
    }

    // ── Check recurrence window ───────────────────────────────────────────
    let setting_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = 'wo_reopen_window_days'",
            [],
        ))
        .await?;
    let window_days: i64 = setting_row
        .and_then(|r| r.try_get::<String>("", "setting_value_json").ok())
        .and_then(|v| v.trim_matches('"').parse::<i64>().ok())
        .unwrap_or(7);

    let window_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                closed_at, \
                ROUND(JULIANDAY('now') - JULIANDAY(closed_at), 2) AS days_since_close \
             FROM work_orders WHERE id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    let days_since_close: f64 = window_row
        .try_get::<f64>("", "days_since_close")
        .map_err(|e| decode_err("days_since_close", e))?;

    if days_since_close > window_days as f64 {
        return Err(AppError::ValidationFailed(vec![format!(
            "La fenetre de reouverture ({window_days} jours) est depassee. \
             L'OT a ete cloture il y a {days_since_close:.1} jours. \
             (Reopen window of {window_days} days exceeded.)"
        )]));
    }

    // ── Transition: closed → technically_verified ─────────────────────────
    let tv_status_id = resolve_status_id(&txn, "technically_verified").await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                reopen_count = reopen_count + 1, \
                last_closed_at = closed_at, \
                closed_at = NULL, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                tv_status_id.into(),
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
        "technically_verified",
        "reopen",
        input.actor_id,
        None,
        Some(&input.reason),
        &now,
    )
    .await?;

    txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM failure_events WHERE source_type = 'work_order' AND source_id = ?",
            [input.wo_id.into()],
        ))
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
// E) get_failure_details — list failure detail rows
// ═══════════════════════════════════════════════════════════════════════════════

/// List all failure detail records for a WO.
pub async fn get_failure_details(
    db: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<Vec<WoFailureDetail>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM work_order_failure_details WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await?;

    rows.iter().map(map_failure_detail).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) get_verifications — list verification rows
// ═══════════════════════════════════════════════════════════════════════════════

/// List all verification records for a WO, ordered by most recent first.
pub async fn get_verifications(
    db: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<Vec<WoVerification>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM work_order_verifications \
             WHERE work_order_id = ? ORDER BY verified_at DESC",
            [wo_id.into()],
        ))
        .await?;

    rows.iter().map(map_verification).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) update_wo_rca — persist root cause / corrective action text
// ═══════════════════════════════════════════════════════════════════════════════

/// Write root_cause_summary and corrective_action_summary onto the WO row.
///
/// These are free-text fields required before closure for corrective/emergency
/// WO types. May be called multiple times; only non-null arguments overwrite
/// existing values.
pub async fn update_wo_rca(
    db: &DatabaseConnection,
    input: UpdateWoRcaInput,
) -> AppResult<()> {
    // Guard: WO must not be in closed or cancelled state.
    let (status_code, _status, _rv) = load_wo_status(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de modifier les champs RCA pour un OT en statut '{status_code}'."
        )]));
    }

    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET \
            root_cause_summary        = COALESCE(?, root_cause_summary), \
            corrective_action_summary = COALESCE(?, corrective_action_summary), \
            updated_at                = ? \
         WHERE id = ?",
        [
            input
                .root_cause_summary
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input
                .corrective_action_summary
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            now.into(),
            input.wo_id.into(),
        ],
    ))
    .await?;

    Ok(())
}
