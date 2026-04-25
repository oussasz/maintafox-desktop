use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificationType {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub name: String,
    pub default_validity_months: Option<i64>,
    pub renewal_lead_days: Option<i64>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationRequirementProfile {
    pub id: i64,
    pub entity_sync_id: String,
    pub profile_name: String,
    pub required_certification_type_ids_json: String,
    pub applies_to_permit_type_codes_json: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelCertification {
    pub id: i64,
    pub entity_sync_id: String,
    pub personnel_id: i64,
    pub certification_type_id: i64,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub issuing_body: Option<String>,
    pub certificate_ref: Option<String>,
    pub verification_status: String,
    pub row_version: i64,
    pub readiness_status: String,
    pub certification_type_code: Option<String>,
    pub certification_type_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificationTypeUpsertInput {
    pub id: Option<i64>,
    pub code: String,
    pub name: String,
    pub default_validity_months: Option<i64>,
    pub renewal_lead_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationRequirementProfileUpsertInput {
    pub id: Option<i64>,
    pub profile_name: String,
    pub required_certification_type_ids_json: String,
    pub applies_to_permit_type_codes_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelCertificationUpsertInput {
    pub id: Option<i64>,
    pub personnel_id: i64,
    pub certification_type_id: i64,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub issuing_body: Option<String>,
    pub certificate_ref: Option<String>,
    pub verification_status: String,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersonnelCertificationListFilter {
    pub personnel_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSession {
    pub id: i64,
    pub entity_sync_id: String,
    pub course_code: String,
    pub scheduled_start: String,
    pub scheduled_end: String,
    pub location: Option<String>,
    pub instructor_id: Option<i64>,
    pub certification_type_id: Option<i64>,
    pub min_pass_score: i64,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAttendance {
    pub id: i64,
    pub entity_sync_id: String,
    pub session_id: i64,
    pub personnel_id: i64,
    pub attendance_status: String,
    pub completed_at: Option<String>,
    pub score: Option<f64>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentAcknowledgement {
    pub id: i64,
    pub entity_sync_id: String,
    pub personnel_id: i64,
    pub document_version_id: i64,
    pub acknowledged_at: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSessionUpsertInput {
    pub id: Option<i64>,
    pub course_code: String,
    pub scheduled_start: String,
    pub scheduled_end: String,
    pub location: Option<String>,
    pub instructor_id: Option<i64>,
    pub certification_type_id: Option<i64>,
    pub min_pass_score: Option<i64>,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAttendanceUpsertInput {
    pub id: Option<i64>,
    pub session_id: i64,
    pub personnel_id: i64,
    pub attendance_status: String,
    pub completed_at: Option<String>,
    pub score: Option<f64>,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentAcknowledgementUpsertInput {
    pub id: Option<i64>,
    pub personnel_id: i64,
    pub document_version_id: i64,
    pub acknowledged_at: String,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrainingAttendanceListFilter {
    pub session_id: Option<i64>,
    pub personnel_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentAcknowledgementListFilter {
    pub personnel_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelReadinessRow {
    pub personnel_id: i64,
    pub permit_type_code: String,
    pub is_qualified: bool,
    pub blocking_reason: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersonnelReadinessFilter {
    pub personnel_id: Option<i64>,
    pub permit_type_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewPermitSkillGapInput {
    pub work_order_id: i64,
    pub personnel_ids: Vec<i64>,
    pub permit_type_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewPermitSkillGapRow {
    pub personnel_id: i64,
    pub is_qualified: bool,
    pub blocking_reason: Option<String>,
    pub missing_certification_type_ids: Vec<i64>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewPermitSkillGapResult {
    pub permit_type_code: String,
    pub work_order_id: i64,
    pub rows: Vec<CrewPermitSkillGapRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelReadinessSnapshot {
    pub id: i64,
    pub entity_sync_id: String,
    pub period: String,
    pub payload_json: String,
    pub row_version: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelReadinessSnapshotUpsertInput {
    pub id: Option<i64>,
    pub period: String,
    pub payload_json: String,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingExpiryAlertEvent {
    pub id: i64,
    pub entity_sync_id: String,
    pub certification_id: i64,
    pub alert_dedupe_key: String,
    pub fired_at: String,
    pub severity: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrainingExpiryAlertEventListFilter {
    pub severity: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificationExpiryDrilldownRow {
    pub certification_id: i64,
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub primary_entity_id: Option<i64>,
    pub certification_type_id: i64,
    pub certification_type_code: String,
    pub expires_at: Option<String>,
    pub verification_status: String,
    pub readiness_status: String,
}
