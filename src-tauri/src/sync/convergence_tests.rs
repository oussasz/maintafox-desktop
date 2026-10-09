//! Two-SQLite convergence harness — Desktop A outbox → fixture exchange → Desktop B materialize.

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;

use crate::sync::domain::{
    ApplySyncBatchInput, StageOutboxItemInput, SyncAckInput, SyncInboundItemInput,
    SYNC_ENTITY_ARTICLES, SYNC_ENTITY_EQUIPMENT, SYNC_ENTITY_INTERVENTION_REQUESTS,
    SYNC_ENTITY_MEDIA_ATTACHMENTS, SYNC_ENTITY_ORG_NODES, SYNC_ENTITY_STOCK_LOCATIONS,
    SYNC_ENTITY_STOCK_MOVEMENTS, SYNC_ENTITY_WAREHOUSES, SYNC_ENTITY_WORK_ORDERS,
    SYNC_ENTITY_WORK_ORDER_TASKS, SYNC_PROTOCOL_VERSION_V1,
};
use crate::sync::materialize::materialize_inbox_batch;
use crate::sync::queries::{apply_sync_batch, list_outbox_items, stage_outbox_item};

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
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

async fn first_unit_value_id(db: &DatabaseConnection) -> i64 {
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT lv.id
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = 'inventory.unit_of_measure'
               AND lv.is_active = 1
               AND lv.deleted_at IS NULL
             ORDER BY lv.id ASC
             LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("query unit")
        .expect("unit of measure seeded");
    row.try_get("", "id").expect("unit id")
}

fn outbox_to_inbound(
    items: &[crate::sync::domain::SyncOutboxItem],
) -> Vec<SyncInboundItemInput> {
    items
        .iter()
        .map(|o| SyncInboundItemInput {
            entity_type: o.entity_type.clone(),
            entity_sync_id: o.entity_sync_id.clone(),
            operation: o.operation.clone(),
            row_version: o.row_version,
            payload_json: o.payload_json.clone(),
        })
        .collect()
}

async fn exchange_a_to_b(
    db_a: &DatabaseConnection,
    db_b: &DatabaseConnection,
    batch_id: &str,
    checkpoint: &str,
) {
    let pending = list_outbox_items(
        db_a,
        crate::sync::domain::ListOutboxFilter {
            status: Some("pending".into()),
            limit: Some(200),
        },
    )
    .await
    .expect("list A outbox");
    assert!(
        !pending.is_empty(),
        "DB-A must have pending outbox before exchange"
    );

    let acks: Vec<SyncAckInput> = pending
        .iter()
        .map(|o| SyncAckInput {
            idempotency_key: o.idempotency_key.clone(),
            entity_sync_id: o.entity_sync_id.clone(),
            operation: o.operation.clone(),
        })
        .collect();

    let batch = ApplySyncBatchInput {
        protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
        server_batch_id: batch_id.to_string(),
        checkpoint_token: checkpoint.to_string(),
        acknowledged_items: acks,
        rejected_items: vec![],
        inbound_items: outbox_to_inbound(&pending),
        policy_metadata_json: None,
    };

    apply_sync_batch(db_a, batch.clone())
        .await
        .expect("ack on A");
    apply_sync_batch(db_b, batch).await.expect("ingest on B");
    let mat = materialize_inbox_batch(db_b, Some(200))
        .await
        .expect("materialize B");
    assert!(
        mat.errors.is_empty(),
        "materialize errors on B: {:?}",
        mat.errors
    );
    assert_eq!(mat.conflicts, 0, "unexpected conflicts on B");
    assert!(mat.applied > 0, "expected applied rows on B");
}

#[tokio::test]
async fn two_db_equipment_converges() {
    let db_a = setup_db().await;
    let db_b = setup_db().await;

    let esid = "eq-conv-001";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("equipment:{esid}:v1"),
            entity_type: SYNC_ENTITY_EQUIPMENT.to_string(),
            entity_sync_id: esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: serde_json::json!({
                "entity_sync_id": esid,
                "row_version": 1,
                "asset_code": "EQ-CONV-1",
                "asset_name": "Convergence Pump",
                "status_code": "active_in_service"
            })
            .to_string(),
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage equipment on A");

    exchange_a_to_b(&db_a, &db_b, "srv-conv-eq", "ckpt-conv-eq").await;

    let row = db_b
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT asset_id_code, name FROM equipment WHERE entity_sync_id = ?",
            [esid.into()],
        ))
        .await
        .expect("query")
        .expect("equipment on B");
    let code: String = row.try_get("", "asset_id_code").expect("code");
    assert_eq!(code, "EQ-CONV-1");
}

#[tokio::test]
async fn two_db_org_then_equipment_order_converges() {
    let db_a = setup_db().await;
    let db_b = setup_db().await;

    // Seed B with a node type + structure model so org materialize can insert (same as A migrations seed).
    let org_esid = "org-conv-001";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("org_nodes:{org_esid}:v1"),
            entity_type: SYNC_ENTITY_ORG_NODES.to_string(),
            entity_sync_id: org_esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: serde_json::json!({
                "entity_sync_id": org_esid,
                "row_version": 1,
                "code": "SITE-CONV",
                "name": "Convergence Site",
                "status": "active",
                "depth": 0,
                "ancestor_path": "/"
            })
            .to_string(),
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage org");

    let eq_esid = "eq-conv-org-1";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("equipment:{eq_esid}:v1"),
            entity_type: SYNC_ENTITY_EQUIPMENT.to_string(),
            entity_sync_id: eq_esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: serde_json::json!({
                "entity_sync_id": eq_esid,
                "row_version": 1,
                "asset_code": "EQ-ORG-1",
                "asset_name": "Linked Asset",
                "status_code": "active_in_service",
                "org_node_entity_sync_id": org_esid
            })
            .to_string(),
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage equipment with org FK");

    exchange_a_to_b(&db_a, &db_b, "srv-conv-org", "ckpt-conv-org").await;

    let eq = db_b
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT e.asset_id_code, o.code AS org_code
             FROM equipment e
             LEFT JOIN org_nodes o ON o.id = e.installed_at_node_id
             WHERE e.entity_sync_id = ?",
            [eq_esid.into()],
        ))
        .await
        .expect("query")
        .expect("equipment");
    let code: String = eq.try_get("", "asset_id_code").expect("code");
    assert_eq!(code, "EQ-ORG-1");
}

#[tokio::test]
async fn two_db_work_order_with_task_converges() {
    let db_a = setup_db().await;
    let db_b = setup_db().await;

    let wo_esid = "wo-conv-001";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("work_orders:{wo_esid}:v1"),
            entity_type: SYNC_ENTITY_WORK_ORDERS.to_string(),
            entity_sync_id: wo_esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
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
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage WO");

    let task_esid = "wot-conv-001";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("work_order_tasks:{task_esid}:v1"),
            entity_type: SYNC_ENTITY_WORK_ORDER_TASKS.to_string(),
            entity_sync_id: task_esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
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
            origin_machine_id: Some("machine-a".into()),
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
async fn two_db_di_converges() {
    let db_a = setup_db().await;
    let db_b = setup_db().await;

    let org_esid = "org-di-conv";
    let eq_esid = "eq-di-conv";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("org_nodes:{org_esid}:v1"),
            entity_type: SYNC_ENTITY_ORG_NODES.to_string(),
            entity_sync_id: org_esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: serde_json::json!({
                "entity_sync_id": org_esid,
                "row_version": 1,
                "code": "SITE-DI",
                "name": "DI Site",
                "status": "active",
                "depth": 0,
                "ancestor_path": "/"
            })
            .to_string(),
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage org");

    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("equipment:{eq_esid}:v1"),
            entity_type: SYNC_ENTITY_EQUIPMENT.to_string(),
            entity_sync_id: eq_esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: serde_json::json!({
                "entity_sync_id": eq_esid,
                "row_version": 1,
                "asset_code": "EQ-DI-1",
                "asset_name": "DI Asset",
                "status_code": "active_in_service",
                "org_node_entity_sync_id": org_esid
            })
            .to_string(),
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage equipment");

    let di_esid = "di-conv-001";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("intervention_requests:{di_esid}:v1"),
            entity_type: SYNC_ENTITY_INTERVENTION_REQUESTS.to_string(),
            entity_sync_id: di_esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: serde_json::json!({
                "entity_sync_id": di_esid,
                "row_version": 1,
                "code": "DI-CONV-1",
                "title": "Leak report",
                "description": "Seal drip",
                "status": "submitted",
                "origin_type": "field",
                "equipment_entity_sync_id": eq_esid,
                "org_node_entity_sync_id": org_esid
            })
            .to_string(),
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage DI");

    exchange_a_to_b(&db_a, &db_b, "srv-conv-di", "ckpt-conv-di").await;

    let row = db_b
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT code, title FROM intervention_requests WHERE entity_sync_id = ?",
            [di_esid.into()],
        ))
        .await
        .expect("query")
        .expect("di on B");
    let code: String = row.try_get("", "code").expect("code");
    assert_eq!(code, "DI-CONV-1");
}

#[tokio::test]
async fn two_db_stock_movement_converges() {
    let db_a = setup_db().await;
    let db_b = setup_db().await;
    let unit_id = first_unit_value_id(&db_a).await;

    let art = "art-conv-1";
    let wh = "wh-conv-1";
    let loc = "loc-conv-1";
    let mv = "mv-conv-1";

    for (key, entity_type, esid, payload) in [
        (
            "articles",
            SYNC_ENTITY_ARTICLES,
            art,
            serde_json::json!({
                "entity_sync_id": art,
                "row_version": 1,
                "article_code": "ART-CONV",
                "article_name": "Gasket",
                "is_active": true,
                "unit_value_id": unit_id
            }),
        ),
        (
            "warehouses",
            SYNC_ENTITY_WAREHOUSES,
            wh,
            serde_json::json!({
                "entity_sync_id": wh,
                "row_version": 1,
                "code": "WH-CONV",
                "name": "Main WH",
                "is_active": true
            }),
        ),
        (
            "stock_locations",
            SYNC_ENTITY_STOCK_LOCATIONS,
            loc,
            serde_json::json!({
                "entity_sync_id": loc,
                "row_version": 1,
                "code": "BIN-A",
                "name": "Bin A",
                "warehouse_entity_sync_id": wh,
                "is_active": true
            }),
        ),
        (
            "inventory_transactions",
            SYNC_ENTITY_STOCK_MOVEMENTS,
            mv,
            serde_json::json!({
                "entity_sync_id": mv,
                "row_version": 1,
                "article_entity_sync_id": art,
                "warehouse_entity_sync_id": wh,
                "location_entity_sync_id": loc,
                "movement_type": "adjust",
                "quantity": 5.0,
                "source_type": "sync",
                "notes": "seed adjust"
            }),
        ),
    ] {
        stage_outbox_item(
            &db_a,
            StageOutboxItemInput {
                idempotency_key: format!("{key}:{esid}:v1"),
                entity_type: entity_type.to_string(),
                entity_sync_id: esid.to_string(),
                operation: "upsert".to_string(),
                row_version: 1,
                payload_json: payload.to_string(),
                origin_machine_id: Some("machine-a".into()),
            },
        )
        .await
        .expect("stage inventory chain");
    }

    exchange_a_to_b(&db_a, &db_b, "srv-conv-inv", "ckpt-conv-inv").await;

    let row = db_b
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT quantity, movement_type FROM inventory_transactions WHERE entity_sync_id = ?",
            [mv.into()],
        ))
        .await
        .expect("query")
        .expect("movement on B");
    let qty: f64 = row.try_get("", "quantity").expect("qty");
    assert!((qty - 5.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn two_db_media_envelope_converges() {
    let db_a = setup_db().await;
    let db_b = setup_db().await;

    let esid = "media-conv-001";
    let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    stage_outbox_item(
        &db_a,
        StageOutboxItemInput {
            idempotency_key: format!("media_attachments:{esid}:v1"),
            entity_type: SYNC_ENTITY_MEDIA_ATTACHMENTS.to_string(),
            entity_sync_id: esid.to_string(),
            operation: "upsert".to_string(),
            row_version: 1,
            payload_json: serde_json::json!({
                "entity_sync_id": esid,
                "row_version": 1,
                "parent_entity_type": "work_orders",
                "parent_entity_sync_id": "wo-parent-1",
                "content_sha256": sha,
                "mime_type": "image/jpeg",
                "byte_size": 12,
                "object_key": format!("dev/media/local/work_orders/wo-parent-1/{sha}"),
                "file_name": "photo.jpg"
            })
            .to_string(),
            origin_machine_id: Some("machine-a".into()),
        },
    )
    .await
    .expect("stage media");

    exchange_a_to_b(&db_a, &db_b, "srv-conv-media", "ckpt-conv-media").await;

    let row = db_b
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT file_name, content_sha256 FROM media_attachments WHERE entity_sync_id = ?",
            [esid.into()],
        ))
        .await
        .expect("query")
        .expect("media on B");
    let name: String = row.try_get("", "file_name").expect("name");
    assert_eq!(name, "photo.jpg");
}

#[tokio::test]
async fn golden_exchange_request_fixture_validates() {
    let raw = include_str!("fixtures/exchange_request_v1.json");
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let value: serde_json::Value = serde_json::from_str(raw).expect("parse request fixture");
    crate::vps::guards::validate_sync_exchange_request_json(&value)
        .expect("request fixture must satisfy VPS contract");
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

#[tokio::test]
async fn stock_movement_entity_constant_present() {
    // Compile-time / registry sanity — movements are append-only sync entities.
    assert_eq!(SYNC_ENTITY_STOCK_MOVEMENTS, "inventory_transactions");
}
