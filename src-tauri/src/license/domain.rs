use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseRejectionReason {
    pub code: String,
    pub message: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseEnforcementDecision {
    pub permission: String,
    pub capability_class: String,
    pub allowed: bool,
    pub degraded_to_read_only: bool,
    pub reason: Option<LicenseRejectionReason>,
    pub entitlement_state: String,
    pub activation_state: String,
    pub trust_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseStatusView {
    pub entitlement_state: String,
    pub activation_state: String,
    pub trust_state: String,
    pub policy_sync_pending: bool,
    pub pending_local_writes: i64,
    pub last_admin_action: Option<String>,
    pub last_admin_action_at: Option<String>,
    pub actionable_message: String,
    pub recovery_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyAdminLicenseActionInput {
    pub action: String,
    pub reason: String,
    pub expected_entitlement_state: Option<String>,
    pub expected_activation_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyAdminLicenseActionResult {
    pub action_id: String,
    pub action: String,
    pub applied_at: String,
    pub entitlement_state_after: String,
    pub activation_state_after: String,
    pub pending_local_writes: i64,
    pub queued_local_writes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseTraceEvent {
    pub id: String,
    pub correlation_id: String,
    pub event_type: String,
    pub source: String,
    pub subject_type: String,
    pub subject_id: Option<String>,
    pub reason_code: Option<String>,
    pub outcome: String,
    pub occurred_at: String,
    pub payload_hash: String,
    pub previous_hash: Option<String>,
    pub event_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyLicensingCompromiseResponseInput {
    pub issuer: String,
    pub key_id: String,
    pub reason: String,
    pub force_revocation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyLicensingCompromiseResponseResult {
    pub issuer: String,
    pub key_id: String,
    pub policy_sync_pending: bool,
    pub forced_revocation: bool,
    pub applied_at: String,
}
