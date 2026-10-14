//! Training / qualification IPC (PRD §6.20).

use tauri::State;

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::qualification::domain::{
    CertificationExpiryDrilldownRow, CertificationType, CertificationTypeUpsertInput, CrewPermitSkillGapInput,
    CrewPermitSkillGapResult, DocumentAcknowledgement, DocumentAcknowledgementListFilter,
    DocumentAcknowledgementUpsertInput, PersonnelCertification, PersonnelCertificationListFilter,
    PersonnelCertificationUpsertInput, PersonnelReadinessFilter, PersonnelReadinessRow, PersonnelReadinessSnapshot,
    PersonnelReadinessSnapshotUpsertInput, QualificationRequirementProfile, QualificationRequirementProfileUpsertInput,
    TrainingAttendance, TrainingAttendanceListFilter, TrainingAttendanceUpsertInput, TrainingExpiryAlertEvent,
    TrainingExpiryAlertEventListFilter, TrainingSession, TrainingSessionUpsertInput,
};
use crate::qualification::expiry_alerts;
use crate::qualification::gate::{
    self, PersonQualificationCheck, QualificationOverrideFilter, QualificationOverrideInput, QualificationOverrideRow,
    WoQualificationAssessment,
};
use crate::qualification::matrix::{self, QualificationMatrix, QualificationMatrixFilter};
use crate::qualification::requirements::{
    self, QualificationRequirement, QualificationRequirementFilter, QualificationRequirementUpsertInput,
};
use crate::qualification::queries;
use crate::qualification::readiness;
use crate::qualification::training;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

async fn linked_personnel_id(state: &State<'_, AppState>, user_id: i32) -> AppResult<Option<i64>> {
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get("", "personnel_id").ok()).flatten())
}

#[tauri::command]
pub async fn list_certification_types(state: State<'_, AppState>) -> AppResult<Vec<CertificationType>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    queries::list_certification_types(&state.db).await
}

#[tauri::command]
pub async fn upsert_certification_type(
    input: CertificationTypeUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<CertificationType> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    queries::upsert_certification_type(&state.db, input).await
}

#[tauri::command]
pub async fn list_qualification_requirement_profiles(
    state: State<'_, AppState>,
) -> AppResult<Vec<QualificationRequirementProfile>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    queries::list_qualification_requirement_profiles(&state.db).await
}

#[tauri::command]
pub async fn upsert_qualification_requirement_profile(
    input: QualificationRequirementProfileUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<QualificationRequirementProfile> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    queries::upsert_qualification_requirement_profile(&state.db, input).await
}

#[tauri::command]
pub async fn list_personnel_certifications(
    filter: PersonnelCertificationListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelCertification>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    queries::list_personnel_certifications(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_personnel_certification(
    input: PersonnelCertificationUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<PersonnelCertification> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_CERTIFY,
        PermissionScope::Global
    );
    // Certifying (marking verified) is the dangerous act the permission registry gates with a
    // step-up; recording a certificate still to be checked is not.
    if input.verification_status == "verified" {
        require_step_up!(state);
    }
    queries::upsert_personnel_certification(&state.db, input).await
}

#[tauri::command]
pub async fn list_training_sessions(state: State<'_, AppState>) -> AppResult<Vec<TrainingSession>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    training::list_training_sessions(&state.db).await
}

#[tauri::command]
pub async fn upsert_training_session(
    input: TrainingSessionUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<TrainingSession> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    training::upsert_training_session(&state.db, input).await
}

#[tauri::command]
pub async fn list_training_attendance(
    filter: TrainingAttendanceListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<TrainingAttendance>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    training::list_training_attendance(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_training_attendance(
    input: TrainingAttendanceUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<TrainingAttendance> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    training::upsert_training_attendance(&state.db, input).await
}

#[tauri::command]
pub async fn list_document_acknowledgements(
    filter: DocumentAcknowledgementListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<DocumentAcknowledgement>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    training::list_document_acknowledgements(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_document_acknowledgement(
    input: DocumentAcknowledgementUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<DocumentAcknowledgement> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    training::upsert_document_acknowledgement(&state.db, input).await
}

#[tauri::command]
pub async fn list_my_training_sessions(state: State<'_, AppState>) -> AppResult<Vec<TrainingAttendance>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    let Some(pid) = linked_personnel_id(&state, user.user_id).await? else {
        return Ok(vec![]);
    };
    training::list_training_attendance(
        &state.db,
        TrainingAttendanceListFilter {
            session_id: None,
            personnel_id: Some(pid),
            limit: Some(200),
        },
    )
    .await
}

#[tauri::command]
pub async fn list_my_personnel_certifications(state: State<'_, AppState>) -> AppResult<Vec<PersonnelCertification>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    let Some(pid) = linked_personnel_id(&state, user.user_id).await? else {
        return Ok(vec![]);
    };
    queries::list_personnel_certifications(
        &state.db,
        PersonnelCertificationListFilter {
            personnel_id: Some(pid),
            limit: Some(200),
        },
    )
    .await
}

#[tauri::command]
pub async fn list_personnel_readiness(
    filter: PersonnelReadinessFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelReadinessRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    readiness::list_personnel_readiness(&state.db, filter).await
}

#[tauri::command]
pub async fn evaluate_crew_permit_skill_gaps(
    input: CrewPermitSkillGapInput,
    state: State<'_, AppState>,
) -> AppResult<CrewPermitSkillGapResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    readiness::evaluate_crew_permit_skill_gaps(&state.db, input).await
}

#[tauri::command]
pub async fn list_personnel_readiness_snapshots(
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelReadinessSnapshot>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    readiness::list_personnel_readiness_snapshots(&state.db).await
}

#[tauri::command]
pub async fn upsert_personnel_readiness_snapshot(
    input: PersonnelReadinessSnapshotUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<PersonnelReadinessSnapshot> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    readiness::upsert_personnel_readiness_snapshot(&state.db, input).await
}

#[tauri::command]
pub async fn refresh_personnel_readiness_snapshot(
    period: String,
    state: State<'_, AppState>,
) -> AppResult<PersonnelReadinessSnapshot> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    readiness::refresh_personnel_readiness_snapshot_payload(&state.db, period).await
}

#[tauri::command]
pub async fn list_training_expiry_alert_events(
    filter: TrainingExpiryAlertEventListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<TrainingExpiryAlertEvent>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    expiry_alerts::list_training_expiry_alert_events(&state.db, filter).await
}

#[tauri::command]
pub async fn scan_training_expiry_alerts(
    lookahead_days: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<TrainingExpiryAlertEvent>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    expiry_alerts::scan_training_expiry_alerts(&state.db, lookahead_days.unwrap_or(90)).await
}

#[tauri::command]
pub async fn list_certification_expiry_drilldown(
    entity_id: Option<i64>,
    lookahead_days: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<CertificationExpiryDrilldownRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    expiry_alerts::list_certification_expiry_drilldown(&state.db, entity_id, lookahead_days.unwrap_or(90)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Job requirements, the qualification gate, overrides, matrix
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_qualification_requirements(
    filter: QualificationRequirementFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<QualificationRequirement>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    requirements::list_requirements(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_qualification_requirement(
    input: QualificationRequirementUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<QualificationRequirement> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_MANAGE,
        PermissionScope::Global
    );
    requirements::upsert_requirement(&state.db, input, i64::from(user.user_id)).await
}

/// Whole-crew qualification picture of a work order (OT screen, planning, readiness).
#[tauri::command]
pub async fn get_wo_qualification_assessment(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<WoQualificationAssessment> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::OT_VIEW,
        PermissionScope::Global
    );
    gate::assess_work_order(&state.db, wo_id).await
}

/// One person against a work order, before booking or assigning them.
#[tauri::command]
pub async fn check_person_for_work_order(
    personnel_id: i64,
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<PersonQualificationCheck> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::OT_VIEW,
        PermissionScope::Global
    );
    gate::check_person_for_work_order(&state.db, personnel_id, wo_id).await
}

/// One person against a LOTO role on a permit.
#[tauri::command]
pub async fn check_person_for_permit_role(
    personnel_id: i64,
    permit_id: i64,
    role: String,
    state: State<'_, AppState>,
) -> AppResult<PersonQualificationCheck> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    gate::check_person_for_permit_role(&state.db, personnel_id, permit_id, &role).await
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RecordQualificationOverrideInput {
    pub personnel_id: i64,
    /// `work_order` | `permit_role`.
    pub context_kind: String,
    pub work_order_id: Option<i64>,
    pub permit_id: Option<i64>,
    pub role: Option<String>,
    pub reason: String,
}

/// An authorised supervisor lets a person work although a requirement is not met (recorded,
/// synchronised and audited; the assignment can then go ahead). Returns the person's check
/// after the override.
#[tauri::command]
pub async fn record_qualification_override(
    input: RecordQualificationOverrideInput,
    state: State<'_, AppState>,
) -> AppResult<PersonQualificationCheck> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_CERTIFY,
        PermissionScope::Global
    );
    require_step_up!(state);
    let override_input = QualificationOverrideInput {
        reason: input.reason,
        approved_by_id: i64::from(user.user_id),
    };
    let txn = state.db.begin().await?;
    let check = match input.context_kind.as_str() {
        "work_order" => {
            let wo_id = input.work_order_id.ok_or_else(|| {
                crate::qualification::issue("REQUIREMENT_TARGET_REQUIRED", "Choose the work order.", &[])
            })?;
            let before = gate::check_person_for_work_order(&txn, input.personnel_id, wo_id).await?;
            if before.qualified {
                return Err(crate::qualification::issue(
                    "QUALIFICATION_OVERRIDE_NOT_NEEDED",
                    "This person already meets the requirements; no override is needed.",
                    &[("person", before.full_name)],
                ));
            }
            gate::enforce_work_order_assignment(&txn, input.personnel_id, wo_id, Some(&override_input)).await?;
            gate::check_person_for_work_order(&txn, input.personnel_id, wo_id).await?
        }
        "permit_role" => {
            let permit_id = input.permit_id.ok_or_else(|| {
                crate::qualification::issue("REQUIREMENT_TARGET_REQUIRED", "Choose the permit.", &[])
            })?;
            let role = input.role.unwrap_or_default();
            let before = gate::check_person_for_permit_role(&txn, input.personnel_id, permit_id, &role).await?;
            if before.qualified {
                return Err(crate::qualification::issue(
                    "QUALIFICATION_OVERRIDE_NOT_NEEDED",
                    "This person already meets the requirements; no override is needed.",
                    &[("person", before.full_name)],
                ));
            }
            gate::enforce_permit_role(&txn, input.personnel_id, permit_id, &role, Some(&override_input)).await?;
            gate::check_person_for_permit_role(&txn, input.personnel_id, permit_id, &role).await?
        }
        other => {
            return Err(crate::qualification::issue(
                "REQUIREMENT_TARGET_UNKNOWN",
                "Choose a work order or a permit role.",
                &[("target", other.to_string())],
            ))
        }
    };
    txn.commit().await?;
    Ok(check)
}

#[tauri::command]
pub async fn list_qualification_overrides(
    filter: QualificationOverrideFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<QualificationOverrideRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    gate::list_overrides(&state.db, filter).await
}

#[tauri::command]
pub async fn get_qualification_matrix(
    filter: QualificationMatrixFilter,
    state: State<'_, AppState>,
) -> AppResult<QualificationMatrix> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::TRN_VIEW,
        PermissionScope::Global
    );
    matrix::get_qualification_matrix(&state.db, filter).await
}
