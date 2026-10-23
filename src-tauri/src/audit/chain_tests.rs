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

/// `"L2:" ‖ SHA-256(prev ‖ "\n" ‖ digest)`: the stub-era hash, computed here independently of
/// `audit::chain`.
fn stub_era(prev: Option<&str>, digest: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(prev.unwrap_or("").as_bytes());
    hasher.update(b"\n");
    hasher.update(digest.as_bytes());
    format!("L2:{}", hex::encode(hasher.finalize()))
}

/// Puts a stub of the other device's chain at `seq` in place of the one received (stubs are
/// never updated: removed and inserted again; there is no delete trigger).
async fn replace_stub(db: &DatabaseConnection, seq: i64, prev: Option<String>, hash: String, digest: Option<String>) {
    remove_other(db, "audit_chain_stubs", seq).await;
    exec(
        db,
        &format!(
            "INSERT INTO audit_chain_stubs \
             (entity_sync_id, chain_origin, chain_seq, chain_prev_hash, chain_hash, content_digest) \
             VALUES ('forged-' || ?, (SELECT chain_origin FROM audit_events WHERE {OTHER} LIMIT 1), ?, ?, ?, ?)"
        ),
        vec![seq.into(), seq.into(), prev.into(), hash.into(), digest.into()],
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
         AND s.chain_hash LIKE 'L2:%' AND length(s.content_digest) = 64",
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
    // A stub holds nothing but the chain position and the content digest.
    let other_columns = scalar_i64(
        &b,
        "SELECT COUNT(*) FROM pragma_table_info('audit_chain_stubs') \
         WHERE name NOT IN ('id', 'entity_sync_id', 'chain_origin', 'chain_seq', 'chain_prev_hash', \
         'chain_hash', 'content_digest')",
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
    replace_stub(&b, 3, prev_3.clone(), hash_3, None).await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.audit_event_id, problem.kind),
        (3, None, ChainProblemKind::HashMismatch)
    );

    // A local-form hash its digest does not give.
    replace_stub(
        &b,
        3,
        prev_3.clone(),
        format!("L2:{}", "a".repeat(64)),
        Some("d".repeat(64)),
    )
    .await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.audit_event_id, problem.kind),
        (3, None, ChainProblemKind::HashMismatch)
    );

    // A consistent forged stub: the next event still points to the travelling event's hash.
    let digest = "d".repeat(64);
    replace_stub(
        &b,
        3,
        prev_3.clone(),
        stub_era(prev_3.as_deref(), &digest),
        Some(digest),
    )
    .await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!((problem.chain_seq, problem.kind), (4, ChainProblemKind::PrevMismatch));
    assert!(problem.audit_event_id.is_some());
}

/// Review B1: deleting the travelling event just before a kept position, forging a stub in its
/// place and relinking the kept position's stub to it must not verify.
#[tokio::test]
async fn a_travelling_event_hidden_by_relinking_the_next_stub_is_detected() {
    let (_a, b) = a_then_b(&[false, true, false]).await;
    let hash_2 = other_text(&b, "audit_chain_stubs", "chain_hash", 2)
        .await
        .expect("stub hash");
    let digest_2 = other_text(&b, "audit_chain_stubs", "content_digest", 2).await;
    assert!(chain_of_other(&b).await.intact);

    // 1. Delete E1. 2. A forged, self-consistent stub at 1. 3. Stub 2 relinked to it, its hash
    // and digest kept (E3 still points to that hash).
    remove_other(&b, "audit_events", 1).await;
    let forged_digest = "e".repeat(64);
    let forged_hash = stub_era(None, &forged_digest);
    replace_stub(&b, 1, None, forged_hash.clone(), Some(forged_digest)).await;
    replace_stub(&b, 2, Some(forged_hash), hash_2, digest_2).await;

    let from_a = chain_of_other(&b).await;
    assert!(!from_a.intact, "{from_a:?}");
    let problem = from_a.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.audit_event_id, problem.kind),
        (2, None, ChainProblemKind::HashMismatch)
    );
}

#[tokio::test]
async fn a_changed_or_removed_stub_is_detected() {
    let (_a, b) = a_then_b(&[false, true, false, false]).await;
    let hash_2 = other_text(&b, "audit_chain_stubs", "chain_hash", 2)
        .await
        .expect("stub hash");
    let prev_2 = other_text(&b, "audit_chain_stubs", "chain_prev_hash", 2).await;
    let digest_2 = other_text(&b, "audit_chain_stubs", "content_digest", 2).await;

    // Changed digest, hash recomputed from it: the next event no longer points to it.
    let other_digest = "f".repeat(64);
    replace_stub(
        &b,
        2,
        prev_2.clone(),
        stub_era(prev_2.as_deref(), &other_digest),
        Some(other_digest),
    )
    .await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!((problem.chain_seq, problem.kind), (3, ChainProblemKind::PrevMismatch));

    // Changed hash only: it no longer matches its own link and digest.
    let digest = &hash_2[3..];
    let changed = format!("L2:{}{}", if digest.starts_with('0') { "1" } else { "0" }, &digest[1..]);
    replace_stub(&b, 2, prev_2.clone(), changed, digest_2.clone()).await;
    let problem = chain_of_other(&b).await.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.audit_event_id, problem.kind),
        (2, None, ChainProblemKind::HashMismatch)
    );

    // Changed link to the previous position.
    replace_stub(&b, 2, Some("b".repeat(64)), hash_2.clone(), digest_2.clone()).await;
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
    replace_stub(&b, 2, prev_2, hash_2, digest_2).await;
    assert!(chain_of_other(&b).await.intact);
}

/// Turns this database's kept position `seq` into one written by the previous version: hashed
/// `L1:`, no stub.
async fn as_before_stubs(db: &DatabaseConnection, seq: i64) {
    exec(db, "DELETE FROM audit_chain_stubs", vec![]).await;
    let sync_id = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM audit_events WHERE chain_seq = ?",
            [seq.into()],
        ))
        .await
        .expect("query")
        .expect("event")
        .try_get_by_index::<String>(0)
        .expect("sync id");
    crate::audit::chain::reseal_before_stubs(db, &sync_id)
        .await
        .expect("reseal");
}

#[tokio::test]
async fn one_position_kept_before_stubs_existed_gets_its_own_status() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    write_sequence(&a, &[false, true]).await;
    as_before_stubs(&a, 2).await;
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

/// Review S1: a run of two missing positions before an `L1:` pointer is not "kept": the first
/// one may be a removed travelling event.
#[tokio::test]
async fn a_longer_run_before_an_older_kept_position_is_not_intact() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    write_sequence(&a, &[false, true]).await;
    as_before_stubs(&a, 2).await;
    write_sequence(&a, &[false]).await;
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    assert!(chain_of_other(&b).await.intact);

    remove_other(&b, "audit_events", 1).await;
    let from_a = chain_of_other(&b).await;
    assert!(!from_a.intact, "{from_a:?}");
    assert_eq!(from_a.kept_before_stubs, 0);
    let problem = from_a.first_problem.expect("problem");
    assert_eq!(
        (problem.chain_seq, problem.kind),
        (3, ChainProblemKind::MissingPositions)
    );
}

#[tokio::test]
async fn migration_250_gives_this_pcs_older_kept_events_their_stub() {
    let db = started_db().await;
    write_sequence(&db, &[false, true]).await;
    as_before_stubs(&db, 2).await;
    // An `L1:` event of another PC (one that travelled by mistake) gets no stub here.
    exec(
        &db,
        "INSERT INTO audit_events (entity_sync_id, action_code, auth_context, result, retention_class, \
         happened_at, chain_origin, chain_seq, chain_hash) VALUES ('other-l1', 'x.y', 'system', 'success', \
         'standard', '2026-01-01T00:00:00Z', 'another-pc', 1, 'L1:' || hex(randomblob(32)))",
        vec![],
    )
    .await;
    exec(&db, &crate::migrations::audit_chain_stubs_backfill_sql(), vec![]).await;
    let stub = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM audit_chain_stubs s JOIN audit_events e \
         ON e.chain_origin = s.chain_origin AND e.chain_seq = s.chain_seq AND e.chain_hash = s.chain_hash \
         AND e.chain_prev_hash = s.chain_prev_hash \
         WHERE s.chain_seq = 2 AND s.chain_hash LIKE 'L1:%' AND s.content_digest IS NULL \
         AND length(s.entity_sync_id) = 36",
        vec![],
    )
    .await;
    assert_eq!(stub, 1);
    let total = scalar_i64(&db, "SELECT COUNT(*) FROM audit_chain_stubs", vec![]).await;
    assert_eq!(total, 1, "travelling events and other PCs' events get no stub");
}

// ── Stubs received by synchronisation (review S2) ───────────────────────────────────────────

fn stub_payload(
    origin: &str,
    seq: i64,
    prev: Option<&str>,
    hash: &str,
    digest: Option<&str>,
) -> serde_json::Map<String, serde_json::Value> {
    let value = serde_json::json!({
        "chain_origin": origin,
        "chain_seq": seq,
        "chain_prev_hash": prev,
        "chain_hash": hash,
        "content_digest": digest,
    });
    value.as_object().expect("object").clone()
}

#[tokio::test]
async fn received_stubs_are_checked_on_arrival() {
    use crate::audit::chain::inbound_stub_check;
    let db = started_db().await;
    let own = crate::sync::queries::local_replica_id(&db).await.expect("replica id");
    let d = "a".repeat(64);
    let p = "c".repeat(64);
    let good = stub_era(Some(&p), &d);
    let check = |payload| {
        let db = &db;
        async move { inbound_stub_check(db, &payload).await.expect("check") }
    };
    assert_eq!(check(stub_payload("other", 2, Some(&p), &good, Some(&d))).await, None);
    let first = stub_era(None, &d);
    assert_eq!(check(stub_payload("other", 1, None, &first, Some(&d))).await, None);
    let older = format!("L1:{d}");
    assert_eq!(check(stub_payload("other", 2, Some(&p), &older, None)).await, None);

    let refused = [
        ("this PC's own chain", stub_payload(&own, 2, Some(&p), &good, Some(&d))),
        ("position 0", stub_payload("other", 0, None, &first, Some(&d))),
        ("no link after 1", stub_payload("other", 2, None, &good, Some(&d))),
        ("a link at 1", stub_payload("other", 1, Some(&p), &good, Some(&d))),
        ("malformed link", stub_payload("other", 2, Some("x"), &good, Some(&d))),
        ("not a kept hash", stub_payload("other", 2, Some(&p), &d, Some(&d))),
        ("no digest", stub_payload("other", 2, Some(&p), &good, None)),
        (
            "malformed digest",
            stub_payload("other", 2, Some(&p), &good, Some("zz")),
        ),
        (
            "digest on an older stub",
            stub_payload("other", 2, Some(&p), &older, Some(&d)),
        ),
    ];
    for (what, payload) in refused {
        assert!(check(payload).await.is_some(), "{what} must be refused");
    }
}

#[tokio::test]
async fn a_stub_for_this_pcs_own_chain_is_refused_and_never_blocks_its_kept_events() {
    let mut server = TestServer::new();
    let a = started_device_db().await;
    let b = started_device_db().await;
    write_sequence(&b, &[false]).await;
    let b_origin = crate::sync::queries::local_replica_id(&b).await.expect("replica id");
    // A (modified, or a copy) sends a stub for B's next position.
    let d = "a".repeat(64);
    let prev = other_text_own(&b).await;
    exec(
        &a,
        "INSERT INTO audit_chain_stubs \
         (entity_sync_id, chain_origin, chain_seq, chain_prev_hash, chain_hash, content_digest) \
         VALUES ('stub-for-b', ?, 2, ?, ?, ?)",
        vec![
            b_origin.clone().into(),
            prev.clone().into(),
            stub_era(prev.as_deref(), &d).into(),
            d.clone().into(),
        ],
    )
    .await;
    server.exchange("A", &a).await;
    let report = server.exchange("B", &b).await;
    assert_eq!(report.materialized.conflicts, 1, "{report:?}");
    let refused = scalar_i64(
        &b,
        "SELECT COUNT(*) FROM sync_conflicts WHERE entity_type = 'audit_chain_stubs' \
         AND conflict_type = 'AUDIT_STUB_REFUSED'",
        vec![],
    )
    .await;
    assert_eq!(refused, 1);
    let held = scalar_i64(&b, "SELECT COUNT(*) FROM audit_chain_stubs", vec![]).await;
    assert_eq!(held, 0);

    // Even with such a row already here (a copied database), B's next kept event is written.
    exec(
        &b,
        "INSERT INTO audit_chain_stubs \
         (entity_sync_id, chain_origin, chain_seq, chain_prev_hash, chain_hash, content_digest) \
         VALUES ('stale-own', ?, 2, NULL, ?, ?)",
        vec![b_origin.into(), format!("L2:{d}").into(), d.into()],
    )
    .await;
    write_audit_event(&b, kept(9)).await.expect("kept event still written");
    assert!(verify_audit_chain(&b).await.expect("verify on B").chains[0].intact);
}

/// This device's last chain hash.
async fn other_text_own(db: &DatabaseConnection) -> Option<String> {
    db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT chain_hash FROM audit_events WHERE chain_seq IS NOT NULL ORDER BY chain_seq DESC LIMIT 1".to_string(),
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get_by_index::<Option<String>>(0)
    .expect("text")
}

/// Hash stored for an audit event.
async fn stored_hash(db: &DatabaseConnection, id: i64) -> String {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT chain_hash FROM audit_events WHERE id = ?",
        [id.into()],
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get_by_index::<String>(0)
    .expect("hash")
}

/// The audits of a work-order type's nature, of a qualification requirement and of an absence
/// block's critical flag travel (sealed over their sync payload); the audits of a session of this
/// PC stay on it (local form). An absence block deleted after its audit leaves the chain intact:
/// the audit names the block by its sync id, which does not depend on the block still existing.
#[tokio::test]
async fn audits_of_types_requirements_and_absence_blocks_travel_and_session_audits_stay() {
    let db = started_db().await;
    let mut factory = RowFactory::new();
    let wo_type = factory.create(&db, "reference_values").await.expect("type");
    let requirement = factory
        .create(&db, "qualification_requirements")
        .await
        .expect("requirement");
    let block = factory
        .create(&db, "personnel_availability_blocks")
        .await
        .expect("block");
    let audit = |target_type: &str, target_id: String| AuditEventInput {
        target_type: Some(target_type.to_string()),
        target_id: Some(target_id),
        ..event("test.target", None, None)
    };
    let travelling = [
        audit("work_order_type", wo_type.rowid.to_string()),
        audit("qualification_requirement", requirement.rowid.to_string()),
        audit("personnel_availability_block", block.entity_sync_id.clone()),
    ];
    for input in travelling {
        let what = input.target_type.clone();
        let id = write_audit_event(&db, input).await.expect("write");
        let hash = stored_hash(&db, id).await;
        assert!(!hash.starts_with("L"), "{what:?} travels: {hash}");
    }
    let session = write_audit_event(&db, audit("app_session", "7".to_string()))
        .await
        .expect("write session audit");
    let hash = stored_hash(&db, session).await;
    assert!(
        hash.starts_with(crate::audit::chain::LOCAL_FORM_PREFIX),
        "a session audit stays on its PC: {hash}"
    );

    exec(
        &db,
        "DELETE FROM personnel_availability_blocks WHERE id = ?",
        vec![block.rowid.into()],
    )
    .await;
    let report = verify_audit_chain(&db).await.expect("verify");
    assert!(report.intact, "{report:?}");
}
