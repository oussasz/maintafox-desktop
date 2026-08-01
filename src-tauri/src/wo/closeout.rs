//! WO close-out — failure detail capture, technical verification, closure quality gate,
//! and reopen logic.
//!
//! Phase 2 - Sub-phase 05 - File 03 - Sprint S1.
//!
//! Functions:
//!   save_failure_detail     — upsert structured failure taxonomy (symptom/mode/cause/effect)
//!   save_verification       — supervisor review action on completed (no status change)
//!   close_wo                — quality-gated closure completed → closed
//!   reopen_wo               — completed → in_progress or planning by reason
//!   get_failure_details     — list failure detail rows for a WO
//!   get_verifications       — list verification rows for a WO

use crate::activity::emitter;
use crate::audit;
use crate::auth::password;
use crate::errors::{AppError, AppResult};
use crate::inventory::queries as inventory_queries;
use crate::reliability::advanced_rams::queries as advanced_rams_queries;
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;
use crate::reliability::queries as reliability_queries;
use crate::wo::queries;
use crate::wo::sync_stage;
use crate::wo::time::now_utc_z;
use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
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
    #[serde(default)]
    pub failure_mode_label: Option<String>,
    #[serde(default)]
    pub failure_cause_label: Option<String>,
    #[serde(default)]
    pub failure_effect_label: Option<String>,
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
    #[serde(default)]
    pub fmeca_parts_override_reason: Option<String>,
    #[serde(default)]
    pub fmeca_parts_override_signed_by_id: Option<i64>,
    #[serde(default)]
    pub fmeca_parts_override_signer_password: Option<String>,
}

/// Input for reopening a recently closed WO.
#[derive(Debug, Clone, Deserialize)]
pub struct WoReopenInput {
    pub wo_id: i64,
    pub actor_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
    /// Target status: `in_progress` (default) or `planning`.
    #[serde(default)]
    pub target_status: Option<String>,
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

const FAILURE_DETAIL_SELECT: &str = "\
    SELECT fd.id, fd.work_order_id, fd.symptom_id, fd.failure_mode_id, fd.failure_cause_id, \
           fd.failure_effect_id, fd.is_temporary_repair, fd.is_permanent_repair, \
           fd.cause_not_determined, fd.notes, \
           fm.label AS failure_mode_label, \
           fc.label AS failure_cause_label, \
           fe.label AS failure_effect_label \
    FROM work_order_failure_details fd \
    LEFT JOIN reference_values fm ON fm.id = fd.failure_mode_id \
    LEFT JOIN reference_values fc ON fc.id = fd.failure_cause_id \
    LEFT JOIN reference_values fe ON fe.id = fd.failure_effect_id";

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WO closeout row decode failed for column '{column}': {e}"
    ))
}

async fn validate_failure_mode_governance(
    db: &impl ConnectionTrait,
    failure_mode_id: Option<i64>,
) -> AppResult<Option<String>> {
    let published_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM reference_values rv
             INNER JOIN reference_sets rs ON rs.id = rv.set_id
             INNER JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
               AND rs.status = 'published'
               AND rv.is_active = 1",
            [],
        ))
        .await?;
    let published_count: i64 = published_row
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("WORK.FAILURE_MODES published count missing")))?
        .try_get("", "c")
        .map_err(|e| decode_err("work_failure_modes_published_count", e))?;
    if published_count <= 0 {
        return Ok(Some("GATE_CLOSEOUT_REFERENCE_MODES_NOT_PUBLISHED".into()));
    }

    if let Some(mode_id) = failure_mode_id {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c
                 FROM reference_values rv
                 INNER JOIN reference_sets rs ON rs.id = rv.set_id
                 INNER JOIN reference_domains rd ON rd.id = rs.domain_id
                 WHERE rv.id = ?
                   AND rv.is_active = 1
                   AND rs.status = 'published'
                   AND UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))",
                [mode_id.into()],
            ))
            .await?;
        let governed_count: i64 = row
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failure mode governance count missing")))?
            .try_get("", "c")
            .map_err(|e| decode_err("governed_failure_mode_count", e))?;
        if governed_count <= 0 {
            return Ok(Some(format!(
                "GATE_CLOSEOUT_FAILURE_MODE_NOT_GOVERNED:failure_mode_id={mode_id}"
            )));
        }
    }
    Ok(None)
}

fn map_failure_detail(row: &sea_orm::QueryResult) -> AppResult<WoFailureDetail> {
    Ok(WoFailureDetail {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
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
        failure_mode_label: row.try_get::<Option<String>>("", "failure_mode_label").unwrap_or(None),
        failure_cause_label: row.try_get::<Option<String>>("", "failure_cause_label").unwrap_or(None),
        failure_effect_label: row
            .try_get::<Option<String>>("", "failure_effect_label")
            .unwrap_or(None),
    })
}

fn map_verification(row: &sea_orm::QueryResult) -> AppResult<WoVerification> {
    Ok(WoVerification {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
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

/// Resolve `status_id` for a given status code.
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
    require_fmeca_parts_for_critical: bool,
    fmeca_parts_override_reason_min_length: i64,
    fmeca_parts_override_require_distinct_signer: bool,
    fmeca_parts_override_allowed_roles: Vec<String>,
}

async fn load_closeout_policy(txn: &impl ConnectionTrait, policy_id: i64) -> AppResult<CloseoutPolicyRow> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, require_downtime_if_production_impact, \
             allow_close_with_cause_not_determined, allow_close_with_cause_mode_only, \
             require_verification_return_to_service, notes_min_length_when_cnd, \
             require_fmeca_parts_for_critical, fmeca_parts_override_reason_min_length, \
             fmeca_parts_override_require_distinct_signer, fmeca_parts_override_allowed_roles_json \
             FROM closeout_validation_policies WHERE id = ?",
            [policy_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("closeout_validation_policies missing id {policy_id}")))?;

    let allowed_roles_json: String = row
        .try_get::<Option<String>>("", "fmeca_parts_override_allowed_roles_json")
        .map_err(|e| decode_err("fmeca_parts_override_allowed_roles_json", e))?
        .unwrap_or_else(|| "[\"Supervisor\",\"Maintenance Supervisor\",\"Administrator\",\"Superadmin\"]".into());
    let allowed_roles = serde_json::from_str::<Vec<String>>(&allowed_roles_json)
        .unwrap_or_else(|_| {
            vec![
                "Supervisor".into(),
                "Maintenance Supervisor".into(),
                "Administrator".into(),
                "Superadmin".into(),
            ]
        })
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>();

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
        require_fmeca_parts_for_critical: row
            .try_get::<i64>("", "require_fmeca_parts_for_critical")
            .map_err(|e| decode_err("require_fmeca_parts_for_critical", e))?
            != 0,
        fmeca_parts_override_reason_min_length: row
            .try_get::<i64>("", "fmeca_parts_override_reason_min_length")
            .map_err(|e| decode_err("fmeca_parts_override_reason_min_length", e))?,
        fmeca_parts_override_require_distinct_signer: row
            .try_get::<i64>("", "fmeca_parts_override_require_distinct_signer")
            .map_err(|e| decode_err("fmeca_parts_override_require_distinct_signer", e))?
            != 0,
        fmeca_parts_override_allowed_roles: allowed_roles,
    })
}

async fn has_active_user(db: &impl ConnectionTrait, user_id: i64) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM user_accounts WHERE id = ? AND is_active = 1 AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("active user count missing")))?;
    let count: i64 = row.try_get("", "c").map_err(|e| decode_err("active_user_count", e))?;
    Ok(count > 0)
}

async fn verify_user_password(db: &impl ConnectionTrait, user_id: i64, password_raw: &str) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT password_hash
             FROM user_accounts
             WHERE id = ? AND is_active = 1 AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let hash: Option<String> = row
        .try_get("", "password_hash")
        .map_err(|e| decode_err("override_signer_password_hash", e))?;
    let Some(hash) = hash else {
        return Ok(false);
    };
    password::verify_password(password_raw, &hash)
}

async fn has_supervisor_override_role(
    db: &impl ConnectionTrait,
    user_id: i64,
    allowed_role_names: &[String],
) -> AppResult<bool> {
    if allowed_role_names.is_empty() {
        return Ok(false);
    }
    let role_list = allowed_role_names
        .iter()
        .map(|v| format!("'{}'", v.replace('\'', "''").to_lowercase()))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT COUNT(*) AS c
         FROM user_scope_assignments usa
         INNER JOIN roles r ON r.id = usa.role_id
         WHERE usa.user_id = ?
           AND usa.deleted_at IS NULL
           AND r.deleted_at IS NULL
           AND LOWER(TRIM(r.name)) IN ({role_list})"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("override role count missing")))?;
    let count: i64 = row.try_get("", "c").map_err(|e| decode_err("override_role_count", e))?;
    Ok(count > 0)
}

async fn has_critical_fmeca_part_suggestions(
    db: &impl ConnectionTrait,
    equipment_id: i64,
    failure_mode_id: i64,
    rpn_critical: i64,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM fmeca_analyses fa
             INNER JOIN fmeca_items fi ON fi.analysis_id = fa.id
             INNER JOIN fmeca_item_parts fip ON fip.fmeca_item_id = fi.id
             WHERE fa.equipment_id = ?
               AND fi.failure_mode_id = ?
               AND COALESCE(fi.rpn, COALESCE(fi.severity, 0) * COALESCE(fi.occurrence, 0) * COALESCE(fi.detectability, 0)) >= ?",
            [equipment_id.into(), failure_mode_id.into(), rpn_critical.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("critical fmeca suggestion count missing")))?;
    let count: i64 = row
        .try_get("", "c")
        .map_err(|e| decode_err("critical_fmeca_suggestion_count", e))?;
    Ok(count > 0)
}

async fn has_consumed_critical_suggested_part(
    db: &impl ConnectionTrait,
    wo_id: i64,
    equipment_id: i64,
    failure_mode_id: i64,
    rpn_critical: i64,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM work_order_parts wop
             WHERE wop.work_order_id = ?
               AND COALESCE(wop.quantity_used, 0) > 0
               AND wop.article_id IN (
                 SELECT DISTINCT fip.article_id
                 FROM fmeca_analyses fa
                 INNER JOIN fmeca_items fi ON fi.analysis_id = fa.id
                 INNER JOIN fmeca_item_parts fip ON fip.fmeca_item_id = fi.id
                 WHERE fa.equipment_id = ?
                   AND fi.failure_mode_id = ?
                   AND COALESCE(fi.rpn, COALESCE(fi.severity, 0) * COALESCE(fi.occurrence, 0) * COALESCE(fi.detectability, 0)) >= ?
               )",
            [
                wo_id.into(),
                equipment_id.into(),
                failure_mode_id.into(),
                rpn_critical.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("consumed critical suggested part count missing")))?;
    let count: i64 = row
        .try_get("", "c")
        .map_err(|e| decode_err("consumed_critical_suggested_part_count", e))?;
    Ok(count > 0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) save_failure_detail — upsert failure taxonomy
// ═══════════════════════════════════════════════════════════════════════════════

/// Upsert a failure detail record for a WO (one row per WO).
/// WO must not be in closed or cancelled state.
pub async fn save_failure_detail(db: &DatabaseConnection, input: SaveFailureDetailInput) -> AppResult<WoFailureDetail> {
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
        let existing_id: i64 = existing_row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?;

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
            &format!("{FAILURE_DETAIL_SELECT} WHERE fd.work_order_id = ?"),
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-fetch failure detail after upsert")))?;

    map_failure_detail(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) save_verification — supervisor review on completed (no status change)
// ═══════════════════════════════════════════════════════════════════════════════

/// Record a supervisor verification while WO stays in `completed`.
///
/// Enforces:
///   - WO must be in `completed`
///   - `result` must be one of: pass, fail, monitor
///   - `verified_by_id` must differ from `primary_responsible_id` (no self-verification)
///   - Pass + return_to_service stamps `technically_verified_at` but does not change status
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

    // ── Guard: WO must be in completed ────────────────────────────────────
    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    if current_status != WoStatus::Completed {
        return Err(AppError::ValidationFailed(vec![format!(
            "La revue superviseur n'est disponible qu'en statut completed (actuel: '{from_code}')."
        )]));
    }
    use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
    assert_action_allowed(&current_status, WoAction::SupervisorReview)
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
    let now = now_utc_z();

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

    // Stamp review fields; status stays completed until Close.
    if input.result == "pass" && input.return_to_service_confirmed {
        let result = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET \
                    technically_verified_at = ?, \
                    recurrence_risk_level = COALESCE(?, recurrence_risk_level), \
                    row_version = row_version + 1, \
                    updated_at = ? \
                 WHERE id = ? AND row_version = ?",
                [
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
    }

    crate::wo::workflow::events::emit_action_event(
        &txn,
        input.wo_id,
        "review_recorded",
        Some(input.verified_by_id),
        Some(&from_code),
        Some(&from_code),
        None,
    )
    .await?;

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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-fetch verification after insert")))?;
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
    // Load settings before opening the transaction — SQLite single-connection pools
    // deadlock if we call back into `db` while `txn` holds the only connection.
    let rpn_critical = advanced_rams_queries::fmeca_rpn_critical_threshold_i64(db).await?;

    let txn = db.begin().await?;

    // ── Guard: completed → closed ─────────────────────────────────────────
    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
    assert_action_allowed(&current_status, WoAction::Close).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::Closed).map_err(|e| AppError::ValidationFailed(vec![e]))?;

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
    let alh: f64 = labor_row.try_get::<f64>("", "alh").map_err(|e| decode_err("alh", e))?;
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
    let pac: i64 = parts_row.try_get::<i64>("", "pac").map_err(|e| decode_err("pac", e))?;
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
                if let Some(gov_err) = validate_failure_mode_governance(&txn, failure_mode_id).await? {
                    preflight.add(gov_err);
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
            preflight.add("Resume de cause racine requis. (Root cause summary required.)");
        }
    }

    // (c3) Critical FMECA suggested-part requirement
    if is_failure_required && policy.require_fmeca_parts_for_critical {
        let row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT wo.equipment_id, fd.failure_mode_id
                 FROM work_orders wo
                 LEFT JOIN work_order_failure_details fd ON fd.work_order_id = wo.id
                 WHERE wo.id = ?",
                [input.wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: input.wo_id.to_string(),
            })?;
        let equipment_id: Option<i64> = row
            .try_get::<Option<i64>>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?;
        let failure_mode_id: Option<i64> = row
            .try_get::<Option<i64>>("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?;

        if let (Some(eq_id), Some(mode_id)) = (equipment_id, failure_mode_id) {
            let has_suggestions = has_critical_fmeca_part_suggestions(&txn, eq_id, mode_id, rpn_critical).await?;
            if has_suggestions {
                let has_matched_consumed_parts =
                    has_consumed_critical_suggested_part(&txn, input.wo_id, eq_id, mode_id, rpn_critical).await?;
                let reason_min = policy.fmeca_parts_override_reason_min_length.max(1) as usize;
                let override_reason_ok = input
                    .fmeca_parts_override_reason
                    .as_deref()
                    .map(|s| s.trim().len() >= reason_min)
                    .unwrap_or(false);
                let signer_id_opt = input.fmeca_parts_override_signed_by_id;
                let signer_id = signer_id_opt.unwrap_or_default();
                let signer_exists = signer_id > 0 && has_active_user(&txn, signer_id).await?;
                let signer_has_role = signer_exists
                    && has_supervisor_override_role(&txn, signer_id, &policy.fmeca_parts_override_allowed_roles)
                        .await?;
                let signer_is_distinct = !policy.fmeca_parts_override_require_distinct_signer
                    || (signer_id > 0 && signer_id != input.actor_id);
                let signer_password_ok = if signer_exists {
                    match input.fmeca_parts_override_signer_password.as_deref() {
                        Some(raw) if !raw.trim().is_empty() => verify_user_password(&txn, signer_id, raw).await?,
                        _ => false,
                    }
                } else {
                    false
                };
                let signer_ok = signer_exists && signer_has_role && signer_is_distinct && signer_password_ok;

                if !(has_matched_consumed_parts || (override_reason_ok && signer_ok)) {
                    preflight.add("GATE_CLOSEOUT_FMECA_PARTS_REQUIRED");
                }
                if !has_matched_consumed_parts && !override_reason_ok {
                    preflight.add(format!(
                        "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_REASON_REQUIRED:min_len={reason_min}"
                    ));
                }
                if !has_matched_consumed_parts && !signer_ok {
                    preflight.add("GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNATURE_REQUIRED");
                }
                if !has_matched_consumed_parts && signer_id_opt == Some(input.actor_id) {
                    preflight.add("GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNER_MUST_DIFFER");
                }
                if !has_matched_consumed_parts && signer_exists && !signer_has_role {
                    preflight.add("GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNER_ROLE_INVALID");
                }
                if !has_matched_consumed_parts
                    && signer_exists
                    && signer_has_role
                    && signer_is_distinct
                    && !signer_password_ok
                {
                    preflight.add("GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNER_AUTH_FAILED");
                }
            }
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
        let dt_cnt: i64 = dt_row.try_get::<i64>("", "cnt").map_err(|e| decode_err("dt_cnt", e))?;
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
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Verification RTS count returned no row")))?;
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
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Verification count returned no row")))?;
        let ver_cnt: i64 = ver_row
            .try_get::<i64>("", "cnt")
            .map_err(|e| decode_err("ver_cnt", e))?;
        if ver_cnt == 0 {
            preflight.add("Verification technique requise. (Technical verification required.)");
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
            [input.wo_id.into(), input.wo_id.into(), input.wo_id.into()],
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

    let now = now_utc_z();

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
    let fmeca_override_reason = input.fmeca_parts_override_reason.clone();
    let fmeca_override_signed_by_id = input.fmeca_parts_override_signed_by_id;

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
                fmeca_parts_override_reason = ?, \
                fmeca_parts_override_signed_by_id = ?, \
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
                fmeca_override_reason
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<String>)),
                fmeca_override_signed_by_id
                    .map(sea_orm::Value::from)
                    .unwrap_or(sea_orm::Value::from(None::<i64>)),
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

    if let Err(e) = reliability_queries::ingest_failure_event_from_closed_wo(db, input.wo_id, input.actor_id).await {
        tracing::warn!(target: "maintafox", "ingest_failure_event_from_closed_wo: {e}");
    }

    // Optimistic immediate RAMS KPI refresh for the impacted equipment.
    if let Ok(Some(wo)) = queries::get_work_order(db, input.wo_id).await {
        if let Some(equipment_id) = wo.equipment_id {
            let period_end = Utc::now();
            let period_start = period_end - Duration::days(365);
            let refresh_input = RefreshReliabilityKpiSnapshotInput {
                equipment_id,
                period_start: period_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                period_end: period_end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                min_sample_n: Some(1),
                repeat_lookback_days: Some(30),
            };
            if let Err(e) = reliability_queries::refresh_reliability_kpi_snapshot(db, refresh_input).await {
                tracing::warn!(target: "maintafox", "refresh_reliability_kpi_snapshot after close: {e}");
            }
        }
    }

    let wo_id_str = input.wo_id.to_string();
    let _ = emitter::emit_wo_event(db, input.wo_id, "wo.closed", Some(input.actor_id), None, None).await;

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
// D) reopen_wo — completed → in_progress or planning
// ═══════════════════════════════════════════════════════════════════════════════

/// Reopen a completed WO to execution (`in_progress`) or plan (`planning`).
pub async fn reopen_wo(db: &DatabaseConnection, input: WoReopenInput) -> AppResult<WorkOrder> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Une raison de reouverture est obligatoire. (Reopen reason is required.)".to_string(),
        ]));
    }

    let target = match input.target_status.as_deref().unwrap_or("in_progress") {
        "planning" => WoStatus::Planning,
        "in_progress" => WoStatus::InProgress,
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Cible de reouverture invalide '{other}'. Utilisez 'in_progress' ou 'planning'."
            )]));
        }
    };

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
    assert_action_allowed(&current_status, WoAction::Reopen).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    if current_status != WoStatus::Completed {
        return Err(AppError::ValidationFailed(vec![format!(
            "Seuls les OT completed peuvent etre reouverts. Statut actuel : '{from_code}'."
        )]));
    }
    guard_wo_transition(&current_status, &target).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    let target_status_id = resolve_status_id(&txn, target.as_str()).await?;
    let now = now_utc_z();

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                reopen_count = reopen_count + 1, \
                mechanically_completed_at = NULL, \
                technically_verified_at = NULL, \
                actual_end = NULL, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                target_status_id.into(),
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
        target.as_str(),
        "reopen",
        input.actor_id,
        None,
        Some(&input.reason),
        &now,
    )
    .await?;

    crate::wo::workflow::events::emit_action_event(
        &txn,
        input.wo_id,
        "reopened",
        Some(input.actor_id),
        Some(&from_code),
        Some(target.as_str()),
        Some(&input.reason),
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
// E) get_failure_details — list failure detail rows
// ═══════════════════════════════════════════════════════════════════════════════

/// List all failure detail records for a WO.
pub async fn get_failure_details(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<WoFailureDetail>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!("{FAILURE_DETAIL_SELECT} WHERE fd.work_order_id = ?"),
            [wo_id.into()],
        ))
        .await?;

    rows.iter().map(map_failure_detail).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) get_verifications — list verification rows
// ═══════════════════════════════════════════════════════════════════════════════

/// List all verification records for a WO, ordered by most recent first.
pub async fn get_verifications(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<WoVerification>> {
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
pub async fn update_wo_rca(db: &DatabaseConnection, input: UpdateWoRcaInput) -> AppResult<()> {
    // Guard: WO must not be in closed or cancelled state.
    let (status_code, _status, _rv) = load_wo_status(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible de modifier les champs RCA pour un OT en statut '{status_code}'."
        )]));
    }

    let now = now_utc_z();

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
