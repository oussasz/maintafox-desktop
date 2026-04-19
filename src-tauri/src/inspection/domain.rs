use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionTemplate {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub name: String,
    pub org_scope_id: Option<i64>,
    pub route_scope: Option<String>,
    pub estimated_duration_minutes: Option<i64>,
    pub is_active: bool,
    pub current_version_id: Option<i64>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionTemplateVersion {
    pub id: i64,
    pub entity_sync_id: String,
    pub template_id: i64,
    pub version_no: i64,
    pub effective_from: Option<String>,
    pub checkpoint_package_json: String,
    pub tolerance_rules_json: Option<String>,
    pub escalation_rules_json: Option<String>,
    pub requires_review: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionCheckpoint {
    pub id: i64,
    pub entity_sync_id: String,
    pub template_version_id: i64,
    pub sequence_order: i64,
    pub asset_id: Option<i64>,
    pub component_id: Option<i64>,
    pub checkpoint_code: String,
    pub check_type: String,
    pub measurement_unit: Option<String>,
    pub normal_min: Option<f64>,
    pub normal_max: Option<f64>,
    pub warning_min: Option<f64>,
    pub warning_max: Option<f64>,
    pub requires_photo: bool,
    pub requires_comment_on_exception: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRound {
    pub id: i64,
    pub entity_sync_id: String,
    pub template_id: i64,
    pub template_version_id: i64,
    pub scheduled_at: Option<String>,
    pub assigned_to_id: Option<i64>,
    pub status: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionCheckpointDraft {
    pub sequence_order: i64,
    pub asset_id: Option<i64>,
    pub component_id: Option<i64>,
    pub checkpoint_code: String,
    pub check_type: String,
    pub measurement_unit: Option<String>,
    pub normal_min: Option<f64>,
    pub normal_max: Option<f64>,
    pub warning_min: Option<f64>,
    pub warning_max: Option<f64>,
    pub requires_photo: Option<bool>,
    pub requires_comment_on_exception: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInspectionTemplateInput {
    pub code: String,
    pub name: String,
    pub org_scope_id: Option<i64>,
    pub route_scope: Option<String>,
    pub estimated_duration_minutes: Option<i64>,
    pub is_active: Option<bool>,
    pub checkpoints: Vec<InspectionCheckpointDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishInspectionTemplateVersionInput {
    pub template_id: i64,
    pub expected_row_version: i64,
    pub effective_from: Option<String>,
    pub requires_review: Option<bool>,
    pub tolerance_rules_json: Option<String>,
    pub escalation_rules_json: Option<String>,
    pub checkpoints: Vec<InspectionCheckpointDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleInspectionRoundInput {
    pub template_id: i64,
    pub scheduled_at: Option<String>,
    pub assigned_to_id: Option<i64>,
    pub explicit_template_version_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionTemplateVersionsFilter {
    pub template_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionCheckpointsFilter {
    pub template_version_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionResult {
    pub id: i64,
    pub entity_sync_id: String,
    pub round_id: i64,
    pub checkpoint_id: i64,
    pub result_status: String,
    pub numeric_value: Option<f64>,
    pub text_value: Option<String>,
    pub boolean_value: Option<bool>,
    pub comment: Option<String>,
    pub recorded_at: String,
    pub recorded_by_id: i64,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionEvidence {
    pub id: i64,
    pub result_id: i64,
    pub evidence_type: String,
    pub file_path_or_value: String,
    pub captured_at: String,
    pub entity_sync_id: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionAnomaly {
    pub id: i64,
    pub round_id: i64,
    pub result_id: Option<i64>,
    pub anomaly_type: String,
    pub severity: i64,
    pub description: String,
    pub linked_di_id: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub requires_permit_review: bool,
    pub resolution_status: String,
    pub routing_decision: Option<String>,
    pub entity_sync_id: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionOfflineQueueItem {
    pub id: i64,
    pub payload_json: String,
    pub local_temp_id: String,
    pub sync_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordInspectionResultInput {
    pub round_id: i64,
    pub checkpoint_id: i64,
    pub result_status: Option<String>,
    pub numeric_value: Option<f64>,
    pub text_value: Option<String>,
    pub boolean_value: Option<bool>,
    pub comment: Option<String>,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddInspectionEvidenceInput {
    pub result_id: i64,
    pub evidence_type: String,
    pub file_path_or_value: String,
    pub captured_at: Option<String>,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInspectionAnomalyInput {
    pub id: i64,
    pub resolution_status: String,
    pub linked_di_id: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub requires_permit_review: Option<bool>,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionResultsFilter {
    pub round_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionEvidenceFilter {
    pub result_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionAnomaliesFilter {
    pub round_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnqueueInspectionOfflineInput {
    pub payload_json: String,
    pub local_temp_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionReliabilitySignal {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub period_start: String,
    pub period_end: String,
    pub warning_count: i64,
    pub fail_count: i64,
    pub anomaly_open_count: i64,
    pub checkpoint_coverage_ratio: f64,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionReliabilitySignalsFilter {
    pub equipment_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshInspectionReliabilitySignalsInput {
    pub window_days: i64,
}
