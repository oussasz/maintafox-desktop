use serde::{Deserialize, Serialize};

pub const ENTITLEMENT_SIGNATURE_ALG_V1: &str = "sha256:issuer-key-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementEnvelopeInput {
    pub envelope_id: String,
    pub previous_envelope_id: Option<String>,
    pub lineage_version: i64,
    pub issuer: String,
    pub key_id: String,
    pub signature_alg: String,
    pub tier: String,
    pub state: String,
    pub channel: String,
    pub machine_slots: i64,
    pub feature_flags_json: String,
    pub capabilities_json: String,
    pub policy_json: String,
    pub issued_at: String,
    pub valid_from: String,
    pub valid_until: String,
    pub offline_grace_until: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementEnvelope {
    pub id: i64,
    pub envelope_id: String,
    pub previous_envelope_id: Option<String>,
    pub lineage_version: i64,
    pub issuer: String,
    pub key_id: String,
    pub signature_alg: String,
    pub tier: String,
    pub state: String,
    pub channel: String,
    pub machine_slots: i64,
    pub feature_flags_json: String,
    pub capabilities_json: String,
    pub policy_json: String,
    pub issued_at: String,
    pub valid_from: String,
    pub valid_until: String,
    pub offline_grace_until: String,
    pub payload_hash: String,
    pub signature: String,
    pub verified_at: Option<String>,
    pub verification_result: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementRefreshResult {
    pub envelope_id: String,
    pub verified: bool,
    pub verification_result: String,
    pub effective_state: String,
    pub active_lineage_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementSummary {
    pub envelope_id: Option<String>,
    pub state: String,
    pub effective_state: String,
    pub tier: Option<String>,
    pub channel: Option<String>,
    pub lineage_version: Option<i64>,
    pub valid_until: Option<String>,
    pub offline_grace_until: Option<String>,
    pub last_verified_at: Option<String>,
    pub capability_map_json: String,
    pub feature_flag_map_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementCapabilityCheck {
    pub capability: String,
    pub allowed: bool,
    pub reason: String,
    pub effective_state: String,
    pub envelope_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementDiagnostics {
    pub summary: EntitlementSummary,
    pub last_refresh_at: Option<String>,
    pub last_refresh_error: Option<String>,
    pub lineage: Vec<EntitlementEnvelope>,
    pub runbook_links: Vec<String>,
}
