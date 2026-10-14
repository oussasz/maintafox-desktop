//! Audit hash chain: written events chain per device, tampering is found, other devices verify
//! the chain they received.

use sea_orm::{DatabaseConnection, TransactionTrait};

use crate::audit::chain::{verify_audit_chain, ChainProblemKind};
use crate::audit::writer::{write_audit_event, write_audit_event_in, AuditEventInput};
use crate::sync::test_support::{exec, scalar_i64, started_db, started_device_db, RowFactory, TestServer};

fn event(action: &str, target_id: Option<String>, actor_id: Option<i64>) -> AuditEventInput {
    AuditEventInput {
        action_code: action.to_string(),
        target_type: target_id.as_ref().map(|_| "user_account".to_string()),
        target_id,
        actor_id,
        auth_context: "password".to_string(),
        result: "success".to_string(),
        before_hash: None,
        after_hash: None,
        retention_class: "standard".to_string(),
        details_json: Some(serde_json::json!({ "summary": format!("{action} — événement"), "n": 1 })),
    }
}

async fn write_three(db: &DatabaseConnection) -> Vec<i64> {
    let mut ids = Vec::new();
    for action in ["user.created", "role.assigned", "user.password_changed"] {
        ids.push(write_audit_event(db, event(action, None, None)).await.expect("write"));
    }
    ids
}

#[tokio::test]
async fn events_form_an_intact_chain() {
    let db = started_db().await;
    let ids = write_three(&db).await;
    let report = verify_audit_chain(&db).await.expect("verify");
    assert!(report.intact, "{report:?}");
    assert_eq!(report.chains.len(), 1);
    let chain = &report.chains[0];
    assert!(chain.is_this_device);
    assert_eq!((chain.events, chain.last_seq), (3, 3));
    assert_eq!(chain.device_code.len(), 4);
    let first = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM audit_events WHERE id = ? AND chain_seq = 1 AND chain_prev_hash IS NULL \
         AND length(chain_hash) = 64",
        vec![ids[0].into()],
    )
    .await;
    assert_eq!(first, 1);
    let linked = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM audit_events b JOIN audit_events a ON a.chain_seq = b.chain_seq - 1 \
         AND a.chain_origin = b.chain_origin WHERE b.chain_prev_hash = a.chain_hash",
        vec![],
    )
    .await;
    assert_eq!(linked, 2, "positions 2 and 3 point to their predecessor's hash");
}

#[tokio::test]
async fn changed_event_is_reported_at_its_position() {
    let db = started_db().await;
    let ids = write_three(&db).await;
    exec(
        &db,
        "UPDATE audit_events SET result = 'fail' WHERE id = ?",
        vec![ids[1].into()],
    )
    .await;
    let report = verify_audit_chain(&db).await.expect("verify");
    assert!(!report.intact);
    let problem = report.chains[0].first_problem.clone().expect("problem");
    assert_eq!((problem.chain_seq, problem.audit_event_id), (2, ids[1]));
    assert_eq!(problem.kind, ChainProblemKind::HashMismatch);
}

#[tokio::test]
async fn removed_event_is_reported_as_missing_positions() {
    let db = started_db().await;
    let ids = write_three(&db).await;
    exec(&db, "DELETE FROM audit_events WHERE id = ?", vec![ids[1].into()]).await;
    let report = verify_audit_chain(&db).await.expect("verify");
    let problem = report.chains[0].first_problem.clone().expect("problem");
    assert_eq!(problem.kind, ChainProblemKind::MissingPositions);
    assert_eq!(problem.chain_seq, 3);
}

#[tokio::test]
async fn relinked_chain_after_removal_is_still_detected() {
    // Removing position 2 and renumbering position 3 to 2 keeps the sequence but not the links.
    let db = started_db().await;
    let ids = write_three(&db).await;
    exec(&db, "DELETE FROM audit_events WHERE id = ?", vec![ids[1].into()]).await;
    exec(
        &db,
        "UPDATE audit_events SET chain_seq = 2 WHERE id = ?",
        vec![ids[2].into()],
    )
    .await;
    let report = verify_audit_chain(&db).await.expect("verify");
    let problem = report.chains[0].first_problem.clone().expect("problem");
    assert_eq!(problem.kind, ChainProblemKind::PrevMismatch);
}

#[tokio::test]
async fn rolled_back_event_does_not_use_a_position() {
    let db = started_db().await;
    write_audit_event(&db, event("user.created", None, None))
        .await
        .expect("first");
    let txn = db.begin().await.expect("txn");
    write_audit_event_in(&txn, event("personnel.import.apply", None, None))
        .await
        .expect("in txn");
    txn.rollback().await.expect("rollback");
    write_audit_event(&db, event("user.deactivated", None, None))
        .await
        .expect("second");
    let report = verify_audit_chain(&db).await.expect("verify");
    assert!(report.intact, "{report:?}");
    assert_eq!(report.chains[0].last_seq, 2);
}

#[tokio::test]
async fn older_unchained_events_are_counted_not_failed() {
    let db = started_db().await;
    exec(
        &db,
        "INSERT INTO audit_events (action_code, auth_context, result, retention_class, happened_at) \
         VALUES ('legacy.event', 'system', 'success', 'standard', '2026-01-01T00:00:00Z')",
        vec![],
    )
    .await;
    write_audit_event(&db, event("user.created", None, None))
        .await
        .expect("write");
    let report = verify_audit_chain(&db).await.expect("verify");
    assert!(report.intact);
    assert_eq!(report.unchained_events, 1);
}

#[tokio::test]
async fn another_device_verifies_the_chain_it_received() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut factory_a = RowFactory::for_device(1);
    let mut factory_b = RowFactory::for_device(2);
    // Different local ids on the two devices for the same user.
    for _ in 0..3 {
        factory_b.create(&b, "user_accounts").await.expect("user on B");
    }
    let user = factory_a.create(&a, "user_accounts").await.expect("user on A");
    for action in ["role.assigned", "user.deactivated"] {
        write_audit_event(&a, event(action, Some(user.rowid.to_string()), Some(user.rowid)))
            .await
            .expect("write on A");
    }
    write_audit_event(&b, event("user.created", None, None))
        .await
        .expect("write on B");

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");

    let on_b = verify_audit_chain(&b).await.expect("verify on B");
    assert!(on_b.intact, "{on_b:?}");
    assert_eq!(on_b.chains.len(), 2, "B holds its own chain and A's");
    let from_a = on_b.chains.iter().find(|c| !c.is_this_device).expect("A's chain");
    assert_eq!((from_a.events, from_a.last_seq), (2, 2));
    let user_on_b = scalar_i64(
        &b,
        "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
        vec![user.entity_sync_id.clone().into()],
    )
    .await;
    assert_ne!(user_on_b, user.rowid, "the hash does not depend on local ids");
}

#[tokio::test]
async fn events_the_sync_codec_cannot_encode_are_chained_in_local_form() {
    // A target type sync does not map: the event stays on this device and is hashed as stored.
    let db = started_db().await;
    let mut input = event("case.reviewed", Some("42".into()), None);
    input.target_type = Some("case".into());
    let id = write_audit_event(&db, input).await.expect("write");
    write_audit_event(&db, event("user.created", None, None))
        .await
        .expect("next event");
    let prefixed = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM audit_events WHERE id = ? AND chain_hash LIKE 'L1:%'",
        vec![id.into()],
    )
    .await;
    assert_eq!(prefixed, 1);
    assert!(verify_audit_chain(&db).await.expect("verify").intact);

    exec(
        &db,
        "UPDATE audit_events SET target_id = '43' WHERE id = ?",
        vec![id.into()],
    )
    .await;
    let report = verify_audit_chain(&db).await.expect("verify");
    let problem = report.chains[0].first_problem.clone().expect("problem");
    assert_eq!(problem.kind, ChainProblemKind::HashMismatch);
    assert_eq!(problem.audit_event_id, id);
}
