//! Permit, LOTO, work-order start and qualification-override rules re-checked on rows received by
//! synchronisation (`permit::sync_check`, `qualification::sync_check`), through the real apply path
//! (`sync::row_codec::apply_row`, and the inbox pass for rows that arrive together).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Map, Value};

use super::tests::{
    add_isolation, advance_to_issued, apply_and_verify, assigned_wo_requiring_permit, create_from_wo, create_permit,
    insert_equipment, set_status, setup, APPLIER, VERIFIER,
};
use crate::permit::domain::WorkPermit;
use crate::permit::queries;
use crate::sync::materialize::ApplyOutcome;

type Payload = Map<String, Value>;

async fn text(db: &DatabaseConnection, sql: &str) -> Option<String> {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten())
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("exec");
}

async fn user_sync(db: &DatabaseConnection, id: i64) -> String {
    text(db, &format!("SELECT entity_sync_id FROM user_accounts WHERE id = {id}"))
        .await
        .expect("user sync id")
}

/// The row as this device would send it.
async fn wire(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    super::sync_check::local_wire(db, entity_type, sync_id)
        .await
        .expect("payload")
        .expect("row")
}

/// Applies a received row as the inbox pass does: in a transaction kept only when the row is
/// applied (a refused row, or a refused row of its group, leaves nothing behind).
async fn apply(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    use sea_orm::TransactionTrait;
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::row_codec::apply_row(
        &tx,
        &layout,
        sync_id,
        "upsert",
        &serde_json::to_string(payload).expect("json"),
    )
    .await
    .expect("apply");
    if matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged) {
        tx.commit().await.expect("commit");
    } else {
        tx.rollback().await.expect("rollback");
    }
    crate::sync::row_codec::clear_ref_cache();
    outcome
}

fn code(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

fn with(mut p: Payload, changes: &[(&str, Value)]) -> Payload {
    for (k, v) in changes {
        p.insert((*k).to_string(), v.clone());
    }
    p
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

async fn permit_status(db: &DatabaseConnection, wp: &WorkPermit) -> String {
    text(db, &format!("SELECT status FROM work_permits WHERE id = {}", wp.id))
        .await
        .expect("status")
}

#[tokio::test]
async fn a_received_activation_is_rechecked_on_its_isolations_and_times() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-1").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "MCC-1 breaker").await;
    let applied = queries::apply_permit_isolation(&db, super::tests::action(&iso), APPLIER)
        .await
        .expect("apply");
    let issued = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let active = with(
        issued.clone(),
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );

    // Activated with an isolation nobody verified: refused, nothing written.
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert_eq!(code(&outcome), Some("LOTO_NOT_VERIFIED"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "issued");

    queries::verify_permit_isolation(&db, super::tests::action(&applied), VERIFIER)
        .await
        .expect("verify");
    // Active without its activation time: malformed.
    let no_time = with(active.clone(), &[("activated_at", Value::Null)]);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &no_time).await;
    assert_eq!(code(&outcome), Some("SYNC_STATE_TIME_MISSING"), "{outcome:?}");
    // Activated after its expiry (judged at the activation time written in the row).
    let late = with(
        active.clone(),
        &[
            ("activated_at", json!("2021-01-01T00:00:00Z")),
            ("expires_at", json!("2020-12-31T00:00:00Z")),
        ],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &late).await;
    assert_eq!(code(&outcome), Some("PERMIT_EXPIRED"), "{outcome:?}");
    // Dated in the future: not consistent.
    let future = with(active.clone(), &[("activated_at", json!("2999-01-01T00:00:00Z"))]);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &future).await;
    assert_eq!(code(&outcome), Some("SYNC_TIME_INCONSISTENT"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "issued");

    // Every isolation verified by a second person: applied.
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "active");

    // Closed with a lock still on: refused; out of a terminal status backwards: refused.
    let closed = with(
        active.clone(),
        &[("status", json!("closed")), ("closed_at", json!(now()))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert_eq!(code(&outcome), Some("PERMIT_REMOVAL_PENDING"), "{outcome:?}");
    exec(
        &db,
        &format!("UPDATE work_permits SET status = 'cancelled' WHERE id = {}", wp.id),
    )
    .await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert_eq!(code(&outcome), Some("PERMIT_STATUS_BACKWARDS"), "{outcome:?}");
}

#[tokio::test]
async fn a_received_isolation_keeps_the_two_person_rule() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-2").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "Valve V-12").await;
    queries::apply_permit_isolation(&db, super::tests::action(&iso), APPLIER)
        .await
        .expect("apply");
    let applier = user_sync(&db, APPLIER).await;
    let verifier = user_sync(&db, VERIFIER).await;
    let applied = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    let verified_sql = format!("SELECT verified_at FROM permit_isolations WHERE id = {}", iso.id);
    let verified_at = || text(&db, &verified_sql);

    let own = with(
        applied.clone(),
        &[("verified_by_sync_id", json!(applier)), ("verified_at", json!(now()))],
    );
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &own).await;
    assert_eq!(code(&outcome), Some("PERMIT_TWO_PERSON_VERIFY"), "{outcome:?}");
    assert_eq!(verified_at().await, None);

    let second = with(
        applied.clone(),
        &[("verified_by_sync_id", json!(verifier)), ("verified_at", json!(now()))],
    );
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &second).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(verified_at().await.is_some());

    // Lock removal verified by the person who fitted it: refused, nothing written.
    let removal = with(
        second,
        &[
            ("removal_verified_by_sync_id", json!(applier)),
            ("removal_verified_at", json!(now())),
        ],
    );
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &removal).await;
    assert_eq!(code(&outcome), Some("PERMIT_TWO_PERSON_REMOVAL"), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT removal_verified_at FROM permit_isolations WHERE id = {}",
                iso.id
            )
        )
        .await,
        None
    );
}

/// A permit created and activated on another device arrives with its isolation: both are applied in
/// one pass (the isolation waiting in the inbox counts), never left waiting for each other.
#[tokio::test]
async fn a_permit_and_its_isolation_received_together_are_judged_together() {
    for verified in [true, false] {
        let db = setup().await;
        let asset = insert_equipment(&db, "EQ-SYNC-3").await;
        let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
        let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-3 breaker").await).await;
        let wp = set_status(&db, &wp, "active").await.expect("activate");
        let permit_wire = wire(&db, "work_permits", &wp.entity_sync_id).await;
        let mut iso_wire = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
        if !verified {
            iso_wire.insert("verified_at".into(), Value::Null);
            iso_wire.insert("verified_by_sync_id".into(), Value::Null);
        }
        // This device has never seen them.
        exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", iso.id)).await;
        exec(&db, &format!("DELETE FROM work_permits WHERE id = {}", wp.id)).await;
        for (n, (entity_type, sync_id, payload)) in [
            ("work_permits", &wp.entity_sync_id, &permit_wire),
            ("permit_isolations", &iso.entity_sync_id, &iso_wire),
        ]
        .into_iter()
        .enumerate()
        {
            exec(
                &db,
                &format!(
                    "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
                     row_version, payload_json, payload_hash) VALUES ('b1', 'c1', '{entity_type}', '{sync_id}', \
                     'upsert', 2, '{}', 'h{n}')",
                    serde_json::to_string(payload).expect("json").replace('\'', "''")
                ),
            )
            .await;
        }
        let result = crate::sync::materialize::materialize_inbox_batch(&db, None)
            .await
            .expect("materialize");
        let inbox_status = |sid: String| {
            let db = &db;
            async move {
                text(
                    db,
                    &format!("SELECT apply_status FROM sync_inbox WHERE entity_sync_id = '{sid}'"),
                )
                .await
            }
        };
        let synced = |sid: String| {
            let db = &db;
            async move {
                text(
                    db,
                    &format!(
                        "SELECT CAST(synced_row_version AS TEXT) FROM sync_entity_state WHERE entity_sync_id = '{sid}'"
                    ),
                )
                .await
            }
        };
        if verified {
            // Applied once (with the permit), recorded, then skipped when its own turn came.
            assert_eq!(
                (result.applied, result.skipped, result.conflicts),
                (1, 1, 0),
                "{result:?}"
            );
            assert_eq!(
                inbox_status(wp.entity_sync_id.clone()).await.as_deref(),
                Some("applied")
            );
            assert_eq!(
                inbox_status(iso.entity_sync_id.clone()).await.as_deref(),
                Some("applied")
            );
            assert_eq!(synced(iso.entity_sync_id.clone()).await.as_deref(), Some("2"));
        } else {
            // The permit is a conflict; the isolation was judged on its own and waits for it.
            assert_eq!(
                inbox_status(wp.entity_sync_id.clone()).await.as_deref(),
                Some("conflict")
            );
            assert_eq!(
                inbox_status(iso.entity_sync_id.clone()).await.as_deref(),
                Some("deferred")
            );
            assert_eq!(synced(iso.entity_sync_id.clone()).await, None);
        }
        let status = text(
            &db,
            &format!(
                "SELECT status FROM work_permits WHERE entity_sync_id = '{}'",
                wp.entity_sync_id
            ),
        )
        .await;
        let conflict = text(
            &db,
            &format!(
                "SELECT conflict_type FROM sync_conflicts WHERE entity_sync_id = '{}'",
                wp.entity_sync_id
            ),
        )
        .await;
        if verified {
            assert_eq!(status.as_deref(), Some("active"));
            assert_eq!(conflict, None);
        } else {
            assert_eq!(status, None, "the unverified activation is not written");
            assert_eq!(conflict.as_deref(), Some("LOTO_NOT_VERIFIED"));
        }
        // The isolation waits for its permit when the permit was refused.
        let iso_held = text(
            &db,
            &format!(
                "SELECT entity_sync_id FROM permit_isolations WHERE entity_sync_id = '{}'",
                iso.entity_sync_id
            ),
        )
        .await;
        assert_eq!(iso_held.is_some(), verified);
    }
}

#[tokio::test]
async fn a_work_order_started_elsewhere_passes_the_permit_gate() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-4").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let in_progress = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'in_progress'",
    )
    .await
    .expect("status sync id");
    let status_sql = format!(
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = {wo_id}"
    );
    let status_of = || text(&db, &status_sql);
    let ready = status_of().await;
    let started = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[("status_sync_id", json!(in_progress)), ("actual_start", json!(now()))],
    );

    // Requires a permit and none has been received: waits (the permit may still be coming),
    // nothing written.
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(status_of().await, ready);
    // In progress without its start time: malformed.
    let no_start = with(started.clone(), &[("actual_start", Value::Null)]);
    let outcome = apply(&db, "work_orders", &wo_sync, &no_start).await;
    assert_eq!(code(&outcome), Some("SYNC_STATE_TIME_MISSING"), "{outcome:?}");

    // A linked permit not active yet: refused.
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-4 breaker").await).await;
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert_eq!(code(&outcome), Some("PERMIT_NOT_ACTIVE"), "{outcome:?}");

    // Active, not expired at the start, isolations verified: applied (started after the activation;
    // times are compared exactly).
    set_status(&db, &wp, "active").await.expect("activate");
    let started = with(started, &[("actual_start", json!(now()))]);
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(status_of().await.as_deref(), Some("in_progress"));
}

#[tokio::test]
async fn a_received_qualification_override_is_rechecked() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-5").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    exec(
        &db,
        "INSERT INTO personnel (entity_sync_id, employee_code, full_name, availability_status) \
         VALUES ('pers-sync-verifier', 'SYNC-P1', 'PTW Verifier', 'available')",
    )
    .await;
    exec(
        &db,
        &format!(
            "UPDATE user_accounts SET personnel_id = (SELECT id FROM personnel WHERE entity_sync_id = 'pers-sync-verifier') \
             WHERE id = {VERIFIER}"
        ),
    )
    .await;
    let admin = user_sync(&db, APPLIER).await;
    let verifier = user_sync(&db, VERIFIER).await;
    let base = json!({
        "personnel_sync_id": "pers-sync-verifier",
        "user_sync_id": null,
        "context_kind": "work_order",
        "work_order_sync_id": wo_sync,
        "permit_sync_id": null,
        "role_code": null,
        "gaps_json": "[]",
        "reason": "Habilitation en cours de renouvellement",
        "approved_by_sync_id": admin,
        "approved_at": now(),
        "created_at": now(),
        "_sync_format": "row.v1",
    })
    .as_object()
    .cloned()
    .expect("object");
    let held = || text(&db, "SELECT entity_sync_id FROM qualification_overrides");

    let short = with(base.clone(), &[("reason", json!("ok"))]);
    let outcome = apply(&db, "qualification_overrides", "qo-1", &short).await;
    assert_eq!(
        code(&outcome),
        Some("QUALIFICATION_OVERRIDE_REASON_SHORT"),
        "{outcome:?}"
    );
    let no_context = with(base.clone(), &[("context_kind", json!("permit_role"))]);
    let outcome = apply(&db, "qualification_overrides", "qo-1", &no_context).await;
    assert_eq!(code(&outcome), Some("QUALIFICATION_OVERRIDE_INVALID"), "{outcome:?}");
    // The person overrides their own gap.
    let own = with(base.clone(), &[("approved_by_sync_id", json!(verifier))]);
    let outcome = apply(&db, "qualification_overrides", "qo-1", &own).await;
    assert_eq!(code(&outcome), Some("QUALIFICATION_OVERRIDE_SELF"), "{outcome:?}");
    // Approved by someone without the certification right here.
    exec(
        &db,
        &format!("UPDATE user_accounts SET personnel_id = NULL WHERE id = {VERIFIER}"),
    )
    .await;
    let outcome = apply(&db, "qualification_overrides", "qo-1", &own).await;
    assert_eq!(
        code(&outcome),
        Some("QUALIFICATION_OVERRIDE_UNAUTHORISED"),
        "{outcome:?}"
    );
    // An approver not received yet: wait.
    let unknown = with(base.clone(), &[("approved_by_sync_id", json!("user-not-here"))]);
    let outcome = apply(&db, "qualification_overrides", "qo-1", &unknown).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(held().await, None);

    // A valid override from an administrator: applied.
    let outcome = apply(&db, "qualification_overrides", "qo-1", &base).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(held().await.as_deref(), Some("qo-1"));
}

/// A row waiting in the inbox, as a pull leaves it.
async fn inbox(
    db: &DatabaseConnection,
    entity_type: &str,
    sync_id: &str,
    version: i64,
    status: &str,
    payload: &Payload,
) {
    exec(
        db,
        &format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
             row_version, payload_json, payload_hash, apply_status) VALUES ('b-{sync_id}-{version}', 'c1', \
             '{entity_type}', '{sync_id}', 'upsert', {version}, '{}', 'h-{sync_id}-{version}', '{status}')",
            serde_json::to_string(payload).expect("json").replace('\'', "''")
        ),
    )
    .await;
}

async fn delete(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> ApplyOutcome {
    delete_as(db, entity_type, sync_id, "delete").await
}

async fn delete_as(db: &DatabaseConnection, entity_type: &str, sync_id: &str, operation: &str) -> ApplyOutcome {
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    let payload = crate::sync::row_codec::delete_payload(&layout, None);
    crate::sync::row_codec::apply_row(
        db,
        &layout,
        sync_id,
        operation,
        &serde_json::to_string(&payload).expect("json"),
    )
    .await
    .expect("apply")
}

/// Waiting isolations count toward an activation only when they keep the isolation rules, are
/// pending (not deferred) and newer than the local row.
#[tokio::test]
async fn only_valid_newer_pending_isolations_count_toward_an_activation() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-6").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-6 breaker").await).await;
    let verified = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    let applier = user_sync(&db, APPLIER).await;
    let active = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", iso.id)).await;

    // Waiting, but its lock removal names the person who fitted it: not counted.
    let removed_by_applier = with(
        verified.clone(),
        &[
            ("removal_verified_by_sync_id", json!(applier)),
            ("removal_verified_at", json!("2030-01-01T00:00:00Z")),
        ],
    );
    inbox(
        &db,
        "permit_isolations",
        &iso.entity_sync_id,
        3,
        "pending",
        &removed_by_applier,
    )
    .await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert_eq!(code(&outcome), Some("LOTO_NO_ISOLATION"), "{outcome:?}");
    exec(&db, "DELETE FROM sync_inbox").await;

    // Valid but deferred for another reason (waiting for something that may never come): not
    // counted.
    inbox(&db, "permit_isolations", &iso.entity_sync_id, 3, "deferred", &verified).await;
    exec(&db, "UPDATE sync_inbox SET rejection_code = 'MISSING_FIELD'").await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert_eq!(code(&outcome), Some("LOTO_NO_ISOLATION"), "{outcome:?}");
    // Deferred only because it arrived before its permit: counted, and applied with it.
    exec(&db, "UPDATE sync_inbox SET rejection_code = 'MISSING_PARENT'").await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(&db, "SELECT apply_status FROM sync_inbox").await.as_deref(),
        Some("applied")
    );
    exec(&db, "DELETE FROM sync_inbox").await;
    exec(
        &db,
        &format!("UPDATE work_permits SET status = 'issued' WHERE id = {}", wp.id),
    )
    .await;
    exec(
        &db,
        &format!(
            "DELETE FROM permit_isolations WHERE entity_sync_id = '{}'",
            iso.entity_sync_id
        ),
    )
    .await;
    exec(&db, "DELETE FROM sync_entity_state").await;

    // Valid, pending: counted.
    inbox(&db, "permit_isolations", &iso.entity_sync_id, 3, "pending", &verified).await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn an_older_waiting_isolation_does_not_hide_the_local_one() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-7").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-7 breaker").await).await;
    let verified = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    // Here the verification was withdrawn (newer local version).
    exec(
        &db,
        &format!(
            "UPDATE permit_isolations SET verified_at = NULL, verified_by_id = NULL, row_version = row_version + 5 \
             WHERE id = {}",
            iso.id
        ),
    )
    .await;
    // This PC applied version 10 of it from the server before.
    exec(
        &db,
        &format!(
            "INSERT INTO sync_entity_state (entity_type, entity_sync_id, synced_row_version, synced_payload_hash,              source, updated_at) VALUES ('permit_isolations', '{}', 10, 'h-local', 'inbound_apply', '2026-01-01T00:00:00Z')",
            iso.entity_sync_id
        ),
    )
    .await;
    let local_version = 9;
    let active = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );
    inbox(
        &db,
        "permit_isolations",
        &iso.entity_sync_id,
        local_version,
        "pending",
        &verified,
    )
    .await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert_eq!(code(&outcome), Some("LOTO_NOT_VERIFIED"), "{outcome:?}");
    inbox(
        &db,
        "permit_isolations",
        &iso.entity_sync_id,
        local_version + 2,
        "pending",
        &verified,
    )
    .await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn permits_isolations_and_suspensions_are_not_deleted_by_sync_while_they_protect_work() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-8").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-8 breaker").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let (wp, suspension) = queries::suspend_work_permit(
        &db,
        crate::permit::domain::PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            reason: "Gas detected".into(),
            reactivation_conditions: None,
        },
        APPLIER,
    )
    .await
    .expect("suspend");
    let count = |table: &'static str, sid: String| {
        let db = &db;
        async move {
            text(
                db,
                &format!("SELECT entity_sync_id FROM {table} WHERE entity_sync_id = '{sid}'"),
            )
            .await
            .is_some()
        }
    };
    for (entity_type, sid) in [
        ("permit_isolations", iso.entity_sync_id.clone()),
        ("permit_suspensions", suspension.entity_sync_id.clone()),
        ("work_permits", wp.entity_sync_id.clone()),
    ] {
        let outcome = delete(&db, entity_type, &sid).await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_DELETE_REFUSED"),
            "{entity_type}: {outcome:?}"
        );
        assert!(count(entity_type, sid).await, "{entity_type} kept");
    }
    // An archive is a delete: refused the same way.
    let outcome = delete_as(&db, "permit_isolations", &iso.entity_sync_id, "archive").await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
    // A draft permit (never protected anything) may be deleted.
    let draft = create_permit(&db, "loto", asset).await;
    let outcome = delete(&db, "work_permits", &draft.entity_sync_id).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // The isolations of a finished permit are its LOTO history: kept too.
    exec(
        &db,
        &format!("UPDATE work_permits SET status = 'cancelled' WHERE id = {}", wp.id),
    )
    .await;
    let outcome = delete(&db, "permit_isolations", &iso.entity_sync_id).await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
    // A permit type is edited, never deleted.
    let type_sync = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'loto'")
        .await
        .expect("type");
    let outcome = delete(&db, "permit_types", &type_sync).await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
    let outcome = delete(&db, "work_permits", &wp.entity_sync_id).await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
}

#[tokio::test]
async fn an_isolation_never_changes_after_its_permit_protects_the_work() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-9").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let first = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-9 breaker").await).await;
    let second = add_isolation(&db, wp.id, "Valve V-9").await;
    let applier = user_sync(&db, APPLIER).await;
    let verifier = user_sync(&db, VERIFIER).await;

    // Lock removal verified while the permit is only issued: refused.
    let removal_early = with(
        wire(&db, "permit_isolations", &first.entity_sync_id).await,
        &[
            ("removal_verified_by_sync_id", json!(verifier)),
            ("removal_verified_at", json!(now())),
        ],
    );
    let outcome = apply(&db, "permit_isolations", &first.entity_sync_id, &removal_early).await;
    assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_WRONG_STATUS"), "{outcome:?}");

    // Removed from the second isolation (unapplied) so the permit can be activated without it.
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", second.id)).await;
    set_status(&db, &wp, "active").await.expect("activate");
    // An isolation applied and verified an hour after the permit started protecting the work:
    // refused (isolations are fitted while a permit is prepared or suspended).
    let late = (chrono::Utc::now() + chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let late_verify = with(
        wire(&db, "permit_isolations", &first.entity_sync_id).await,
        &[
            ("applied_at", json!(late)),
            ("verified_at", json!(late)),
            ("applied_by_sync_id", json!(applier)),
            ("verified_by_sync_id", json!(verifier)),
        ],
    );
    let outcome = apply(&db, "permit_isolations", &first.entity_sync_id, &late_verify).await;
    assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_WRONG_STATUS"), "{outcome:?}");

    // Moved to another permit: refused.
    let other = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let moved = with(
        wire(&db, "permit_isolations", &first.entity_sync_id).await,
        &[("permit_sync_id", json!(other.entity_sync_id))],
    );
    let outcome = apply(&db, "permit_isolations", &first.entity_sync_id, &moved).await;
    assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_MOVED"), "{outcome:?}");

    // Lock removal verified by a second person while the permit is active: applied.
    let removal = with(
        wire(&db, "permit_isolations", &first.entity_sync_id).await,
        &[
            ("removal_verified_by_sync_id", json!(verifier)),
            ("removal_verified_at", json!(now())),
        ],
    );
    let outcome = apply(&db, "permit_isolations", &first.entity_sync_id, &removal).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn hand_back_suspension_and_revalidation_are_rechecked() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-10").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-10 breaker").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let active = wire(&db, "work_permits", &wp.entity_sync_id).await;

    // Suspended without a suspension: refused.
    let suspended = with(active.clone(), &[("status", json!("suspended"))]);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &suspended).await;
    assert_eq!(code(&outcome), Some("PERMIT_SUSPENSION_MISSING"), "{outcome:?}");

    // Suspended here, then revalidated elsewhere after its expiry: refused.
    let (wp, suspension) = queries::suspend_work_permit(
        &db,
        crate::permit::domain::PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            reason: "Gas detected".into(),
            reactivation_conditions: None,
        },
        APPLIER,
    )
    .await
    .expect("suspend");
    // Activated two hours ago, expired an hour ago: revalidated now (after its expiry) is refused,
    // revalidated before its expiry is applied.
    let ago = |minutes: i64| {
        (chrono::Utc::now() - chrono::Duration::minutes(minutes))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    exec(
        &db,
        &format!(
            "UPDATE work_permits SET activated_at = '{}', expires_at = '{}' WHERE id = {}",
            ago(120),
            ago(60),
            wp.id
        ),
    )
    .await;
    let revalidated = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("active"))],
    );
    let reinstated_at = |at: String| {
        let db = &db;
        let sid = suspension.entity_sync_id.clone();
        async move {
            with(
                wire(db, "permit_suspensions", &sid).await,
                &[
                    ("reinstated_at", json!(at)),
                    ("reinstated_by_sync_id", json!(user_sync(db, VERIFIER).await)),
                ],
            )
        }
    };
    inbox(
        &db,
        "permit_suspensions",
        &suspension.entity_sync_id,
        9,
        "pending",
        &reinstated_at(now()).await,
    )
    .await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &revalidated).await;
    assert_eq!(code(&outcome), Some("PERMIT_EXPIRED"), "{outcome:?}");
    exec(&db, "DELETE FROM sync_inbox").await;
    inbox(
        &db,
        "permit_suspensions",
        &suspension.entity_sync_id,
        9,
        "pending",
        &reinstated_at(ago(90)).await,
    )
    .await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &revalidated).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    // Closed with every lock removal verified, then handed back without a handover log: refused.
    exec(&db, "DELETE FROM sync_inbox").await;
    exec(
        &db,
        &format!("UPDATE work_permits SET status = 'active' WHERE id = {}", wp.id),
    )
    .await;
    let removed = with(
        wire(&db, "permit_isolations", &iso.entity_sync_id).await,
        &[
            ("removal_verified_by_sync_id", json!(user_sync(&db, VERIFIER).await)),
            ("removal_verified_at", json!(now())),
        ],
    );
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &removed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let closed = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("closed")), ("closed_at", json!(now()))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let handed_back = with(
        closed.clone(),
        &[("status", json!("handed_back")), ("handed_back_at", json!(now()))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &handed_back).await;
    assert_eq!(code(&outcome), Some("PERMIT_HANDOVER_REQUIRED"), "{outcome:?}");
    queries::append_permit_handover_log(
        &db,
        crate::permit::domain::PermitHandoverLogInput {
            permit_id: wp.id,
            handed_from_role: "Exécutant".into(),
            handed_to_role: "Exploitation".into(),
            confirmation_note: "Zone rendue propre".into(),
        },
    )
    .await
    .expect("handover");
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &handed_back).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A work order started elsewhere arrives before its permit and isolation (still in the inbox):
/// judged with them; a permit suspended after the start does not refuse it.
#[tokio::test]
async fn a_started_work_order_is_judged_with_its_waiting_permit() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-11").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let in_progress = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'in_progress'",
    )
    .await
    .expect("status sync id");
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-11 breaker").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let started = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[("status_sync_id", json!(in_progress)), ("actual_start", json!(now()))],
    );
    // Suspended after the start (a later version of the permit).
    queries::suspend_work_permit(
        &db,
        crate::permit::domain::PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            reason: "Break".into(),
            reactivation_conditions: None,
        },
        APPLIER,
    )
    .await
    .expect("suspend");
    exec(
        &db,
        &format!(
            "UPDATE permit_suspensions SET suspended_at = '2999-01-01T00:00:00Z' WHERE permit_id = {}",
            wp.id
        ),
    )
    .await;
    let permit_wire = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let iso_wire = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    let suspension_sync = text(
        &db,
        &format!(
            "SELECT entity_sync_id FROM permit_suspensions WHERE permit_id = {}",
            wp.id
        ),
    )
    .await
    .expect("suspension");
    let suspension_wire = wire(&db, "permit_suspensions", &suspension_sync).await;
    // This PC has not applied the permit and isolation yet: they wait in the inbox.
    exec(
        &db,
        &format!("DELETE FROM permit_suspensions WHERE permit_id = {}", wp.id),
    )
    .await;
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", iso.id)).await;
    exec(&db, &format!("DELETE FROM work_permits WHERE id = {}", wp.id)).await;
    inbox(&db, "work_permits", &wp.entity_sync_id, 5, "pending", &permit_wire).await;
    inbox(&db, "permit_isolations", &iso.entity_sync_id, 5, "pending", &iso_wire).await;
    inbox(
        &db,
        "permit_suspensions",
        &suspension_sync,
        5,
        "pending",
        &suspension_wire,
    )
    .await;

    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn an_override_on_a_login_with_a_personnel_record_is_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-12").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    exec(
        &db,
        "INSERT INTO personnel (entity_sync_id, employee_code, full_name, availability_status) \
         VALUES ('pers-sync-12', 'SYNC-P12', 'PTW Verifier', 'available')",
    )
    .await;
    exec(
        &db,
        &format!(
            "UPDATE user_accounts SET personnel_id = (SELECT id FROM personnel WHERE entity_sync_id = 'pers-sync-12') \
             WHERE id = {VERIFIER}"
        ),
    )
    .await;
    let account_override = json!({
        "personnel_sync_id": null,
        "user_sync_id": user_sync(&db, VERIFIER).await,
        "context_kind": "work_order",
        "work_order_sync_id": wo_sync,
        "permit_sync_id": null,
        "role_code": null,
        "gaps_json": "[]",
        "reason": "Habilitation en cours de renouvellement",
        "approved_by_sync_id": user_sync(&db, APPLIER).await,
        "approved_at": now(),
        "created_at": now(),
        "_sync_format": "row.v1",
    })
    .as_object()
    .cloned()
    .expect("object");
    let outcome = apply(&db, "qualification_overrides", "qo-12", &account_override).await;
    assert_eq!(code(&outcome), Some("QUALIFICATION_OVERRIDE_USE_PERSON"), "{outcome:?}");
}

/// The isolations a permit's activation is judged on are applied with it: one refused by its own
/// checks (verified an hour after the activation) refuses the activation too, and nothing is kept.
#[tokio::test]
async fn an_activation_and_its_waiting_isolations_are_applied_together_or_not_at_all() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-13").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-13 breaker").await).await;
    let late = (chrono::Utc::now() + chrono::Duration::minutes(5))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let verified_late = with(
        wire(&db, "permit_isolations", &iso.entity_sync_id).await,
        &[("verified_at", json!(late))],
    );
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", iso.id)).await;
    inbox(
        &db,
        "permit_isolations",
        &iso.entity_sync_id,
        3,
        "pending",
        &verified_late,
    )
    .await;
    let active = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert_eq!(code(&outcome), Some("SYNC_GROUP_REFUSED"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "issued");
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT entity_sync_id FROM permit_isolations WHERE entity_sync_id = '{}'",
                iso.entity_sync_id
            )
        )
        .await,
        None
    );

    // Verified before the activation: both applied.
    exec(&db, "DELETE FROM sync_inbox").await;
    let verified = with(verified_late, &[("verified_at", json!("2026-01-01T00:00:00Z"))]);
    let verified = with(verified, &[("applied_at", json!("2025-12-31T23:00:00Z"))]);
    inbox(&db, "permit_isolations", &iso.entity_sync_id, 3, "pending", &verified).await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "active");
    assert!(text(
        &db,
        &format!(
            "SELECT verified_at FROM permit_isolations WHERE entity_sync_id = '{}'",
            iso.entity_sync_id
        )
    )
    .await
    .is_some());
}

/// A waiting isolation whose fitter is not applied here (only waiting itself) is not counted: the
/// activation waits for it rather than trusting a row that may never be applied.
#[tokio::test]
async fn an_activation_waits_for_the_people_named_by_its_isolations() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-14").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-14 breaker").await).await;
    let foreign = with(
        wire(&db, "permit_isolations", &iso.entity_sync_id).await,
        &[("applied_by_sync_id", json!("user-only-in-inbox"))],
    );
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", iso.id)).await;
    inbox(&db, "permit_isolations", &iso.entity_sync_id, 3, "pending", &foreign).await;
    let ghost = json!({ "username": "ghost", "_sync_format": "row.v1" })
        .as_object()
        .cloned()
        .expect("object");
    inbox(&db, "user_accounts", "user-only-in-inbox", 1, "pending", &ghost).await;
    let active = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "issued");
}

/// Once a permit protects the work, no isolation is added to it and nobody's name on an isolation
/// changes (outside the activation it was judged on).
#[tokio::test]
async fn no_isolation_is_added_or_reassigned_under_a_protecting_permit() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-15").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-15 breaker").await).await;
    let spare = add_isolation(&db, wp.id, "Valve V-15").await;
    let spare = apply_and_verify(&db, &spare).await;
    let spare_wire = wire(&db, "permit_isolations", &spare.entity_sync_id).await;
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", spare.id)).await;
    set_status(&db, &wp, "active").await.expect("activate");

    let outcome = apply(&db, "permit_isolations", &spare.entity_sync_id, &spare_wire).await;
    assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_WRONG_STATUS"), "{outcome:?}");

    let admin = user_sync(&db, 1).await;
    let swapped = with(
        wire(&db, "permit_isolations", &iso.entity_sync_id).await,
        &[
            ("verified_by_sync_id", json!(admin)),
            ("applied_by_sync_id", json!(user_sync(&db, VERIFIER).await)),
        ],
    );
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &swapped).await;
    assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_WRONG_STATUS"), "{outcome:?}");
}

#[tokio::test]
async fn a_start_time_later_than_its_work_order_row_is_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-16").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let in_progress = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'in_progress'",
    )
    .await
    .expect("status sync id");
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-16 breaker").await).await;
    set_status(&db, &wp, "active").await.expect("activate");
    let written = now();
    let started = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[
            ("status_sync_id", json!(in_progress)),
            ("actual_start", json!(written)),
            ("updated_at", json!(written)),
        ],
    );
    // A resume dated an hour after the row was written.
    exec(
        &db,
        &format!(
            "INSERT INTO wo_state_transition_log (wo_id, from_status, to_status, action, actor_id, acted_at, lifecycle_state) \
             VALUES ({wo_id}, 'on_hold', 'in_progress', 'resume', 1, '{}', 'in_progress')",
            (chrono::Utc::now() + chrono::Duration::hours(1)).format("%Y-%m-%dT%H:%M:%SZ")
        ),
    )
    .await;
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert_eq!(code(&outcome), Some("SYNC_TIME_INCONSISTENT"), "{outcome:?}");
}

#[tokio::test]
async fn a_qualification_requirement_is_never_deleted_by_sync() {
    let db = setup().await;
    exec(
        &db,
        "INSERT INTO qualification_requirements (entity_sync_id, target_kind, target_code, skill_reference_value_id,          coverage, created_at, updated_at)          SELECT 'req-sync-loto', 'loto_role', 'isolation_apply', rv.id, 'each', '2026-01-01T00:00:00Z',                 '2026-01-01T00:00:00Z'            FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id            JOIN reference_domains d ON d.id = rs.domain_id           WHERE d.code = 'PERSONNEL.SKILLS' ORDER BY rv.id LIMIT 1",
    )
    .await;
    let req = text(
        &db,
        "SELECT entity_sync_id FROM qualification_requirements WHERE entity_sync_id = 'req-sync-loto'",
    )
    .await
    .expect("requirement");
    let outcome = delete(&db, "qualification_requirements", &req).await;
    assert_eq!(
        code(&outcome),
        Some("QUALIFICATION_REQUIREMENT_DELETE_REFUSED"),
        "{outcome:?}"
    );
}

/// A received work order whose permit flag set by a plant rule is cleared says who cleared it (with
/// the permit-issue right here), when and why.
#[tokio::test]
async fn a_cleared_rule_permit_flag_is_rechecked() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-17").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    exec(
        &db,
        &format!("UPDATE work_orders SET permit_rule_code = 'rule-elec', requires_permit = 1 WHERE id = {wo_id}"),
    )
    .await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let flagged = wire(&db, "work_orders", &wo_sync).await;
    let cleared = |by: Value, reason: Value| {
        with(
            flagged.clone(),
            &[
                ("requires_permit", json!(0)),
                ("permit_rule_cleared_by_sync_id", by),
                ("permit_rule_cleared_at", json!(now())),
                ("permit_rule_clear_reason", reason),
            ],
        )
    };
    let admin = json!(user_sync(&db, APPLIER).await);
    let requires = || {
        let db = &db;
        async move {
            text(
                db,
                &format!("SELECT CAST(requires_permit AS TEXT) FROM work_orders WHERE id = {wo_id}"),
            )
            .await
        }
    };

    let outcome = apply(&db, "work_orders", &wo_sync, &cleared(admin.clone(), json!("court"))).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(Value::Null, json!("Consignation faite par l'exploitant")),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(
            json!(user_sync(&db, VERIFIER).await),
            json!("Consignation faite par l'exploitant"),
        ),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_UNAUTHORISED"), "{outcome:?}");
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(json!("user-not-here"), json!("Consignation faite par l'exploitant")),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(requires().await.as_deref(), Some("1"));

    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(admin, json!("Consignation faite par l'exploitant")),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(requires().await.as_deref(), Some("0"));
}

/// A waiting isolation of a row edited here and not sent yet is a concurrent change: it is not
/// counted (its own apply would be a conflict), and the activation is judged on the local row.
#[tokio::test]
async fn a_waiting_isolation_with_an_unsent_local_change_is_not_counted() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-18").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let iso = add_isolation(&db, wp.id, "MCC-18 breaker").await;
    let iso = apply_and_verify(&db, &iso).await;
    let verified = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    // Here, the verification was withdrawn and that change is not sent yet.
    exec(
        &db,
        &format!(
            "UPDATE permit_isolations SET verified_at = NULL, verified_by_id = NULL WHERE id = {}",
            iso.id
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version,              payload_json, payload_hash, status) VALUES ('k-unsent', 'permit_isolations', '{}', 'upsert', 3, '{{}}',              'h-unsent', 'pending')",
            iso.entity_sync_id
        ),
    )
    .await;
    inbox(&db, "permit_isolations", &iso.entity_sync_id, 50, "pending", &verified).await;
    let active = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &active).await;
    assert_eq!(code(&outcome), Some("LOTO_NOT_VERIFIED"), "{outcome:?}");
}

/// A suspended permit may receive a new isolation, judged on its new status.
#[tokio::test]
async fn a_suspension_arrives_with_the_isolation_added_during_it() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-19").await;
    let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-19 breaker").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let active = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let (_, suspension) = queries::suspend_work_permit(
        &db,
        crate::permit::domain::PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: wp.row_version,
            reason: "Gas detected".into(),
            reactivation_conditions: None,
        },
        APPLIER,
    )
    .await
    .expect("suspend");
    let added = apply_and_verify(&db, &add_isolation(&db, wp.id, "Valve V-19").await).await;
    let suspended = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let suspension_wire = wire(&db, "permit_suspensions", &suspension.entity_sync_id).await;
    let added_wire = wire(&db, "permit_isolations", &added.entity_sync_id).await;
    // This PC still has the permit active, without the suspension and the new isolation.
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", added.id)).await;
    exec(
        &db,
        &format!("DELETE FROM permit_suspensions WHERE id = {}", suspension.id),
    )
    .await;
    exec(
        &db,
        &format!(
            "UPDATE work_permits SET status = 'active', row_version = row_version WHERE id = {}",
            wp.id
        ),
    )
    .await;
    let _ = active;
    inbox(
        &db,
        "permit_suspensions",
        &suspension.entity_sync_id,
        2,
        "pending",
        &suspension_wire,
    )
    .await;
    inbox(
        &db,
        "permit_isolations",
        &added.entity_sync_id,
        2,
        "pending",
        &added_wire,
    )
    .await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &suspended).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "suspended");
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = '{}'",
                added.entity_sync_id
            )
        )
        .await
        .as_deref(),
        Some("applied")
    );
}

/// A work order flagged by a plant rule starts only with a live permit of the rule's type, as here.
#[tokio::test]
async fn a_rule_flagged_start_needs_a_permit_of_the_rule_type() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-20").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let in_progress = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'in_progress'",
    )
    .await
    .expect("status sync id");
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-20 breaker").await).await;
    set_status(&db, &wp, "active").await.expect("activate");
    let start = |rule_type: &str| {
        let db = &db;
        let (wo_sync, in_progress, rule_type) = (wo_sync.clone(), in_progress.clone(), rule_type.to_string());
        async move {
            exec(
                db,
                &format!(
                    "UPDATE work_orders SET permit_rule_code = 'rule-1', permit_rule_permit_type = '{rule_type}' \
                     WHERE id = {wo_id}"
                ),
            )
            .await;
            with(
                wire(db, "work_orders", &wo_sync).await,
                &[("status_sync_id", json!(in_progress)), ("actual_start", json!(now()))],
            )
        }
    };
    let other_type = start("hot_work").await;
    let outcome = apply(&db, "work_orders", &wo_sync, &other_type).await;
    assert_eq!(code(&outcome), Some("PERMIT_TYPE_REQUIRED_TO_START"), "{outcome:?}");
    let loto = start("loto").await;
    let outcome = apply(&db, "work_orders", &wo_sync, &loto).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// The isolations a received start was judged on are applied with it, also those of a permit
/// already on this PC.
#[tokio::test]
async fn a_started_work_order_applies_the_waiting_isolations_it_was_judged_on() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-21").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let in_progress = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'in_progress'",
    )
    .await
    .expect("status sync id");
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-21 breaker").await).await;
    set_status(&db, &wp, "active").await.expect("activate");
    let relabelled = with(
        wire(&db, "permit_isolations", &iso.entity_sync_id).await,
        &[("lock_number", json!("L-0021"))],
    );
    inbox(&db, "permit_isolations", &iso.entity_sync_id, 4, "pending", &relabelled).await;
    let started = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[("status_sync_id", json!(in_progress)), ("actual_start", json!(now()))],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(&db, "SELECT apply_status FROM sync_inbox").await.as_deref(),
        Some("applied")
    );
    assert_eq!(
        text(
            &db,
            &format!("SELECT lock_number FROM permit_isolations WHERE id = {}", iso.id)
        )
        .await
        .as_deref(),
        Some("L-0021")
    );
}
