//! Vendor console: customer workspace, entitlement lifecycle, machine activation, and bulk safety (PRD section 10).
//!
//! Contracts for VPS admin APIs; desktop persists local entitlements separately — this module is the control-plane contract.

use serde::{Deserialize, Serialize};

use crate::vps::domain::{VpsContractFamily, VpsTypedError};

// ─── Entitlement lifecycle (state matrix) ───────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntitlementLifecycleState {
    Active,
    Grace,
    Expired,
    Suspended,
    Revoked,
}

impl EntitlementLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Grace => "grace",
            Self::Expired => "expired",
            Self::Suspended => "suspended",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "grace" => Some(Self::Grace),
            "expired" => Some(Self::Expired),
            "suspended" => Some(Self::Suspended),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntitlementLifecycleAction {
    Issue,
    Renew,
    Suspend,
    Revoke,
    EmergencyLock,
    ResumeFromSuspension,
}

impl EntitlementLifecycleAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::Renew => "renew",
            Self::Suspend => "suspend",
            Self::Revoke => "revoke",
            Self::EmergencyLock => "emergency_lock",
            Self::ResumeFromSuspension => "resume_from_suspension",
        }
    }
}

/// Allowed transitions (from → action). Server is authoritative; UI uses this for hints.
pub fn entitlement_transition_allowed(from: EntitlementLifecycleState, action: EntitlementLifecycleAction) -> bool {
    use EntitlementLifecycleAction::*;
    use EntitlementLifecycleState::*;
    match (from, action) {
        (Expired | Revoked, Issue) => true,
        (Active | Grace, Renew) => true,
        (Active | Grace, Suspend) => true,
        (Suspended, ResumeFromSuspension) => true,
        (Active | Grace | Suspended, Revoke) => true,
        (Active | Grace, EmergencyLock) => true,
        _ => false,
    }
}

/// Normalize: Issue from non-terminal is often modeled as renew — reject ambiguous Issue from Active.
pub fn validate_entitlement_transition(
    from: EntitlementLifecycleState,
    action: EntitlementLifecycleAction,
) -> Result<(), VpsTypedError> {
    if entitlement_transition_allowed(from, action) {
        return Ok(());
    }
    Err(VpsTypedError {
        family: VpsContractFamily::Admin,
        code: "entitlement_transition_invalid".to_string(),
        message: format!(
            "Action {} is not allowed from state {}",
            action.as_str(),
            from.as_str()
        ),
        http_status: 409,
        retryable: false,
    })
}

// ─── Signed claim preview (pre-activation) ─────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateChannel {
    Stable,
    Pilot,
    Internal,
}

impl UpdateChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Pilot => "pilot",
            Self::Internal => "internal",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stable" => Some(Self::Stable),
            "pilot" => Some(Self::Pilot),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }
}

/// Mirrors envelope fields operators must confirm before activation (signed payload abstracted as hashes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedClaimPreviewV1 {
    pub schema_version: u32,
    pub tenant_id: String,
    pub tier: String,
    pub machine_slots: u32,
    pub offline_grace_hours: u32,
    pub update_channel: UpdateChannel,
    pub valid_from_rfc3339: String,
    pub valid_until_rfc3339: String,
    pub feature_flags_digest: String,
    pub capabilities_digest: String,
    pub issuer: String,
    pub key_id: String,
    pub payload_sha256: String,
    pub signature_alg: String,
}

// ─── Destructive actions: dual-confirm + reason ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DestructiveEntitlementAction {
    Revocation,
    ImmediateExpiry,
    MachineSlotReduction,
}

impl DestructiveEntitlementAction {
    pub fn requires_dual_confirmation(self) -> bool {
        true
    }

    pub fn requires_reason_code(self) -> bool {
        true
    }

    pub fn requires_step_up(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditableApprovalContextV1 {
    pub actor_id: String,
    pub second_actor_id: Option<String>,
    pub reason_code: String,
    pub free_text_rationale: String,
    pub previous_claim_snapshot_sha256: String,
    pub correlation_id: String,
}

// ─── Machine activation (monitor + policy) ────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeartbeatFreshness {
    Live,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineActivationRowV1 {
    pub machine_id: String,
    pub tenant_id: String,
    pub last_heartbeat_rfc3339: Option<String>,
    pub app_version: Option<String>,
    pub trusted_device: bool,
    pub activation_source: String,
    pub anomaly_flags: Vec<String>,
    pub heartbeat_freshness: HeartbeatFreshness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustedDeviceOperatorAction {
    SlotRelease,
    DeviceRebind,
    SoftSuspend,
    PolicyRefreshTrigger,
}

impl TrustedDeviceOperatorAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlotRelease => "slot_release",
            Self::DeviceRebind => "device_rebind",
            Self::SoftSuspend => "soft_suspend",
            Self::PolicyRefreshTrigger => "policy_refresh_trigger",
        }
    }

    /// High-disruption guard: require explicit tenant id match + step-up on VPS.
    pub fn requires_tenant_lockout_guard(self) -> bool {
        matches!(self, Self::SoftSuspend | Self::DeviceRebind)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflinePolicyControlsV1 {
    pub grace_hours: u32,
    pub trust_revocation_disconnects_immediately: bool,
    pub reconnect_requires_fresh_heartbeat: bool,
}

impl Default for OfflinePolicyControlsV1 {
    fn default() -> Self {
        Self {
            grace_hours: 72,
            trust_revocation_disconnects_immediately: false,
            reconnect_requires_fresh_heartbeat: true,
        }
    }
}

// ─── Bulk + concurrency + slots ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkEntitlementOperationRequestV1 {
    pub dry_run: bool,
    pub tenant_ids: Vec<String>,
    pub target_channel: Option<UpdateChannel>,
    pub expected_lineage_version_by_tenant: Vec<(String, i64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkEntitlementOperationResultV1 {
    pub dry_run: bool,
    pub would_affect_count: u32,
    pub failures: Vec<BulkFailureRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkFailureRow {
    pub tenant_id: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimisticConcurrencyV1 {
    pub resource_id: String,
    pub expected_version: i64,
}

pub fn validate_slot_limits(machine_slots: u32, active_machines: u32) -> Result<(), VpsTypedError> {
    if active_machines <= machine_slots {
        return Ok(());
    }
    Err(VpsTypedError {
        family: VpsContractFamily::Admin,
        code: "machine_slots_exceeded".to_string(),
        message: format!(
            "Active machines ({active_machines}) exceed entitlement slots ({machine_slots})."
        ),
        http_status: 409,
        retryable: false,
    })
}

pub fn channel_policy_consistent(license_channel: UpdateChannel, rollout_channel: UpdateChannel) -> bool {
    license_channel == rollout_channel
}

pub fn bulk_concurrency_ok(
    expected: &[(String, i64)],
    current: &[(String, i64)],
) -> Result<(), VpsTypedError> {
    for (tid, ev) in expected {
        let Some((_, cv)) = current.iter().find(|(t, _)| t == tid) else {
            return Err(VpsTypedError {
                family: VpsContractFamily::Admin,
                code: "bulk_tenant_not_found".to_string(),
                message: format!("Tenant {tid} not found for concurrency check."),
                http_status: 404,
                retryable: false,
            });
        };
        if ev != cv {
            return Err(VpsTypedError {
                family: VpsContractFamily::Admin,
                code: "optimistic_concurrency_conflict".to_string(),
                message: format!("Version mismatch for tenant {tid}: expected {ev}, current {cv}."),
                http_status: 409,
                retryable: true,
            });
        }
    }
    Ok(())
}
