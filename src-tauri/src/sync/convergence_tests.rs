//! Two-SQLite convergence harness — Desktop A outbox → fixture exchange → Desktop B materialize.
//!
//! Entities on the generic capture/apply engine are covered by `row_sync_tests`; this file keeps
//! the entities still staged by hand-written stagers (work orders and tasks) and the fixtures.

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;

use crate::sync::domain::{
    ApplySyncBatchInput, StageOutboxItemInput, SyncAckInput, SyncInboundItemInput, SYNC_ENTITY_WORK_ORDERS,
    SYNC_ENTITY_WORK_ORDER_TASKS, SYNC_PROTOCOL_VERSION_V1,
};
use crate::sync::materialize::materialize_inbox_batch;
use crate::sync::queries::{apply_sync_batch, list_outbox_items, stage_outbox_item};

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db)
        .await
        .expect("seed system data");
    // Org materialize needs at least one active node type + structure model.
    let has_type = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id FROM org_node_types WHERE is_active = 1 LIMIT 1".to_string(),
        ))
        .await
        .expect("query node types");
    if has_type.is_none() {
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO org_structure_models (entity_sync_id, version_number, status, description, created_at, updated_at)
             VALUES ('conv-test-structure', 1, 'active', 'convergence', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
                .to_string(),
        ))
        .await
        .expect("structure model");
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO org_node_types (
                entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, can_carry_cost_center,
                can_aggregate_kpis, can_receive_permits, is_root_type, is_active, created_at, updated_at
             ) VALUES (
                'conv-test-site-type',
                (SELECT id FROM org_structure_models WHERE entity_sync_id = 'conv-test-structure'),
                'SITE', 'Site', 1, 1, 0, 1, 0, 1, 1,
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )"
            .to_string(),
        ))
        .await
        .expect("node type");
    }
    db
}

fn outbox_to_inbound(items: &[crate::sync::domain::SyncOutboxItem]) -> Vec<SyncInboundItemInput> {
    items
        .iter()
        .map(|o| SyncInboundItemInput {
            entity_type: o.entity_type.clone(),
            entity_sync_id: o.entity_sync_id.clone(),
            operation: o.operation.clone(),
            row_version: o.row_version,
            payload_json: o.payload_json.clone(),
            ..Default::default()
        })
        .collect()
}

async fn exchange_a_to_b(db_a: &DatabaseConnection, db_b: &DatabaseConnection, batch_id: &str, checkpoint: &str) {
    let pending = list_outbox_items(
        db_a,
        crate::sync::domain::ListOutboxFilter {
            status: Some("pending".into()),
            limit: Some(200),
        },
    )
    .await
    .expect("list A outbox");
    assert!(!pending.is_empty(), "DB-A must have pending outbox before exchange");

    let acks: Vec<SyncAckInput> = pending
        .iter()
        .map(|o| SyncAckInput {
            idempotency_key: o.idempotency_key.clone(),
            entity_sync_id: o.entity_sync_id.clone(),
            operation: o.operation.clone(),
            ..Default::default()
        })
        .collect();

    let batch = ApplySyncBatchInput {
        protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
        server_batch_id: batch_id.to_string(),
        checkpoint_token: checkpoint.to_string(),
        acknowledged_items: acks,
        rejected_items: vec![],
        inbound_items: outbox_to_inbound(&pending),
        conflict_items: vec![],
        policy_metadata_json: None,
    };

    apply_sync_batch(db_a, batch.clone()).await.expect("ack on A");
    apply_sync_batch(db_b, batch).await.expect("ingest on B");
    let mat = materialize_inbox_batch(db_b, Some(200)).await.expect("materialize B");
    assert!(mat.errors.is_empty(), "materialize errors on B: {:?}", mat.errors);
    assert_eq!(mat.conflicts, 0, "unexpected conflicts on B");
    assert!(mat.applied > 0, "expected applied rows on B");
}

#[tokio::test]
async fn two_db_work_order_with_task_converges() {
    let db_a = setup_db().await;
    let db_b = setup_db().await;

    let wo_esid = "wo-conv-001";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_WORK_ORDERS.to_string(),
            entity_sync_id: wo_esid.to_string(),
            operation: "upsert".to_string(),
            payload_json: serde_json::json!({
                "entity_sync_id": wo_esid,
                "row_version": 1,
                "code": "OT-CONV-1",
                "title": "Convergence WO",
                "status": "draft",
                "maintenance_type_code": "corrective",
                "requires_permit": false,
                "closeout_validation_passed": false
            })
            .to_string(),
        },
    )
    .await
    .expect("stage WO");

    let task_esid = "wot-conv-001";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_WORK_ORDER_TASKS.to_string(),
            entity_sync_id: task_esid.to_string(),
            operation: "upsert".to_string(),
            payload_json: serde_json::json!({
                "entity_sync_id": task_esid,
                "row_version": 1,
                "work_order_entity_sync_id": wo_esid,
                "task_description": "Inspect seal",
                "sequence_order": 1,
                "is_mandatory": true,
                "is_completed": false
            })
            .to_string(),
        },
    )
    .await
    .expect("stage WO task");

    exchange_a_to_b(&db_a, &db_b, "srv-conv-wo", "ckpt-conv-wo").await;

    let row = db_b
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT code, title FROM work_orders WHERE entity_sync_id = ?",
            [wo_esid.into()],
        ))
        .await
        .expect("query")
        .expect("wo on B");
    let code: String = row.try_get("", "code").expect("code");
    assert_eq!(code, "OT-CONV-1");

    let task = db_b
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT task_description FROM work_order_tasks WHERE entity_sync_id = ?",
            [task_esid.into()],
        ))
        .await
        .expect("query")
        .expect("task on B");
    let desc: String = task.try_get("", "task_description").expect("desc");
    assert_eq!(desc, "Inspect seal");
}

#[tokio::test]
async fn golden_exchange_request_fixture_validates() {
    let raw = include_str!("fixtures/exchange_request_v1.json");
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let value: serde_json::Value = serde_json::from_str(raw).expect("parse request fixture");
    crate::vps::guards::validate_sync_exchange_request_json(&value).expect("request fixture must satisfy VPS contract");
}

#[tokio::test]
async fn golden_exchange_response_fixture_applies() {
    let db = setup_db().await;
    let raw = include_str!("fixtures/exchange_response_v1.json");
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let batch: ApplySyncBatchInput = serde_json::from_str(raw).expect("parse response fixture");
    assert_eq!(batch.protocol_version, SYNC_PROTOCOL_VERSION_V1);
    apply_sync_batch(&db, batch).await.expect("apply fixture");
    let mat = materialize_inbox_batch(&db, Some(50))
        .await
        .expect("materialize fixture");
    assert!(mat.applied >= 1);
}
