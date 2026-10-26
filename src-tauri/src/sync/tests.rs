use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::sync::domain::{
    ApplySyncBatchInput, ExecuteSyncRepairInput, ReplaySyncFailuresInput, ResolveSyncConflictInput,
    StageOutboxItemInput, SyncAckInput, SyncConflictFilter, SyncInboundItemInput, SyncRejectedItemInput,
    SyncRepairPreviewInput, SYNC_PROTOCOL_VERSION_V1,
};
use crate::sync::queries;

async fn setup_db() -> DatabaseConnection {
    crate::db::test_db::migrated().await
}

/// A person who resolves conflicts (the choice is audited under their account).
async fn operator(db: &DatabaseConnection, username: &str) -> i64 {
    db.execute(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, \
         is_admin, force_password_change, created_at, updated_at) \
         VALUES (?, ?, ?, 'local', 1, 0, 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
         strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [format!("user-{username}").into(), username.into(), username.into()],
    ))
    .await
    .expect("insert operator");
    db.query_one(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT id FROM user_accounts WHERE username = ?",
        [username.into()],
    ))
    .await
    .expect("query operator")
    .expect("operator row")
    .try_get("", "id")
    .expect("operator id")
}

/// Idempotency key the outbox assigned to the latest staged change of an entity.
async fn outbox_key(db: &DatabaseConnection, entity_sync_id: &str) -> String {
    db.query_one(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT idempotency_key FROM sync_outbox WHERE entity_sync_id = ? ORDER BY id DESC LIMIT 1",
        [entity_sync_id.into()],
    ))
    .await
    .expect("query outbox key")
    .expect("staged outbox item")
    .try_get("", "idempotency_key")
    .expect("idempotency_key")
}

#[tokio::test]
async fn staging_outbox_is_idempotent_for_duplicate_envelope() {
    let db = setup_db().await;
    let input = StageOutboxItemInput {
        entity_type: "work_orders".to_string(),
        entity_sync_id: "wo-sync-001".to_string(),
        operation: "update".to_string(),
        payload_json: r#"{"status":"in_progress"}"#.to_string(),
    };

    let first = queries::stage_outbox_item(&db, input.clone())
        .await
        .expect("first staging");
    let second = queries::stage_outbox_item(&db, input).await.expect("duplicate staging");

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
            entity_type: "work_orders".to_string(),
            entity_sync_id: "wo-sync-ack-1".to_string(),
            operation: "update".to_string(),
            payload_json: r#"{"status":"done"}"#.to_string(),
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
                idempotency_key: outbox_key(&db, "wo-sync-ack-1").await,
                entity_sync_id: "wo-sync-ack-1".to_string(),
                operation: "update".to_string(),
                ..Default::default()
            }],
            rejected_items: vec![],
            inbound_items: vec![SyncInboundItemInput {
                entity_type: "work_orders".to_string(),
                entity_sync_id: "wo-in-001".to_string(),
                operation: "update".to_string(),
                row_version: 1,
                payload_json: "{invalid-json}".to_string(),
                ..Default::default()
            }],
            conflict_items: vec![],
            policy_metadata_json: Some(r#"{"entitlement_state":"active"}"#.to_string()),
        },
    )
    .await
    .expect("apply batch");

    assert_eq!(result.acknowledged_count, 1);
    assert_eq!(result.inbound_applied_count, 0);
    assert!(!result.checkpoint_advanced);
    assert_eq!(result.typed_rejections.len(), 1);

    let state = queries::get_sync_state_summary(&db).await.expect("state summary");
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
            ..Default::default()
        }],
        conflict_items: vec![],
        policy_metadata_json: Some(r#"{"channel":"stable"}"#.to_string()),
    };

    let first = queries::apply_sync_batch(&db, input.clone())
        .await
        .expect("first apply");
    let second = queries::apply_sync_batch(&db, input).await.expect("replay apply");

    assert!(first.checkpoint_advanced);
    assert!(second.checkpoint_advanced);
    assert_eq!(first.inbound_applied_count, 1);
    assert_eq!(second.inbound_applied_count, 0);
    assert_eq!(second.inbound_duplicate_count, 1);
    assert_eq!(first.checkpoint_token, second.checkpoint_token);

    let push = queries::get_sync_push_payload(&db, Some(50))
        .await
        .expect("push payload");
    assert_eq!(push.protocol_version, crate::sync::domain::SYNC_PROTOCOL_VERSION_V2);
    assert_eq!(push.checkpoint_token.as_deref(), Some("ckpt-002"));
}

#[tokio::test]
async fn conflict_routing_creates_operator_review_records() {
    let db = setup_db().await;
    queries::stage_outbox_item(
        &db,
        StageOutboxItemInput {
            entity_type: "work_orders".to_string(),
            entity_sync_id: "wo-sync-conflict-1".to_string(),
            operation: "update".to_string(),
            payload_json: r#"{"status":"planning"}"#.to_string(),
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
                idempotency_key: outbox_key(&db, "wo-sync-conflict-1").await,
                entity_sync_id: "wo-sync-conflict-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "Remote authority wins for a protected field.".to_string(),
            }],
            inbound_items: vec![],
            conflict_items: vec![],
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
    assert_eq!(conflicts[0].recommended_action, "accept_remote");
}

#[tokio::test]
async fn replay_checkpoint_guard_and_resolution_enable_replay() {
    let db = setup_db().await;
    let actor_id = operator(&db, "replay-operator").await;
    queries::stage_outbox_item(
        &db,
        StageOutboxItemInput {
            entity_type: "work_orders".to_string(),
            entity_sync_id: "wo-sync-guard-1".to_string(),
            operation: "update".to_string(),
            payload_json: r#"{"priority":"high"}"#.to_string(),
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
                idempotency_key: outbox_key(&db, "wo-sync-guard-1").await,
                entity_sync_id: "wo-sync-guard-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "Governed field cannot be auto-merged.".to_string(),
            }],
            inbound_items: vec![],
            conflict_items: vec![],
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
    assert_eq!(replay.checkpoint_token_after.as_deref(), Some("ckpt-rollback-target"));
}

#[derive(Clone)]
struct MockVpsContract {
    record_class: &'static str,
    batch_id: &'static str,
    checkpoint_token: &'static str,
}

async fn stage_record_class_outbox(db: &impl ConnectionTrait, record_class: &str, suffix: &str) {
    queries::stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: record_class.to_string(),
            entity_sync_id: format!("{record_class}-sync-{suffix}"),
            operation: "update".to_string(),
            payload_json: r#"{"status":"queued"}"#.to_string(),
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
                idempotency_key: outbox_key(db, &base_sync_id).await,
                entity_sync_id: base_sync_id,
                operation: "update".to_string(),
                ..Default::default()
            }],
            rejected_items: vec![SyncRejectedItemInput {
                idempotency_key: outbox_key(db, &second_sync_id).await,
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
                ..Default::default()
            }],
            conflict_items: vec![],
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
            record_class: "inventory_transactions",
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
    assert_eq!(
        summary
            .checkpoint
            .as_ref()
            .and_then(|c| c.checkpoint_token.clone())
            .as_deref(),
        Some("ckpt-103")
    );
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
                idempotency_key: outbox_key(&db, "work_orders-sync-1").await,
                entity_sync_id: "work_orders-sync-1".to_string(),
                operation: "update".to_string(),
                ..Default::default()
            }],
            rejected_items: vec![],
            inbound_items: vec![],
            conflict_items: vec![],
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
                idempotency_key: outbox_key(&db, "work_orders-sync-2").await,
                entity_sync_id: "work_orders-sync-2".to_string(),
                operation: "update".to_string(),
                ..Default::default()
            }],
            rejected_items: vec![],
            inbound_items: vec![],
            conflict_items: vec![],
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
                idempotency_key: outbox_key(&db, "work_orders-sync-1").await,
                entity_sync_id: "work_orders-sync-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "Needs operator repair.".to_string(),
            }],
            inbound_items: vec![],
            conflict_items: vec![],
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
    let actor_id = operator(&db, "proof-operator").await;
    stage_record_class_outbox(&db, "intervention_requests", "1").await;
    queries::apply_sync_batch(
        &db,
        ApplySyncBatchInput {
            protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
            server_batch_id: "srv-observability".to_string(),
            checkpoint_token: "ckpt-401".to_string(),
            acknowledged_items: vec![],
            rejected_items: vec![SyncRejectedItemInput {
                idempotency_key: outbox_key(&db, "intervention_requests-sync-1").await,
                entity_sync_id: "intervention_requests-sync-1".to_string(),
                operation: "update".to_string(),
                rejection_code: "AUTHORITY_MISMATCH".to_string(),
                rejection_message: "force conflict".to_string(),
            }],
            inbound_items: vec![],
            conflict_items: vec![],
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
            action: "accept_local".to_string(),
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
    assert!(report.alerts.iter().all(|alert| alert
        .runbook_url
        .starts_with("https://docs.maintafox.com/runbooks/sync/")));
}

#[tokio::test]
async fn inbox_ingest_is_pending_then_materialize_applies_equipment() {
    let db = setup_db().await;

    let batch = ApplySyncBatchInput {
        protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
        server_batch_id: "srv-mat-1".to_string(),
        checkpoint_token: "ckpt-mat-1".to_string(),
        acknowledged_items: vec![],
        rejected_items: vec![],
        inbound_items: vec![SyncInboundItemInput {
            entity_type: "equipment".to_string(),
            entity_sync_id: "eq-sync-mat-1".to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: r#"{"_sync_format":"row.v1","asset_id_code":"EQ-MAT-1","name":"Pump","lifecycle_status":"active_in_service","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"#.to_string(),
            ..Default::default()
        }],
        conflict_items: vec![],
        policy_metadata_json: None,
    };

    let ingest = queries::apply_sync_batch(&db, batch).await.expect("ingest");
    assert_eq!(ingest.inbound_applied_count, 1);

    let inbox = queries::list_inbox_items(&db, Some("pending".into()), Some(10))
        .await
        .expect("list pending");
    assert_eq!(inbox.len(), 1);

    let mat = crate::sync::materialize::materialize_inbox_batch(&db, Some(50))
        .await
        .expect("materialize");
    assert_eq!(mat.applied, 1);
    assert_eq!(mat.conflicts, 0);

    let row = db
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT asset_id_code, name FROM equipment WHERE entity_sync_id = ?",
            ["eq-sync-mat-1".into()],
        ))
        .await
        .expect("query")
        .expect("equipment row");
    let code: String = row.try_get("", "asset_id_code").expect("code");
    assert_eq!(code, "EQ-MAT-1");
}

// ── Replay of one item (AU-44): the same rules as "retry later" on its conflict ─────────────────

/// A conflict of `conflict_type` on `entity_sync_id`, holding the outgoing change `outbox_id`.
async fn seed_conflict(
    db: &DatabaseConnection,
    conflict_type: &str,
    entity_type: &str,
    entity_sync_id: &str,
    outbox_id: Option<i64>,
) -> i64 {
    queries::record_sync_conflict(
        db,
        queries::NewSyncConflict {
            source_scope: "outbox",
            source_batch_id: None,
            linked_outbox_id: outbox_id,
            linked_inbox_id: None,
            entity_type: entity_type.to_string(),
            entity_sync_id: entity_sync_id.to_string(),
            operation: "update".to_string(),
            conflict_type: conflict_type.to_string(),
            local_payload_json: Some("{}".to_string()),
            inbound_payload_json: None,
            checkpoint_token: None,
            local_row_version: Some(1),
            inbound_row_version: None,
            detail_message: None,
        },
    )
    .await
    .expect("record conflict");
    db.query_one(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT id FROM sync_conflicts WHERE entity_sync_id = ? ORDER BY id DESC LIMIT 1",
        [entity_sync_id.into()],
    ))
    .await
    .expect("query conflict")
    .expect("conflict row")
    .try_get("", "id")
    .expect("conflict id")
}

/// A staged change of `entity_sync_id`, given up after its delivery attempts.
async fn dead_letter_outbox(db: &DatabaseConnection, entity_sync_id: &str) -> i64 {
    let item = queries::stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: "work_orders".to_string(),
            entity_sync_id: entity_sync_id.to_string(),
            operation: "update".to_string(),
            payload_json: r#"{"priority":"high"}"#.to_string(),
        },
    )
    .await
    .expect("stage outbox");
    db.execute(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "UPDATE sync_outbox SET status = 'dead_letter', attempt_count = 5 WHERE id = ?",
        [item.id.into()],
    ))
    .await
    .expect("dead letter");
    item.id
}

fn single_item(key: &str, conflict_id: Option<i64>, outbox_id: Option<i64>) -> ReplaySyncFailuresInput {
    ReplaySyncFailuresInput {
        replay_key: key.to_string(),
        mode: "single_item".to_string(),
        reason: "Renvoi demandé par un opérateur".to_string(),
        conflict_id,
        outbox_id,
        server_batch_id: None,
        window_start: None,
        window_end: None,
        checkpoint_token: None,
    }
}

async fn status_of(db: &DatabaseConnection, table: &str, id: i64) -> String {
    db.query_one(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        format!("SELECT status FROM {table} WHERE id = ?"),
        [id.into()],
    ))
    .await
    .expect("query status")
    .expect("row")
    .try_get("", "status")
    .expect("status")
}

async fn replay_runs(db: &DatabaseConnection) -> i64 {
    db.query_one(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "SELECT COUNT(*) AS n FROM sync_replay_runs",
    ))
    .await
    .expect("count runs")
    .expect("row")
    .try_get("", "n")
    .expect("n")
}

#[tokio::test]
async fn replaying_a_conflict_requeues_its_own_change_and_reopens_it() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-own").await;
    let outbox = dead_letter_outbox(&db, "wo-replay-own").await;
    let conflict = seed_conflict(&db, "AUTHORITY_MISMATCH", "work_orders", "wo-replay-own", Some(outbox)).await;

    let result = queries::replay_sync_failures(&db, actor, single_item("r-own", Some(conflict), Some(outbox)))
        .await
        .expect("replay");
    assert_eq!(result.requeued_outbox_count, 1);
    assert_eq!(result.transitioned_conflict_count, 1);
    assert_eq!(status_of(&db, "sync_outbox", outbox).await, "pending");
    assert_eq!(status_of(&db, "sync_conflicts", conflict).await, "triaged");
}

#[tokio::test]
async fn replaying_a_given_up_change_also_reopens_its_dead_letter_conflict() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-dead").await;
    let outbox = dead_letter_outbox(&db, "wo-replay-dead").await;
    let conflict = seed_conflict(&db, "OUTBOX_DEAD_LETTER", "work_orders", "wo-replay-dead", Some(outbox)).await;

    let result = queries::replay_sync_failures(&db, actor, single_item("r-dead", None, Some(outbox)))
        .await
        .expect("replay");
    assert_eq!(result.requeued_outbox_count, 1);
    assert_eq!(result.transitioned_conflict_count, 1);
    assert_eq!(status_of(&db, "sync_outbox", outbox).await, "pending");
    assert_eq!(status_of(&db, "sync_conflicts", conflict).await, "triaged");
}

#[tokio::test]
async fn replay_refuses_a_change_that_is_not_the_conflicts_own() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-mismatch").await;
    let own = dead_letter_outbox(&db, "wo-replay-mm-1").await;
    let other = dead_letter_outbox(&db, "wo-replay-mm-2").await;
    let conflict = seed_conflict(&db, "AUTHORITY_MISMATCH", "work_orders", "wo-replay-mm-1", Some(own)).await;

    let err = queries::replay_sync_failures(&db, actor, single_item("r-mm", Some(conflict), Some(other)))
        .await
        .expect_err("another conflict's change is refused");
    assert!(format!("{err:?}").contains("GATE_RECORD_CHANGED_ELSEWHERE"), "{err:?}");
    // Nothing happened: no run, the change still given up, the conflict still new.
    assert_eq!(replay_runs(&db).await, 0);
    assert_eq!(status_of(&db, "sync_outbox", other).await, "dead_letter");
    assert_eq!(status_of(&db, "sync_conflicts", conflict).await, "new");
}

#[tokio::test]
async fn replay_refuses_a_closed_conflict() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-closed").await;
    let outbox = dead_letter_outbox(&db, "wo-replay-closed").await;
    let conflict = seed_conflict(
        &db,
        "AUTHORITY_MISMATCH",
        "work_orders",
        "wo-replay-closed",
        Some(outbox),
    )
    .await;
    db.execute(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Sqlite,
        "UPDATE sync_conflicts SET status = 'dismissed' WHERE id = ?",
        [conflict.into()],
    ))
    .await
    .expect("dismiss");

    let err = queries::replay_sync_failures(&db, actor, single_item("r-closed", Some(conflict), Some(outbox)))
        .await
        .expect_err("a closed conflict is not reopened");
    assert!(format!("{err:?}").contains("GATE_SYNC_CONFLICT_CLOSED"), "{err:?}");
    assert_eq!(status_of(&db, "sync_conflicts", conflict).await, "dismissed");
    assert_eq!(status_of(&db, "sync_outbox", outbox).await, "dead_letter");
    assert_eq!(replay_runs(&db).await, 0);
}

#[tokio::test]
async fn replay_follows_the_conflict_kinds_that_refuse_retry_later() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-kinds").await;
    let cases = [
        (
            crate::permit::ended_under_work::codes::ENDED_BEFORE_START,
            "work_orders",
            "GATE_SYNC_CONFLICT_REVIEW_ONLY",
        ),
        (
            crate::permit::ended_under_work::codes::ENDED_DURING_WORK,
            "work_orders",
            "GATE_SYNC_CONFLICT_REVIEW_ONLY",
        ),
        (
            crate::planning::sync_check::PLANNING_CONFLICTS[0],
            "work_orders",
            "GATE_SYNC_CONFLICT_PLAN_KEEP_ONLY",
        ),
        // A refused history row (append-only) that is not held here.
        (
            crate::inventory::count_four_eyes::SYNC_APPROVAL_RECORD_INVALID,
            "inventory_transactions",
            "GATE_SYNC_CONFLICT_REVIEW_ONLY",
        ),
    ];
    for (i, (kind, entity_type, gate)) in cases.into_iter().enumerate() {
        let sync_id = format!("replay-kind-{i}");
        let outbox = dead_letter_outbox(&db, &sync_id).await;
        let conflict = seed_conflict(&db, kind, entity_type, &sync_id, Some(outbox)).await;
        let err = queries::replay_sync_failures(
            &db,
            actor,
            single_item(&format!("r-kind-{i}"), Some(conflict), Some(outbox)),
        )
        .await
        .expect_err("refused like retry later");
        assert!(format!("{err:?}").contains(gate), "{kind}: {err:?}");
        assert_eq!(status_of(&db, "sync_outbox", outbox).await, "dead_letter", "{kind}");
        // Reached from the given-up change too: the conflict it holds refuses the same way.
        let err = queries::replay_sync_failures(&db, actor, single_item(&format!("r-kind-o-{i}"), None, Some(outbox)))
            .await
            .expect_err("refused from the change too");
        assert!(format!("{err:?}").contains(gate), "{kind}: {err:?}");
    }
    assert_eq!(replay_runs(&db).await, 0);
}

#[tokio::test]
async fn replay_reopens_a_count_settled_at_its_origin_like_retry_later() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-settled").await;
    let outbox = dead_letter_outbox(&db, "replay-settled").await;
    let conflict = seed_conflict(
        &db,
        crate::inventory::count_four_eyes::SYNC_SELF_APPROVAL,
        "inventory_count_lines",
        "replay-settled",
        Some(outbox),
    )
    .await;

    let result = queries::replay_sync_failures(&db, actor, single_item("r-settled", Some(conflict), Some(outbox)))
        .await
        .expect("retry later is accepted on a count settled at its origin");
    assert_eq!(result.transitioned_conflict_count, 1);
    assert_eq!(status_of(&db, "sync_conflicts", conflict).await, "triaged");
}

#[tokio::test]
async fn replay_by_conflict_checks_and_reopens_every_open_conflict_of_the_change() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-siblings").await;

    // The named conflict accepts retry later, but the same change also holds an open planning
    // four-eyes refusal: re-sending it is refused, nothing changes.
    let outbox = dead_letter_outbox(&db, "wo-replay-sib-1").await;
    let named = seed_conflict(
        &db,
        "AUTHORITY_MISMATCH",
        "work_orders",
        "wo-replay-sib-1",
        Some(outbox),
    )
    .await;
    let planning = seed_conflict(
        &db,
        crate::planning::sync_check::PLANNING_CONFLICTS[0],
        "work_orders",
        "wo-replay-sib-1",
        Some(outbox),
    )
    .await;
    let err = queries::replay_sync_failures(&db, actor, single_item("r-sib-1", Some(named), Some(outbox)))
        .await
        .expect_err("the planning refusal on the same change refuses the replay");
    assert!(
        format!("{err:?}").contains("GATE_SYNC_CONFLICT_PLAN_KEEP_ONLY"),
        "{err:?}"
    );
    assert_eq!(status_of(&db, "sync_outbox", outbox).await, "dead_letter");
    assert_eq!(status_of(&db, "sync_conflicts", named).await, "new");
    assert_eq!(status_of(&db, "sync_conflicts", planning).await, "new");

    // Both conflicts of a change accept retry later: one replay reopens both with the send.
    let outbox = dead_letter_outbox(&db, "wo-replay-sib-2").await;
    let named = seed_conflict(
        &db,
        "AUTHORITY_MISMATCH",
        "work_orders",
        "wo-replay-sib-2",
        Some(outbox),
    )
    .await;
    let dead = seed_conflict(
        &db,
        "OUTBOX_DEAD_LETTER",
        "work_orders",
        "wo-replay-sib-2",
        Some(outbox),
    )
    .await;
    let result = queries::replay_sync_failures(&db, actor, single_item("r-sib-2", Some(named), Some(outbox)))
        .await
        .expect("replay");
    assert_eq!(result.requeued_outbox_count, 1);
    assert_eq!(result.transitioned_conflict_count, 2);
    assert_eq!(status_of(&db, "sync_conflicts", named).await, "triaged");
    assert_eq!(status_of(&db, "sync_conflicts", dead).await, "triaged");
}

#[tokio::test]
async fn whole_batch_and_time_window_replays_are_refused() {
    let db = setup_db().await;
    let actor = operator(&db, "replay-scopes").await;
    let outbox = dead_letter_outbox(&db, "wo-replay-scope").await;
    let conflict = seed_conflict(
        &db,
        "AUTHORITY_MISMATCH",
        "work_orders",
        "wo-replay-scope",
        Some(outbox),
    )
    .await;

    for (mode, batch, start, end) in [
        ("batch", Some("srv-batch-1"), None, None),
        (
            "window",
            None,
            Some("2000-01-01T00:00:00Z"),
            Some("2100-01-01T00:00:00Z"),
        ),
    ] {
        let mut input = single_item(&format!("r-scope-{mode}"), None, None);
        input.mode = mode.to_string();
        input.server_batch_id = batch.map(str::to_string);
        input.window_start = start.map(str::to_string);
        input.window_end = end.map(str::to_string);
        let err = queries::replay_sync_failures(&db, actor, input)
            .await
            .expect_err("only one named item is replayed");
        assert!(format!("{err:?}").contains("mode must be one of"), "{mode}: {err:?}");
    }
    assert_eq!(replay_runs(&db).await, 0);
    assert_eq!(status_of(&db, "sync_outbox", outbox).await, "dead_letter");
    assert_eq!(status_of(&db, "sync_conflicts", conflict).await, "new");
}
