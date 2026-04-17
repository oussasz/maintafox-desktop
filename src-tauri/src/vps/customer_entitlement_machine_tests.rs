#![cfg(test)]

use crate::vps::customer_entitlement_machine::{
    bulk_concurrency_ok, channel_policy_consistent, entitlement_transition_allowed,
    validate_entitlement_transition, validate_slot_limits, BulkEntitlementOperationRequestV1,
    DestructiveEntitlementAction, EntitlementLifecycleAction, EntitlementLifecycleState,
    OfflinePolicyControlsV1, OptimisticConcurrencyV1, TrustedDeviceOperatorAction, UpdateChannel,
};

#[test]
fn transition_active_renew_ok() {
    assert!(entitlement_transition_allowed(
        EntitlementLifecycleState::Active,
        EntitlementLifecycleAction::Renew
    ));
}

#[test]
fn transition_active_issue_rejected() {
    assert!(!entitlement_transition_allowed(
        EntitlementLifecycleState::Active,
        EntitlementLifecycleAction::Issue
    ));
    assert!(validate_entitlement_transition(EntitlementLifecycleState::Active, EntitlementLifecycleAction::Issue).is_err());
}

#[test]
fn transition_expired_issue_ok() {
    assert!(entitlement_transition_allowed(
        EntitlementLifecycleState::Expired,
        EntitlementLifecycleAction::Issue
    ));
}

#[test]
fn destructive_actions_require_dual_confirm() {
    assert!(DestructiveEntitlementAction::Revocation.requires_dual_confirmation());
    assert!(DestructiveEntitlementAction::MachineSlotReduction.requires_reason_code());
}

#[test]
fn slot_validation_rejects_over_allocation() {
    assert!(validate_slot_limits(2, 3).is_err());
    assert!(validate_slot_limits(10, 3).is_ok());
}

#[test]
fn channel_policy_match() {
    assert!(channel_policy_consistent(UpdateChannel::Stable, UpdateChannel::Stable));
    assert!(!channel_policy_consistent(UpdateChannel::Pilot, UpdateChannel::Stable));
}

#[test]
fn bulk_concurrency_detects_stale() {
    let expected = vec![("t1".to_string(), 2_i64)];
    let current_ok = vec![("t1".to_string(), 2_i64)];
    let current_bad = vec![("t1".to_string(), 3_i64)];
    assert!(bulk_concurrency_ok(&expected, &current_ok).is_ok());
    assert!(bulk_concurrency_ok(&expected, &current_bad).is_err());
}

#[test]
fn trusted_action_guard_flags() {
    assert!(TrustedDeviceOperatorAction::SoftSuspend.requires_tenant_lockout_guard());
    assert!(!TrustedDeviceOperatorAction::PolicyRefreshTrigger.requires_tenant_lockout_guard());
}

#[test]
fn bulk_request_fields() {
    let r = BulkEntitlementOperationRequestV1 {
        dry_run: true,
        tenant_ids: vec!["a".into()],
        target_channel: Some(UpdateChannel::Pilot),
        expected_lineage_version_by_tenant: vec![("a".into(), 1)],
    };
    assert!(r.dry_run);
    assert_eq!(r.tenant_ids.len(), 1);
}

#[test]
fn optimistic_concurrency_round_trip() {
    let o = OptimisticConcurrencyV1 {
        resource_id: "env:1".into(),
        expected_version: 7,
    };
    assert_eq!(o.expected_version, 7);
}

#[test]
fn offline_defaults_reasonable() {
    let d = OfflinePolicyControlsV1::default();
    assert!(d.reconnect_requires_fresh_heartbeat);
}
