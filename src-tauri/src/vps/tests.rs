use crate::vps::domain::{
    AuthBoundary, PolicyDeliveryContract, VpsContractFamily, VpsRequestContext, VPS_API_VERSION_V1,
};
use crate::vps::guards::{enforce_request_context, route_contract_for_family, validate_policy_delivery};

fn base_tenant_context() -> VpsRequestContext {
    VpsRequestContext {
        correlation_id: "corr-001".to_string(),
        api_version: VPS_API_VERSION_V1.to_string(),
        auth_boundary: AuthBoundary::TenantRuntime,
        actor_id: "machine-01".to_string(),
        tenant_id: Some("tenant-a".to_string()),
        token_tenant_id: Some("tenant-a".to_string()),
        permissions: vec![
            "license.runtime".to_string(),
            "sync.runtime".to_string(),
            "updates.runtime".to_string(),
            "relay.runtime".to_string(),
        ],
        idempotency_key: Some("idk-001".to_string()),
        request_nonce: Some("nonce-001".to_string()),
        checkpoint_token: None,
    }
}

#[test]
fn tenant_isolation_is_deny_by_default_without_tenant_context() {
    let route = route_contract_for_family(VpsContractFamily::Sync).expect("sync route");
    let mut ctx = base_tenant_context();
    ctx.tenant_id = None;
    let err = enforce_request_context(&route, &ctx).expect_err("must reject missing tenant context");
    assert_eq!(err.code, "tenant_context_required");
}

#[test]
fn split_auth_boundaries_reject_vendor_admin_context_on_runtime_route() {
    let route = route_contract_for_family(VpsContractFamily::License).expect("license route");
    let mut ctx = base_tenant_context();
    ctx.auth_boundary = AuthBoundary::VendorAdmin;
    ctx.tenant_id = None;
    ctx.token_tenant_id = None;
    ctx.permissions = vec!["platform.health".to_string()];
    let err = enforce_request_context(&route, &ctx).expect_err("must reject wrong boundary");
    assert_eq!(err.code, "auth_boundary_violation");
}

#[test]
fn split_auth_boundaries_reject_tenant_context_on_admin_route() {
    let route = route_contract_for_family(VpsContractFamily::Admin).expect("admin route");
    let mut ctx = base_tenant_context();
    ctx.auth_boundary = AuthBoundary::VendorAdmin;
    ctx.permissions = vec!["platform.health".to_string()];
    let err = enforce_request_context(&route, &ctx).expect_err("must reject tenant context on admin");
    assert_eq!(err.code, "tenant_context_forbidden");
}

#[test]
fn version_compatibility_rejects_non_v1_requests() {
    let route = route_contract_for_family(VpsContractFamily::Relay).expect("relay route");
    let mut ctx = base_tenant_context();
    ctx.api_version = "v2".to_string();
    let err = enforce_request_context(&route, &ctx).expect_err("must reject unsupported version");
    assert_eq!(err.code, "unsupported_api_version");
}

#[test]
fn replay_safe_contract_requires_idempotency_or_checkpoint_metadata() {
    let route = route_contract_for_family(VpsContractFamily::Sync).expect("sync route");
    let mut ctx = base_tenant_context();
    ctx.idempotency_key = None;
    ctx.request_nonce = None;
    ctx.checkpoint_token = None;
    let err = enforce_request_context(&route, &ctx).expect_err("must enforce replay-safe metadata");
    assert_eq!(err.code, "idempotency_key_required");
}

#[test]
fn policy_delivery_contract_requires_runtime_policy_fields() {
    let bad = PolicyDeliveryContract {
        entitlement_state: "".to_string(),
        offline_grace_until: None,
        trusted_device_policy: "strict".to_string(),
        rollout_channel: "stable".to_string(),
        urgent_notice: None,
    };
    let err = validate_policy_delivery(&bad).expect_err("must reject empty entitlement state");
    assert_eq!(err.code, "policy_entitlement_state_required");
}

#[test]
fn valid_runtime_sync_context_passes_readiness_gate() {
    let route = route_contract_for_family(VpsContractFamily::Sync).expect("sync route");
    let ctx = base_tenant_context();
    enforce_request_context(&route, &ctx).expect("valid runtime context");
}
