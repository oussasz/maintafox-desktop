use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationPolicySnapshot {
    pub fingerprint_max_drift: i64,
    pub grace_hours: i64,
    pub offline_allowed_states: Vec<String>,
    pub reconnect_revocation_blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyMachineActivationInput {
    pub contract_id: String,
    pub machine_id: String,
    pub slot_assignment_id: String,
    pub slot_number: i64,
    pub slot_limit: i64,
    pub trust_score: i64,
    pub vps_version: i64,
    pub response_nonce: String,
    pub issued_at: String,
    pub expires_at: String,
    pub offline_grace_until: String,
    pub revocation_state: String,
    pub revocation_reason: Option<String>,
    pub anchor_hashes_json: String,
    pub policy_snapshot_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineActivationStatus {
    pub contract_id: Option<String>,
    pub machine_id: Option<String>,
    pub slot_assignment_id: Option<String>,
    pub slot_number: Option<i64>,
    pub slot_limit: Option<i64>,
    pub trust_score: Option<i64>,
    pub revocation_state: String,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub offline_grace_until: Option<String>,
    pub drift_score: i64,
    pub drift_within_tolerance: bool,
    pub denial_code: Option<String>,
    pub denial_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineActivationApplyResult {
    pub contract_id: String,
    pub trusted_binding: bool,
    pub drift_score: i64,
    pub slot_assignment_consistent: bool,
    pub replay_rejected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineActivationDecision {
    pub allowed: bool,
    pub denial_code: Option<String>,
    pub denial_message: Option<String>,
    pub requires_online_reconnect: bool,
    pub grace_hours_remaining: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationLineageRecord {
    pub id: String,
    pub event_code: String,
    pub contract_id: Option<String>,
    pub slot_assignment_id: Option<String>,
    pub detail_json: String,
    pub occurred_at: String,
    pub actor_user_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineActivationDiagnostics {
    pub status: MachineActivationStatus,
    pub last_reconnect_at: Option<String>,
    pub last_revocation_applied_at: Option<String>,
    pub lineage: Vec<ActivationLineageRecord>,
    pub runbook_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateActivationSecretInput {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateActivationSecretResult {
    pub rotated: bool,
    pub rotated_at: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebindMachineActivationInput {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebindMachineActivationResult {
    pub previous_contract_id: Option<String>,
    pub rebind_required: bool,
    pub rebind_requested_at: String,
    pub reason: String,
}
