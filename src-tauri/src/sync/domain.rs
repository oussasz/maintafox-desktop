use serde::{Deserialize, Serialize};

pub const SYNC_PROTOCOL_VERSION_V1: &str = "v1";

/// Device-side tenant context included with sync exchange payloads for control-plane isolation checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfigSyncPayload {
    pub tenant_id: String,
    pub is_activated: bool,
    pub company_display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOutboxItem {
    pub id: i64,
    pub idempotency_key: String,
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub row_version: i64,
    pub payload_json: String,
    pub payload_hash: String,
    pub status: String,
    pub acknowledged_at: Option<String>,
    pub rejection_code: Option<String>,
    pub rejection_message: Option<String>,
    pub origin_machine_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncInboxItem {
    pub id: i64,
    pub server_batch_id: String,
    pub checkpoint_token: String,
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub row_version: i64,
    pub payload_json: String,
    pub payload_hash: String,
    pub apply_status: String,
    pub rejection_code: Option<String>,
    pub rejection_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCheckpoint {
    pub id: i64,
    pub checkpoint_token: Option<String>,
    pub last_idempotency_key: Option<String>,
    pub protocol_version: String,
    pub policy_metadata_json: Option<String>,
    pub last_sync_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageOutboxItemInput {
    pub idempotency_key: String,
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub row_version: i64,
    pub payload_json: String,
    pub origin_machine_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListOutboxFilter {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAckInput {
    pub idempotency_key: String,
    pub entity_sync_id: String,
    pub operation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRejectedItemInput {
    pub idempotency_key: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub rejection_code: String,
    pub rejection_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncInboundItemInput {
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub row_version: i64,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplySyncBatchInput {
    pub protocol_version: String,
    pub server_batch_id: String,
    pub checkpoint_token: String,
    pub acknowledged_items: Vec<SyncAckInput>,
    pub rejected_items: Vec<SyncRejectedItemInput>,
    pub inbound_items: Vec<SyncInboundItemInput>,
    pub policy_metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTypedRejection {
    pub scope: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub rejection_code: String,
    pub rejection_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplySyncBatchResult {
    pub protocol_version: String,
    pub checkpoint_token: Option<String>,
    pub checkpoint_advanced: bool,
    pub acknowledged_count: i64,
    pub rejected_count: i64,
    pub inbound_applied_count: i64,
    pub inbound_duplicate_count: i64,
    pub typed_rejections: Vec<SyncTypedRejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushPayload {
    pub protocol_version: String,
    pub checkpoint_token: Option<String>,
    pub outbox_batch: Vec<SyncOutboxItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_config: Option<TenantConfigSyncPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStateSummary {
    pub protocol_version: String,
    pub checkpoint: Option<SyncCheckpoint>,
    pub pending_outbox_count: i64,
    pub rejected_outbox_count: i64,
    pub inbox_error_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncConflictFilter {
    pub statuses: Option<Vec<String>>,
    pub conflict_type: Option<String>,
    pub requires_operator_review: Option<bool>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflictRecord {
    pub id: i64,
    pub conflict_key: String,
    pub source_scope: String,
    pub source_batch_id: Option<String>,
    pub linked_outbox_id: Option<i64>,
    pub linked_inbox_id: Option<i64>,
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub conflict_type: String,
    pub local_payload_json: Option<String>,
    pub inbound_payload_json: Option<String>,
    pub authority_side: String,
    pub checkpoint_token: Option<String>,
    pub auto_resolution_policy: String,
    pub requires_operator_review: bool,
    pub recommended_action: String,
    pub status: String,
    pub resolution_action: Option<String>,
    pub resolution_note: Option<String>,
    pub resolved_by_id: Option<i64>,
    pub resolved_at: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveSyncConflictInput {
    pub conflict_id: i64,
    pub expected_row_version: i64,
    pub action: String,
    pub resolution_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySyncFailuresInput {
    pub replay_key: String,
    pub mode: String,
    pub reason: String,
    pub conflict_id: Option<i64>,
    pub outbox_id: Option<i64>,
    pub server_batch_id: Option<String>,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub checkpoint_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncReplayRun {
    pub id: i64,
    pub replay_key: String,
    pub mode: String,
    pub status: String,
    pub reason: String,
    pub requested_by_id: i64,
    pub scope_json: Option<String>,
    pub pre_replay_checkpoint: Option<String>,
    pub post_replay_checkpoint: Option<String>,
    pub result_json: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySyncFailuresResult {
    pub run: SyncReplayRun,
    pub requeued_outbox_count: i64,
    pub transitioned_conflict_count: i64,
    pub checkpoint_token_after: Option<String>,
    pub guard_applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRepairPreviewInput {
    pub mode: String,
    pub reason: String,
    pub outbox_ids: Option<Vec<i64>>,
    pub conflict_ids: Option<Vec<i64>>,
    pub server_batch_id: Option<String>,
    pub checkpoint_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRepairPreview {
    pub plan_id: String,
    pub mode: String,
    pub reason: String,
    pub affected_outbox_count: i64,
    pub affected_conflict_count: i64,
    pub projected_checkpoint_token: Option<String>,
    pub warnings: Vec<String>,
    pub requires_confirmation: bool,
    pub risk_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteSyncRepairInput {
    pub plan_id: String,
    pub confirm_phrase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRepairExecutionResult {
    pub plan_id: String,
    pub mode: String,
    pub status: String,
    pub requeued_outbox_count: i64,
    pub transitioned_conflict_count: i64,
    pub checkpoint_token_after: Option<String>,
    pub executed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRepairActionRecord {
    pub id: i64,
    pub plan_id: String,
    pub mode: String,
    pub status: String,
    pub reason: String,
    pub created_by_id: i64,
    pub executed_by_id: Option<i64>,
    pub scope_json: Option<String>,
    pub preview_json: Option<String>,
    pub result_json: Option<String>,
    pub created_at: String,
    pub executed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHealthMetrics {
    pub generated_at: String,
    pub pending_outbox_count: i64,
    pub rejected_outbox_count: i64,
    pub unresolved_conflict_count: i64,
    pub replay_runs_last_24h: i64,
    pub repair_runs_last_24h: i64,
    pub checkpoint_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHealthAlert {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub runbook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRecoveryProof {
    pub workflow: String,
    pub reference_id: String,
    pub failure_at: String,
    pub recovered_at: String,
    pub duration_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncObservabilityReport {
    pub metrics: SyncHealthMetrics,
    pub alerts: Vec<SyncHealthAlert>,
    pub recovery_proofs: Vec<SyncRecoveryProof>,
    pub diagnostics_links: Vec<String>,
}

pub const SYNC_ENTITY_PERMIT_TYPES: &str = "permit_types";
pub const SYNC_ENTITY_WORK_ORDERS: &str = "work_orders";
pub const SYNC_ENTITY_DATA_INTEGRITY_FINDINGS: &str = "data_integrity_findings";
pub const SYNC_ENTITY_DATA_INTEGRITY_REPAIR_ACTIONS: &str = "data_integrity_repair_actions";
pub const SYNC_ENTITY_ANALYTICS_CONTRACT_VERSIONS: &str = "analytics_contract_versions";
pub const SYNC_ENTITY_CLOSEOUT_VALIDATION_POLICIES: &str = "closeout_validation_policies";
pub const SYNC_ENTITY_INTERVENTION_REQUESTS: &str = "intervention_requests";
pub const SYNC_ENTITY_WORK_PERMITS: &str = "work_permits";
pub const SYNC_ENTITY_PERMIT_ISOLATIONS: &str = "permit_isolations";
pub const SYNC_ENTITY_PERMIT_SUSPENSIONS: &str = "permit_suspensions";
pub const SYNC_ENTITY_PERMIT_HANDOVER_LOGS: &str = "permit_handover_logs";
pub const SYNC_ENTITY_LOTO_CARD_PRINT_JOBS: &str = "loto_card_print_jobs";

pub const SYNC_ENTITY_CERTIFICATION_TYPES: &str = "certification_types";
pub const SYNC_ENTITY_PERSONNEL_CERTIFICATIONS: &str = "personnel_certifications";
pub const SYNC_ENTITY_QUALIFICATION_REQUIREMENT_PROFILES: &str = "qualification_requirement_profiles";
pub const SYNC_ENTITY_TRAINING_SESSIONS: &str = "training_sessions";
pub const SYNC_ENTITY_TRAINING_ATTENDANCE: &str = "training_attendance";
pub const SYNC_ENTITY_PERSONNEL_READINESS_SNAPSHOTS: &str = "personnel_readiness_snapshots";
pub const SYNC_ENTITY_TRAINING_EXPIRY_ALERT_EVENTS: &str = "training_expiry_alert_events";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelReadinessSnapshotSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub period: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingExpiryAlertEventSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub certification_id: i64,
    pub alert_dedupe_key: String,
    pub fired_at: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSessionSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub course_code: String,
    pub scheduled_start: String,
    pub scheduled_end: String,
    pub location: Option<String>,
    pub instructor_id: Option<i64>,
    pub certification_type_id: Option<i64>,
    pub min_pass_score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingAttendanceSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub session_id: i64,
    pub personnel_id: i64,
    pub attendance_status: String,
    pub completed_at: Option<String>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificationTypeSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub name: String,
    pub default_validity_months: Option<i64>,
    pub renewal_lead_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelCertificationSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub personnel_id: i64,
    pub certification_type_id: i64,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub issuing_body: Option<String>,
    pub certificate_ref: Option<String>,
    pub verification_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualificationRequirementProfileSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub profile_name: String,
    pub required_certification_type_ids: Vec<i64>,
    pub applies_to_permit_type_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionRequestSyncPayload {
    pub id: i64,
    pub row_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_inspection_anomaly_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataIntegrityFindingSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub severity: String,
    pub domain: String,
    pub record_class: String,
    pub record_id: i64,
    pub finding_code: String,
    pub details_json: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataIntegrityRepairActionSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub finding_id: i64,
    pub action: String,
    pub actor_id: i64,
    pub before_json: String,
    pub after_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsContractVersionSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub contract_id: String,
    pub version_semver: String,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkOrderSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub status: String,
    pub maintenance_type_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closeout_validation_profile_id: Option<i64>,
    pub closeout_validation_passed: bool,
    pub code: String,
    pub status_id: i64,
    pub requires_permit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_inspection_anomaly_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseoutValidationPolicySyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub policy_name: String,
    pub applies_when: serde_json::Value,
    pub require_failure_mode_if_unplanned: bool,
    pub require_downtime_if_production_impact: bool,
    pub allow_close_with_cause_not_determined: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitTypeSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub max_duration_hours: Option<f64>,
    pub mandatory_control_rules_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPermitSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub linked_work_order_id: Option<i64>,
    pub permit_type_id: i64,
    pub status: String,
    pub requested_at: Option<String>,
    pub issued_at: Option<String>,
    pub activated_at: Option<String>,
    pub expires_at: Option<String>,
    pub closed_at: Option<String>,
    pub handed_back_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitIsolationSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub permit_id: i64,
    pub isolation_point: String,
    pub energy_type: String,
    pub lock_number: Option<String>,
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitSuspensionSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub reason: String,
    pub suspended_by_id: i64,
    pub suspended_at: String,
    pub reinstated_by_id: Option<i64>,
    pub reinstated_at: Option<String>,
    pub reactivation_conditions: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitHandoverLogSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub handed_from_role: String,
    pub handed_to_role: String,
    pub confirmation_note: String,
    pub signed_at: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotoCardPrintJobSyncPayload {
    pub id: i64,
    pub permit_id: i64,
    pub isolation_id: i64,
    pub printed_at: String,
    pub printed_by_id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
}

pub const SYNC_ENTITY_INSPECTION_TEMPLATES: &str = "inspection_templates";
pub const SYNC_ENTITY_INSPECTION_TEMPLATE_VERSIONS: &str = "inspection_template_versions";
pub const SYNC_ENTITY_INSPECTION_CHECKPOINTS: &str = "inspection_checkpoints";
pub const SYNC_ENTITY_INSPECTION_ROUNDS: &str = "inspection_rounds";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionTemplateSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub name: String,
    pub is_active: bool,
    pub current_version_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionTemplateVersionSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub template_id: i64,
    pub version_no: i64,
    pub effective_from: Option<String>,
    pub checkpoint_package_json: String,
    pub requires_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionCheckpointSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub template_version_id: i64,
    pub sequence_order: i64,
    pub checkpoint_code: String,
    pub check_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRoundSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub template_version_id: i64,
    pub scheduled_at: Option<String>,
    pub status: String,
    pub assigned_to_id: Option<i64>,
}

pub const SYNC_ENTITY_INSPECTION_RESULTS: &str = "inspection_results";
pub const SYNC_ENTITY_INSPECTION_EVIDENCE: &str = "inspection_evidence";
pub const SYNC_ENTITY_INSPECTION_ANOMALIES: &str = "inspection_anomalies";
pub const SYNC_ENTITY_INSPECTION_RELIABILITY_SIGNALS: &str = "inspection_reliability_signals";

pub const SYNC_ENTITY_BUDGET_VERSIONS: &str = "budget_versions";
pub const SYNC_ENTITY_BUDGET_LINES: &str = "budget_lines";
pub const SYNC_ENTITY_BUDGET_ALERT_CONFIGS: &str = "budget_alert_configs";
pub const SYNC_ENTITY_BUDGET_ALERT_EVENTS: &str = "budget_alert_events";
pub const SYNC_ENTITY_POSTED_EXPORT_BATCHES: &str = "posted_export_batches";
pub const SYNC_ENTITY_INTEGRATION_EXCEPTIONS: &str = "integration_exceptions";

pub const SYNC_ENTITY_FAILURE_HIERARCHIES: &str = "failure_hierarchies";
pub const SYNC_ENTITY_FAILURE_CODES: &str = "failure_codes";
pub const SYNC_ENTITY_FAILURE_EVENTS: &str = "failure_events";
pub const SYNC_ENTITY_RUNTIME_EXPOSURE_LOGS: &str = "runtime_exposure_logs";
pub const SYNC_ENTITY_RELIABILITY_KPI_SNAPSHOTS: &str = "reliability_kpi_snapshots";
pub const SYNC_ENTITY_USER_DISMISSALS: &str = "user_dismissals";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetVersionSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub fiscal_year: i64,
    pub scenario_type: String,
    pub version_no: i64,
    pub status: String,
    pub currency_code: String,
    pub title: Option<String>,
    pub planning_basis: Option<String>,
    pub source_basis_mix_json: Option<String>,
    pub labor_assumptions_json: Option<String>,
    pub baseline_reference: Option<String>,
    pub erp_external_ref: Option<String>,
    pub successor_of_version_id: Option<i64>,
    pub created_by_id: Option<i64>,
    pub approved_at: Option<String>,
    pub approved_by_id: Option<i64>,
    pub frozen_at: Option<String>,
    pub frozen_by_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetLineSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub planned_amount: f64,
    pub source_basis: Option<String>,
    pub justification_note: Option<String>,
    pub asset_family: Option<String>,
    pub work_category: Option<String>,
    pub shutdown_package_ref: Option<String>,
    pub team_id: Option<i64>,
    pub skill_pool_id: Option<i64>,
    pub labor_lane: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertConfigSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub budget_bucket: Option<String>,
    pub alert_type: String,
    pub threshold_pct: Option<f64>,
    pub threshold_amount: Option<f64>,
    pub recipient_user_id: Option<i64>,
    pub recipient_role_id: Option<i64>,
    pub labor_template: Option<String>,
    pub dedupe_window_minutes: i64,
    pub requires_ack: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertEventSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub alert_config_id: Option<i64>,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub alert_type: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub dedupe_key: String,
    pub current_value: f64,
    pub threshold_value: Option<f64>,
    pub variance_amount: Option<f64>,
    pub currency_code: String,
    pub payload_json: Option<String>,
    pub notification_event_id: Option<i64>,
    pub notification_id: Option<i64>,
    pub acknowledged_at: Option<String>,
    pub acknowledged_by_id: Option<i64>,
    pub acknowledgement_note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostedExportBatchSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub batch_uuid: String,
    pub export_kind: String,
    pub tenant_id: Option<String>,
    pub relay_payload_json: String,
    pub total_posted: f64,
    pub line_count: i64,
    pub status: String,
    pub erp_ack_at: Option<String>,
    pub erp_http_code: Option<i64>,
    pub rejection_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationExceptionSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub posted_export_batch_id: i64,
    pub source_record_kind: String,
    pub source_record_id: i64,
    pub maintafox_value_snapshot: String,
    pub external_value_snapshot: Option<String>,
    pub resolution_status: String,
    pub rejection_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionReliabilitySignalSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub equipment_id: i64,
    pub period_start: String,
    pub period_end: String,
    pub warning_count: i64,
    pub fail_count: i64,
    pub anomaly_open_count: i64,
    pub checkpoint_coverage_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionResultSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub round_id: i64,
    pub checkpoint_id: i64,
    pub result_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boolean_value: Option<bool>,
    pub recorded_at: String,
    pub recorded_by_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionEvidenceSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub result_id: i64,
    pub evidence_type: String,
    pub file_path_or_value: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionAnomalySyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub round_id: i64,
    pub result_id: Option<i64>,
    pub anomaly_type: String,
    pub severity: i64,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_di_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_work_order_id: Option<i64>,
    pub requires_permit_review: bool,
    pub resolution_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureHierarchySyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub name: String,
    pub version_no: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCodeSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub hierarchy_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<i64>,
    pub code: String,
    pub code_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iso_14224_annex_ref: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEventSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub source_type: String,
    pub source_id: i64,
    pub equipment_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_at: Option<String>,
    pub downtime_duration_hours: f64,
    pub active_repair_hours: f64,
    pub waiting_hours: f64,
    pub is_planned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_mode_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_cause_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_effect_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_mechanism_id: Option<i64>,
    pub cause_not_determined: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production_impact_level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_impact_level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_by_id: Option<i64>,
    pub verification_status: String,
    pub eligible_flags_json: String,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeExposureLogSyncPayload {
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
pub struct ReliabilityKpiSnapshotSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equipment_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_group_id: Option<i64>,
    pub period_start: String,
    pub period_end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtbf: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mttr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_failure_rate: Option<f64>,
    pub event_count: i64,
    pub data_quality_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection_signal_json: Option<String>,
    pub analysis_dataset_hash_sha256: String,
    pub analysis_input_spec_json: String,
    #[serde(default = "default_plot_payload_json")]
    pub plot_payload_json: String,
    pub row_version: i64,
}

fn default_plot_payload_json() -> String {
    "{}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDismissalSyncPayload {
    pub id: i64,
    pub entity_sync_id: String,
    pub user_id: i64,
    pub equipment_id: i64,
    pub issue_code: String,
    pub scope_key: String,
    pub dismissed_at: String,
    pub row_version: i64,
}
