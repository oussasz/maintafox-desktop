//! Phase 2.2 behaviour (stable identity, versioning, deferred inbound, conflicts that act on data),
//! exercised through the Phase 2.4 capture: business writes are captured by the table triggers and
//! drained into the outbox.

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use sea_orm_migration::MigratorTrait;

use super::domain::{
    ApplySyncBatchInput, ResolveSyncConflictInput, SyncAckInput, SyncConflictFilter, SyncInboundItemInput,
    SYNC_PROTOCOL_VERSION_V1,
};
use super::capture::drain_capture_journal;
use super::materialize::materialize_inbox_batch;
use super::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    super::capture::install_capture(&db).await.expect("install capture");
    db
}

/// Turn captured changes into outbox envelopes, as a push or an inbound apply does.
async fn stage(db: &impl ConnectionTrait) {
    let report = drain_capture_journal(db).await.expect("drain capture journal");
    assert_eq!(report.failed, 0, "staging failed: {report:?}");
}

const FORMAT: &str = "row.v1";

async fn exec(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .expect("fixture statement");
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .expect("scalar query")
        .expect("scalar row")
        .try_get_by_index::<i64>(0)
        .expect("scalar value")
}

async fn scalar_string(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> String {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .expect("scalar query")
        .expect("scalar row")
        .try_get_by_index::<String>(0)
        .expect("scalar value")
}

async fn insert_team(db: &impl ConnectionTrait, sync_id: Option<&str>, code: &str, name: &str) -> i64 {
    exec(
        db,
        "INSERT INTO teams (entity_sync_id, code, name, team_type, status, row_version, created_at, updated_at)
         VALUES (?, ?, ?, 'maintenance', 'active', 1, '2026-10-10T00:00:00Z', '2026-10-10T00:00:00Z')",
        // A blank id stands for "row created without identity".
        vec![sync_id.unwrap_or("").into(), code.into(), name.into()],
    )
    .await;
    scalar_i64(db, "SELECT id FROM teams WHERE code = ?", vec![code.into()]).await
}

fn batch(batch_id: &str, checkpoint: &str, inbound: Vec<SyncInboundItemInput>) -> ApplySyncBatchInput {
    ApplySyncBatchInput {
        protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
        server_batch_id: batch_id.to_string(),
        checkpoint_token: checkpoint.to_string(),
        acknowledged_items: vec![],
        rejected_items: vec![],
        inbound_items: inbound,
        conflict_items: vec![],
        policy_metadata_json: None,
    }
}

fn inbound(entity_type: &str, sync_id: &str, row_version: i64, payload: serde_json::Value) -> SyncInboundItemInput {
    SyncInboundItemInput {
        entity_type: entity_type.to_string(),
        entity_sync_id: sync_id.to_string(),
        operation: "upsert".to_string(),
        row_version,
        payload_json: payload.to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn staging_keeps_the_row_identity_and_chains_versions() {
    let db = setup_db().await;
    let team_id = insert_team(&db, Some("team-sync-1"), "T-1", "Mechanics").await;

    stage(&db).await;
    // Nothing changed: no new envelope.
    stage(&db).await;
    exec(
        &db,
        "UPDATE teams SET name = 'Mechanics B' WHERE id = ?",
        vec![team_id.into()],
    )
    .await;
    stage(&db).await;

    let items = queries::list_outbox_items(&db, Default::default())
        .await
        .expect("list outbox");
    assert_eq!(items.len(), 2, "unchanged payload must not create a new envelope");
    assert!(items.iter().all(|i| i.entity_sync_id == "team-sync-1"));
    assert_eq!(items[0].base_row_version, None, "first change is a create");
    assert_eq!(items[0].row_version, 1);
    assert_eq!(items[1].base_row_version, Some(1));
    assert_eq!(items[1].row_version, 2);
    assert_ne!(items[0].idempotency_key, items[1].idempotency_key);
    let replica = queries::local_replica_id(&db).await.expect("replica id");
    assert!(items
        .iter()
        .all(|i| i.origin_machine_id.as_deref() == Some(replica.as_str())));
}

#[tokio::test]
async fn a_row_created_without_identity_gets_one_at_insert() {
    let db = setup_db().await;
    let team_id = insert_team(&db, None, "T-NOID", "No identity").await;
    let assigned = scalar_string(&db, "SELECT entity_sync_id FROM teams WHERE id = ?", vec![team_id.into()]).await;
    assert!(uuid::Uuid::parse_str(&assigned).is_ok(), "{assigned}");
    stage(&db).await;
    let staged = scalar_string(&db, "SELECT entity_sync_id FROM sync_outbox", vec![]).await;
    assert_eq!(staged, assigned, "staging uses the identity given at insert, never a new one");
}

#[tokio::test]
async fn capture_belongs_to_the_business_transaction() {
    let db = setup_db().await;
    let tx = db.begin().await.expect("begin");
    insert_team(&tx, Some("team-tx"), "T-TX", "Transactional").await;
    let captured = scalar_i64(&tx, "SELECT COUNT(*) FROM sync_capture_journal", vec![]).await;
    assert_eq!(captured, 1, "the write is captured in its own transaction");
    tx.rollback().await.expect("rollback");
    let teams = scalar_i64(&db, "SELECT COUNT(*) FROM teams WHERE code = 'T-TX'", vec![]).await;
    let journal = scalar_i64(&db, "SELECT COUNT(*) FROM sync_capture_journal", vec![]).await;
    assert_eq!((teams, journal), (0, 0), "a rolled-back write leaves nothing to send");
}

#[tokio::test]
async fn push_marks_in_flight_and_ack_records_server_version() {
    let db = setup_db().await;
    let team_id = insert_team(&db, Some("team-sync-ack"), "T-ACK", "Ack").await;

    let push = queries::get_sync_push_payload(&db, Some(50))
        .await
        .expect("push payload");
    assert_eq!(push.outbox_batch.len(), 1);
    let item = &push.outbox_batch[0];
    assert_eq!(item.status, "in_flight");
    assert_eq!(item.attempt_count, 1);

    let summary = queries::get_sync_state_summary(&db).await.expect("summary");
    assert_eq!(summary.in_flight_outbox_count, 1);
    assert_eq!(summary.pending_outbox_count, 0);

    // The exchange failed: the same envelope is sent again.
    let again = queries::get_sync_push_payload(&db, Some(50)).await.expect("push again");
    assert_eq!(again.outbox_batch[0].idempotency_key, item.idempotency_key);
    assert_eq!(again.outbox_batch[0].attempt_count, 2);

    let mut ack = batch("srv-ack", "ckpt-ack-1", vec![]);
    ack.acknowledged_items = vec![SyncAckInput {
        idempotency_key: item.idempotency_key.clone(),
        entity_sync_id: item.entity_sync_id.clone(),
        operation: item.operation.clone(),
        ..Default::default()
    }];
    let result = queries::apply_sync_batch(&db, ack).await.expect("apply ack");
    assert_eq!(result.acknowledged_count, 1);

    // The next local change is based on the acknowledged version.
    exec(
        &db,
        "UPDATE teams SET name = 'Ack 2' WHERE id = ?",
        vec![team_id.into()],
    )
    .await;
    stage(&db).await;
    let pending = queries::list_outbox_items(
        &db,
        super::domain::ListOutboxFilter {
            status: Some("pending".into()),
            limit: Some(10),
        },
    )
    .await
    .expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].base_row_version, Some(1));
    assert_eq!(pending[0].row_version, 2);
}

#[tokio::test]
async fn repeated_delivery_failures_dead_letter_with_a_conflict() {
    let db = setup_db().await;
    insert_team(&db, Some("team-sync-dl"), "T-DL", "Dead letter").await;
    let mut dead = 0;
    for _ in 0..queries::OUTBOX_MAX_DELIVERY_ATTEMPTS {
        let push = queries::get_sync_push_payload(&db, Some(10)).await.expect("push");
        let ids: Vec<i64> = push.outbox_batch.iter().map(|i| i.id).collect();
        dead += queries::record_outbox_delivery_failure(&db, &ids, "network unreachable")
            .await
            .expect("record failure");
    }
    assert_eq!(dead, 1);
    let summary = queries::get_sync_state_summary(&db).await.expect("summary");
    assert_eq!(summary.dead_letter_outbox_count, 1);
    assert_eq!(summary.unresolved_conflict_count, 1);
}

#[tokio::test]
async fn unknown_types_and_rows_missing_a_parent_stay_deferred() {
    let db = setup_db().await;
    queries::apply_sync_batch(
        &db,
        batch(
            "srv-def",
            "ckpt-def-1",
            vec![
                inbound(
                    "stock_locations",
                    "loc-waiting",
                    1,
                    serde_json::json!({
                        "_sync_format": FORMAT, "warehouse_sync_id": "wh-not-received", "code": "LOC-W", "name": "Bin W",
                    }),
                ),
                inbound("future_entity", "future-1", 1, serde_json::json!({})),
            ],
        ),
    )
    .await
    .expect("ingest");
    let result = materialize_inbox_batch(&db, Some(50)).await.expect("materialize");
    assert_eq!(result.applied, 0);
    assert_eq!(result.deferred, 2);
    let applied = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM sync_inbox WHERE apply_status = 'applied'",
        vec![],
    )
    .await;
    assert_eq!(applied, 0, "rows that were not applied must never be marked applied");
    let summary = queries::get_sync_state_summary(&db).await.expect("summary");
    assert_eq!(summary.deferred_inbox_count, 2);
}

#[tokio::test]
async fn child_received_before_its_parent_is_applied_when_the_parent_arrives() {
    let db = setup_db().await;
    queries::apply_sync_batch(
        &db,
        batch(
            "srv-child",
            "ckpt-ord-1",
            vec![inbound(
                "stock_locations",
                "loc-x",
                1,
                serde_json::json!({
                    "_sync_format": FORMAT, "warehouse_sync_id": "wh-x", "code": "LOC-X", "name": "Bin X",
                    "is_default": 0, "is_active": 1,
                    "created_at": "2026-10-10T00:00:00Z", "updated_at": "2026-10-10T00:00:00Z"
                }),
            )],
        ),
    )
    .await
    .expect("ingest child");
    let first = materialize_inbox_batch(&db, Some(50)).await.expect("materialize child");
    assert_eq!(first.applied, 0);
    assert_eq!(first.deferred, 1);

    queries::apply_sync_batch(
        &db,
        batch(
            "srv-parent",
            "ckpt-ord-2",
            vec![inbound(
                "warehouses",
                "wh-x",
                1,
                serde_json::json!({
                    "_sync_format": FORMAT, "code": "WH-X", "name": "Warehouse X", "is_active": 1,
                    "created_at": "2026-10-10T00:00:00Z", "updated_at": "2026-10-10T00:00:00Z"
                }),
            )],
        ),
    )
    .await
    .expect("ingest parent");
    let second = materialize_inbox_batch(&db, Some(50))
        .await
        .expect("materialize parent");
    assert_eq!(second.applied, 2, "parent and the deferred child are both applied");
    assert_eq!(second.deferred, 0);
    let located = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM stock_locations l JOIN warehouses w ON w.id = l.warehouse_id
         WHERE l.entity_sync_id = 'loc-x' AND w.entity_sync_id = 'wh-x'",
        vec![],
    )
    .await;
    assert_eq!(located, 1);
}

#[tokio::test]
async fn materialize_failures_are_listed_as_conflicts() {
    let db = setup_db().await;
    queries::apply_sync_batch(
        &db,
        batch(
            "srv-err",
            "ckpt-err-1",
            vec![inbound(
                "user_accounts",
                "user-x",
                1,
                // Required username missing: the insert fails and becomes a conflict, never a guess.
                serde_json::json!({ "_sync_format": FORMAT, "display_name": "No username" }),
            )],
        ),
    )
    .await
    .expect("ingest");
    let result = materialize_inbox_batch(&db, Some(50)).await.expect("materialize");
    assert_eq!(result.conflicts, 1);
    let conflicts = queries::list_sync_conflicts(
        &db,
        SyncConflictFilter {
            conflict_type: Some("MATERIALIZE_ERROR".into()),
            ..Default::default()
        },
    )
    .await
    .expect("list conflicts");
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].inbound_row_version, Some(1));
    let summary = queries::get_sync_state_summary(&db).await.expect("summary");
    assert_eq!(summary.unresolved_conflict_count, 1);
    assert_eq!(summary.inbox_conflict_count, 1);
}

/// Local team edited (unsent) while the server sends another device's edit of the same team.
async fn concurrent_team_edit(db: &DatabaseConnection) -> super::domain::SyncConflictRecord {
    insert_team(db, Some("team-conc"), "T-CONC", "Local name").await;
    queries::apply_sync_batch(
        db,
        batch(
            "srv-conc",
            "ckpt-conc-1",
            vec![inbound(
                "teams",
                "team-conc",
                2,
                serde_json::json!({
                    "_sync_format": FORMAT, "code": "T-CONC", "name": "Remote name",
                    "team_type": "maintenance", "status": "active"
                }),
            )],
        ),
    )
    .await
    .expect("ingest remote edit");
    let result = materialize_inbox_batch(db, Some(50)).await.expect("materialize");
    assert_eq!(result.conflicts, 1, "a concurrent edit is never applied silently");
    let name = scalar_string(db, "SELECT name FROM teams WHERE entity_sync_id = 'team-conc'", vec![]).await;
    assert_eq!(name, "Local name");
    queries::list_sync_conflicts(
        db,
        SyncConflictFilter {
            conflict_type: Some("CONCURRENT_EDIT".into()),
            ..Default::default()
        },
    )
    .await
    .expect("list conflicts")
    .pop()
    .expect("concurrent edit conflict")
}

#[tokio::test]
async fn take_theirs_applies_the_server_version() {
    let db = setup_db().await;
    let conflict = concurrent_team_edit(&db).await;
    let resolved = queries::resolve_sync_conflict(
        &db,
        1,
        ResolveSyncConflictInput {
            conflict_id: conflict.id,
            expected_row_version: conflict.row_version,
            action: "accept_remote".into(),
            resolution_note: None,
        },
    )
    .await
    .expect("take theirs");
    assert_eq!(resolved.status, "resolved_remote");
    let name = scalar_string(&db, "SELECT name FROM teams WHERE entity_sync_id = 'team-conc'", vec![]).await;
    assert_eq!(name, "Remote name");
    let unsent = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id = 'team-conc' AND status IN ('pending', 'in_flight')",
        vec![],
    )
    .await;
    assert_eq!(unsent, 0, "the replaced local change is not sent");
    let inbox = scalar_string(
        &db,
        "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = 'team-conc'",
        vec![],
    )
    .await;
    assert_eq!(inbox, "applied");
}

#[tokio::test]
async fn keep_mine_restages_the_local_row_on_the_server_version() {
    let db = setup_db().await;
    let conflict = concurrent_team_edit(&db).await;
    let resolved = queries::resolve_sync_conflict(
        &db,
        1,
        ResolveSyncConflictInput {
            conflict_id: conflict.id,
            expected_row_version: conflict.row_version,
            action: "accept_local".into(),
            resolution_note: None,
        },
    )
    .await
    .expect("keep mine");
    assert_eq!(resolved.status, "resolved_local");
    let name = scalar_string(&db, "SELECT name FROM teams WHERE entity_sync_id = 'team-conc'", vec![]).await;
    assert_eq!(name, "Local name");
    let pending = queries::list_outbox_items(
        &db,
        super::domain::ListOutboxFilter {
            status: Some("pending".into()),
            limit: Some(10),
        },
    )
    .await
    .expect("pending outbox");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].base_row_version, Some(2), "built on the server version");
    assert_eq!(pending[0].row_version, 3);
    assert!(pending[0].payload_json.contains("Local name"));
    let superseded = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM sync_outbox WHERE entity_sync_id = 'team-conc' AND status = 'superseded'",
        vec![],
    )
    .await;
    assert_eq!(superseded, 1);
}

#[tokio::test]
async fn equal_version_with_different_content_is_a_conflict() {
    let db = setup_db().await;
    insert_team(&db, Some("team-eq"), "T-EQ", "Same").await;
    let push = queries::get_sync_push_payload(&db, Some(10)).await.expect("push");
    let item = push.outbox_batch[0].clone();
    let mut ack = batch("srv-eq-ack", "ckpt-eq-1", vec![]);
    ack.acknowledged_items = vec![SyncAckInput {
        idempotency_key: item.idempotency_key.clone(),
        entity_sync_id: item.entity_sync_id.clone(),
        operation: item.operation.clone(),
        ..Default::default()
    }];
    queries::apply_sync_batch(&db, ack).await.expect("ack");

    // Server claims version 1 with other content.
    queries::apply_sync_batch(
        &db,
        batch(
            "srv-eq",
            "ckpt-eq-2",
            vec![inbound(
                "teams",
                "team-eq",
                1,
                serde_json::json!({ "_sync_format": FORMAT, "code": "T-EQ", "name": "Other" }),
            )],
        ),
    )
    .await
    .expect("ingest");
    let result = materialize_inbox_batch(&db, Some(10)).await.expect("materialize");
    assert_eq!(result.conflicts, 1);
    let kind = scalar_string(&db, "SELECT conflict_type FROM sync_conflicts", vec![]).await;
    assert_eq!(kind, "EQUAL_VERSION_DIVERGENCE");

    // Our own acknowledged change echoed back is simply current.
    queries::apply_sync_batch(
        &db,
        batch(
            "srv-eq-echo",
            "ckpt-eq-3",
            vec![SyncInboundItemInput {
                entity_type: "teams".into(),
                entity_sync_id: "team-eq".into(),
                operation: item.operation.clone(),
                row_version: item.row_version,
                payload_json: item.payload_json.clone(),
                ..Default::default()
            }],
        ),
    )
    .await
    .expect("ingest echo");
    let echo = materialize_inbox_batch(&db, Some(10)).await.expect("materialize echo");
    assert_eq!(echo.skipped, 1);
    assert_eq!(echo.conflicts, 0);
}

#[tokio::test]
async fn an_update_of_device_local_columns_only_is_not_captured() {
    use super::test_support::{exec, scalar_i64 as count, started_db};
    let db = started_db().await;
    stage(&db).await;
    let journal = "SELECT COUNT(*) FROM sync_capture_journal";
    exec(
        &db,
        "UPDATE user_accounts SET last_login_at = '2026-10-11T08:00:00Z', failed_login_attempts = 2",
        vec![],
    )
    .await;
    assert_eq!(count(&db, journal, vec![]).await, 0, "a login on this device is not a change to send");
    exec(&db, "UPDATE user_accounts SET display_name = 'Administrateur'", vec![]).await;
    assert_eq!(count(&db, journal, vec![]).await, 1, "a change of a synchronised column is captured");
}

#[tokio::test]
async fn many_children_received_before_their_parents_are_applied_parents_first_in_one_call() {
    let db = setup_db().await;
    // More waiting children than one pass takes, stored before their parents (as snapshot pages
    // in entity type order deliver them): the older loop re-read the same waiting rows forever.
    let mut items = Vec::new();
    for i in 0..30 {
        items.push(inbound(
            "stock_locations",
            &format!("loc-{i}"),
            1,
            serde_json::json!({
                "_sync_format": FORMAT, "warehouse_sync_id": format!("wh-{i}"), "code": format!("LOC-{i}"),
                "name": format!("Bin {i}"), "is_default": 0, "is_active": 1,
                "created_at": "2026-10-10T00:00:00Z", "updated_at": "2026-10-10T00:00:00Z"
            }),
        ));
    }
    for i in 0..30 {
        items.push(inbound(
            "warehouses",
            &format!("wh-{i}"),
            1,
            serde_json::json!({
                "_sync_format": FORMAT, "code": format!("WH-{i}"), "name": format!("Warehouse {i}"), "is_active": 1,
                "created_at": "2026-10-10T00:00:00Z", "updated_at": "2026-10-10T00:00:00Z"
            }),
        ));
    }
    queries::apply_sync_batch(&db, batch("srv-many", "ckpt-many", items))
        .await
        .expect("ingest");
    let result = super::materialize::materialize_inbox(&db, 10, true)
        .await
        .expect("materialize");
    assert_eq!(result.applied, 60, "{result:?}");
    assert_eq!(result.processed, 60, "each row tried once: parents are taken first");
    assert_eq!(result.deferred, 0);
    let linked = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM stock_locations l JOIN warehouses w ON w.id = l.warehouse_id
         WHERE substr(l.entity_sync_id, 5) = substr(w.entity_sync_id, 4)",
        vec![],
    )
    .await;
    assert_eq!(linked, 30);
}
