#![cfg(test)]

use crate::vps::audit_support_hardening::{
    audit_record_preimage, compute_record_integrity_sha256, privileged_action_guard_ok,
    verify_audit_chain, verify_record_integrity, AuditEntityRefsV1, ComplianceExportKindV1,
    IncidentRunbookEntryV1, IncidentRunbookIdV1, SupportTicketStateV1, SupportTicketV1,
    VendorAdminAuditActionCategoryV1, VendorAdminAuditRecordV1,
};
use crate::vps::sync_rollout_platform_ops::SyncHealthSeverityV1;

fn sample_record(
    record_id: &str,
    seq: u64,
    chain_prev: Option<&str>,
) -> VendorAdminAuditRecordV1 {
    let mut r = VendorAdminAuditRecordV1 {
        record_id: record_id.into(),
        sequence: seq,
        occurred_at_rfc3339: "2026-04-16T12:00:00Z".into(),
        actor_id: "actor_1".into(),
        action_code: "entitlement_suspend".into(),
        action_category: VendorAdminAuditActionCategoryV1::Entitlement,
        correlation_id: "corr_1".into(),
        scope_tenant_id: Some("ten_a".into()),
        before_snapshot_sha256: Some("aa".repeat(32)),
        after_snapshot_sha256: Some("bb".repeat(32)),
        payload_canonical_sha256: "cc".repeat(32),
        chain_prev_hash: chain_prev.map(String::from),
        record_integrity_sha256: String::new(),
        reason_code: Some("cust_request".into()),
        approval_correlation_id: Some("appr_1".into()),
        entity_refs: AuditEntityRefsV1 {
            tenant_id: Some("ten_a".into()),
            entitlement_id: Some("ent_1".into()),
            machine_id: None,
            sync_batch_id: None,
            release_id: None,
            incident_id: None,
            support_ticket_id: Some("sup_9".into()),
        },
    };
    r.record_integrity_sha256 = compute_record_integrity_sha256(&r);
    r
}

#[test]
fn integrity_round_trip() {
    let r = sample_record("rec_1", 1, None);
    assert!(verify_record_integrity(&r));
    assert!(!audit_record_preimage(&r).contains(&r.record_integrity_sha256));
}

#[test]
fn chain_verifies() {
    let a = sample_record("rec_a", 1, None);
    let b = sample_record("rec_b", 2, Some(&a.record_integrity_sha256));
    verify_audit_chain(&[a, b]).expect("chain ok");
}

#[test]
fn chain_rejects_tamper() {
    let a = sample_record("rec_a", 1, None);
    let b = sample_record("rec_b", 2, Some(&a.record_integrity_sha256));
    let mut b_bad = b.clone();
    b_bad.actor_id = "evil".into();
    assert_eq!(verify_audit_chain(&[a, b_bad]), Err("integrity_mismatch"));
}

#[test]
fn privileged_guard_requires_reason_and_step_up() {
    assert!(!privileged_action_guard_ok(Some("ab"), true, false, false));
    assert!(privileged_action_guard_ok(Some("code"), true, false, false));
    assert!(!privileged_action_guard_ok(Some("code"), false, true, false));
    assert!(!privileged_action_guard_ok(Some("code"), true, false, true));
    assert!(privileged_action_guard_ok(Some("code"), true, true, true));
}

#[test]
fn support_ticket_serde() {
    let t = SupportTicketV1 {
        ticket_id: "t1".into(),
        tenant_id: "ten".into(),
        state: SupportTicketStateV1::Triaged,
        severity: SyncHealthSeverityV1::Warn,
        affected_module: "sync".into(),
        sync_status_hint: "laggy".into(),
        app_version_reported: "1.2.3".into(),
        linked_incident_ids: vec!["inc_1".into()],
        linked_audit_record_ids: vec!["rec_a".into()],
        sla_due_rfc3339: None,
        created_at_rfc3339: "2026-04-16T10:00:00Z".into(),
    };
    let j = serde_json::to_string(&t).unwrap();
    assert!(j.contains("triaged"));
}

#[test]
fn runbook_roundtrip() {
    let rb = IncidentRunbookEntryV1 {
        runbook_id: IncidentRunbookIdV1::SyncBacklogSurge,
        title: "Backlog".into(),
        summary: "Scale workers".into(),
        first_steps: vec!["Check DLQ".into()],
    };
    let json = serde_json::to_string(&rb).unwrap();
    let back: IncidentRunbookEntryV1 = serde_json::from_str(&json).unwrap();
    assert_eq!(back.runbook_id, IncidentRunbookIdV1::SyncBacklogSurge);
}

#[test]
fn compliance_export_kind_json() {
    let k = ComplianceExportKindV1::EntitlementHistory;
    assert_eq!(
        serde_json::to_string(&k).unwrap(),
        "\"entitlement_history\""
    );
}
