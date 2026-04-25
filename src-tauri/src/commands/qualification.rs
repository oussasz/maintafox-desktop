//! Training / qualification IPC (PRD §6.20).

use tauri::State;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

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
use crate::qualification::queries;
use crate::qualification::readiness;
use crate::qualification::training;
use crate::state::AppState;
use crate::{require_permission, require_session};

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
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    queries::list_certification_types(&state.db).await
}

#[tauri::command]
pub async fn upsert_certification_type(
    input: CertificationTypeUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<CertificationType> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    queries::upsert_certification_type(&state.db, input).await
}

#[tauri::command]
pub async fn list_qualification_requirement_profiles(
    state: State<'_, AppState>,
) -> AppResult<Vec<QualificationRequirementProfile>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    queries::list_qualification_requirement_profiles(&state.db).await
}

#[tauri::command]
pub async fn upsert_qualification_requirement_profile(
    input: QualificationRequirementProfileUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<QualificationRequirementProfile> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    queries::upsert_qualification_requirement_profile(&state.db, input).await
}

#[tauri::command]
pub async fn list_personnel_certifications(
    filter: PersonnelCertificationListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelCertification>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    queries::list_personnel_certifications(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_personnel_certification(
    input: PersonnelCertificationUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<PersonnelCertification> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.certify", PermissionScope::Global);
    queries::upsert_personnel_certification(&state.db, input).await
}

#[tauri::command]
pub async fn list_training_sessions(state: State<'_, AppState>) -> AppResult<Vec<TrainingSession>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    training::list_training_sessions(&state.db).await
}

#[tauri::command]
pub async fn upsert_training_session(
    input: TrainingSessionUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<TrainingSession> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    training::upsert_training_session(&state.db, input).await
}

#[tauri::command]
pub async fn list_training_attendance(
    filter: TrainingAttendanceListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<TrainingAttendance>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    training::list_training_attendance(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_training_attendance(
    input: TrainingAttendanceUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<TrainingAttendance> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    training::upsert_training_attendance(&state.db, input).await
}

#[tauri::command]
pub async fn list_document_acknowledgements(
    filter: DocumentAcknowledgementListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<DocumentAcknowledgement>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    training::list_document_acknowledgements(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_document_acknowledgement(
    input: DocumentAcknowledgementUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<DocumentAcknowledgement> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    training::upsert_document_acknowledgement(&state.db, input).await
}

#[tauri::command]
pub async fn list_my_training_sessions(state: State<'_, AppState>) -> AppResult<Vec<TrainingAttendance>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
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
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
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
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    readiness::list_personnel_readiness(&state.db, filter).await
}

#[tauri::command]
pub async fn evaluate_crew_permit_skill_gaps(
    input: CrewPermitSkillGapInput,
    state: State<'_, AppState>,
) -> AppResult<CrewPermitSkillGapResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    readiness::evaluate_crew_permit_skill_gaps(&state.db, input).await
}

#[tauri::command]
pub async fn list_personnel_readiness_snapshots(
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelReadinessSnapshot>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    readiness::list_personnel_readiness_snapshots(&state.db).await
}

#[tauri::command]
pub async fn upsert_personnel_readiness_snapshot(
    input: PersonnelReadinessSnapshotUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<PersonnelReadinessSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    readiness::upsert_personnel_readiness_snapshot(&state.db, input).await
}

#[tauri::command]
pub async fn refresh_personnel_readiness_snapshot(
    period: String,
    state: State<'_, AppState>,
) -> AppResult<PersonnelReadinessSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    readiness::refresh_personnel_readiness_snapshot_payload(&state.db, period).await
}

#[tauri::command]
pub async fn list_training_expiry_alert_events(
    filter: TrainingExpiryAlertEventListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<TrainingExpiryAlertEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    expiry_alerts::list_training_expiry_alert_events(&state.db, filter).await
}

#[tauri::command]
pub async fn scan_training_expiry_alerts(
    lookahead_days: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<TrainingExpiryAlertEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.manage", PermissionScope::Global);
    expiry_alerts::scan_training_expiry_alerts(&state.db, lookahead_days.unwrap_or(90)).await
}

#[tauri::command]
pub async fn list_certification_expiry_drilldown(
    entity_id: Option<i64>,
    lookahead_days: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<CertificationExpiryDrilldownRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "trn.view", PermissionScope::Global);
    expiry_alerts::list_certification_expiry_drilldown(&state.db, entity_id, lookahead_days.unwrap_or(90)).await
}
