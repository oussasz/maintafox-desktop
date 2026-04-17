//! Vendor admin console: MFA, step-up, permission catalog, and admin audit event taxonomy (PRD section 16.5).
//!
//! UI gating is not a security boundary — every admin HTTP handler must re-check permissions server-side.

use serde::{Deserialize, Serialize};

use crate::vps::domain::{VpsContractFamily, VpsTypedError};

/// Console RBAC names (must match `permissions` rows from migration 066).
pub mod permissions {
    pub const CONSOLE_VIEW: &str = "console.view";
    pub const CUSTOMER_MANAGE: &str = "customer.manage";
    pub const ENTITLEMENT_MANAGE: &str = "entitlement.manage";
    pub const SYNC_OPERATE: &str = "sync.operate";
    pub const ROLLOUT_MANAGE: &str = "rollout.manage";
    pub const PLATFORM_OBSERVE: &str = "platform.observe";
    pub const AUDIT_VIEW: &str = "audit.view";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VendorAdminMfaPolicy {
    /// TOTP (or WebAuthn) required before session is fully elevated.
    TotpMandatory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminSessionAuditEventKind {
    LoginSuccess,
    LoginFailure,
    MfaChallengeShown,
    MfaSuccess,
    MfaFailure,
    StepUpPrompted,
    StepUpSatisfied,
    PrivilegedActionDenied,
    PrivilegedActionCommitted,
    RefreshRotated,
    Logout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSessionAuditEventV1 {
    pub kind: AdminSessionAuditEventKind,
    pub correlation_id: String,
    pub actor_id: String,
    pub route: String,
    pub detail_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepUpActionKind {
    MassMachineRevoke,
    EntitlementSuspension,
    ForcedRollback,
    TenantRestore,
    SigningKeyRotation,
    RolloutPublish,
}

impl StepUpActionKind {
    pub fn requires_step_up(self) -> bool {
        true
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MassMachineRevoke => "mass_machine_revoke",
            Self::EntitlementSuspension => "entitlement_suspension",
            Self::ForcedRollback => "forced_rollback",
            Self::TenantRestore => "tenant_restore",
            Self::SigningKeyRotation => "signing_key_rotation",
            Self::RolloutPublish => "rollout_publish",
        }
    }
}

/// Minimum permissions for a control-plane admin route (server must enforce).
pub fn required_permissions_for_console_route(route_key: &str) -> &'static [&'static str] {
    match route_key {
        "vcn.home" | "vcn.overview" => &[permissions::CONSOLE_VIEW],
        "vcn.customers" => &[permissions::CONSOLE_VIEW, permissions::CUSTOMER_MANAGE],
        "vcn.entitlements" => &[permissions::CONSOLE_VIEW, permissions::ENTITLEMENT_MANAGE],
        "vcn.machines" => &[permissions::CONSOLE_VIEW, permissions::ENTITLEMENT_MANAGE],
        "vcn.sync" => &[permissions::CONSOLE_VIEW, permissions::SYNC_OPERATE],
        "vcn.rollouts" => &[permissions::CONSOLE_VIEW, permissions::ROLLOUT_MANAGE],
        "vcn.health" => &[permissions::CONSOLE_VIEW, permissions::PLATFORM_OBSERVE],
        "vcn.audit" => &[permissions::CONSOLE_VIEW, permissions::AUDIT_VIEW],
        _ => &[permissions::CONSOLE_VIEW],
    }
}

pub fn caller_has_all_permissions(caller: &[String], required: &[&str]) -> bool {
    required.iter().all(|p| caller.iter().any(|c| c == *p))
}

pub fn enforce_console_route(
    route_key: &str,
    caller_permissions: &[String],
) -> Result<(), VpsTypedError> {
    let req = required_permissions_for_console_route(route_key);
    if caller_has_all_permissions(caller_permissions, req) {
        return Ok(());
    }
    Err(VpsTypedError {
        family: VpsContractFamily::Admin,
        code: "admin_permission_denied".to_string(),
        message: format!(
            "Missing one of required permissions for {route_key}: {}",
            req.join(", ")
        ),
        http_status: 403,
        retryable: false,
    })
}

pub fn step_up_required_for_action(action: StepUpActionKind) -> bool {
    action.requires_step_up()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorAdminSessionContract {
    pub access_token_ttl_minutes: u32,
    pub refresh_cookie_http_only: bool,
    pub refresh_cookie_same_site: String,
    pub mfa_policy: VendorAdminMfaPolicy,
    pub optional_ip_allowlist: bool,
}

impl Default for VendorAdminSessionContract {
    fn default() -> Self {
        Self {
            access_token_ttl_minutes: 15,
            refresh_cookie_http_only: true,
            refresh_cookie_same_site: "strict".to_string(),
            mfa_policy: VendorAdminMfaPolicy::TotpMandatory,
            optional_ip_allowlist: true,
        }
    }
}

/// Acceptance checklist items (evidence captured outside the desktop repo for VPS).
pub fn admin_security_review_checklist() -> Vec<String> {
    vec![
        "Session expiry forces re-auth; refresh rotation audited.".to_string(),
        "Every admin HTTP handler checks session permissions independently of UI.".to_string(),
        "MFA mandatory before full session elevation; failed MFA emits audit event.".to_string(),
        "Step-up satisfied window is short; privileged mutations require fresh proof.".to_string(),
        "Audit stream includes correlation_id on login, step-up, deny, and commit.".to_string(),
    ]
}
