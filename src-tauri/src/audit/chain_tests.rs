//! Audit hash chain: written events chain per device, tampering is found, other devices verify
//! the chain they received.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::audit::chain::{verify_audit_chain, ChainOriginReport, ChainProblemKind};
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
    assert_eq!((problem.chain_seq, problem.audit_event_id), (2, Some(ids[1])));
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
        "SELECT COUNT(*) FROM audit_events WHERE id = ? AND chain_hash LIKE 'L2:%'",
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
    assert_eq!(problem.audit_event_id, Some(id));
}

// ── Positions kept on their PC: hash-only stubs ─────────────────────────────────────────────

/// Not this device's chain (SQL condition on `chain_origin`).
const OTHER: &str = "chain_origin <> (SELECT replica_id FROM sync_replica_identity WHERE id = 1)";

/// An audit kept on its PC by the `audit_events` row filter (`sync::row_spec`).
fn kept(n: usize) -> AuditEventInput {
    let mut input = event("notification.safety_notice_acknowledged", None, None);
    input.details_json = Some(serde_json::json!({ "notice": n, "private": "stays on this PC" }));
    input
}

/// Writes positions in order (`true` = kept on the PC).
async fn write_sequence(db: &DatabaseConnection, kinds: &[bool]) {
    for (i, kept_here) in kinds.iter().enumerate() {
        let input = if *kept_here {
            kept(i)
        } else {
            event(&format!("user.event_{i}"), None, None)
        };
        write_audit_event(db, input).await.expect("write");
    }
}

/// The other device's chain as this database verifies it.
async fn chain_of_other(db: &DatabaseConnection) -> ChainOriginReport {
    verify_audit_chain(db)
        .await
        .expect("verify")
        .chains
        .into_iter()
        .find(|c| !c.is_this_device)
        .expect("the other device's chain")
}

/// A writes the positions, then A and B synchronise.
async fn a_then_b(kinds: &[bool]) -> (DatabaseConnection, DatabaseConnection) {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    write_sequence(&a, kinds).await;
    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");
    (a, b)
}

/// Text of the other device's chain at a position.
async fn other_text(db: &DatabaseConnection, table: &str, column: &str, seq: i64) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT {column} FROM {table} WHERE {OTHER} AND chain_seq = ?"),
        [seq.into()],
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get_by_index::<Option<String>>(0)
    .expect("text")
}

/// Puts a stub of the other device's chain at `seq` in place of the one received (stubs are
/// never updated: removed and inserted again).
async fn replace_stub(db: &DatabaseConnection, seq: i64, prev: Option<String>, hash: String) {
    exec(
        db,
        &format!("DELETE FROM audit_chain_stubs WHERE {OTHER} AND chain_seq = ?"),
        vec![seq.into()],
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO audit_chain_stubs (entity_sync_id, chain_origin, chain_seq, chain_prev_hash, chain_hash) \
             VALUES ('forged-' || ?, (SELECT chain_origin FROM audit_events WHERE {OTHER} LIMIT 1), ?, ?, ?)"
        ),
        vec![seq.into(), seq.into(), prev.into(), hash.into()],
    )
    .await;
}

async fn remove_other(db: &DatabaseConnection, table: &str, seq: i64) {
    exec(
        db,
        &format!("DELETE FROM {table} WHERE {OTHER} AND chain_seq = ?"),
        vec![seq.into()],
    )
    .await;
}

#[tokio::test]
async fn a_kept_event_between_travelling_ones_verifies_on_another_pc() {
    let (a, b) = a_then_b(&[false, true, false, false]).await;

    // On A: the kept event is chained in its local form with its stub; A verifies its events.
    let on_a = verify_audit_chain(&a).await.expect("verify on A");
    assert!(on_a.intact, "{on_a:?}");
    assert_eq!(on_a.chains[0].kept_positions, 0, "A verifies its own events, not stubs");
    let stub_on_a = scalar_i64(
        &a,
        "SELECT COUNT(*) FROM audit_chain_stubs s JOIN audit_events e \
         ON e.chain_origin = s.chain_origin AND e.chain_seq = s.chain_seq \
         WHERE s.chain_seq = 2 AND s.chain_hash = e.chain_hash AND s.chain_prev_hash = e.chain_prev_hash \
         AND s.chain_hash LIKE 'L2:%'",
        vec![],
    )
    .await;
    assert_eq!(stub_on_a, 1);
    let stubs_on_a = scalar_i64(&a, "SELECT COUNT(*) FROM audit_chain_stubs", vec![]).await;
    assert_eq!(stubs_on_a, 1, "travelling events get no stub");

    // On B: the event itself never arrived, only its stub; the chain links across it.
    let kept_on_b = scalar_i64(
        &b,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = 'notification.safety_notice_acknowledged'",
        vec![],
    )
    .await;
    assert_eq!(kept_on_b, 0, "the kept event never travels");
    let from_a = chain_of_other(&b).await;
    assert!(from_a.intact, "{from_a:?}");
    assert_eq!(
        (
            from_a.events,
            from_a.last_seq,
            from_a.kept_positions,
            from_a.kept_before_stubs
        ),
        (3, 4, 1, 0)
    );
    // A stub holds nothing but the chain position.
    let other_columns = scalar_i64(
        &b,
        "SELECT COUNT(*) FROM pragma_table_info('audit_chain_stubs') \
         WHERE name NOT IN ('id', 'entity_sync_id', 'chain_origin', 'chain_seq', 'chain_prev_hash', 'chain_hash')",
        vec![],
    )
    .await;
    assert_eq!(other_columns, 0);
}

#[tokio::test]
async fn a_removed_travelling_event_is_still_detected_with_stubs() {
    let (_a, b) = a_then_b(&[false, true, false, false]).await;
    remove_other(&b, "audit_events", 3).await;
    let from_a = chain_of_other(&b).await;
    assert!(!from_a.intact);
    let problem = from_a.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.kind),
        (4, ChainProblemKind::MissingPositions)
    );
}

#[tokio::test]
async fn a_travelling_event_replaced_by_a_forged_stub_is_detected() {
    let (_a, b) = a_then_b(&[false, true, false, false]).await;
    let hash_3 = other_text(&b, "audit_events", "chain_hash", 3).await.expect("hash 3");
    let prev_3 = other_text(&b, "audit_events", "chain_prev_hash", 3).await;
    remove_other(&b, "audit_events", 3).await;

    // A stub claiming the travelling event's own hash: not a local-form hash.
    replace_stub(&b, 3, prev_3.clone(), hash_3).await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.audit_event_id, problem.kind),
        (3, None, ChainProblemKind::HashMismatch)
    );

    // A stub with a local-form hash: the next event still points to the travelling event's hash.
    replace_stub(&b, 3, prev_3, format!("L2:{}", "a".repeat(64))).await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!((problem.chain_seq, problem.kind), (4, ChainProblemKind::PrevMismatch));
    assert!(problem.audit_event_id.is_some());
}

#[tokio::test]
async fn a_changed_or_removed_stub_is_detected() {
    let (_a, b) = a_then_b(&[false, true, false, false]).await;
    let hash_2 = other_text(&b, "audit_chain_stubs", "chain_hash", 2)
        .await
        .expect("stub hash");
    let prev_2 = other_text(&b, "audit_chain_stubs", "chain_prev_hash", 2).await;

    // Changed hash (still a well-formed local hash): the next event no longer points to it.
    let digest = &hash_2[3..];
    let changed = format!("L2:{}{}", if digest.starts_with('0') { "1" } else { "0" }, &digest[1..]);
    replace_stub(&b, 2, prev_2.clone(), changed).await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!((problem.chain_seq, problem.kind), (3, ChainProblemKind::PrevMismatch));

    // Changed link to the previous position.
    replace_stub(&b, 2, Some("b".repeat(64)), hash_2.clone()).await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.audit_event_id, problem.kind),
        (2, None, ChainProblemKind::PrevMismatch)
    );

    // Removed stub: a missing position (this chain has stubs), never "kept before stubs".
    remove_other(&b, "audit_chain_stubs", 2).await;
    let from_a = chain_of_other(&b).await;
    assert_eq!(from_a.kept_before_stubs, 0);
    let problem = from_a.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.kind),
        (3, ChainProblemKind::MissingPositions)
    );

    // Put back as received: intact again.
    replace_stub(&b, 2, prev_2, hash_2).await;
    assert!(chain_of_other(&b).await.intact);
}

/// Turns A's kept position 2 into one written by the previous version: hashed `L1:`, no stub.
async fn as_before_stubs(a: &DatabaseConnection) {
    exec(a, "DELETE FROM audit_chain_stubs", vec![]).await;
    exec(
        a,
        "UPDATE audit_events SET chain_hash = 'L1:' || substr(chain_hash, 4) WHERE chain_seq = 2",
        vec![],
    )
    .await;
}

#[tokio::test]
async fn positions_kept_before_stubs_existed_get_their_own_status() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    write_sequence(&a, &[false, true]).await;
    as_before_stubs(&a).await;
    write_sequence(&a, &[false, false]).await;
    // The older local form still verifies on A itself.
    assert!(verify_audit_chain(&a).await.expect("verify on A").intact);

    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 0, "{report:?}");
    assert_eq!(
        scalar_i64(&b, "SELECT COUNT(*) FROM audit_chain_stubs", vec![]).await,
        0
    );
    let from_a = chain_of_other(&b).await;
    assert!(from_a.intact, "a distinct status, not a failure: {from_a:?}");
    assert_eq!(
        (
            from_a.events,
            from_a.last_seq,
            from_a.kept_positions,
            from_a.kept_before_stubs
        ),
        (3, 4, 0, 1)
    );

    // A travelling event removed from that chain is still found: the next position does not
    // point to a local-form hash.
    remove_other(&b, "audit_events", 3).await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.kind),
        (4, ChainProblemKind::MissingPositions)
    );
}

#[tokio::test]
async fn migration_250_gives_older_kept_events_their_stub() {
    let db = started_db().await;
    write_sequence(&db, &[false, true, false]).await;
    as_before_stubs(&db).await;
    exec(&db, &crate::migrations::audit_chain_stubs_backfill_sql(), vec![]).await;
    let stub = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM audit_chain_stubs s JOIN audit_events e \
         ON e.chain_origin = s.chain_origin AND e.chain_seq = s.chain_seq AND e.chain_hash = s.chain_hash \
         AND e.chain_prev_hash = s.chain_prev_hash \
         WHERE s.chain_seq = 2 AND s.chain_hash LIKE 'L1:%' AND length(s.entity_sync_id) = 36",
        vec![],
    )
    .await;
    assert_eq!(stub, 1);
    let total = scalar_i64(&db, "SELECT COUNT(*) FROM audit_chain_stubs", vec![]).await;
    assert_eq!(total, 1, "travelling events get no stub");
}
