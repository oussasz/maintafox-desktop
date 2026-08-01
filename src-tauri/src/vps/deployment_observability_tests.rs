#![cfg(test)]

use crate::vps::deployment_observability::{
    certificate_renewal_runbook_steps, default_control_plane_slos, default_on_call_routing_contract,
    deploy_preflight_checklist, failure_injection_scenarios, hardened_network_segment_rules,
    production_compose_topology_order, rollback_workflow_steps, safe_deploy_workflow_steps,
    sizing_hints, structured_log_contract_v1, ComposeServiceRole, DeploymentReadinessChecklist,
    DeploymentSizingProfile, NetworkExposureTier, ProductionDnsBoundaries, SecretHandlingContract,
    TenantHealthIndicators,
};

#[test]
fn production_topology_lists_core_roles() {
    let t = production_compose_topology_order();
    assert!(t.contains(&ComposeServiceRole::NginxEdge));
    assert!(t.contains(&ComposeServiceRole::Postgres));
}

#[test]
fn dns_split_validation_rejects_shared_hostname() {
    let bad = ProductionDnsBoundaries {
        tenant_runtime_api_hostname_example: "same.example".to_string(),
        vendor_admin_console_hostname_example: "same.example".to_string(),
    };
    assert!(bad.validate_split().is_err());
    let ok = ProductionDnsBoundaries::default();
    ok.validate_split().expect("default examples differ");
}

#[test]
fn hardened_network_disallows_public_to_ssh() {
    let rules = hardened_network_segment_rules();
    let bad = rules.iter().any(|r| {
        r.from_tier == NetworkExposureTier::PublicHttpsEdge
            && r.to_tier == NetworkExposureTier::OpsRestrictedSsh
            && r.allowed
    });
    assert!(!bad);
}

#[test]
fn secret_contract_forbids_plaintext() {
    let c = SecretHandlingContract::default();
    assert!(c.forbid_plaintext_in_compose);
    assert!(c.rotate_after_exposure);
}

#[test]
fn sizing_growth_ge_pilot() {
    let p = sizing_hints(DeploymentSizingProfile::Pilot);
    let g = sizing_hints(DeploymentSizingProfile::Growth);
    assert!(g.postgres_memory_gb >= p.postgres_memory_gb);
    assert!(g.worker_replicas >= p.worker_replicas);
}

#[test]
fn deploy_preflight_has_blocking_db_and_integrity() {
    let items = deploy_preflight_checklist();
    assert!(items.iter().any(|i| i.blocking));
    assert!(!safe_deploy_workflow_steps().is_empty());
    assert!(!rollback_workflow_steps().is_empty());
}

#[test]
fn structured_log_requires_correlation_id() {
    let c = structured_log_contract_v1();
    assert!(c.required_fields.iter().any(|f| f == "correlation_id"));
}

#[test]
fn control_plane_slos_have_targets() {
    for s in default_control_plane_slos() {
        assert!(s.target_ratio > 0.0 && s.target_ratio <= 1.0);
        assert!(!s.slo_id.is_empty());
    }
    assert!(!default_on_call_routing_contract().severity_map.is_empty());
}

#[test]
fn tenant_health_marks_degraded() {
    let mut h = TenantHealthIndicators {
        tenant_id: "t1".to_string(),
        heartbeat_success_ratio_24h: 0.5,
        sync_queue_lag_ms_p95: 10,
        rollout_download_failure_count_24h: 0,
        worker_retry_count_24h: 0,
        worker_dead_letter_count_24h: 0,
        degraded: false,
    };
    h.evaluate_degraded();
    assert!(h.degraded);
}

#[test]
fn failure_injection_scenarios_populated() {
    assert!(failure_injection_scenarios().len() >= 4);
}

#[test]
fn cert_renewal_runbook_non_empty() {
    assert!(!certificate_renewal_runbook_steps().is_empty());
}

#[test]
fn readiness_checklist_all_ok() {
    let mut c = DeploymentReadinessChecklist {
        dr_drills_evidence_present: true,
        failure_scenarios_documented: true,
        post_restore_entitlement_sync_validated: true,
        cert_and_key_runbooks_acknowledged: true,
        all_items_ok: false,
    };
    c.recompute();
    assert!(c.all_items_ok);
}
