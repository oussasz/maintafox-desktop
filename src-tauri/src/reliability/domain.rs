use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureHierarchy {
    pub id: i64,
    pub entity_sync_id: String,
    pub name: String,
    pub asset_scope_json: String,
    pub version_no: i64,
    pub is_active: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCode {
    pub id: i64,
    pub entity_sync_id: String,
    pub hierarchy_id: i64,
    pub parent_id: Option<i64>,
    pub code: String,
    pub label: String,
    pub code_type: String,
    pub iso_14224_annex_ref: Option<String>,
    pub is_active: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureHierarchyUpsertInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub name: String,
    pub asset_scope_json: String,
    pub version_no: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCodeUpsertInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub hierarchy_id: i64,
    pub parent_id: Option<i64>,
    pub code: String,
    pub label: String,
    pub code_type: String,
    pub iso_14224_annex_ref: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCodesFilter {
    pub hierarchy_id: i64,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeactivateFailureCodeInput {
    pub id: i64,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEvent {
    pub id: i64,
    pub entity_sync_id: String,
    pub source_type: String,
    pub source_id: i64,
    pub equipment_id: i64,
    pub component_id: Option<i64>,
    pub detected_at: Option<String>,
    pub failed_at: Option<String>,
    pub restored_at: Option<String>,
    pub downtime_duration_hours: f64,
    pub active_repair_hours: f64,
    pub waiting_hours: f64,
    pub is_planned: bool,
    pub failure_class_id: Option<i64>,
    pub failure_mode_id: Option<i64>,
    pub failure_cause_id: Option<i64>,
    pub failure_effect_id: Option<i64>,
    pub failure_mechanism_id: Option<i64>,
    pub cause_not_determined: bool,
    pub production_impact_level: Option<i64>,
    pub safety_impact_level: Option<i64>,
    pub recorded_by_id: Option<i64>,
    pub verification_status: String,
    pub eligible_flags_json: String,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostOfFailureRow {
    pub equipment_id: i64,
    pub period: String,
    pub total_downtime_cost: f64,
    pub total_corrective_cost: f64,
    pub currency_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostOfFailureFilter {
    pub equipment_id: Option<i64>,
    pub period: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailureEventsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertFailureEventInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub source_type: String,
    pub source_id: i64,
    pub equipment_id: i64,
    pub component_id: Option<i64>,
    pub detected_at: Option<String>,
    pub failed_at: Option<String>,
    pub restored_at: Option<String>,
    pub downtime_duration_hours: f64,
    pub active_repair_hours: f64,
    pub waiting_hours: f64,
    pub is_planned: bool,
    pub failure_class_id: Option<i64>,
    pub failure_mode_id: Option<i64>,
    pub failure_cause_id: Option<i64>,
    pub failure_effect_id: Option<i64>,
    pub failure_mechanism_id: Option<i64>,
    pub cause_not_determined: bool,
    pub production_impact_level: Option<i64>,
    pub safety_impact_level: Option<i64>,
    pub recorded_by_id: Option<i64>,
    pub verification_status: String,
    pub eligible_flags_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeExposureLog {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub exposure_type: String,
    pub value: f64,
    pub recorded_at: String,
    pub source_type: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertRuntimeExposureLogInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub equipment_id: i64,
    pub exposure_type: String,
    pub value: f64,
    pub recorded_at: String,
    pub source_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeExposureLogsFilter {
    pub equipment_id: Option<i64>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityKpiSnapshot {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: Option<i64>,
    pub asset_group_id: Option<i64>,
    pub period_start: String,
    pub period_end: String,
    pub mtbf: Option<f64>,
    pub mttr: Option<f64>,
    pub availability: Option<f64>,
    pub failure_rate: Option<f64>,
    pub repeat_failure_rate: Option<f64>,
    pub event_count: i64,
    pub data_quality_score: f64,
    pub inspection_signal_json: Option<String>,
    pub analysis_dataset_hash_sha256: String,
    pub analysis_input_spec_json: String,
    pub plot_payload_json: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityAnalysisInputEvaluation {
    pub equipment_id: i64,
    pub period_start: String,
    pub period_end: String,
    pub exposure_hours: f64,
    pub eligible_event_count: i64,
    pub min_sample_n: i64,
    pub analysis_dataset_hash_sha256: String,
    pub analysis_input_spec_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshReliabilityKpiSnapshotInput {
    pub equipment_id: i64,
    pub period_start: String,
    pub period_end: String,
    pub min_sample_n: Option<i64>,
    pub repeat_lookback_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReliabilityKpiSnapshotsFilter {
    pub equipment_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamDataQualityIssue {
    pub equipment_id: i64,
    pub issue_code: String,
    pub severity: String,
    pub remediation_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RamDataQualityIssuesFilter {
    pub equipment_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoMissingFailureModeRow {
    pub work_order_id: i64,
    pub equipment_id: i64,
    pub closed_at: Option<String>,
    pub type_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentMissingExposureRow {
    pub equipment_id: i64,
    pub equipment_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamEquipmentQualityBadge {
    pub equipment_id: i64,
    pub data_quality_score: Option<f64>,
    pub badge: String,
    pub blocking_issue_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DismissRamDataQualityIssueInput {
    pub equipment_id: i64,
    pub issue_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDismissal {
    pub id: i64,
    pub entity_sync_id: String,
    pub user_id: i64,
    pub equipment_id: i64,
    pub issue_code: String,
    pub scope_key: String,
    pub dismissed_at: String,
    pub row_version: i64,
}

/// ISO 14224-style failure record completeness (equipment-scoped `failure_events`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Iso14224DatasetCompleteness {
    pub equipment_id: i64,
    pub event_count: i64,
    pub completeness_percent: f64,
    pub dim_equipment_id_pct: f64,
    pub dim_failure_interval_pct: f64,
    pub dim_failure_mode_pct: f64,
    pub dim_corrective_closure_pct: f64,
}
