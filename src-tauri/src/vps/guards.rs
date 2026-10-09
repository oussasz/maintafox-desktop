use crate::vps::domain::{
    AuthBoundary, PolicyDeliveryContract, TenantScope, VpsContractFamily, VpsRequestContext, VpsRouteContract,
    VpsTypedError, VPS_API_VERSION_V1,
};

fn typed_error(
    family: VpsContractFamily,
    code: &str,
    message: &str,
    http_status: u16,
    retryable: bool,
) -> VpsTypedError {
    VpsTypedError {
        family,
        code: code.to_string(),
        message: message.to_string(),
        http_status,
        retryable,
    }
}

pub fn route_contracts_catalog() -> Vec<VpsRouteContract> {
    vec![
        VpsRouteContract {
            family: VpsContractFamily::License,
            owner: "control-plane-license".to_string(),
            route_prefix: "/api/v1/license".to_string(),
            version: VPS_API_VERSION_V1.to_string(),
            required_boundary: AuthBoundary::TenantRuntime,
            tenant_scope: TenantScope::Required,
            required_permissions: vec!["license.runtime".to_string()],
            idempotency_required: true,
            replay_guard_required: true,
        },
        VpsRouteContract {
            family: VpsContractFamily::Sync,
            owner: "control-plane-sync".to_string(),
            route_prefix: "/api/v1/sync".to_string(),
            version: VPS_API_VERSION_V1.to_string(),
            required_boundary: AuthBoundary::TenantRuntime,
            tenant_scope: TenantScope::Required,
            required_permissions: vec!["sync.runtime".to_string()],
            idempotency_required: true,
            replay_guard_required: true,
        },
        VpsRouteContract {
            family: VpsContractFamily::Updates,
            owner: "control-plane-updates".to_string(),
            route_prefix: "/api/v1/updates".to_string(),
            version: VPS_API_VERSION_V1.to_string(),
            required_boundary: AuthBoundary::TenantRuntime,
            tenant_scope: TenantScope::Required,
            required_permissions: vec!["updates.runtime".to_string()],
            idempotency_required: false,
            replay_guard_required: false,
        },
        VpsRouteContract {
            family: VpsContractFamily::Relay,
            owner: "control-plane-relay".to_string(),
            route_prefix: "/api/v1/relay".to_string(),
            version: VPS_API_VERSION_V1.to_string(),
            required_boundary: AuthBoundary::TenantRuntime,
            tenant_scope: TenantScope::Required,
            required_permissions: vec!["relay.runtime".to_string()],
            idempotency_required: true,
            replay_guard_required: true,
        },
        VpsRouteContract {
            family: VpsContractFamily::Admin,
            owner: "vendor-admin-console".to_string(),
            route_prefix: "/admin/v1".to_string(),
            version: VPS_API_VERSION_V1.to_string(),
            required_boundary: AuthBoundary::VendorAdmin,
            tenant_scope: TenantScope::NotAllowed,
            required_permissions: vec!["platform.health".to_string()],
            idempotency_required: true,
            replay_guard_required: false,
        },
    ]
}

pub fn route_contract_for_family(family: VpsContractFamily) -> Result<VpsRouteContract, VpsTypedError> {
    route_contracts_catalog()
        .into_iter()
        .find(|route| route.family == family)
        .ok_or_else(|| {
            typed_error(
                family,
                "route_contract_missing",
                "No route contract is registered for this VPS family.",
                500,
                false,
            )
        })
}

/// Validate additive sync exchange request shape (desktop → Fastify).
/// Unknown optional fields are allowed; required v1 fields must be present.
pub fn validate_sync_exchange_request_json(body: &serde_json::Value) -> Result<(), VpsTypedError> {
    let obj = body.as_object().ok_or_else(|| {
        typed_error(
            VpsContractFamily::Sync,
            "sync_exchange_invalid_body",
            "Sync exchange body must be a JSON object.",
            400,
            false,
        )
    })?;
    let protocol = obj
        .get("protocol_version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if protocol != VPS_API_VERSION_V1 && protocol != "v1" {
        return Err(typed_error(
            VpsContractFamily::Sync,
            "unsupported_protocol_version",
            "protocol_version must be v1.",
            426,
            false,
        ));
    }
    if !obj.contains_key("idempotency_key") {
        return Err(typed_error(
            VpsContractFamily::Sync,
            "idempotency_key_required",
            "idempotency_key is required on sync exchange.",
            400,
            false,
        ));
    }
    if !obj.get("outbox_batch").map(|v| v.is_array()).unwrap_or(false) {
        return Err(typed_error(
            VpsContractFamily::Sync,
            "outbox_batch_required",
            "outbox_batch must be an array.",
            400,
            false,
        ));
    }
    // Additive optional fields — validate shape when present
    if let Some(dc) = obj.get("device_class") {
        match dc.as_str() {
            Some("desktop") | Some("mobile") => {}
            _ => {
                return Err(typed_error(
                    VpsContractFamily::Sync,
                    "device_class_invalid",
                    "device_class must be desktop or mobile when present.",
                    400,
                    false,
                ));
            }
        }
    }
    if let Some(mm) = obj.get("media_manifest") {
        if !mm.is_null() && !mm.is_array() {
            return Err(typed_error(
                VpsContractFamily::Sync,
                "media_manifest_invalid",
                "media_manifest must be an array when present.",
                400,
                false,
            ));
        }
    }
    Ok(())
}

pub fn enforce_request_context(route: &VpsRouteContract, ctx: &VpsRequestContext) -> Result<(), VpsTypedError> {
    if ctx.correlation_id.trim().is_empty() {
        return Err(typed_error(
            route.family.clone(),
            "missing_correlation_id",
            "A correlation_id is required at every route boundary.",
            400,
            false,
        ));
    }

    if ctx.api_version != route.version {
        return Err(typed_error(
            route.family.clone(),
            "unsupported_api_version",
            "Request version is not compatible with the route contract family.",
            426,
            false,
        ));
    }

    if ctx.auth_boundary != route.required_boundary {
        return Err(typed_error(
            route.family.clone(),
            "auth_boundary_violation",
            "Auth boundary does not match this route family.",
            403,
            false,
        ));
    }

    match route.tenant_scope {
        TenantScope::Required => {
            let Some(tenant_id) = ctx.tenant_id.as_deref() else {
                return Err(typed_error(
                    route.family.clone(),
                    "tenant_context_required",
                    "Tenant runtime routes are deny-by-default without tenant context.",
                    403,
                    false,
                ));
            };
            let Some(token_tenant_id) = ctx.token_tenant_id.as_deref() else {
                return Err(typed_error(
                    route.family.clone(),
                    "token_tenant_missing",
                    "Token claim must include tenant id for tenant-scoped routes.",
                    403,
                    false,
                ));
            };
            if tenant_id != token_tenant_id {
                return Err(typed_error(
                    route.family.clone(),
                    "tenant_isolation_violation",
                    "Tenant context does not match token tenant claim.",
                    403,
                    false,
                ));
            }
        }
        TenantScope::NotAllowed => {
            if ctx.tenant_id.is_some() || ctx.token_tenant_id.is_some() {
                return Err(typed_error(
                    route.family.clone(),
                    "tenant_context_forbidden",
                    "Vendor admin routes cannot reuse tenant runtime context.",
                    403,
                    false,
                ));
            }
        }
    }

    for required in &route.required_permissions {
        if !ctx.permissions.iter().any(|p| p == required) {
            return Err(typed_error(
                route.family.clone(),
                "permission_scope_missing",
                "Required permission scope is missing for this route family.",
                403,
                false,
            ));
        }
    }

    if route.idempotency_required
        && ctx
            .idempotency_key
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(typed_error(
            route.family.clone(),
            "idempotency_key_required",
            "Mutation route requires idempotency_key.",
            400,
            false,
        ));
    }

    if route.replay_guard_required {
        let nonce_present = ctx
            .request_nonce
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let checkpoint_present = ctx
            .checkpoint_token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        if !(nonce_present || checkpoint_present) {
            return Err(typed_error(
                route.family.clone(),
                "replay_guard_required",
                "Request must include request_nonce or checkpoint_token for replay protection.",
                400,
                false,
            ));
        }
    }

    Ok(())
}

pub fn validate_policy_delivery(policy: &PolicyDeliveryContract) -> Result<(), VpsTypedError> {
    if policy.entitlement_state.trim().is_empty() {
        return Err(typed_error(
            VpsContractFamily::License,
            "policy_entitlement_state_required",
            "Policy delivery requires entitlement_state.",
            400,
            false,
        ));
    }
    if policy.trusted_device_policy.trim().is_empty() {
        return Err(typed_error(
            VpsContractFamily::License,
            "policy_trusted_device_required",
            "Policy delivery requires trusted_device_policy.",
            400,
            false,
        ));
    }
    if policy.rollout_channel.trim().is_empty() {
        return Err(typed_error(
            VpsContractFamily::Updates,
            "policy_rollout_channel_required",
            "Policy delivery requires rollout_channel.",
            400,
            false,
        ));
    }
    Ok(())
}
