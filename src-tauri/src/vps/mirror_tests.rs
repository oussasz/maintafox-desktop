use crate::vps::mirror::{
    build_tenant_provisioning_plan, control_plane_tables, enqueue_worker_job, mirror_schema_evolution_path,
    process_worker_round, queue_topology, tenant_schema_name, validate_tenant_provisioning, MirrorQueueJob,
    MirrorQueueKind, MirrorRecordClass, MirrorWorkerConfig, MirrorWorkerState,
};

fn worker_cfg() -> MirrorWorkerConfig {
    MirrorWorkerConfig {
        max_batch_size: 100,
        max_retry_attempts: 3,
        tenant_fairness_quantum: 2,
        tenant_lag_alert_threshold: 10,
    }
}

fn base_job(tenant: &str, idempotency_key: &str, queue_kind: MirrorQueueKind) -> MirrorQueueJob {
    MirrorQueueJob {
        job_id: format!("job-{tenant}-{idempotency_key}"),
        tenant_id: tenant.to_string(),
        target_schema: tenant_schema_name(tenant).expect("schema"),
        queue_kind,
        idempotency_key: idempotency_key.to_string(),
        checkpoint_token: Some("ckpt-100".to_string()),
        record_class: MirrorRecordClass::MutableOperational,
        merge_key: format!("merge-{idempotency_key}"),
        local_row_version: 2,
        incoming_row_version: 3,
        attempt: 0,
        injected_failure_code: None,
    }
}

#[test]
fn tenant_provisioning_plan_enforces_baseline_and_invariants() {
    let plan = build_tenant_provisioning_plan("tenant-a").expect("plan");
    let ok = validate_tenant_provisioning(
        &plan,
        &plan.baseline_migrations,
        &plan.required_invariants,
    );
    assert!(ok.is_ok());

    let bad = validate_tenant_provisioning(
        &plan,
        &plan.baseline_migrations[..1].to_vec(),
        &plan.required_invariants,
    )
    .expect_err("missing migrations should fail");
    assert_eq!(bad.code, "tenant_provisioning_incomplete");
}

#[test]
fn migration_path_is_additive_and_backward_compatible() {
    let path = mirror_schema_evolution_path();
    assert!(!path.is_empty());
    assert!(path.iter().all(|step| step.additive_only));
    assert!(path.iter().all(|step| step.backward_reader_compatible));
}

#[test]
fn queue_topology_includes_dead_letter_and_conflict_routing() {
    let topology = queue_topology();
    assert!(topology.contains(&MirrorQueueKind::PushIngestion));
    assert!(topology.contains(&MirrorQueueKind::ReplayRepair));
    assert!(topology.contains(&MirrorQueueKind::ConflictReview));
    assert!(topology.contains(&MirrorQueueKind::DeadLetter));
}

#[test]
fn control_plane_tables_keep_tenant_data_outside_shared_plane() {
    let tables = control_plane_tables();
    assert!(tables.contains(&"cp_sync_checkpoint_state"));
    assert!(tables.iter().all(|name| name.starts_with("cp_")));
}

#[test]
fn dedupe_behavior_is_strict_per_tenant_idempotency_key() {
    let mut state = MirrorWorkerState::default();
    let cfg = worker_cfg();

    enqueue_worker_job(
        &mut state,
        base_job("tenant-a", "idk-1", MirrorQueueKind::PushIngestion),
        &cfg,
    )
    .expect("enqueue");
    enqueue_worker_job(
        &mut state,
        base_job("tenant-a", "idk-1", MirrorQueueKind::PushIngestion),
        &cfg,
    )
    .expect("enqueue duplicate");

    let report = process_worker_round(&mut state, &cfg).expect("process");
    assert_eq!(report.processed_count, 1);
    assert_eq!(report.deduped_count, 1);
}

#[test]
fn stale_checkpoint_is_blocked_and_routed_to_conflict_queue() {
    let mut state = MirrorWorkerState::default();
    let cfg = worker_cfg();
    enqueue_worker_job(
        &mut state,
        base_job("tenant-a", "idk-fresh", MirrorQueueKind::PushIngestion),
        &cfg,
    )
    .expect("enqueue fresh");
    process_worker_round(&mut state, &cfg).expect("process fresh");

    let mut stale = base_job("tenant-a", "idk-stale", MirrorQueueKind::ReplayRepair);
    stale.checkpoint_token = Some("ckpt-099".to_string());
    enqueue_worker_job(&mut state, stale, &cfg).expect("enqueue stale");

    let report = process_worker_round(&mut state, &cfg).expect("process stale");
    assert_eq!(report.stale_checkpoint_blocked_count, 1);
    assert_eq!(report.conflict_routed_count, 1);
    assert_eq!(state.conflict_queue.len(), 1);
}

#[test]
fn conflict_routing_uses_class_aware_merge_rules() {
    let mut state = MirrorWorkerState::default();
    let cfg = worker_cfg();
    let mut governed = base_job("tenant-a", "idk-governed", MirrorQueueKind::PullMaterialization);
    governed.record_class = MirrorRecordClass::GovernedSnapshot;
    governed.local_row_version = 5;
    governed.incoming_row_version = 5;
    enqueue_worker_job(&mut state, governed, &cfg).expect("enqueue governed");

    let report = process_worker_round(&mut state, &cfg).expect("process");
    assert_eq!(report.conflict_routed_count, 1);
    assert_eq!(state.conflict_queue.len(), 1);
}

#[test]
fn retry_and_dead_letter_flow_is_bounded_and_safe() {
    let mut state = MirrorWorkerState::default();
    let cfg = worker_cfg();
    let mut retry_job = base_job("tenant-a", "idk-retry", MirrorQueueKind::PushIngestion);
    retry_job.injected_failure_code = Some("timeout".to_string());
    enqueue_worker_job(&mut state, retry_job, &cfg).expect("enqueue retry");

    let first = process_worker_round(&mut state, &cfg).expect("first round");
    assert_eq!(first.retried_count, 1);
    assert_eq!(state.pending.len(), 1);

    let mut always_fail = base_job("tenant-a", "idk-dlq", MirrorQueueKind::ReplayRepair);
    always_fail.attempt = 2;
    always_fail.injected_failure_code = Some("permanent_failure".to_string());
    enqueue_worker_job(&mut state, always_fail, &cfg).expect("enqueue dlq");

    let second = process_worker_round(&mut state, &cfg).expect("second round");
    assert!(second.dead_lettered_count >= 1);
    assert!(!state.dead_letter_queue.is_empty());
}

#[test]
fn strict_tenant_isolation_rejects_cross_schema_targeting() {
    let mut state = MirrorWorkerState::default();
    let cfg = worker_cfg();
    let mut bad = base_job("tenant-a", "idk-isolation", MirrorQueueKind::PushIngestion);
    bad.target_schema = "tenant_other".to_string();
    enqueue_worker_job(&mut state, bad, &cfg).expect("enqueue");

    let report = process_worker_round(&mut state, &cfg).expect("process");
    assert_eq!(report.tenant_isolation_violations, 1);
    assert_eq!(state.dead_letter_queue.len(), 1);
}

#[test]
fn fairness_guardrails_prevent_single_tenant_starvation() {
    let mut state = MirrorWorkerState::default();
    let mut cfg = worker_cfg();
    cfg.tenant_fairness_quantum = 1;
    for idx in 0..4 {
        enqueue_worker_job(
            &mut state,
            base_job("tenant-a", &format!("a-{idx}"), MirrorQueueKind::PushIngestion),
            &cfg,
        )
        .expect("enqueue tenant-a");
    }
    enqueue_worker_job(
        &mut state,
        base_job("tenant-b", "b-1", MirrorQueueKind::PushIngestion),
        &cfg,
    )
    .expect("enqueue tenant-b");

    process_worker_round(&mut state, &cfg).expect("process");
    let a_processed = state
        .metrics
        .per_tenant_processed
        .get("tenant-a")
        .copied()
        .unwrap_or(0);
    let b_processed = state
        .metrics
        .per_tenant_processed
        .get("tenant-b")
        .copied()
        .unwrap_or(0);
    assert!(a_processed >= 1);
    assert!(b_processed >= 1);
}
