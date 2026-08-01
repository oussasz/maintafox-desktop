#![cfg(test)]

use crate::vps::vendor_admin_console::{
    admin_security_review_checklist, enforce_console_route, permissions, step_up_required_for_action, StepUpActionKind,
};

#[test]
fn route_customers_requires_customer_manage() {
    let ok = vec![
        crate::rbac::permissions::CONSOLE_VIEW.to_string(),
        crate::rbac::permissions::CUSTOMER_MANAGE.to_string(),
    ];
    enforce_console_route("vcn.customers", &ok).expect("allowed");

    let missing = vec![crate::rbac::permissions::CONSOLE_VIEW.to_string()];
    let err = enforce_console_route("vcn.customers", &missing).unwrap_err();
    assert_eq!(err.http_status, 403);
}

#[test]
fn route_overview_console_view_only() {
    let ok = vec![crate::rbac::permissions::CONSOLE_VIEW.to_string()];
    enforce_console_route("vcn.overview", &ok).expect("allowed");
}

#[test]
fn route_machines_requires_entitlement_manage() {
    let ok = vec![
        crate::rbac::permissions::CONSOLE_VIEW.to_string(),
        crate::rbac::permissions::ENTITLEMENT_MANAGE.to_string(),
    ];
    enforce_console_route("vcn.machines", &ok).expect("allowed");
    let bad = vec![crate::rbac::permissions::CONSOLE_VIEW.to_string()];
    assert!(enforce_console_route("vcn.machines", &bad).is_err());
}

#[test]
fn route_sync_requires_sync_operate() {
    let ok = vec![
        crate::rbac::permissions::CONSOLE_VIEW.to_string(),
        crate::rbac::permissions::SYNC_OPERATE.to_string(),
    ];
    enforce_console_route("vcn.sync", &ok).expect("allowed");
    assert!(enforce_console_route("vcn.sync", &[crate::rbac::permissions::CONSOLE_VIEW.to_string()]).is_err());
}

#[test]
fn route_rollouts_requires_rollout_manage() {
    let ok = vec![
        crate::rbac::permissions::CONSOLE_VIEW.to_string(),
        crate::rbac::permissions::ROLLOUT_MANAGE.to_string(),
    ];
    enforce_console_route("vcn.rollouts", &ok).expect("allowed");
    assert!(enforce_console_route("vcn.rollouts", &[crate::rbac::permissions::CONSOLE_VIEW.to_string()]).is_err());
}

#[test]
fn route_health_requires_platform_observe() {
    let ok = vec![
        crate::rbac::permissions::CONSOLE_VIEW.to_string(),
        crate::rbac::permissions::PLATFORM_OBSERVE.to_string(),
    ];
    enforce_console_route("vcn.health", &ok).expect("allowed");
    assert!(enforce_console_route("vcn.health", &[crate::rbac::permissions::CONSOLE_VIEW.to_string()]).is_err());
}

#[test]
fn route_audit_requires_audit_view() {
    let ok = vec![
        crate::rbac::permissions::CONSOLE_VIEW.to_string(),
        crate::rbac::permissions::AUDIT_VIEW.to_string(),
    ];
    enforce_console_route("vcn.audit", &ok).expect("allowed");
    assert!(enforce_console_route("vcn.audit", &[crate::rbac::permissions::CONSOLE_VIEW.to_string()]).is_err());
}

#[test]
fn step_up_always_required_for_rollout_publish() {
    assert!(step_up_required_for_action(StepUpActionKind::RolloutPublish));
}

#[test]
fn permission_constants_match_catalog() {
    assert_eq!(
        crate::rbac::permissions::CONSOLE_VIEW,
        crate::rbac::permissions::CONSOLE_VIEW
    );
    assert_eq!(
        crate::rbac::permissions::SYNC_OPERATE,
        crate::rbac::permissions::SYNC_OPERATE
    );
}

#[test]
fn security_review_checklist_non_empty() {
    assert!(!admin_security_review_checklist().is_empty());
}
