use serde::{Deserialize, Serialize};

pub const SYNC_PROTOCOL_VERSION_V1: &str = "v1";

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
