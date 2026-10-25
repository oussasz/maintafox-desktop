use serde::{Deserialize, Serialize};

pub const SYNC_PROTOCOL_VERSION_V1: &str = "v1";
/// Protocol v2 (`maintafox-vendor-console/deploy/SYNC_PROTOCOL_V2.md`). The local data model
/// supports it; the v2 transport arrives with step 2.3.
pub const SYNC_PROTOCOL_VERSION_V2: &str = "v2";

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
    /// `pending` | `in_flight` | `acked` | `rejected` | `conflict` | `dead_letter` | `superseded`.
    pub status: String,
    pub acknowledged_at: Option<String>,
    pub rejection_code: Option<String>,
    pub rejection_message: Option<String>,
    pub origin_machine_id: Option<String>,
    /// Server row version this change was made on; `None` when the row is created here.
    #[serde(default)]
    pub base_row_version: Option<i64>,
    /// Owning user for user-scoped entities.
    #[serde(default)]
    pub owner_user_sync_id: Option<String>,
    #[serde(default)]
    pub attempt_count: i64,
    #[serde(default)]
    pub last_attempt_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
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
    /// `pending` | `applied` | `deferred` | `conflict` | `rejected` | `superseded`.
    pub apply_status: String,
    pub rejection_code: Option<String>,
    pub rejection_message: Option<String>,
    #[serde(default)]
    pub server_seq: Option<i64>,
    #[serde(default)]
    pub owner_user_sync_id: Option<String>,
    #[serde(default)]
    pub attempt_count: i64,
    #[serde(default)]
    pub last_attempt_at: Option<String>,
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

/// A change to stage in the outbox.
///
/// The envelope's idempotency key, wire `row_version`, `base_row_version`, owner and origin are
/// assigned by `stage_outbox_item`, so every stager follows the same versioning rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageOutboxItemInput {
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListOutboxFilter {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncAckInput {
    pub idempotency_key: String,
    pub entity_sync_id: String,
    pub operation: String,
    /// v2: position of the applied item in the server log.
    #[serde(default)]
    pub server_seq: Option<i64>,
    /// v2: the item was already applied by an earlier request.
    #[serde(default)]
    pub duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRejectedItemInput {
    pub idempotency_key: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub rejection_code: String,
    pub rejection_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncInboundItemInput {
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub row_version: i64,
    pub payload_json: String,
    /// v2: position in the server log.
    #[serde(default)]
    pub server_seq: Option<i64>,
    /// v2: SHA-256 of `payload_json` computed by the server; checked on ingest when present.
    #[serde(default)]
    pub payload_hash: Option<String>,
    /// v2: owning user for user-scoped entities.
    #[serde(default)]
    pub owner_user_sync_id: Option<String>,
    /// v2: tombstone (delivered with operation `delete`).
    #[serde(default)]
    pub deleted: bool,
}

/// v2: an outbox item the server refused because the row changed on the server meanwhile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncServerConflictInput {
    pub idempotency_key: String,
    pub entity_type: String,
    pub entity_sync_id: String,
    pub local_row_version: i64,
    pub local_base_row_version: Option<i64>,
    pub server_row_version: i64,
    pub server_operation: String,
    pub server_payload_json: Option<String>,
    #[serde(default)]
    pub server_payload_hash: Option<String>,
    #[serde(default)]
    pub server_owner_user_sync_id: Option<String>,
    #[serde(default)]
    pub server_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplySyncBatchInput {
    pub protocol_version: String,
    pub server_batch_id: String,
    pub checkpoint_token: String,
    pub acknowledged_items: Vec<SyncAckInput>,
    pub rejected_items: Vec<SyncRejectedItemInput>,
    pub inbound_items: Vec<SyncInboundItemInput>,
    /// v2 only; always empty for v1 responses.
    #[serde(default)]
    pub conflict_items: Vec<SyncServerConflictInput>,
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
    /// v2 server conflicts recorded for a person to resolve.
    #[serde(default)]
    pub conflict_count: i64,
    pub typed_rejections: Vec<SyncTypedRejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMediaManifestItem {
    pub entity_sync_id: String,
    pub content_sha256: String,
    pub object_key: String,
    pub byte_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushPayload {
    pub protocol_version: String,
    pub checkpoint_token: Option<String>,
    pub outbox_batch: Vec<SyncOutboxItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_config: Option<TenantConfigSyncPayload>,
    /// Additive v1 — desktop always `"desktop"`; mobile may filter later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_manifest: Option<Vec<SyncMediaManifestItem>>,
    /// v2: users known on this device; user-scoped rows are delivered only for them.
    #[serde(default)]
    pub current_user_sync_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStateSummary {
    pub protocol_version: String,
    pub checkpoint: Option<SyncCheckpoint>,
    pub pending_outbox_count: i64,
    pub rejected_outbox_count: i64,
    pub inbox_error_count: i64,
    /// Sent but not yet acknowledged (the exchange may have failed; they are sent again).
    pub in_flight_outbox_count: i64,
    /// Gave up after the maximum number of delivery attempts; listed as conflicts.
    pub dead_letter_outbox_count: i64,
    /// Inbound rows waiting for a parent row or for a materializer.
    pub deferred_inbox_count: i64,
    /// Inbound rows that could not be applied and wait for a person.
    pub inbox_conflict_count: i64,
    /// Conflicts not yet resolved by a person (all sources).
    pub unresolved_conflict_count: i64,
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
    /// Version of the local change involved (outbox side), when known.
    #[serde(default)]
    pub local_row_version: Option<i64>,
    /// Version of the server/inbound row involved, when known.
    #[serde(default)]
    pub inbound_row_version: Option<i64>,
    /// The error or rule text recorded when the conflict was found (technical details).
    #[serde(default)]
    pub detail_message: Option<String>,
    /// That text as a plain, coded reason, for a row the database refused (`sync::apply_reason`).
    #[serde(default)]
    pub reason: Option<crate::sync::apply_reason::ConflictReason>,
    /// A stock count held back here for a second approval (`COUNT_SELF_APPROVAL`): its reference,
    /// counter and posting time, read from what this PC holds (CMMS Advisor Q12).
    #[serde(default)]
    pub count_hold: Option<crate::inventory::count_four_eyes::CountHoldContext>,
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
pub const SYNC_ENTITY_WORK_ORDER_TASKS: &str = "work_order_tasks";
pub const SYNC_ENTITY_WORK_ORDER_INTERVENERS: &str = "work_order_interveners";
pub const SYNC_ENTITY_DATA_INTEGRITY_FINDINGS: &str = "data_integrity_findings";
pub const SYNC_ENTITY_DATA_INTEGRITY_REPAIR_ACTIONS: &str = "data_integrity_repair_actions";
pub const SYNC_ENTITY_ANALYTICS_CONTRACT_VERSIONS: &str = "analytics_contract_versions";
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

// Entity type names used by stagers and materializers. The full list of replicated tables,
// their scope and apply order lives in `sync::registry`.
pub const SYNC_ENTITY_EQUIPMENT: &str = "equipment";
pub const SYNC_ENTITY_USER_ACCOUNTS: &str = "user_accounts";
pub const SYNC_ENTITY_ROLES: &str = "roles";
pub const SYNC_ENTITY_ORG_NODES: &str = "org_nodes";
pub const SYNC_ENTITY_PERSONNEL: &str = "personnel";
pub const SYNC_ENTITY_TEAMS: &str = "teams";
pub const SYNC_ENTITY_PM_PLANS: &str = "pm_plans";
pub const SYNC_ENTITY_PM_PLAN_VERSIONS: &str = "pm_plan_versions";
pub const SYNC_ENTITY_PM_OCCURRENCES: &str = "pm_occurrences";
pub const SYNC_ENTITY_ARTICLES: &str = "articles";
pub const SYNC_ENTITY_WAREHOUSES: &str = "warehouses";
pub const SYNC_ENTITY_STOCK_LOCATIONS: &str = "stock_locations";
pub const SYNC_ENTITY_STOCK_MOVEMENTS: &str = "inventory_transactions";
pub const SYNC_ENTITY_STOCK_RESERVATIONS: &str = "stock_reservations";
pub const SYNC_ENTITY_MEDIA_ATTACHMENTS: &str = "media_attachments";
pub const SYNC_ENTITY_REFERENCE_VALUES: &str = "reference_values";

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
pub struct WorkOrderSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance_type_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_responsible_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_team_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closeout_validation_profile_id: Option<i64>,
    #[serde(default)]
    pub closeout_validation_passed: bool,
    #[serde(default)]
    pub requires_permit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_inspection_anomaly_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkOrderTaskSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub work_order_entity_sync_id: String,
    pub task_description: String,
    #[serde(default)]
    pub sequence_order: i64,
    #[serde(default)]
    pub is_mandatory: bool,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkOrderIntervenerSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub work_order_entity_sync_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intervener_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours_worked: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockReservationSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub article_entity_sync_id: String,
    pub warehouse_entity_sync_id: String,
    pub location_entity_sync_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_order_entity_sync_id: Option<String>,
    pub quantity: f64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanVersionSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub pm_plan_entity_sync_id: String,
    pub version_no: i64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_definition_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmOccurrenceSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub pm_plan_entity_sync_id: String,
    pub pm_plan_version_entity_sync_id: String,
    pub due_basis: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_work_order_entity_sync_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub status_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_node_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionRequestSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub origin_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_node_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitter_entity_sync_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachmentSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub parent_entity_type: String,
    pub parent_entity_sync_id: String,
    pub content_sha256: String,
    pub mime_type: String,
    pub byte_size: i64,
    pub object_key: String,
    pub file_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_meta_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNodeSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub name: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_entity_sync_id: Option<String>,
    #[serde(default)]
    pub depth: i64,
    #[serde(default)]
    pub ancestor_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccountSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Password hash travels in the tenant replica; never log this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
    #[serde(default)]
    pub identity_mode: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub role_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_system: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub employee_code: String,
    pub full_name: String,
    #[serde(default)]
    pub availability_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceValueSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub set_code: String,
    pub code: String,
    pub label: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub strategy_type: String,
    pub asset_scope_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_team_entity_sync_id: Option<String>,
    #[serde(default)]
    pub lifecycle_status: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub article_code: String,
    pub article_name: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_value_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarehouseSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockLocationSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub code: String,
    pub name: String,
    pub warehouse_entity_sync_id: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockMovementSyncPayload {
    pub entity_sync_id: String,
    pub row_version: i64,
    pub article_entity_sync_id: String,
    pub warehouse_entity_sync_id: String,
    pub location_entity_sync_id: String,
    pub movement_type: String,
    pub quantity: f64,
    #[serde(default)]
    pub source_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
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
