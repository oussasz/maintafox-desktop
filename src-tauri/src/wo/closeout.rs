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
use crate::auth::second_signer;
use crate::errors::{AppError, AppResult};
use crate::reliability::advanced_rams::queries as advanced_rams_queries;
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;
use crate::reliability::queries as reliability_queries;
use crate::wo::queries;
use crate::wo::time::{now_utc_second, now_utc_z, stamp};
use crate::wo::workflow::state_machine::status_i18n_key;
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
    /// ISO 14224 B.2 failure mechanism (`WORK.FAILURE_MECHANISMS`).
    #[serde(default)]
    pub failure_mechanism_id: Option<i64>,
    #[serde(default)]
    pub failure_mechanism_label: Option<String>,
    /// ISO 14224 B.4 detection method (`WORK.DETECTION_METHODS`).
    #[serde(default)]
    pub detection_method_id: Option<i64>,
    #[serde(default)]
    pub detection_method_label: Option<String>,
}

/// IPC input for saving failure coding: the base taxonomy plus the ISO 14224 mechanism and
/// detection method (kept apart so existing `SaveFailureDetailInput` callers are unchanged).
#[derive(Debug, Clone, Deserialize)]
pub struct SaveFailureCodingInput {
    #[serde(flatten)]
    pub base: SaveFailureDetailInput,
    #[serde(default)]
    pub failure_mechanism_id: Option<i64>,
    #[serde(default)]
    pub detection_method_id: Option<i64>,
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
    /// Version of the work order the screen loaded: the save is refused when someone changed the
    /// work order since (another person, or another PC through sync), and it bumps the version.
    pub expected_row_version: i64,
}

/// A saved failure detail with the work order's new version, so the close-out screen can chain
/// its next version-checked save (root cause, service cost) without reloading.
#[derive(Debug, Clone, Serialize)]
pub struct FailureDetailSaved {
    #[serde(flatten)]
    pub detail: WoFailureDetail,
    pub wo_row_version: i64,
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

/// Tenant setting (`app_settings`, seeded): days after closure during which a closed WO may
/// still be reopened (PRD §6.5 "configurable recurrence window").
pub const SETTING_REOPEN_WINDOW_DAYS: &str = "wo.reopen_window_days";

/// Reads the reopen window. A missing or malformed setting is an error: a closed WO is never
/// reopened on a guessed window.
pub async fn reopen_window_days(db: &impl ConnectionTrait) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings WHERE setting_key = ? AND setting_scope = 'tenant'",
            [SETTING_REOPEN_WINDOW_DAYS.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "GATE_REOPEN_WINDOW_NOT_SET: Setting {SETTING_REOPEN_WINDOW_DAYS} is missing; a closed work order cannot be reopened."
            )])
        })?;
    let raw: String = row
        .try_get("", "setting_value_json")
        .map_err(|e| decode_err("setting_value_json", e))?;
    let days: i64 = serde_json::from_str(raw.trim()).map_err(|_| {
        AppError::ValidationFailed(vec![format!(
            "GATE_REOPEN_WINDOW_INVALID: Setting {SETTING_REOPEN_WINDOW_DAYS} must be a whole number of days (got {raw})."
        )])
    })?;
    if days < 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_REOPEN_WINDOW_NEGATIVE: Setting {SETTING_REOPEN_WINDOW_DAYS} must not be negative."
        )]));
    }
    Ok(days)
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
    /// Reopening to in progress: estimated remaining time (hours, > 0) for the permit gate instead
    /// of the full expected duration (`permit::wo_gate`); recorded on the reopening's
    /// transition-log row, never written over the planned or expected duration. Absent: the full
    /// expected duration.
    #[serde(default)]
    pub estimated_remaining_hours: Option<f64>,
}

/// Input for updating root cause analysis fields on the WO.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateWoRcaInput {
    pub wo_id: i64,
    pub root_cause_summary: Option<String>,
    pub corrective_action_summary: Option<String>,
    /// Version of the work order the screen loaded (refused when changed since; bumped).
    pub expected_row_version: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

const VALID_VERIFICATION_RESULTS: &[&str] = &["pass", "fail", "monitor"];

const VALID_RECURRENCE_LEVELS: &[&str] = &["none", "low", "medium", "high"];

/// The one failure-coding row of work order `wo` (an SQL expression) that every reader uses — the
/// close-out form and gate, the execution gate, the failure event, the reliability and FMECA
/// figures — so that they read the same row on every PC when two PCs each coded the same work
/// order before they synced (#185 N-R2-2 interim, CMMS Advisor 2026-10-27: earliest created_at,
/// then entity_sync_id). `work_order_failure_details` has no `created_at` column, so the order is
/// by `entity_sync_id` (the same on every PC); `id` (local) only breaks a tie between rows not yet
/// given a sync identity. Rows are neither merged nor deleted here.
pub(crate) fn failure_detail_of_wo_sql(wo: &str) -> String {
    format!(
        "(SELECT x.id FROM work_order_failure_details x WHERE x.work_order_id = {wo} \
         ORDER BY x.entity_sync_id, x.id LIMIT 1)"
    )
}

const FAILURE_DETAIL_SELECT: &str = "\
    SELECT fd.id, fd.work_order_id, fd.symptom_id, fd.failure_mode_id, fd.failure_cause_id, \
           fd.failure_effect_id, fd.is_temporary_repair, fd.is_permanent_repair, \
           fd.cause_not_determined, fd.notes, \
           fm.label AS failure_mode_label, \
           fc.label AS failure_cause_label, \
           fe.label AS failure_effect_label, \
           fd.failure_mechanism_id, fmech.label AS failure_mechanism_label, \
           fd.detection_method_id, fdet.label AS detection_method_label \
    FROM work_order_failure_details fd \
    LEFT JOIN reference_values fm ON fm.id = fd.failure_mode_id \
    LEFT JOIN reference_values fc ON fc.id = fd.failure_cause_id \
    LEFT JOIN reference_values fe ON fe.id = fd.failure_effect_id \
    LEFT JOIN reference_values fmech ON fmech.id = fd.failure_mechanism_id \
    LEFT JOIN reference_values fdet ON fdet.id = fd.detection_method_id";

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
        failure_mechanism_id: row.try_get::<Option<i64>>("", "failure_mechanism_id").unwrap_or(None),
        failure_mechanism_label: row
            .try_get::<Option<String>>("", "failure_mechanism_label")
            .unwrap_or(None),
        detection_method_id: row.try_get::<Option<i64>>("", "detection_method_id").unwrap_or(None),
        detection_method_label: row
            .try_get::<Option<String>>("", "detection_method_label")
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
pub(crate) fn check_concurrency(rows_affected: u64) -> AppResult<()> {
    if rows_affected == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a ete modifie par un autre utilisateur. \
             Veuillez recharger et reessayer."
                .to_string(),
        ]));
    }
    Ok(())
}

/// Claims the work order at the version the screen loaded: bumps `row_version` (and
/// `updated_at`) only when it is still `expected_row_version`, else the usual
/// `GATE_RECORD_CHANGED_ELSEWHERE` refusal. Returns the new version. Run inside the transaction
/// of the write it guards so a refused claim leaves nothing behind.
pub(crate) async fn claim_wo_version(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    expected_row_version: i64,
) -> AppResult<i64> {
    let result = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET row_version = row_version + 1, updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [now_utc_z().into(), wo_id.into(), expected_row_version.into()],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;
    Ok(expected_row_version + 1)
}

/// Current `row_version` of a work order (tests that chain version-checked writes).
#[cfg(test)]
pub(crate) async fn wo_row_version(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<i64> {
    Ok(load_wo_status(conn, wo_id).await?.2)
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

/// The account whose password the close-out override checks (the signer), when one is given.
fn signer_password_account(input: &WoCloseInput) -> Option<i32> {
    let has_password = input
        .fmeca_parts_override_signer_password
        .as_deref()
        .is_some_and(|p| !p.trim().is_empty());
    input
        .fmeca_parts_override_signed_by_id
        .filter(|id| *id > 0 && has_password)
        .and_then(|id| i32::try_from(id).ok())
}

/// The signer's password outcome, counted toward the signer's account lockout as at sign-in
/// (`auth::second_signer::record_outcome`): written outside the close-out transaction, so a
/// refused close-out still counts. When the signer is the signed-in user (self-signing allowed by
/// the policy), the command ends the session if this locks the account (`commands::wo::close_wo`).
async fn record_signer_password_outcome(
    db: &DatabaseConnection,
    input: &WoCloseInput,
    signer: Option<i32>,
    checked: Option<bool>,
) -> AppResult<()> {
    second_signer::record_outcome(
        db,
        signer,
        checked,
        &second_signer::SignatureContext {
            actor_id: input.actor_id,
            context: "closeout_override",
            summary: "Close-out override refused: wrong signer password",
            detail: serde_json::json!({ "work_order_id": input.wo_id }),
        },
    )
    .await
}

pub(crate) async fn has_supervisor_override_role(
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
    // An assignment in force only: an expired or lapsed emergency grant signs nothing.
    let sql = format!(
        "SELECT COUNT(*) AS c
         FROM user_scope_assignments usa
         INNER JOIN roles r ON r.id = usa.role_id
         WHERE usa.user_id = ?
           AND {}
           AND r.deleted_at IS NULL
           AND LOWER(TRIM(r.name)) IN ({role_list})",
        crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
    );
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(crate::rbac::resolver::in_force_binds(&now));
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
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
               AND fi.review_required = 0
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
                   AND fi.review_required = 0
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
/// WO must not be in closed or cancelled state, and still at the version the screen loaded.
pub async fn save_failure_detail(
    db: &DatabaseConnection,
    input: SaveFailureDetailInput,
) -> AppResult<FailureDetailSaved> {
    let wo_id = input.wo_id;
    let txn = db.begin().await?;
    let wo_row_version = upsert_failure_detail(&txn, input).await?;
    let detail = load_failure_detail(&txn, wo_id).await?;
    txn.commit().await?;
    Ok(FailureDetailSaved { detail, wo_row_version })
}

async fn load_failure_detail(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoFailureDetail> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{FAILURE_DETAIL_SELECT} WHERE fd.id = {}",
                failure_detail_of_wo_sql("?")
            ),
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-fetch failure detail after upsert")))?;
    map_failure_detail(&row)
}

/// Guards, claims the WO version and upserts the row; returns the WO's new version. The caller
/// owns the transaction.
async fn upsert_failure_detail(db: &impl ConnectionTrait, input: SaveFailureDetailInput) -> AppResult<i64> {
    // ── Validate: cannot be both temporary AND permanent ──────────────────
    if input.is_temporary_repair && input.is_permanent_repair {
        return Err(AppError::ValidationFailed(vec![
            "GATE_CLOSEOUT_ACTION_TEMP_AND_PERM: L'action ne peut pas etre a la fois temporaire et permanente. \
             (Action cannot be both temporary and permanent.)"
                .to_string(),
        ]));
    }

    // ── Guard: WO not closed/cancelled ────────────────────────────────────
    let (status_code, _status, _rv) = load_wo_status(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_CLOSEOUT_FAILURE_DETAILS_LOCKED:status={}: Impossible de modifier les details de defaillance pour un OT en statut '{status_code}'.",
            status_i18n_key(&status_code)
        )]));
    }
    // ── Guard: nobody changed the WO since the screen loaded it ───────────
    let wo_row_version = claim_wo_version(db, input.wo_id, input.expected_row_version).await?;

    let temp = if input.is_temporary_repair { 1i64 } else { 0 };
    let perm = if input.is_permanent_repair { 1i64 } else { 0 };
    let cnd = if input.cause_not_determined { 1i64 } else { 0 };

    // ── Check for existing row (upsert) ───────────────────────────────────
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT id FROM work_order_failure_details WHERE id = {}",
                failure_detail_of_wo_sql("?")
            ),
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

    Ok(wo_row_version)
}

/// True when `id` is an active value of the reference domain `domain_code`.
async fn is_value_of_domain(db: &impl ConnectionTrait, id: i64, domain_code: &str) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE rv.id = ? AND rv.is_active = 1 AND UPPER(TRIM(d.code)) = ?",
            [id.into(), domain_code.into()],
        ))
        .await?;
    Ok(row.is_some())
}

/// Save failure coding including the ISO 14224 mechanism and detection method.
/// One transaction with the version claim: a refused save writes nothing.
pub async fn save_failure_coding(
    db: &DatabaseConnection,
    input: SaveFailureCodingInput,
) -> AppResult<FailureDetailSaved> {
    use crate::wo::closeout_reference_catalog::{DOMAIN_DETECTION_METHODS, DOMAIN_FAILURE_MECHANISMS};
    let mut errors = Vec::new();
    if let Some(id) = input.failure_mechanism_id {
        if !is_value_of_domain(db, id, DOMAIN_FAILURE_MECHANISMS).await? {
            errors.push("GATE_CLOSEOUT_MECHANISM_UNKNOWN: Le mécanisme de défaillance choisi n'existe plus dans le référentiel.".to_string());
        }
    }
    if let Some(id) = input.detection_method_id {
        if !is_value_of_domain(db, id, DOMAIN_DETECTION_METHODS).await? {
            errors.push(
                "GATE_CLOSEOUT_DETECTION_UNKNOWN: La méthode de détection choisie n'existe plus dans le référentiel."
                    .to_string(),
            );
        }
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    let wo_id = input.base.wo_id;
    let txn = db.begin().await?;
    let wo_row_version = upsert_failure_detail(&txn, input.base).await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_failure_details SET failure_mechanism_id = ?, detection_method_id = ? \
         WHERE work_order_id = ?",
        [
            input.failure_mechanism_id.into(),
            input.detection_method_id.into(),
            wo_id.into(),
        ],
    ))
    .await?;
    let detail = load_failure_detail(&txn, wo_id).await?;
    txn.commit().await?;
    Ok(FailureDetailSaved { detail, wo_row_version })
}

/// A verifier who is the WO's primary responsible.
pub(crate) const VERIFIER_IS_RESPONSIBLE: &str = "WO_SELF_VERIFICATION";
/// Without a responsible, a verifier who recorded labour on the WO (CMMS Advisor D7).
pub(crate) const VERIFIER_RECORDED_LABOUR: &str = "WO_VERIFIER_RECORDED_LABOUR";

/// Why `verifier_user_id` may not verify the WO, if so: they are its primary responsible; or the
/// WO has no responsible and they recorded labour on it (as the intervener user, or as the
/// personnel record of their account). Used by the local save and by the re-check of a received
/// verification (`wo::closeout_sync`).
pub(crate) async fn verifier_conflict(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    verifier_user_id: i64,
) -> AppResult<Option<&'static str>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT primary_responsible_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let responsible: Option<i64> = row
        .try_get("", "primary_responsible_id")
        .map_err(|e| decode_err("primary_responsible_id", e))?;
    if let Some(resp_id) = responsible {
        return Ok((resp_id == verifier_user_id).then_some(VERIFIER_IS_RESPONSIBLE));
    }
    let worked = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS w FROM work_order_interveners i \
             WHERE i.work_order_id = ? \
               AND (i.intervener_id = ? \
                    OR (i.personnel_id IS NOT NULL AND i.personnel_id = \
                        (SELECT ua.personnel_id FROM user_accounts ua WHERE ua.id = ?))) \
             LIMIT 1",
            [wo_id.into(), verifier_user_id.into(), verifier_user_id.into()],
        ))
        .await?
        .is_some();
    Ok(worked.then_some(VERIFIER_RECORDED_LABOUR))
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
            "GATE_CLOSEOUT_VERIFY_RESULT_INVALID: Resultat de verification invalide : '{}'. Valeurs autorisees : pass, fail, monitor.",
            input.result
        )]));
    }

    // ── Validate recurrence risk level if provided ────────────────────────
    if let Some(ref level) = input.recurrence_risk_level {
        if !VALID_RECURRENCE_LEVELS.contains(&level.as_str()) {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_CLOSEOUT_RECURRENCE_RISK_INVALID: Niveau de risque de recurrence invalide : '{}'. Valeurs autorisees : none, low, medium, high.",
                level
            )]));
        }
    }

    let txn = db.begin().await?;

    // ── Guard: WO must be in completed ────────────────────────────────────
    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    if current_status != WoStatus::Completed {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_CLOSEOUT_REVIEW_STATUS:status={}: La revue superviseur n'est disponible qu'en statut completed (actuel: '{from_code}').",
            status_i18n_key(&from_code)
        )]));
    }
    use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
    assert_action_allowed(&current_status, WoAction::SupervisorReview)
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // ── No self-verification (D7: without a responsible, nobody who recorded labour) ──
    match verifier_conflict(&txn, input.wo_id, input.verified_by_id).await? {
        Some(VERIFIER_IS_RESPONSIBLE) => {
            return Err(AppError::ValidationFailed(vec![
                "GATE_CLOSEOUT_SELF_VERIFICATION: L'auto-verification n'est pas autorisee : le verificateur doit etre different \
                 du responsable principal. (Self-verification is not allowed.)"
                    .to_string(),
            ]));
        }
        Some(_) => {
            return Err(AppError::ValidationFailed(vec![
                "GATE_CLOSEOUT_VERIFIER_RECORDED_LABOUR: Sans responsable principal, le vérificateur doit être différent \
                 de chaque personne qui a saisi de la main-d'œuvre sur l'OT. \
                 (Without a responsible person, the verifier must not have recorded labour on the work order.)"
                    .to_string(),
            ]));
        }
        None => {}
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

    // ── Re-fetch verification and WO ──────────────────────────────────────
    let ver_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM work_order_verifications \
             WHERE work_order_id = ? ORDER BY id DESC LIMIT 1",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-fetch verification after insert")))?;
    let verification = map_verification(&ver_row)?;

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    txn.commit().await?;
    Ok((verification, wo))
}

/// The signature password of a FMECA parts override, as the close can check it (shared with the
/// other second-person signatures).
pub(crate) use crate::auth::second_signer::SignerPassword;

/// What a close brings besides the stored work order: the closer, the no-downtime attestation and
/// the FMECA parts override. For a received close, read from the work order row just written and
/// from its `close` transition (`wo::closeout_sync`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CloseEvidence<'a> {
    /// Who closes; `None` when not known here (a received close without its transition): a
    /// signer then cannot be shown distinct from the closer.
    pub closer_user_id: Option<i64>,
    pub no_downtime_attestation: Option<bool>,
    pub no_downtime_attestation_reason: Option<&'a str>,
    pub fmeca_override_reason: Option<&'a str>,
    pub fmeca_override_signer_id: Option<i64>,
    pub fmeca_override_signer_password: SignerPassword<'a>,
}

/// The close quality gate on the stored state of a WO: every blocking reason (empty: the close may
/// go on), the validation profile it was judged with, and the outcome of the override signer's
/// password check when it was checked (`Some(ok)`; counted toward the signer's account lockout by
/// `close_wo`, outside its transaction). The same function judges a close made here (`close_wo`)
/// and one received by synchronisation (`wo::closeout_sync`), so the two never drift. Reads only.
pub(crate) async fn close_preflight(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    ev: &CloseEvidence<'_>,
    rpn_critical: i64,
) -> AppResult<(Vec<String>, i64, Option<bool>)> {
    // Some(true/false) once the signer's password was actually checked.
    let mut signer_password_checked: Option<bool> = None;
    // ── Corrective work (EN 13306 nature) requires failure coding and root cause ──
    let is_failure_required = crate::wo::types::work_order_nature(conn, wo_id)
        .await?
        .is_some_and(|n| n.is_corrective());

    let wo_scope = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT closeout_validation_profile_id, production_impact_id \
             FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let profile_id: i64 = wo_scope
        .try_get::<Option<i64>>("", "closeout_validation_profile_id")
        .map_err(|e| decode_err("closeout_validation_profile_id", e))?
        .unwrap_or(1);
    let production_impact_id: Option<i64> = wo_scope
        .try_get::<Option<i64>>("", "production_impact_id")
        .map_err(|e| decode_err("production_impact_id", e))?;

    let policy = load_closeout_policy(conn, profile_id).await?;

    // ── Quality gate — collect ALL blocking errors ────────────────────────
    let mut preflight = PreflightResult::default();

    // (a) Labor actuals required
    let labor_row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                COALESCE(wo.active_labor_hours, 0) AS alh, \
                (SELECT COUNT(*) FROM work_order_interveners WHERE work_order_id = wo.id) AS labor_cnt \
             FROM work_orders wo WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let alh: f64 = labor_row.try_get::<f64>("", "alh").map_err(|e| decode_err("alh", e))?;
    let labor_cnt: i64 = labor_row
        .try_get::<i64>("", "labor_cnt")
        .map_err(|e| decode_err("labor_cnt", e))?;
    if alh <= 0.0 && labor_cnt == 0 {
        preflight.add("GATE_CLOSEOUT_LABOR_REQUIRED: Heures de main-d'oeuvre requises. (Labor actuals required.)");
    }

    // (b) Parts actuals required
    let parts_row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                wo.parts_actuals_confirmed AS pac, \
                (SELECT COUNT(*) FROM work_order_parts WHERE work_order_id = wo.id AND quantity_used > 0) AS parts_cnt \
             FROM work_orders wo WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let pac: i64 = parts_row.try_get::<i64>("", "pac").map_err(|e| decode_err("pac", e))?;
    let parts_cnt: i64 = parts_row
        .try_get::<i64>("", "parts_cnt")
        .map_err(|e| decode_err("parts_cnt", e))?;
    if pac == 0 && parts_cnt == 0 {
        preflight.add("GATE_CLOSEOUT_PARTS_REQUIRED: Consommation de pieces requise. (Parts actuals required.)");
    }

    // (c) Failure coding + RCA for corrective/emergency (ISO 14224 close-out)
    if is_failure_required {
        let fd_row = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT failure_mode_id, failure_cause_id, cause_not_determined, notes \
                     FROM work_order_failure_details WHERE id = {}",
                    failure_detail_of_wo_sql("?")
                ),
                [wo_id.into()],
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
                        "GATE_CLOSEOUT_CND_NOT_ALLOWED: La politique n'autorise pas la cloture avec cause non determinee. \
                         (Policy disallows close with cause not determined.)",
                    );
                }
                let nmin = policy.notes_min_length_when_cnd.max(1) as usize;
                if fd_notes.as_deref().is_none_or(|s| s.trim().len() < nmin) {
                    preflight.add(format!(
                        "GATE_CLOSEOUT_CND_NOTES_REQUIRED:min={nmin}: Notes obligatoires (cause non determinee), minimum {nmin} caracteres. \
                         (Notes required when cause not determined.)"
                    ));
                }
            } else {
                if failure_mode_id.is_none() {
                    preflight.add(
                        "GATE_CLOSEOUT_FAILURE_MODE_REQUIRED: Mode de defaillance obligatoire (ISO 14224 — failure mode). \
                         (Failure mode is required.)",
                    );
                }
                if let Some(gov_err) = validate_failure_mode_governance(conn, failure_mode_id).await? {
                    preflight.add(gov_err);
                }
                if failure_cause_id.is_none() && !policy.allow_close_with_cause_mode_only {
                    preflight.add(
                        "GATE_CLOSEOUT_FAILURE_CAUSE_REQUIRED: Cause de defaillance obligatoire (ISO 14224 — failure cause), \
                         sauf politique mode-seul. (Failure cause is required.)",
                    );
                }
            }
        } else {
            preflight.add(
                "GATE_CLOSEOUT_FAILURE_CODING_REQUIRED: Codification de defaillance requise pour un OT correctif/urgence. \
                 (Failure coding required for corrective/emergency work.)",
            );
        }

        // (d) Root cause summary
        let rcs_row = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT root_cause_summary FROM work_orders WHERE id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: wo_id.to_string(),
            })?;
        let rcs: Option<String> = rcs_row
            .try_get::<Option<String>>("", "root_cause_summary")
            .map_err(|e| decode_err("root_cause_summary", e))?;
        if rcs.as_deref().is_none_or(|s| s.trim().is_empty()) {
            preflight.add(
                "GATE_CLOSEOUT_ROOT_CAUSE_REQUIRED: Resume de cause racine requis. (Root cause summary required.)",
            );
        }
    }

    // (c3) Critical FMECA suggested-part requirement
    if is_failure_required && policy.require_fmeca_parts_for_critical {
        let row = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT wo.equipment_id, fd.failure_mode_id
                     FROM work_orders wo
                     LEFT JOIN work_order_failure_details fd ON fd.id = {}
                     WHERE wo.id = ?",
                    failure_detail_of_wo_sql("wo.id")
                ),
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: wo_id.to_string(),
            })?;
        let equipment_id: Option<i64> = row
            .try_get::<Option<i64>>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?;
        let failure_mode_id: Option<i64> = row
            .try_get::<Option<i64>>("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?;

        if let (Some(eq_id), Some(mode_id)) = (equipment_id, failure_mode_id) {
            let has_suggestions = has_critical_fmeca_part_suggestions(conn, eq_id, mode_id, rpn_critical).await?;
            if has_suggestions {
                let has_matched_consumed_parts =
                    has_consumed_critical_suggested_part(conn, wo_id, eq_id, mode_id, rpn_critical).await?;
                let reason_min = policy.fmeca_parts_override_reason_min_length.max(1) as usize;
                let override_reason_ok = ev
                    .fmeca_override_reason
                    .map(|s| s.trim().len() >= reason_min)
                    .unwrap_or(false);
                let signer_id_opt = ev.fmeca_override_signer_id;
                let signer_id = signer_id_opt.unwrap_or_default();
                let signer_exists = signer_id > 0 && has_active_user(conn, signer_id).await?;
                let signer_has_role = signer_exists
                    && has_supervisor_override_role(conn, signer_id, &policy.fmeca_parts_override_allowed_roles)
                        .await?;
                let signer_is_distinct = !policy.fmeca_parts_override_require_distinct_signer
                    || (signer_id > 0 && ev.closer_user_id.is_some_and(|closer| closer != signer_id));
                let signer_password_ok = if signer_exists {
                    match ev.fmeca_override_signer_password {
                        SignerPassword::Given(Some(raw)) if !raw.trim().is_empty() => {
                            let ok = second_signer::verify_user_password(conn, signer_id, raw).await?;
                            signer_password_checked = Some(ok);
                            ok
                        }
                        SignerPassword::Given(_) => false,
                        // The signer's account is locked out here: the password is not checked.
                        SignerPassword::Locked => false,
                        // A received close: the signer typed the password on the PC that closed;
                        // it cannot be checked again here (docs/KNOWN_LIMITS.md), and no lockout
                        // counter of this PC is touched.
                        SignerPassword::NotVerifiableHere => true,
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
                if !has_matched_consumed_parts && signer_id_opt.is_some() && signer_id_opt == ev.closer_user_id {
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
                    preflight.add(if matches!(ev.fmeca_override_signer_password, SignerPassword::Locked) {
                        "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNER_LOCKED"
                    } else {
                        "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNER_AUTH_FAILED"
                    });
                }
            }
        }
    }

    // (c2) Downtime or attestation when production impact is recorded
    if policy.require_downtime_if_production_impact && production_impact_id.is_some() {
        let dt_row = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM work_order_downtime_segments \
                 WHERE work_order_id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("downtime count")))?;
        let dt_cnt: i64 = dt_row.try_get::<i64>("", "cnt").map_err(|e| decode_err("dt_cnt", e))?;
        let attest = ev.no_downtime_attestation == Some(true);
        let reason_ok = ev
            .no_downtime_attestation_reason
            .map(|s| s.trim().len() >= 8)
            .unwrap_or(false);
        if dt_cnt == 0 && !(attest && reason_ok) {
            preflight.add(
                "GATE_CLOSEOUT_DOWNTIME_REQUIRED: Temps d'arret production : enregistrer un segment d'arret ou attester \
                 explicitement l'absence d'arret avec justification. \
                 (Downtime segment or explicit no-downtime attestation required.)",
            );
        }
    }

    // (e) Technical verification, by a verifier the self-verification rules still accept now (the
    // responsible and, without one, the labour recorded since the verification: D7, checked again
    // at close).
    let rts_required = is_failure_required && policy.require_verification_return_to_service;
    let verifiers = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            if rts_required {
                "SELECT verified_by_id FROM work_order_verifications \
                 WHERE work_order_id = ? AND result = 'pass' AND return_to_service_confirmed = 1"
            } else {
                "SELECT verified_by_id FROM work_order_verifications \
                 WHERE work_order_id = ? AND result IN ('pass', 'monitor')"
            },
            [wo_id.into()],
        ))
        .await?;
    let mut accepted = 0;
    for r in &verifiers {
        let verifier: Option<i64> = r
            .try_get("", "verified_by_id")
            .map_err(|e| decode_err("verified_by_id", e))?;
        let Some(verifier) = verifier else {
            continue;
        };
        if verifier_conflict(conn, wo_id, verifier).await?.is_none() {
            accepted += 1;
        }
    }
    if accepted == 0 && !verifiers.is_empty() {
        preflight.add(
            "GATE_CLOSEOUT_VERIFIER_NOT_INDEPENDENT: La vérification a été faite par le responsable de l'OT ou, sans \
             responsable, par une personne qui a saisi de la main-d'œuvre : une vérification indépendante est requise. \
             (The verification was made by the responsible or by someone who recorded labour: an independent \
             verification is required.)",
        );
    } else if accepted == 0 && rts_required {
        preflight.add(
            "GATE_CLOSEOUT_RTS_VERIFICATION_REQUIRED: Verification avec retour en service confirme requis. \
             (Verification with return-to-service confirmed is required.)",
        );
    } else if accepted == 0 {
        preflight.add(
            "GATE_CLOSEOUT_VERIFICATION_REQUIRED: Verification technique requise. (Technical verification required.)",
        );
    }

    // (f) Every mandatory task closed with an accepted result (D3; a legacy or received
    // cancelled / deferred / bare NOK mandatory task counts as open).
    let open_tasks = crate::wo::tasks::open_mandatory_tasks(conn, wo_id).await?;
    if !open_tasks.is_empty() {
        preflight.add(format!(
            "GATE_CLOSEOUT_TASKS_OPEN: Tâches obligatoires non clôturées avec un résultat accepté : {}. \
             (Mandatory tasks not closed with OK, N/A or NOK with a finding or follow-up.)",
            open_tasks.join(", ")
        ));
    }

    // (g) Nothing of its permits still holds the plant (AU-02, owner decision D4): every permit
    // closed, handed back, cancelled or expired, and every lock fitted under them with its removal
    // verified.
    let holds = crate::permit::wo_gate::permit_holds(conn, wo_id).await?;
    for refusal in crate::permit::wo_gate::end_of_work_refusals(holds, crate::permit::wo_gate::WorkOrderEnd::Close) {
        preflight.add(refusal);
    }

    // (h) Its downtime has ended (owner decision D4, CMMS Advisor condition 2): completion may leave
    // it open while a lock is fitted, until the removal of the last lock is verified; a work order
    // is never closed with its equipment still recorded down.
    let open_downtime = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM work_order_downtime_segments WHERE work_order_id = ? AND ended_at IS NULL",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("open downtime count")))?
        .try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("open downtime cnt", e))?;
    if open_downtime > 0 {
        preflight.add(format!(
            "GATE_CLOSEOUT_OPEN_DOWNTIME: Clôture bloquée : {open_downtime} segment(s) de temps d'arrêt encore \
             ouvert(s). ({open_downtime} downtime segment(s) still open.)"
        ));
    }

    Ok((preflight.errors, profile_id, signer_password_checked))
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
}

/// Close a WO after passing the mandatory quality gate.
///
/// All blocking conditions are collected and returned together so the
/// user sees every issue at once, not one-at-a-time.
pub async fn close_wo(db: &DatabaseConnection, input: WoCloseInput) -> AppResult<WorkOrder> {
    // Load settings before opening the transaction — SQLite single-connection pools
    // deadlock if we call back into `db` while `txn` holds the only connection.
    let rpn_critical = advanced_rams_queries::fmeca_rpn_critical_threshold_i64(db).await?;

    // The override signer's password counts toward the signer's account lockout, as at sign-in
    // (`auth::lockout`): a locked-out signer is not checked at all. Read before the transaction
    // (the lockout check may write); the outcome is recorded after it.
    let signer_for_password = signer_password_account(&input);
    let signer_locked = match signer_for_password {
        Some(id) => second_signer::lockout_state(db, id).await?,
        None => false,
    };

    let txn = db.begin().await?;

    // ── Guard: completed → closed ─────────────────────────────────────────
    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
    assert_action_allowed(&current_status, WoAction::Close).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    guard_wo_transition(&current_status, &WoStatus::Closed).map_err(|e| AppError::ValidationFailed(vec![e]))?;

    // ── Quality gate — collect ALL blocking errors (`close_preflight`) ────
    let (preflight_errors, profile_id, signer_password_checked) = close_preflight(
        &txn,
        input.wo_id,
        &CloseEvidence {
            closer_user_id: Some(input.actor_id),
            no_downtime_attestation: input.no_downtime_attestation,
            no_downtime_attestation_reason: input.no_downtime_attestation_reason.as_deref(),
            fmeca_override_reason: input.fmeca_parts_override_reason.as_deref(),
            fmeca_override_signer_id: input.fmeca_parts_override_signed_by_id,
            fmeca_override_signer_password: if signer_locked {
                SignerPassword::Locked
            } else {
                SignerPassword::Given(input.fmeca_parts_override_signer_password.as_deref())
            },
        },
        rpn_critical,
    )
    .await?;

    // ── Return all blocking errors at once ────────────────────────────────
    if !preflight_errors.is_empty() {
        txn.rollback().await?;
        record_signer_password_outcome(db, &input, signer_for_password, signer_password_checked).await?;
        return Err(AppError::ValidationFailed(preflight_errors));
    }

    // Labour still open (left by an older version: completion refuses it now) is closed with the
    // close, before the costs are rolled up.
    crate::wo::labor::close_open_labor_at_end(
        &txn,
        input.wo_id,
        &now_utc_z(),
        input.actor_id,
        crate::wo::labor::LaborEnd::Closed,
    )
    .await?;

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

    // Audit P1-6: the governed failure event is written in the close transaction, so a failed ingest
    // aborts the close instead of silently losing the reliability record.
    reliability_queries::ingest_failure_event_from_closed_wo(&txn, input.wo_id, input.actor_id).await?;

    // Cost settlement into the budget, once per close (pending with its reason when it cannot post).
    crate::finance::settlement::settle_closed_work_order(&txn, input.wo_id, input.actor_id).await?;

    // Its schedule commitments are closed with it (`planning::lifecycle`).
    crate::planning::lifecycle::close_commitments_of_work_order(&txn, input.wo_id).await?;
    // D13: the stock still reserved for it is released with the close, each release audited.
    crate::inventory::reservations::release_for_closed_wo(
        &txn,
        input.wo_id,
        input.actor_id,
        crate::inventory::reservations::WoEnd::Closed,
    )
    .await?;
    txn.commit().await?;
    // The close is committed: a failure to record the signer outcome is logged, never turned
    // into an error (the follow-ups below must still run).
    if let Err(e) = record_signer_password_outcome(db, &input, signer_for_password, signer_password_checked).await {
        tracing::error!(error = %e, wo_id = input.wo_id, "close-out: signer password outcome not recorded");
    }

    crate::pm::generation_control::on_work_order_closed_for_pm(db, input.wo_id, Some(input.actor_id)).await;

    // Optimistic immediate RAMS KPI refresh for the impacted equipment.
    if let Ok(Some(wo)) = queries::get_work_order(db, input.wo_id).await {
        if let Some(equipment_id) = wo.equipment_id {
            let period_end = Utc::now();
            let period_start = period_end - Duration::days(365);
            let refresh_input = RefreshReliabilityKpiSnapshotInput {
                equipment_id,
                period_start: period_start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                period_end: period_end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                min_sample_n: None,
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
// D) reopen_wo — completed / recently closed → in_progress or planning
// ═══════════════════════════════════════════════════════════════════════════════

/// Reopen a WO to execution (`in_progress`) or plan (`planning`) — from `completed`, or from
/// `closed` within the tenant reopen window. The original close-out evidence is kept: the
/// verification and mechanical-completion stamps stay, the closure date moves to
/// `last_closed_at`, and the transition log records who reopened it and why.
pub async fn reopen_wo(db: &DatabaseConnection, input: WoReopenInput) -> AppResult<WorkOrder> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_REOPEN_REASON_REQUIRED: Une raison de reouverture est obligatoire. (Reopen reason is required.)"
                .to_string(),
        ]));
    }

    let target = match input.target_status.as_deref().unwrap_or("in_progress") {
        "planning" => WoStatus::Planning,
        "in_progress" => WoStatus::InProgress,
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_REOPEN_TARGET_INVALID: Cible de reouverture invalide '{other}'. Utilisez 'in_progress' ou 'planning'."
            )]));
        }
    };

    // Only a reopening to in progress restarts the work: the remaining time is used (and recorded)
    // for it alone.
    let remaining = crate::permit::wo_gate::checked_remaining_hours(input.estimated_remaining_hours)?
        .filter(|_| target == WoStatus::InProgress);
    // One clock read: the moment judged by the permit gate is the reopening recorded below.
    let at = now_utc_second();

    let txn = db.begin().await?;

    let (from_code, current_status, _rv) = load_wo_status(&txn, input.wo_id).await?;
    use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction};
    match current_status {
        WoStatus::Completed => {
            assert_action_allowed(&current_status, WoAction::Reopen)
                .map_err(|e| AppError::ValidationFailed(vec![e]))?;
            guard_wo_transition(&current_status, &target).map_err(|e| AppError::ValidationFailed(vec![e]))?;
        }
        // Closed is terminal for every normal flow; reopening it is the supervisor exception
        // of PRD §6.5, bounded by the reopen window.
        WoStatus::Closed => ensure_within_reopen_window(&txn, input.wo_id).await?,
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_REOPEN_STATUS:status={}: Only completed or recently closed work orders can be reopened (current status '{from_code}').",
                status_i18n_key(&from_code)
            )]));
        }
    }

    // Work restarts straight away: a plant rule that came from another PC and was not derived here,
    // while this PC's list gives another rule or permit type, is never restarted under (the rule
    // of a started work order is not changed by this PC): reopen it to planning, where it is
    // judged again (check 6, C6-S2).
    if target == WoStatus::InProgress {
        if let Some((rule, permit_type)) =
            crate::permit::requirement_rules::received_rule_differs_here(&txn, input.wo_id).await?
        {
            let permit_type = permit_type.as_deref().unwrap_or("?");
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_REOPEN_RULE_RECHECK:rule={rule},permit_type={permit_type}: The plant permit rule of this work \
                 order came from another PC; the plant applies rule {rule} ({permit_type} permit) here. Reopen it \
                 to planning."
            )]));
        }
    }
    // Work restarts: the same permit / LOTO gate as starting or resuming it.
    if target == WoStatus::InProgress {
        crate::permit::wo_gate::assert_in_progress_permit_gate_with(&txn, input.wo_id, remaining, at).await?;
    }

    let target_status_id = resolve_status_id(&txn, target.as_str()).await?;
    let now = stamp(at);

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                status_id = ?, \
                reopen_count = reopen_count + 1, \
                last_closed_at = COALESCE(closed_at, last_closed_at), \
                closed_at = NULL, \
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

    // The remaining time entered (if any) is recorded on this row: who, when, the value.
    crate::wo::workflow::transition::log_transition_with_remaining(
        &txn,
        input.wo_id,
        &from_code,
        target.as_str(),
        "reopen",
        input.actor_id,
        None,
        Some(&input.reason),
        &now,
        remaining,
    )
    .await?;

    // Back to planning: a rule that came from another PC and was not derived here is judged again by
    // the list here before the work is planned again (audit finding AU-04 R5-S1).
    if target == WoStatus::Planning {
        crate::permit::requirement_rules::recheck_received_rule(
            &txn,
            input.wo_id,
            crate::permit::requirement_rules::stamp_causes::REOPENED,
        )
        .await?;
    }

    // Reverse the cost settlement of the closed cycle (nothing to do when reopened from completed).
    crate::finance::settlement::reverse_work_order_settlement(&txn, input.wo_id, input.actor_id, &input.reason).await?;

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

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    txn.commit().await?;
    Ok(wo)
}

/// A closed WO may be reopened only while `closed_at` is within the tenant reopen window.
async fn ensure_within_reopen_window(txn: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    let window = reopen_window_days(txn).await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT closed_at FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let closed_at: Option<String> = row.try_get("", "closed_at").map_err(|e| decode_err("closed_at", e))?;
    let closed = closed_at
        .as_deref()
        .and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok())
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_REOPEN_CLOSED_AT_UNKNOWN: The closure date of this work order is unknown; it cannot be reopened."
                    .into(),
            ])
        })?;
    let age_days = (chrono::Utc::now() - closed.with_timezone(&chrono::Utc)).num_seconds() as f64 / 86_400.0;
    if age_days > window as f64 {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_REOPEN_WINDOW_EXPIRED:days={window}: This work order was closed more than {window} day(s) ago (reopen window); create a new work order instead."
        )]));
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) get_failure_details — list failure detail rows
// ═══════════════════════════════════════════════════════════════════════════════

/// List all failure detail records for a WO, the one every reader uses
/// ([`failure_detail_of_wo_sql`]) first.
pub async fn get_failure_details(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<WoFailureDetail>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{FAILURE_DETAIL_SELECT} WHERE fd.work_order_id = ? ORDER BY fd.entity_sync_id, fd.id"),
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
/// existing values. Refused when the WO is no longer at `expected_row_version`; bumps it and
/// returns the new version.
pub async fn update_wo_rca(db: &DatabaseConnection, input: UpdateWoRcaInput) -> AppResult<i64> {
    // Guard: WO must not be in closed or cancelled state.
    let (status_code, _status, _rv) = load_wo_status(db, input.wo_id).await?;
    if matches!(status_code.as_str(), "closed" | "cancelled") {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_CLOSEOUT_RCA_LOCKED:status={}: Impossible de modifier les champs RCA pour un OT en statut '{status_code}'.",
            status_i18n_key(&status_code)
        )]));
    }

    let now = now_utc_z();

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                root_cause_summary        = COALESCE(?, root_cause_summary), \
                corrective_action_summary = COALESCE(?, corrective_action_summary), \
                row_version               = row_version + 1, \
                updated_at                = ? \
             WHERE id = ? AND row_version = ?",
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
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    Ok(input.expected_row_version + 1)
}
