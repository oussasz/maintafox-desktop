#![cfg(test)]

use crate::vps::sync_rollout_platform_ops::{
    repair_action_allowed, rollout_recall_requires_step_up, tenant_safe_drill_through,
    worst_severity, IncidentDrillThroughRefsV1, OpsAlertStateV1, OpsAlertV1, PlatformServiceKindV1,
    PlatformServiceStatusV1, RepairQueueActionV1, RepairQueueItemV1, RolloutGovernanceStateV1,
    SyncFailureDrillDownRowV1, SyncHealthSeverityV1,
};

fn sample_item(
    severity: SyncHealthSeverityV1,
    queue_kind: &str,
) -> RepairQueueItemV1 {
    RepairQueueItemV1 {
        item_id: "rq-1".into(),
        tenant_id: "t-1".into(),
        queue_kind: queue_kind.into(),
        severity,
        summary: "test".into(),
        recommended_action: RepairQueueActionV1::Replay,
    }
}

#[test]
fn escalate_allowed_for_warn_or_critical() {
    let w = sample_item(SyncHealthSeverityV1::Warn, "pull_materialization");
    assert!(repair_action_allowed(&w, RepairQueueActionV1::Escalate));
    let c = sample_item(SyncHealthSeverityV1::Critical, "pull_materialization");
    assert!(repair_action_allowed(&c, RepairQueueActionV1::Escalate));
    let i = sample_item(SyncHealthSeverityV1::Info, "pull_materialization");
    assert!(!repair_action_allowed(&i, RepairQueueActionV1::Escalate));
}

#[test]
fn acknowledge_critical_requires_dead_letter_queue() {
    let dl = sample_item(SyncHealthSeverityV1::Critical, "dead_letter");
    assert!(repair_action_allowed(&dl, RepairQueueActionV1::Acknowledge));
    let not_dl = sample_item(SyncHealthSeverityV1::Critical, "pull_materialization");
    assert!(!repair_action_allowed(&not_dl, RepairQueueActionV1::Acknowledge));
}

#[test]
fn tenant_safe_rejects_email_like_hints() {
    let bad = IncidentDrillThroughRefsV1 {
        tenant_id_hint: Some("user@tenant.example".into()),
        ..Default::default()
    };
    assert!(!tenant_safe_drill_through(&bad));
    let ok = IncidentDrillThroughRefsV1 {
        tenant_id_hint: Some("ten_acme_001".into()),
        correlation_id: Some("corr_abc".into()),
        ..Default::default()
    };
    assert!(tenant_safe_drill_through(&ok));
}

#[test]
fn recall_always_step_up() {
    assert!(rollout_recall_requires_step_up());
}

#[test]
fn worst_severity_picks_max() {
    assert_eq!(
        worst_severity(SyncHealthSeverityV1::Info, SyncHealthSeverityV1::Critical),
        SyncHealthSeverityV1::Critical
    );
}

#[test]
fn serde_round_trip_drill_row() {
    let row = SyncFailureDrillDownRowV1 {
        batch_id: "b1".into(),
        entity_type: "work_order".into(),
        failure_reason_code: "merge_conflict".into(),
        idempotency_key: "idem-1".into(),
        last_attempt_rfc3339: "2026-04-16T12:00:00Z".into(),
        attempt_count: 3,
    };
    let json = serde_json::to_string(&row).expect("serialize");
    let back: SyncFailureDrillDownRowV1 = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.batch_id, "b1");
}

#[test]
fn platform_status_enum_snake() {
    let s = PlatformServiceStatusV1 {
        service: PlatformServiceKindV1::ObjectStorage,
        severity: SyncHealthSeverityV1::Warn,
        detail: "latency".into(),
    };
    let j = serde_json::to_string(&s).expect("serialize");
    assert!(j.contains("\"object_storage\""));
}

#[test]
fn governance_states_parse() {
    let c = serde_json::json!({
        "channel": "stable",
        "cohort_label": "wave-a",
        "tenant_count": 10,
        "machine_count": 40,
        "governance": "recalled",
        "paused_at_rfc3339": null
    });
    let v: crate::vps::sync_rollout_platform_ops::CohortRolloutStageV1 =
        serde_json::from_value(c).expect("parse");
    assert_eq!(v.governance, RolloutGovernanceStateV1::Recalled);
}

#[test]
fn ops_alert_open_state() {
    let a = OpsAlertV1 {
        alert_id: "a1".into(),
        title: "queue backlog".into(),
        severity: SyncHealthSeverityV1::Warn,
        state: OpsAlertStateV1::Open,
        owner_actor_id: None,
        acknowledged_at_rfc3339: None,
        notes: vec![],
        drill_refs: IncidentDrillThroughRefsV1::default(),
    };
    assert_eq!(a.state, OpsAlertStateV1::Open);
}
