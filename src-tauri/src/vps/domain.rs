use serde::{Deserialize, Serialize};

pub const VPS_API_VERSION_V1: &str = "v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VpsContractFamily {
    License,
    Sync,
    Updates,
    Admin,
    Relay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthBoundary {
    TenantRuntime,
    VendorAdmin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TenantScope {
    Required,
    NotAllowed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsRouteContract {
    pub family: VpsContractFamily,
    pub owner: String,
    pub route_prefix: String,
    pub version: String,
    pub required_boundary: AuthBoundary,
    pub tenant_scope: TenantScope,
    pub required_permissions: Vec<String>,
    pub idempotency_required: bool,
    pub replay_guard_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsRequestContext {
    pub correlation_id: String,
    pub api_version: String,
    pub auth_boundary: AuthBoundary,
    pub actor_id: String,
    pub tenant_id: Option<String>,
    pub token_tenant_id: Option<String>,
    pub permissions: Vec<String>,
    pub idempotency_key: Option<String>,
    pub request_nonce: Option<String>,
    pub checkpoint_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsTypedError {
    pub family: VpsContractFamily,
    pub code: String,
    pub message: String,
    pub http_status: u16,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDeliveryContract {
    pub entitlement_state: String,
    pub offline_grace_until: Option<String>,
    pub trusted_device_policy: String,
    pub rollout_channel: String,
    pub urgent_notice: Option<String>,
}
