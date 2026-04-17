use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::sync::domain::{
    ApplySyncBatchInput, ExecuteSyncRepairInput, ReplaySyncFailuresInput, ResolveSyncConflictInput,
    StageOutboxItemInput, SyncAckInput, SyncConflictFilter, SyncInboundItemInput, SyncRepairPreviewInput,
    SyncRejectedItemInput, SYNC_PROTOCOL_VERSION_V1,
};
use crate::sync::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
    db
}

#[tokio::test]
async fn staging_outbox_is_idempotent_for_duplicate_envelope() {
    let db = setup_db().await;
    let input = StageOutboxItemInput {
        idempotency_key: "batch-001".to_string(),
        entity_type: "work_orders".to_string(),
        entity_sync_id: "wo-sync-001".to_string(),
        operation: "update".to_string(),
        row_version: 3,
        payload_json: r#"{"status":"in_progress"}"#.to_string(),
        origin_machine_id: Some("machine-a".to_string()),
    };

    let first = queries::stage_outbox_item(&db, input.clone())
        .await
        .expect("first staging");
    let second = queries::stage_outbox_item(&db, input)
        .await
        .expect("duplicate staging");

    assert_eq!(first.id, second.id);
    assert_eq!(second.status, "pending");

    let all = queries::list_outbox_items(&db, Default::default())
        .await
        .expect("list outbox");
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn partial_apply_with_invalid_inbound_does_not_advance_checkpoint() {
    let db = setup_db().await;
    queries::stage_outbox_item(
        &db,
        StageOutboxItemInput {
            idempotency_key: "batch-ack-1".to_string(),
            entity_type: "work_orders".to_string(),
            entity_sync_id: "wo-sync-ack-1".to_string(),
            operation: "update".to_string(),
            row_version: 5,
            payload_json: r#"{"status":"done"}"#.to_string(),
            origin_machine_id: Some("machine-a".to_string()),
        },
    )
    .await
    .expect("stage outbox");

    let result = queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-001".to_string(),
            checkpoint_token: "ckpt-001".to_string(),
            acknowledged_items: vec![crate::sync::domain::SyncAckInput {
                idempotency_key: "batch-ack-1".to_string(),
                entity_sync_id: "wo-sync-ack-1".to_string(),
                operation: "update".to_string(),
            }],
            rejected_items: vec![],
            inbound_items: vec![SyncInboundItemInput {
                entity_type: "work_orders".to_string(),
                entity_sync_id: "wo-in-001".to_string(),
                operation: "update".to_string(),
                row_version: 1,
                payload_json: "{invalid-json}".to_string(),
            }],
            policy_metadata_json: Some(r#"{"entitlement_state":"active"}"#.to_string()),
        },
    )
    .await
    .expect("apply batch");

    assert_eq!(result.acknowledged_count, 1);
    assert_eq!(result.inbound_applied_count, 0);
    assert!(!result.checkpoint_advanced);
    assert_eq!(result.typed_rejections.len(), 1);

    let state = queries::get_sync_state_summary(&db)
        .await
        .expect("state summary");
    assert!(state.checkpoint.is_none());
}

#[tokio::test]
async fn replaying_same_batch_is_duplicate_safe_and_checkpoint_stable() {
    let db = setup_db().await;
    let input = ApplySyncBatchInput {
        protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
        server_batch_id: "srv-002".to_string(),
        checkpoint_token: "ckpt-002".to_string(),
        acknowledged_items: vec![],
        rejected_items: vec![SyncRejectedItemInput {
            idempotency_key: "missing-idempotency".to_string(),
            entity_sync_id: "missing-entity".to_string(),
            operation: "update".to_string(),
            rejection_code: "NOT_FOUND".to_string(),
            rejection_message: "No matching outbox record.".to_string(),
        }],
        inbound_items: vec![SyncInboundItemInput {
            entity_type: "work_orders".to_string(),
            entity_sync_id: "wo-in-dup-1".to_string(),
            operation: "upsert".to_string(),
            row_version: 7,
            payload_json: r#"{"code":"WO-1001"}"#.to_string(),
        }],
        policy_metadata_json: Some(r#"{"channel":"stable"}"#.to_string()),
    };

    let first = queries::apply_sync_batch(&db, input.clone())
        .await
        .expect("first apply");
    let second = queries::apply_sync_batch(&db, input)
        .await
        .expect("replay apply");

    assert!(first.checkpoint_advanced);
    assert!(second.checkpoint_advanced);
    assert_eq!(first.inbound_applied_count, 1);
    assert_eq!(second.inbound_applied_count, 0);
    assert_eq!(second.inbound_duplicate_count, 1);
    assert_eq!(first.checkpoint_token, second.checkpoint_token);

    let push = queries::get_sync_push_payload(&db, Some(50))
        .await
        .expect("push payload");
    assert_eq!(push.protocol_version, SYNC_PROTOCOL_VERSION_V1);
    assert_eq!(push.checkpoint_token.as_deref(), Some("ckpt-002"));
}

#[tokio::test]
async fn conflict_routing_creates_operator_review_records() {
    let db = setup_db().await;
    queries::stage_outbox_item(
        &db,
        StageOutboxItemInput {
            idempotency_key: "batch-conflict-1".to_string(),
            entity_type: "work_orders".to_string(),
            entity_sync_id: "wo-sync-conflict-1".to_string(),
            operation: "update".to_string(),
            row_version: 2,
            payload_json: r#"{"status":"planned"}"#.to_string(),
            origin_machine_id: Some("machine-z".to_string()),
        },
    )
    .await
    .expect("stage outbox");

    queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-conf-001".to_string(),
            checkpoint_token: "ckpt-conf-001".to_string(),
            acknowledged_items: vec![],
            rejected_items: vec![SyncRejectedItemInput {
                idempotency_key: "batch-conflict-1".to_string(),
                entity_sync_id: "wo-sync-conflict-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "Remote authority wins for a protected field.".to_string(),
            }],
            inbound_items: vec![],
            policy_metadata_json: Some(r#"{"entitlement_state":"active"}"#.to_string()),
        },
    )
    .await
    .expect("apply conflict batch");

    let conflicts = queries::list_sync_conflicts(
        &db,
        SyncConflictFilter {
            statuses: Some(vec!["new".to_string()]),
            conflict_type: Some("AUTHORITY_MISMATCH".to_string()),
            requires_operator_review: Some(true),
            limit: Some(50),
        },
    )
    .await
    .expect("list conflicts");
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].requires_operator_review);
    assert_eq!(conflicts[0].recommended_action, "merge_fields");
}

#[tokio::test]
async fn replay_checkpoint_guard_and_resolution_enable_replay() {
    let db = setup_db().await;
    let actor_id = 1_i64;
    queries::stage_outbox_item(
        &db,
        StageOutboxItemInput {
            idempotency_key: "batch-replay-guard-1".to_string(),
            entity_type: "work_orders".to_string(),
            entity_sync_id: "wo-sync-guard-1".to_string(),
            operation: "update".to_string(),
            row_version: 3,
            payload_json: r#"{"priority":"high"}"#.to_string(),
            origin_machine_id: Some("machine-y".to_string()),
        },
    )
    .await
    .expect("stage outbox");

    queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-guard-001".to_string(),
            checkpoint_token: "ckpt-guard-001".to_string(),
            acknowledged_items: vec![],
            rejected_items: vec![SyncRejectedItemInput {
                idempotency_key: "batch-replay-guard-1".to_string(),
                entity_sync_id: "wo-sync-guard-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "Governed field cannot be auto-merged.".to_string(),
            }],
            inbound_items: vec![],
            policy_metadata_json: None,
        },
    )
    .await
    .expect("apply batch");

    let blocked = queries::replay_sync_failures(
        &db,
        actor_id,
        ReplaySyncFailuresInput {
            replay_key: "replay-guard-blocked".to_string(),
            mode: "checkpoint_rollback".to_string(),
            reason: "Attempt rollback before conflict resolution".to_string(),
            conflict_id: None,
            outbox_id: None,
            server_batch_id: None,
            window_start: None,
            window_end: None,
            checkpoint_token: Some("ckpt-rollback-target".to_string()),
        },
    )
    .await
    .expect_err("guard should block unresolved operator conflicts");
    assert!(format!("{blocked}").contains("checkpoint safety guard"));

    let conflict = queries::list_sync_conflicts(
        &db,
        SyncConflictFilter {
            statuses: Some(vec!["new".to_string()]),
            conflict_type: Some("AUTHORITY_MISMATCH".to_string()),
            requires_operator_review: Some(true),
            limit: Some(10),
        },
    )
    .await
    .expect("list conflict")
    .pop()
    .expect("conflict exists");

    let resolved = queries::resolve_sync_conflict(
        &db,
        actor_id,
        ResolveSyncConflictInput {
            conflict_id: conflict.id,
            expected_row_version: conflict.row_version,
            action: "accept_local".to_string(),
            resolution_note: Some("Operator-approved local payload retention.".to_string()),
        },
    )
    .await
    .expect("resolve conflict");
    assert_eq!(resolved.status, "resolved_local");

    let replay = queries::replay_sync_failures(
        &db,
        actor_id,
        ReplaySyncFailuresInput {
            replay_key: "replay-guard-success".to_string(),
            mode: "checkpoint_rollback".to_string(),
            reason: "Rollback after conflict resolution".to_string(),
            conflict_id: None,
            outbox_id: None,
            server_batch_id: None,
            window_start: None,
            window_end: None,
            checkpoint_token: Some("ckpt-rollback-target".to_string()),
        },
    )
    .await
    .expect("replay succeeds");
    assert!(replay.guard_applied);
    assert_eq!(
        replay.checkpoint_token_after.as_deref(),
        Some("ckpt-rollback-target")
    );
}

#[derive(Clone)]
struct MockVpsContract {
    record_class: &'static str,
    batch_id: &'static str,
    checkpoint_token: &'static str,
}

async fn stage_record_class_outbox(db: &DatabaseConnection, record_class: &str, suffix: &str) {
    queries::stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("idk-{record_class}-{suffix}"),
            entity_type: record_class.to_string(),
            entity_sync_id: format!("{record_class}-sync-{suffix}"),
            operation: "update".to_string(),
            row_version: 1,
            payload_json: r#"{"status":"queued"}"#.to_string(),
            origin_machine_id: Some("test-machine".to_string()),
        },
    )
    .await
    .expect("stage outbox");
}

async fn apply_mock_vps_contract(db: &DatabaseConnection, contract: &MockVpsContract) {
    let base_sync_id = format!("{}-sync-1", contract.record_class);
    let second_sync_id = format!("{}-sync-2", contract.record_class);
    queries::apply_sync_batch(
        db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: contract.batch_id.to_string(),
            checkpoint_token: contract.checkpoint_token.to_string(),
            acknowledged_items: vec![SyncAckInput {
                idempotency_key: format!("idk-{}-1", contract.record_class),
                entity_sync_id: base_sync_id,
                operation: "update".to_string(),
            }],
            rejected_items: vec![SyncRejectedItemInput {
                idempotency_key: format!("idk-{}-2", contract.record_class),
                entity_sync_id: second_sync_id,
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "Mock VPS partial acceptance conflict.".to_string(),
            }],
            inbound_items: vec![SyncInboundItemInput {
                entity_type: contract.record_class.to_string(),
                entity_sync_id: format!("{}-inbound-1", contract.record_class),
                operation: "upsert".to_string(),
                row_version: 1,
                payload_json: r#"{"upstream":"ok"}"#.to_string(),
            }],
            policy_metadata_json: Some(r#"{"entitlement_state":"active"}"#.to_string()),
        },
    )
    .await
    .expect("apply mock contract");
}

#[tokio::test]
async fn deterministic_sync_matrix_covers_record_classes_partial_acceptance_replay_restart() {
    let db = setup_db().await;
    let contracts = vec![
        MockVpsContract {
            record_class: "work_orders",
            batch_id: "srv-matrix-wo",
            checkpoint_token: "ckpt-101",
        },
        MockVpsContract {
            record_class: "intervention_requests",
            batch_id: "srv-matrix-di",
            checkpoint_token: "ckpt-102",
        },
        MockVpsContract {
            record_class: "inventory_movements",
            batch_id: "srv-matrix-inv",
            checkpoint_token: "ckpt-103",
        },
    ];

    for contract in &contracts {
        stage_record_class_outbox(&db, contract.record_class, "1").await;
        stage_record_class_outbox(&db, contract.record_class, "2").await;
        apply_mock_vps_contract(&db, contract).await;

        // Replay the same contract to verify idempotent behavior after "restart/replay".
        apply_mock_vps_contract(&db, contract).await;
    }

    let summary = queries::get_sync_state_summary(&db)
        .await
        .expect("summary after matrix");
    assert_eq!(summary.checkpoint.as_ref().and_then(|c| c.checkpoint_token.clone()).as_deref(), Some("ckpt-103"));
    assert_eq!(summary.pending_outbox_count, 0);
    assert!(summary.rejected_outbox_count >= 3);

    let conflicts = queries::list_sync_conflicts(
        &db,
        SyncConflictFilter {
            statuses: Some(vec!["new".to_string(), "triaged".to_string(), "escalated".to_string()]),
            conflict_type: Some("AUTHORITY_MISMATCH".to_string()),
            requires_operator_review: Some(true),
            limit: Some(100),
        },
    )
    .await
    .expect("list conflicts");
    assert!(conflicts.len() >= 3);
}

#[tokio::test]
async fn stale_checkpoint_is_rejected_deterministically() {
    let db = setup_db().await;
    stage_record_class_outbox(&db, "work_orders", "1").await;
    queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-stale-a".to_string(),
            checkpoint_token: "ckpt-200".to_string(),
            acknowledged_items: vec![SyncAckInput {
                idempotency_key: "idk-work_orders-1".to_string(),
                entity_sync_id: "work_orders-sync-1".to_string(),
                operation: "update".to_string(),
            }],
            rejected_items: vec![],
            inbound_items: vec![],
            policy_metadata_json: None,
        },
    )
    .await
    .expect("apply fresh checkpoint");

    stage_record_class_outbox(&db, "work_orders", "2").await;
    let stale = queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-stale-b".to_string(),
            checkpoint_token: "ckpt-199".to_string(),
            acknowledged_items: vec![SyncAckInput {
                idempotency_key: "idk-work_orders-2".to_string(),
                entity_sync_id: "work_orders-sync-2".to_string(),
                operation: "update".to_string(),
            }],
            rejected_items: vec![],
            inbound_items: vec![],
            policy_metadata_json: None,
        },
    )
    .await
    .expect_err("stale checkpoint must fail");
    assert!(format!("{stale}").contains("Stale checkpoint token"));
}

#[tokio::test]
async fn repair_preview_execute_flow_is_scoped_audited_and_non_destructive() {
    let db = setup_db().await;
    let actor_id = 7_i64;
    stage_record_class_outbox(&db, "work_orders", "1").await;
    queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-repair".to_string(),
            checkpoint_token: "ckpt-300".to_string(),
            acknowledged_items: vec![],
            rejected_items: vec![SyncRejectedItemInput {
                idempotency_key: "idk-work_orders-1".to_string(),
                entity_sync_id: "work_orders-sync-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "Needs operator repair.".to_string(),
            }],
            inbound_items: vec![],
            policy_metadata_json: None,
        },
    )
    .await
    .expect("seed rejected row");

    let destructive_preview = queries::preview_sync_repair(
        &db,
        actor_id,
        SyncRepairPreviewInput {
            mode: "full_reset".to_string(),
            reason: "should be blocked".to_string(),
            outbox_ids: None,
            conflict_ids: None,
            server_batch_id: None,
            checkpoint_token: None,
        },
    )
    .await
    .expect_err("destructive reset path must be blocked");
    assert!(format!("{destructive_preview}").contains("Unsupported repair mode"));

    let preview = queries::preview_sync_repair(
        &db,
        actor_id,
        SyncRepairPreviewInput {
            mode: "requeue_rejected_outbox".to_string(),
            reason: "Recover rejected outbox records".to_string(),
            outbox_ids: None,
            conflict_ids: None,
            server_batch_id: Some("srv-repair".to_string()),
            checkpoint_token: None,
        },
    )
    .await
    .expect("preview repair");
    assert!(preview.requires_confirmation);
    assert_eq!(preview.affected_outbox_count, 1);

    let missing_confirm = queries::execute_sync_repair(
        &db,
        actor_id,
        ExecuteSyncRepairInput {
            plan_id: preview.plan_id.clone(),
            confirm_phrase: "WRONG".to_string(),
        },
    )
    .await
    .expect_err("confirm phrase required");
    assert!(format!("{missing_confirm}").contains("confirm_phrase"));

    let executed = queries::execute_sync_repair(
        &db,
        actor_id,
        ExecuteSyncRepairInput {
            plan_id: preview.plan_id.clone(),
            confirm_phrase: "CONFIRM_SYNC_REPAIR".to_string(),
        },
    )
    .await
    .expect("execute repair");
    assert_eq!(executed.status, "executed");
    assert_eq!(executed.requeued_outbox_count, 1);

    let actions = queries::list_sync_repair_actions(&db, Some(20))
        .await
        .expect("list repair actions");
    assert!(!actions.is_empty());

    let audit_count: i64 = db
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count
             FROM audit_events
             WHERE action_code IN ('sync.repair.preview', 'sync.repair.execute')",
            [],
        ))
        .await
        .expect("audit query")
        .expect("audit row")
        .try_get("", "count")
        .expect("audit count");
    assert_eq!(audit_count, 2);
}

#[tokio::test]
async fn sync_observability_report_includes_alerts_runbooks_and_recovery_proofs() {
    let db = setup_db().await;
    let actor_id = 11_i64;
    stage_record_class_outbox(&db, "intervention_requests", "1").await;
    queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-observability".to_string(),
            checkpoint_token: "ckpt-401".to_string(),
            acknowledged_items: vec![],
            rejected_items: vec![SyncRejectedItemInput {
                idempotency_key: "idk-intervention_requests-1".to_string(),
                entity_sync_id: "intervention_requests-sync-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "force conflict".to_string(),
            }],
            inbound_items: vec![],
            policy_metadata_json: None,
        },
    )
    .await
    .expect("seed conflict");

    let conflict = queries::list_sync_conflicts(
        &db,
        SyncConflictFilter {
            statuses: Some(vec!["new".to_string()]),
            conflict_type: Some("AUTHORITY_MISMATCH".to_string()),
            requires_operator_review: Some(true),
            limit: Some(10),
        },
    )
    .await
    .expect("list conflict")
    .pop()
    .expect("conflict row");
    queries::resolve_sync_conflict(
        &db,
        actor_id,
        ResolveSyncConflictInput {
            conflict_id: conflict.id,
            expected_row_version: conflict.row_version,
            action: "accept_remote".to_string(),
            resolution_note: Some("resolved for proof workflow".to_string()),
        },
    )
    .await
    .expect("resolve conflict");

    let preview = queries::preview_sync_repair(
        &db,
        actor_id,
        SyncRepairPreviewInput {
            mode: "checkpoint_realign".to_string(),
            reason: "runbook-guided checkpoint realign".to_string(),
            outbox_ids: None,
            conflict_ids: None,
            server_batch_id: None,
            checkpoint_token: Some("ckpt-402".to_string()),
        },
    )
    .await
    .expect("preview checkpoint repair");
    queries::execute_sync_repair(
        &db,
        actor_id,
        ExecuteSyncRepairInput {
            plan_id: preview.plan_id,
            confirm_phrase: "CONFIRM_SYNC_REPAIR".to_string(),
        },
    )
    .await
    .expect("execute checkpoint repair");

    let report = queries::get_sync_observability_report(&db)
        .await
        .expect("observability report");
    assert!(!report.alerts.is_empty());
    assert!(!report.diagnostics_links.is_empty());
    assert!(!report.recovery_proofs.is_empty());
    assert!(report
        .alerts
        .iter()
        .all(|alert| alert.runbook_url.starts_with("https://docs.maintafox.com/runbooks/sync/")));
}
