//! Permit, LOTO, work-order start and qualification-override rules re-checked on rows received by
//! synchronisation (`permit::sync_check`, `qualification::sync_check`), through the real apply path
//! (`sync::row_codec::apply_row`, and the inbox pass for rows that arrive together).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Map, Value};

use super::tests::{
    add_isolation, advance_to_issued, apply_and_verify, assigned_wo_requiring_permit, create_from_wo, create_permit,
    insert_equipment, set_status, setup, user_with, APPLIER, VERIFIER,
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
    super::tests::add_handover_role(&db, "Exécutant").await;
    super::tests::add_handover_role(&db, "Exploitation").await;
    queries::append_permit_handover_log(
        &db,
        crate::permit::domain::PermitHandoverLogInput {
            permit_id: wp.id,
            handed_from_role: "Exécutant".into(),
            handed_to_role: "Exploitation".into(),
            confirmation_note: "Zone rendue propre".into(),
            received_by_id: None,
        },
        super::tests::APPLIER,
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

/// The plant rule that requires a work order's permit is never removed or changed by a received row,
/// and an invented clearing is judged in full, whatever the permit flag says.
#[tokio::test]
async fn a_rule_flag_is_never_dropped_or_forged_by_a_received_row() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-22").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET permit_rule_code = 'rule-elec', permit_rule_permit_type = 'loto', \
             requires_permit = 1 WHERE id = {wo_id}"
        ),
    )
    .await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let flagged = wire(&db, "work_orders", &wo_sync).await;

    // Rule removed, or its permit type changed, with the flag dropped and no clearing: refused.
    for changes in [
        vec![("permit_rule_code", Value::Null), ("requires_permit", json!(0))],
        vec![("permit_rule_permit_type", json!("hot_work"))],
    ] {
        let outcome = apply(&db, "work_orders", &wo_sync, &with(flagged.clone(), &changes)).await;
        assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CHANGED"), "{outcome:?}");
    }
    // Still flagged but with an invented clearing (no reason, no clearer): judged in full.
    let invented = with(flagged.clone(), &[("permit_rule_cleared_at", json!(now()))]);
    let outcome = apply(&db, "work_orders", &wo_sync, &invented).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");

    let admin = json!(user_sync(&db, APPLIER).await);
    let clear = |at: String| {
        with(
            flagged.clone(),
            &[
                ("requires_permit", json!(0)),
                ("permit_rule_cleared_by_sync_id", admin.clone()),
                ("permit_rule_cleared_at", json!(at)),
                ("permit_rule_clear_reason", json!("Consignation faite par l'exploitant")),
            ],
        )
    };
    // Dated in the future: refused.
    let outcome = apply(&db, "work_orders", &wo_sync, &clear("2999-01-01T00:00:00Z".into())).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    // After the work started: refused.
    let after_start = with(clear(now()), &[("actual_start", json!("2026-01-01T00:00:00Z"))]);
    let outcome = apply(&db, "work_orders", &wo_sync, &after_start).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
}

/// A clearer whose rights are still waiting in the inbox: the work order waits (never applied
/// without the right).
#[tokio::test]
async fn a_clearing_waits_for_the_rights_of_its_clearer() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-23").await;
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
    let verifier = user_sync(&db, VERIFIER).await;
    let cleared = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[
            ("requires_permit", json!(0)),
            ("permit_rule_cleared_by_sync_id", json!(verifier)),
            ("permit_rule_cleared_at", json!(now())),
            ("permit_rule_clear_reason", json!("Consignation faite par l'exploitant")),
        ],
    );
    // A plant-wide assignment of the clearer to a role carrying the permit-issue right.
    let issuing_role = text(
        &db,
        &format!(
            "SELECT r.entity_sync_id FROM roles r JOIN role_permissions rp ON rp.role_id = r.id \
             JOIN permissions p ON p.id = rp.permission_id WHERE p.name = '{}' ORDER BY r.id LIMIT 1",
            crate::rbac::permissions::PTW_ISSUE
        ),
    )
    .await
    .expect("a role carrying the right");
    let assignment = json!({
        "user_sync_id": verifier,
        "role_sync_id": issuing_role,
        "scope_type": "tenant",
        "is_emergency": 0,
        "_sync_format": "row.v1",
    })
    .as_object()
    .cloned()
    .expect("object");
    inbox(&db, "user_scope_assignments", "usa-verifier", 1, "pending", &assignment).await;
    let outcome = apply(&db, "work_orders", &wo_sync, &cleared).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    // Nothing waiting for that person: refused.
    exec(&db, "DELETE FROM sync_inbox").await;
    let outcome = apply(&db, "work_orders", &wo_sync, &cleared).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_UNAUTHORISED"), "{outcome:?}");
}

/// A row of the group that waits (here a wait sent by an older version, without its hold kind)
/// makes the received start wait too, instead of a conflict.
#[tokio::test]
async fn a_waiting_row_of_the_group_makes_the_start_wait() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-24").await;
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
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-24 breaker").await).await;
    set_status(&db, &wp, "active").await.expect("activate");
    exec(
        &db,
        &format!(
            "INSERT INTO work_order_delay_segments (entity_sync_id, work_order_id, started_at, ended_at, hold_kind, \
             delay_reason_id) VALUES ('seg-sync-24', {wo_id}, '2026-01-01T08:00:00Z', '2026-01-01T09:00:00Z', 'pause', \
             (SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
              JOIN reference_domains d ON d.id = rs.domain_id WHERE UPPER(d.code) = 'WORK.DELAY_REASONS' LIMIT 1))"
        ),
    )
    .await;
    let mut segment = wire(&db, "work_order_delay_segments", "seg-sync-24").await;
    segment.remove("hold_kind");
    exec(
        &db,
        "DELETE FROM work_order_delay_segments WHERE entity_sync_id = 'seg-sync-24'",
    )
    .await;
    inbox(&db, "work_order_delay_segments", "seg-sync-24", 1, "pending", &segment).await;
    let started = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[("status_sync_id", json!(in_progress)), ("actual_start", json!(now()))],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
}

/// An override whose approver lacks the right here waits only for rows that could give THAT person
/// the right (an assignment, or a permission of a role they hold); an unrelated waiting role does
/// not hold it.
#[tokio::test]
async fn an_override_waits_only_for_rights_of_its_own_approver() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-25").await;
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
         VALUES ('pers-sync-25', 'SYNC-P25', 'Opérateur', 'available')",
    )
    .await;
    let verifier = user_sync(&db, VERIFIER).await;
    let by_verifier = json!({
        "personnel_sync_id": "pers-sync-25",
        "user_sync_id": null,
        "context_kind": "work_order",
        "work_order_sync_id": wo_sync,
        "permit_sync_id": null,
        "role_code": null,
        "gaps_json": "[]",
        "reason": "Habilitation en cours de renouvellement",
        "approved_by_sync_id": verifier,
        "approved_at": now(),
        "created_at": now(),
        "_sync_format": "row.v1",
    })
    .as_object()
    .cloned()
    .expect("object");

    // An unrelated role waiting: refused at once.
    let role = json!({ "name": "Unrelated", "_sync_format": "row.v1" })
        .as_object()
        .cloned()
        .expect("object");
    inbox(&db, "roles", "role-unrelated", 1, "deferred", &role).await;
    let outcome = apply(&db, "qualification_overrides", "qo-25", &by_verifier).await;
    assert_eq!(
        code(&outcome),
        Some("QUALIFICATION_OVERRIDE_UNAUTHORISED"),
        "{outcome:?}"
    );

    // The approver holds a role here without the right; a permission of that role for the right is
    // still waiting: wait.
    let role_sync = text(
        &db,
        &format!(
            "SELECT r.entity_sync_id FROM roles r WHERE NOT EXISTS (SELECT 1 FROM role_permissions rp \
         JOIN permissions p ON p.id = rp.permission_id WHERE rp.role_id = r.id AND p.name = '{}') \
         AND r.entity_sync_id IS NOT NULL ORDER BY r.id LIMIT 1",
            crate::rbac::permissions::TRN_CERTIFY
        ),
    )
    .await
    .expect("a role without the right");
    exec(
        &db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
             created_at, updated_at, row_version) VALUES ('usa-sync-25', {VERIFIER}, \
             (SELECT id FROM roles WHERE entity_sync_id = '{role_sync}'), 'tenant', NULL, \
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
        ),
    )
    .await;
    let grant = json!({ "role_sync_id": role_sync, "permission_key": crate::rbac::permissions::TRN_CERTIFY, "_sync_format": "row.v1" })
        .as_object()
        .cloned()
        .expect("object");
    inbox(&db, "role_permissions", "rp-sync-25", 1, "pending", &grant).await;
    let outcome = apply(&db, "qualification_overrides", "qo-25", &by_verifier).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
}

/// A hand-over log received from another PC is refused when the person who signed it also receives
/// it, and such a log never lets a permit be handed back. A log naming another receiver is applied;
/// one without a signer (sent by a PC from before signers were recorded) is accepted; one naming a
/// signer not on this PC yet waits for that person.
#[tokio::test]
async fn a_received_handover_signed_and_received_by_one_person_is_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-HO").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    let wp = set_status(&db, &wp, "active").await.expect("activate");
    let wp = set_status(&db, &wp, "closed").await.expect("close");
    super::tests::add_handover_role(&db, "Maintenance").await;
    super::tests::add_handover_role(&db, "Operations").await;
    let log = queries::append_permit_handover_log(
        &db,
        crate::permit::domain::PermitHandoverLogInput {
            permit_id: wp.id,
            handed_from_role: "Maintenance".into(),
            handed_to_role: "Operations".into(),
            confirmation_note: "Zone left clean".into(),
            received_by_id: Some(VERIFIER),
        },
        APPLIER,
    )
    .await
    .expect("handover");
    let valid = wire(&db, "permit_handover_logs", &log.entity_sync_id).await;
    // The log was recorded on another PC: not here yet.
    exec(&db, &format!("DELETE FROM permit_handover_logs WHERE id = {}", log.id)).await;
    let logs_sql = format!(
        "SELECT CAST(COUNT(*) AS TEXT) FROM permit_handover_logs WHERE permit_id = {}",
        wp.id
    );
    let logs = || text(&db, &logs_sql);
    let signer = user_sync(&db, APPLIER).await;
    let to_self = with(valid.clone(), &[("received_by_sync_id", json!(signer))]);

    // Received by the person who signed it: refused, nothing written.
    let outcome = apply(&db, "permit_handover_logs", &log.entity_sync_id, &to_self).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_HANDOVER_RECEIVER_IS_SIGNER"),
        "{outcome:?}"
    );
    assert_eq!(logs().await.as_deref(), Some("0"));

    // A permit handed back with only such a log waiting: refused, the permit stays closed.
    let handed_back = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("handed_back")), ("handed_back_at", json!(now()))],
    );
    inbox(&db, "permit_handover_logs", &log.entity_sync_id, 2, "pending", &to_self).await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &handed_back).await;
    assert_eq!(code(&outcome), Some("PERMIT_HANDOVER_REQUIRED"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "closed");
    assert_eq!(logs().await.as_deref(), Some("0"));

    // A log naming a signer not on this PC yet waits for that person.
    let unknown_signer = with(valid.clone(), &[("signed_by_sync_id", json!("user-not-here-yet"))]);
    let outcome = apply(&db, "permit_handover_logs", "ho-unknown-signer", &unknown_signer).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(logs().await.as_deref(), Some("0"));

    // Received by someone else: the permit and its log are applied together.
    exec(&db, "DELETE FROM sync_inbox").await;
    inbox(&db, "permit_handover_logs", &log.entity_sync_id, 2, "pending", &valid).await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &handed_back).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "handed_back");
    assert_eq!(logs().await.as_deref(), Some("1"));

    // Sent by a PC from before signers were recorded (no signer, no receiver): accepted.
    let mut legacy = valid.clone();
    legacy.remove("signed_by_sync_id");
    legacy.remove("received_by_sync_id");
    let outcome = apply(&db, "permit_handover_logs", "ho-legacy", &legacy).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(logs().await.as_deref(), Some("2"));
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COALESCE(signed_by_id, 0) AS TEXT) FROM permit_handover_logs WHERE entity_sync_id = 'ho-legacy'"
        )
        .await
        .as_deref(),
        Some("0"),
        "no signer invented"
    );
}

/// Three-person LOTO chain (a setting of the permit type): a received lock removal verified by the
/// isolation's verifier is refused when the type requires it, applied when it does not.
#[tokio::test]
async fn a_received_removal_keeps_the_three_person_chain_of_its_permit_type() {
    for three_person in [true, false] {
        let db = setup().await;
        let asset = insert_equipment(&db, "EQ-SYNC-26").await;
        exec(
            &db,
            &format!(
                "UPDATE permit_types SET requires_three_person_removal = {} WHERE code = 'loto'",
                i64::from(three_person)
            ),
        )
        .await;
        let wp = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
        let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-26 breaker").await).await;
        set_status(&db, &wp, "active").await.expect("activate");
        let verifier = user_sync(&db, VERIFIER).await;
        let removal = with(
            wire(&db, "permit_isolations", &iso.entity_sync_id).await,
            &[
                ("removal_verified_by_sync_id", json!(verifier)),
                ("removal_verified_at", json!(now())),
            ],
        );
        let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &removal).await;
        if three_person {
            assert_eq!(code(&outcome), Some("PERMIT_THREE_PERSON_REMOVAL"), "{outcome:?}");
        } else {
            assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
        }
    }
}

/// The rights of an override's approver are waited for only on rows that could give THAT person
/// the right: a waiting permission row of another role, or for another permission, a waiting
/// assignment of another person, or a site-scoped or emergency assignment of the approver, never
/// hold it; a plant-wide assignment of the approver to a role carrying the right does.
#[tokio::test]
async fn only_rows_that_could_give_the_approver_the_right_hold_an_override() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-27").await;
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
         VALUES ('pers-sync-27', 'SYNC-P27', 'Opérateur', 'available')",
    )
    .await;
    let verifier = user_sync(&db, VERIFIER).await;
    let by_verifier = json!({
        "personnel_sync_id": "pers-sync-27",
        "user_sync_id": null,
        "context_kind": "work_order",
        "work_order_sync_id": wo_sync,
        "permit_sync_id": null,
        "role_code": null,
        "gaps_json": "[]",
        "reason": "Habilitation en cours de renouvellement",
        "approved_by_sync_id": verifier,
        "approved_at": now(),
        "created_at": now(),
        "_sync_format": "row.v1",
    })
    .as_object()
    .cloned()
    .expect("object");
    let refused = |db: &DatabaseConnection, why: &'static str| {
        let payload = by_verifier.clone();
        let db = db.clone();
        async move {
            let outcome = apply(&db, "qualification_overrides", "qo-27", &payload).await;
            assert_eq!(
                code(&outcome),
                Some("QUALIFICATION_OVERRIDE_UNAUTHORISED"),
                "{why}: {outcome:?}"
            );
            exec(&db, "DELETE FROM sync_inbox").await;
        }
    };
    let row = |v: serde_json::Value| v.as_object().cloned().expect("object");
    let certifying_role = text(
        &db,
        &format!(
            "SELECT r.entity_sync_id FROM roles r JOIN role_permissions rp ON rp.role_id = r.id \
             JOIN permissions p ON p.id = rp.permission_id WHERE p.name = '{}' ORDER BY r.id LIMIT 1",
            crate::rbac::permissions::TRN_CERTIFY
        ),
    )
    .await
    .expect("a role carrying the right");

    // No roles at all, nothing waiting.
    refused(&db, "no roles").await;
    // A permission row of a role the approver does not hold.
    let other_role = row(json!({
        "role_sync_id": "role-not-held",
        "permission_key": crate::rbac::permissions::TRN_CERTIFY,
        "_sync_format": "row.v1",
    }));
    inbox(&db, "role_permissions", "rp-other-role", 1, "pending", &other_role).await;
    refused(&db, "another role").await;
    // An assignment of another person.
    let other_user = row(json!({
        "user_sync_id": "someone-else",
        "role_sync_id": certifying_role,
        "scope_type": "tenant",
        "is_emergency": 0,
        "_sync_format": "row.v1",
    }));
    inbox(&db, "user_scope_assignments", "usa-other", 1, "pending", &other_user).await;
    refused(&db, "another person").await;
    // Site-scoped or emergency assignments of the approver.
    for (scope, emergency) in [("site", 0), ("tenant", 1)] {
        let assignment = row(json!({
            "user_sync_id": verifier,
            "role_sync_id": certifying_role,
            "scope_type": scope,
            "is_emergency": emergency,
            "_sync_format": "row.v1",
        }));
        inbox(&db, "user_scope_assignments", "usa-scoped", 1, "pending", &assignment).await;
        refused(&db, "site-scoped or emergency assignment").await;
    }
    // The approver holds a role here; a permission row of it for ANOTHER permission is waiting.
    let held_role = text(
        &db,
        &format!(
            "SELECT r.entity_sync_id FROM roles r WHERE NOT EXISTS (SELECT 1 FROM role_permissions rp \
             JOIN permissions p ON p.id = rp.permission_id WHERE rp.role_id = r.id AND p.name = '{}') \
             AND r.entity_sync_id IS NOT NULL ORDER BY r.id LIMIT 1",
            crate::rbac::permissions::TRN_CERTIFY
        ),
    )
    .await
    .expect("a role without the right");
    exec(
        &db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
             created_at, updated_at, row_version) VALUES ('usa-sync-27', {VERIFIER}, \
             (SELECT id FROM roles WHERE entity_sync_id = '{held_role}'), 'tenant', NULL, \
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
        ),
    )
    .await;
    let other_permission = row(json!({
        "role_sync_id": held_role,
        "permission_key": crate::rbac::permissions::PTW_ISSUE,
        "_sync_format": "row.v1",
    }));
    inbox(
        &db,
        "role_permissions",
        "rp-other-permission",
        1,
        "pending",
        &other_permission,
    )
    .await;
    refused(&db, "another permission").await;
    // The approver now holds a role: a permission row of a role they do not hold still holds nothing.
    inbox(&db, "role_permissions", "rp-other-role", 1, "pending", &other_role).await;
    refused(&db, "another role, approver holding roles").await;
    // A waiting assignment to a role not here yet, whose waiting permission row is for another
    // permission: nothing could give the right, refused.
    let new_role = row(json!({ "name": "Certificateur 27", "_sync_format": "row.v1" }));
    let new_role_assignment = row(json!({
        "user_sync_id": verifier,
        "role_sync_id": "role-new-27",
        "scope_type": "tenant",
        "is_emergency": 0,
        "_sync_format": "row.v1",
    }));
    let new_role_other_permission = row(json!({
        "role_sync_id": "role-new-27",
        "permission_key": crate::rbac::permissions::PTW_ISSUE,
        "_sync_format": "row.v1",
    }));
    inbox(&db, "roles", "role-new-27", 1, "pending", &new_role).await;
    inbox(
        &db,
        "user_scope_assignments",
        "usa-new-27",
        1,
        "pending",
        &new_role_assignment,
    )
    .await;
    inbox(
        &db,
        "role_permissions",
        "rp-new-27-other",
        1,
        "pending",
        &new_role_other_permission,
    )
    .await;
    refused(&db, "a waiting role without the right").await;
    // The role, the assignment and a permission row of that role for the right all still waiting:
    // the override waits (fail-closed toward waiting).
    let new_role_grant = row(json!({
        "role_sync_id": "role-new-27",
        "permission_key": crate::rbac::permissions::TRN_CERTIFY,
        "_sync_format": "row.v1",
    }));
    inbox(&db, "roles", "role-new-27", 1, "pending", &new_role).await;
    inbox(
        &db,
        "user_scope_assignments",
        "usa-new-27",
        1,
        "pending",
        &new_role_assignment,
    )
    .await;
    inbox(&db, "role_permissions", "rp-new-27", 1, "deferred", &new_role_grant).await;
    let outcome = apply(&db, "qualification_overrides", "qo-27", &by_verifier).await;
    assert!(
        matches!(outcome, ApplyOutcome::Deferred { .. }),
        "waiting role: {outcome:?}"
    );
    exec(&db, "DELETE FROM sync_inbox").await;

    // A waiting plant-wide assignment of the approver to a role carrying the right: wait.
    let assignment = row(json!({
        "user_sync_id": verifier,
        "role_sync_id": certifying_role,
        "scope_type": "tenant",
        "is_emergency": 0,
        "_sync_format": "row.v1",
    }));
    inbox(
        &db,
        "user_scope_assignments",
        "usa-certifying",
        1,
        "pending",
        &assignment,
    )
    .await;
    let outcome = apply(&db, "qualification_overrides", "qo-27", &by_verifier).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
}

/// The whole clearing of a rule permit flag is its audit trace: a received row that changes who
/// cleared it or why is judged in full, and a clearing recorded here is never removed nor partly
/// rewritten.
#[tokio::test]
async fn the_clearing_of_a_rule_permit_flag_is_never_removed_or_partly_rewritten() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-28").await;
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
    // Still flagged, with only a reason (or a clearer) slipped in: judged in full, refused.
    for (key, value) in [
        ("permit_rule_clear_reason", json!("Consignation faite par l'exploitant")),
        ("permit_rule_cleared_by_sync_id", json!(user_sync(&db, APPLIER).await)),
    ] {
        let outcome = apply(&db, "work_orders", &wo_sync, &with(flagged.clone(), &[(key, value)])).await;
        assert_eq!(
            code(&outcome),
            Some("WO_PERMIT_RULE_CLEAR_INVALID"),
            "{key}: {outcome:?}"
        );
    }
    // A valid clearing applies.
    let cleared = with(
        flagged.clone(),
        &[
            ("requires_permit", json!(0)),
            ("permit_rule_cleared_by_sync_id", json!(user_sync(&db, APPLIER).await)),
            ("permit_rule_cleared_at", json!(now())),
            ("permit_rule_clear_reason", json!("Consignation faite par l'exploitant")),
        ],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &cleared).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // Flagged again by the general edit (`set_permit_requirement`), the clearing kept: applies.
    let reflagged = with(cleared.clone(), &[("requires_permit", json!(1))]);
    let outcome = apply(&db, "work_orders", &wo_sync, &reflagged).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "re-flag: {outcome:?}");
    // No local action removes a recorded clearing or a part of it: a received row doing so is refused.
    for (what, changes) in [
        ("reason removed", vec![("permit_rule_clear_reason", json!(null))]),
        ("clearer removed", vec![("permit_rule_cleared_by_sync_id", json!(null))]),
        ("time removed", vec![("permit_rule_cleared_at", json!(null))]),
        (
            "reason removed, time rewritten",
            vec![
                ("permit_rule_clear_reason", json!(null)),
                ("permit_rule_cleared_at", json!("2026-01-01T00:00:00Z")),
            ],
        ),
        (
            "removed",
            vec![
                ("permit_rule_cleared_by_sync_id", json!(null)),
                ("permit_rule_cleared_at", json!(null)),
                ("permit_rule_clear_reason", json!(null)),
            ],
        ),
    ] {
        let outcome = apply(&db, "work_orders", &wo_sync, &with(reflagged.clone(), &changes)).await;
        assert_eq!(
            code(&outcome),
            Some("WO_PERMIT_RULE_CLEAR_INVALID"),
            "{what}: {outcome:?}"
        );
    }
    // Flagged again here with the clearing kept, a received row setting the flag back to "not
    // required" with the same clearing: the clearing is judged again in full (its clearer still holds
    // the permit-issue right here), so it applies.
    let outcome = apply(&db, "work_orders", &wo_sync, &cleared).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied),
        "back to not required: {outcome:?}"
    );
    assert_eq!(
        text(
            &db,
            &format!("SELECT CAST(requires_permit AS TEXT) FROM work_orders WHERE id = {wo_id}"),
        )
        .await
        .as_deref(),
        Some("0")
    );
}

/// Two PCs that each cleared the same work order's rule permit flag offline converge: the clearing
/// received over a different one recorded here is judged in full like a new one, and applied when
/// valid.
#[tokio::test]
async fn a_different_complete_clearing_received_is_judged_and_applied() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-29").await;
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
    let clearing = |by: &str, at: &str, reason: &str| {
        with(
            flagged.clone(),
            &[
                ("requires_permit", json!(0)),
                ("permit_rule_cleared_by_sync_id", json!(by)),
                ("permit_rule_cleared_at", json!(at)),
                ("permit_rule_clear_reason", json!(reason)),
            ],
        )
    };
    let recorded = || {
        let db = &db;
        async move {
            text(
                db,
                &format!(
                    "SELECT u.entity_sync_id || '|' || w.permit_rule_cleared_at || '|' || w.permit_rule_clear_reason \
                     FROM work_orders w JOIN user_accounts u ON u.id = w.permit_rule_cleared_by_id \
                     WHERE w.id = {wo_id}"
                ),
            )
            .await
        }
    };
    // This PC cleared it (X).
    let admin = user_sync(&db, APPLIER).await;
    let mine_at = now();
    let mine = clearing(&admin, &mine_at, "Consignation faite par l'exploitant");
    let outcome = apply(&db, "work_orders", &wo_sync, &mine).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let x = recorded().await;

    // Another PC cleared it too (Y): each invalid complete clearing is refused, X stays.
    let verifier = user_sync(&db, VERIFIER).await;
    let theirs_at = now();
    for (what, received, expected) in [
        (
            "short reason",
            clearing(&admin, &theirs_at, "court"),
            "WO_PERMIT_RULE_CLEAR_INVALID",
        ),
        (
            "future",
            clearing(&admin, "2999-01-01T00:00:00Z", "Autre consignation par l'exploitant"),
            "WO_PERMIT_RULE_CLEAR_INVALID",
        ),
        (
            "after the start",
            with(
                clearing(&admin, &theirs_at, "Autre consignation par l'exploitant"),
                &[("actual_start", json!("2026-01-01T00:00:00Z"))],
            ),
            "WO_PERMIT_RULE_CLEAR_INVALID",
        ),
        (
            "clearer without the right",
            clearing(&verifier, &theirs_at, "Autre consignation par l'exploitant"),
            "WO_PERMIT_RULE_CLEAR_UNAUTHORISED",
        ),
    ] {
        let outcome = apply(&db, "work_orders", &wo_sync, &received).await;
        assert_eq!(code(&outcome), Some(expected), "{what}: {outcome:?}");
        assert_eq!(recorded().await, x, "{what}: the clearing recorded here stays");
    }

    // The other clearer holds the permit-issue right here: Y is valid and applied.
    let issuing_role = text(
        &db,
        &format!(
            "SELECT r.entity_sync_id FROM roles r JOIN role_permissions rp ON rp.role_id = r.id \
             JOIN permissions p ON p.id = rp.permission_id WHERE p.name = '{}' ORDER BY r.id LIMIT 1",
            crate::rbac::permissions::PTW_ISSUE
        ),
    )
    .await
    .expect("a role carrying the right");
    exec(
        &db,
        &format!(
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
             created_at, updated_at, row_version) VALUES ('usa-sync-29', {VERIFIER}, \
             (SELECT id FROM roles WHERE entity_sync_id = '{issuing_role}'), 'tenant', NULL, \
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)"
        ),
    )
    .await;
    let theirs = clearing(&verifier, &theirs_at, "Autre consignation par l'exploitant");
    let outcome = apply(&db, "work_orders", &wo_sync, &theirs).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        recorded().await.as_deref(),
        Some(format!("{verifier}|{theirs_at}|Autre consignation par l'exploitant").as_str())
    );
    // A partial rewrite of Y is still refused.
    let partial = with(theirs.clone(), &[("permit_rule_clear_reason", json!(null))]);
    let outcome = apply(&db, "work_orders", &wo_sync, &partial).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
}

/// A change of a permit type's three-person lock-removal setting received from another PC is
/// applied only with who changed it (holding the settings right here), when and why; a row that
/// does not change the setting passes as received.
#[tokio::test]
async fn a_received_three_person_change_is_rechecked_on_its_author_and_reason() {
    let db = setup().await;
    let ty = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
        .await
        .expect("type sync id");
    let held = wire(&db, "permit_types", &ty).await;
    let chain = |sync: String| {
        let db = &db;
        async move {
            text(
                db,
                &format!(
                    "SELECT CAST(requires_three_person_removal AS TEXT) FROM permit_types WHERE entity_sync_id = '{sync}'"
                ),
            )
            .await
        }
    };
    let changed = |value: i64, by: Value, at: Value, reason: Value| {
        with(
            held.clone(),
            &[
                ("requires_three_person_removal", json!(value)),
                ("three_person_changed_by_sync_id", by),
                ("three_person_changed_at", at),
                ("three_person_change_reason", reason),
            ],
        )
    };
    let admin = json!(user_sync(&db, APPLIER).await);
    let reason = json!("Site rule after the 2026 LOTO audit");

    for (row, expected) in [
        (
            changed(1, Value::Null, Value::Null, Value::Null),
            "PERMIT_TYPE_THREE_PERSON_UNAUDITED",
        ),
        (
            changed(1, admin.clone(), json!(now()), json!("  too short  ")),
            "PERMIT_TYPE_THREE_PERSON_UNAUDITED",
        ),
        (
            changed(1, admin.clone(), Value::Null, reason.clone()),
            "PERMIT_TYPE_THREE_PERSON_UNAUDITED",
        ),
        (
            changed(1, admin.clone(), json!("2999-01-01T00:00:00Z"), reason.clone()),
            "PERMIT_TYPE_THREE_PERSON_UNAUDITED",
        ),
        (
            changed(1, Value::Null, json!(now()), reason.clone()),
            "PERMIT_TYPE_THREE_PERSON_UNAUTHORISED",
        ),
        (
            changed(1, json!(user_sync(&db, VERIFIER).await), json!(now()), reason.clone()),
            "PERMIT_TYPE_THREE_PERSON_UNAUTHORISED",
        ),
    ] {
        let outcome = apply(&db, "permit_types", &ty, &row).await;
        assert_eq!(code(&outcome), Some(expected), "{row:?} -> {outcome:?}");
        assert_eq!(chain(ty.clone()).await.as_deref(), Some("0"), "nothing written");
    }
    // The person who changed it is not here yet: waits.
    let outcome = apply(
        &db,
        "permit_types",
        &ty,
        &changed(1, json!("user-not-here"), json!(now()), reason.clone()),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(chain(ty.clone()).await.as_deref(), Some("0"));

    // By an administrator, with a reason and a time: applied.
    let on = changed(1, admin.clone(), json!(now()), reason.clone());
    let outcome = apply(&db, "permit_types", &ty, &on).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(chain(ty.clone()).await.as_deref(), Some("1"));

    // Another field changed, the setting not: passes, as received.
    let renamed = with(
        wire(&db, "permit_types", &ty).await,
        &[("name", json!("Hot work (renamed)"))],
    );
    let outcome = apply(&db, "permit_types", &ty, &renamed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // The setting turned off with the record already held here: not a recorded change.
    let replayed = with(renamed, &[("requires_three_person_removal", json!(0))]);
    let outcome = apply(&db, "permit_types", &ty, &replayed).await;
    assert_eq!(code(&outcome), Some("PERMIT_TYPE_THREE_PERSON_STALE"), "{outcome:?}");
    assert_eq!(chain(ty.clone()).await.as_deref(), Some("1"));

    // A new type: off passes without a record, on is judged the same way (with its own record: the
    // record of the change above would be that change copied).
    let new_type = |code: &str, value: i64, by: Value| {
        with(
            held.clone(),
            &[
                ("code", json!(code)),
                ("requires_three_person_removal", json!(value)),
                ("three_person_changed_by_sync_id", by),
                ("three_person_changed_at", json!(now())),
                (
                    "three_person_change_reason",
                    json!("New type for work on the press line"),
                ),
            ],
        )
    };
    let off = with(
        new_type("new_off", 0, Value::Null),
        &[
            ("three_person_changed_at", Value::Null),
            ("three_person_change_reason", Value::Null),
        ],
    );
    let outcome = apply(&db, "permit_types", "type-new-off", &off).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let outcome = apply(&db, "permit_types", "type-new-on", &new_type("new_on", 1, Value::Null)).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUTHORISED"),
        "{outcome:?}"
    );
    let outcome = apply(&db, "permit_types", "type-new-on", &new_type("new_on", 1, admin)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(chain("type-new-on".into()).await.as_deref(), Some("1"));
}

/// The author of a received three-person change whose settings right is still waiting in the inbox
/// makes it wait; with nothing waiting for that person, it is refused.
#[tokio::test]
async fn a_three_person_change_waits_for_the_rights_of_its_author() {
    let db = setup().await;
    let ty = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
        .await
        .expect("type sync id");
    let verifier = user_sync(&db, VERIFIER).await;
    let on = with(
        wire(&db, "permit_types", &ty).await,
        &[
            ("requires_three_person_removal", json!(1)),
            ("three_person_changed_by_sync_id", json!(verifier)),
            ("three_person_changed_at", json!(now())),
            (
                "three_person_change_reason",
                json!("Site rule after the 2026 LOTO audit"),
            ),
        ],
    );
    let settings_role = text(
        &db,
        &format!(
            "SELECT r.entity_sync_id FROM roles r JOIN role_permissions rp ON rp.role_id = r.id \
             JOIN permissions p ON p.id = rp.permission_id WHERE p.name = '{}' ORDER BY r.id LIMIT 1",
            crate::rbac::permissions::ADM_SETTINGS
        ),
    )
    .await
    .expect("a role carrying the settings right");
    let assignment = json!({
        "user_sync_id": verifier,
        "role_sync_id": settings_role,
        "scope_type": "tenant",
        "is_emergency": 0,
        "_sync_format": "row.v1",
    })
    .as_object()
    .cloned()
    .expect("object");
    inbox(&db, "user_scope_assignments", "usa-settings", 1, "pending", &assignment).await;
    let outcome = apply(&db, "permit_types", &ty, &on).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    exec(&db, "DELETE FROM sync_inbox").await;
    let outcome = apply(&db, "permit_types", &ty, &on).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUTHORISED"),
        "{outcome:?}"
    );
}

/// The record of a three-person change received from another PC must be a new change: dated later
/// than the change recorded here for the type, and not the record held here for another type. A row
/// that leaves the setting alone but rewrites the record is judged the same way, so the record of
/// who last changed the setting is never overwritten unchecked.
#[tokio::test]
async fn a_three_person_record_is_never_replayed_copied_or_overwritten_unchecked() {
    let db = setup().await;
    let type_sync = |code: &'static str| {
        let db = &db;
        async move {
            text(
                db,
                &format!("SELECT entity_sync_id FROM permit_types WHERE code = '{code}'"),
            )
            .await
            .expect("type sync id")
        }
    };
    let (hot, loto) = (type_sync("hot_work").await, type_sync("loto").await);
    let record_of = |sync: String| {
        let db = &db;
        async move {
            text(
                db,
                &format!(
                    "SELECT CAST(requires_three_person_removal AS TEXT) || '|' || \
                     COALESCE(three_person_changed_at, '-') || '|' || COALESCE(three_person_change_reason, '-') \
                     FROM permit_types WHERE entity_sync_id = '{sync}'"
                ),
            )
            .await
            .expect("record")
        }
    };
    let admin = json!(user_sync(&db, APPLIER).await);
    let reason = "Site rule after the 2026 LOTO audit";
    // Dates relative to now, so the test keeps its meaning whenever it runs.
    let days_ago = |days: i64| {
        (chrono::Utc::now() - chrono::Duration::days(days))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    let (earlier, before_earlier) = (days_ago(30), days_ago(31));
    let earlier = earlier.as_str();
    // Another type holds a change by the administrator at `earlier`.
    exec(
        &db,
        &format!(
            "UPDATE permit_types SET three_person_changed_by_id = {APPLIER}, three_person_changed_at = '{earlier}', \
             three_person_change_reason = '{reason}' WHERE entity_sync_id = '{loto}'"
        ),
    )
    .await;
    let changed = |base: &Payload, value: Value, at: &str, why: &str| {
        with(
            base.clone(),
            &[
                ("requires_three_person_removal", value),
                ("three_person_changed_by_sync_id", admin.clone()),
                ("three_person_changed_at", json!(at)),
                ("three_person_change_reason", json!(why)),
            ],
        )
    };
    let held = wire(&db, "permit_types", &hot).await;

    // The other type's record copied onto this one: refused.
    let copied = changed(&held, json!(1), earlier, reason);
    let outcome = apply(&db, "permit_types", &hot, &copied).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUDITED"),
        "{outcome:?}"
    );
    assert_eq!(record_of(hot.clone()).await, "0|-|-");
    // The same person and time with this change's own reason: applied.
    let own = changed(&held, json!(1), earlier, "Contractor work on the press line");
    let outcome = apply(&db, "permit_types", &hot, &own).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    // Turned off with a record dated before the one held here (an earlier change of this type
    // replayed), or at the same time: refused, the chain stays on.
    for at in [before_earlier.as_str(), earlier] {
        let replayed = changed(&own, json!(0), at, "Back to two persons for the shutdown");
        let outcome = apply(&db, "permit_types", &hot, &replayed).await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_TYPE_THREE_PERSON_STALE"),
            "{at}: {outcome:?}"
        );
        assert_eq!(
            record_of(hot.clone()).await,
            format!("1|{earlier}|Contractor work on the press line")
        );
    }

    // The setting left alone but its record rewritten: judged like a change. Wiped, or naming
    // someone without the settings right: refused, the record held here is kept.
    let current = wire(&db, "permit_types", &hot).await;
    let wiped = with(
        current.clone(),
        &[
            ("three_person_changed_by_sync_id", Value::Null),
            ("three_person_changed_at", Value::Null),
            ("three_person_change_reason", Value::Null),
        ],
    );
    let outcome = apply(&db, "permit_types", &hot, &wiped).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUDITED"),
        "{outcome:?}"
    );
    let by_verifier = with(
        changed(&current, json!(1), &now(), reason),
        &[("three_person_changed_by_sync_id", json!(user_sync(&db, VERIFIER).await))],
    );
    let outcome = apply(&db, "permit_types", &hot, &by_verifier).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUTHORISED"),
        "{outcome:?}"
    );
    assert_eq!(
        record_of(hot.clone()).await,
        format!("1|{earlier}|Contractor work on the press line")
    );
    // A later, complete record by the administrator (the setting turned off and on again
    // elsewhere): applied.
    let later = now();
    let outcome = apply(&db, "permit_types", &hot, &changed(&current, json!(1), &later, reason)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(record_of(hot.clone()).await, format!("1|{later}|{reason}"));
}

/// The received setting is read the way SQLite stores it: 1, 1.0, "1" and true are on; 0, 0.0, "0"
/// and false are off; any other value is refused. A non-canonical "on" is judged like any other.
#[tokio::test]
async fn a_received_three_person_setting_is_read_as_it_is_stored() {
    let db = setup().await;
    let ty = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
        .await
        .expect("type sync id");
    let stored = || {
        let db = &db;
        let ty = ty.clone();
        async move {
            text(
                db,
                &format!(
                    "SELECT CAST(requires_three_person_removal AS TEXT) FROM permit_types WHERE entity_sync_id = '{ty}'"
                ),
            )
            .await
        }
    };
    let admin = json!(user_sync(&db, APPLIER).await);
    let first = chrono::Utc::now() - chrono::Duration::days(1);
    let mut second = 0;
    let mut change = |base: Payload, value: Value, with_record: bool| {
        let row = with(base, &[("requires_three_person_removal", value)]);
        if !with_record {
            return row;
        }
        second += 1;
        with(
            row,
            &[
                ("three_person_changed_by_sync_id", admin.clone()),
                (
                    "three_person_changed_at",
                    json!((first + chrono::Duration::seconds(second))
                        .format("%Y-%m-%dT%H:%M:%SZ")
                        .to_string()),
                ),
                (
                    "three_person_change_reason",
                    json!("Site rule after the 2026 LOTO audit"),
                ),
            ],
        )
    };
    let held = wire(&db, "permit_types", &ty).await;

    // Values neither on nor off: refused, nothing written.
    for bad in [
        json!(2),
        json!(0.5),
        json!("yes"),
        json!("true"),
        json!(" 1"),
        Value::Null,
        json!([1]),
    ] {
        let outcome = apply(&db, "permit_types", &ty, &change(held.clone(), bad.clone(), true)).await;
        assert_eq!(code(&outcome), Some("INVALID_VALUE"), "{bad}: {outcome:?}");
        assert_eq!(stored().await.as_deref(), Some("0"));
    }
    // "1" without a record: judged as turning the chain on (not passed as unchanged).
    let outcome = apply(&db, "permit_types", &ty, &change(held.clone(), json!("1"), false)).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUDITED"),
        "{outcome:?}"
    );
    assert_eq!(stored().await.as_deref(), Some("0"));
    // Each spelling, with a record, is applied and stored as read.
    for (value, expected) in [
        (json!("1"), "1"),
        (json!("0"), "0"),
        (json!(1.0), "1"),
        (json!(0.0), "0"),
        (json!(true), "1"),
        (json!(false), "0"),
    ] {
        let received = change(wire(&db, "permit_types", &ty).await, value.clone(), true);
        let outcome = apply(&db, "permit_types", &ty, &received).await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{value}: {outcome:?}");
        assert_eq!(stored().await.as_deref(), Some(expected), "{value}");
    }
}

/// A permit keeps the type it was created with (no local edit changes it): a received permit naming
/// another type is refused, in draft as past it, and nothing is written.
#[tokio::test]
async fn a_received_permit_never_changes_its_type() {
    let db = setup().await;
    let loto = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'loto'")
        .await
        .expect("loto sync id");
    let asset = insert_equipment(&db, "EQ-SYNC-TYPE").await;
    let draft = create_permit(&db, "hot_work", asset).await;
    let issued = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    for wp in [&draft, &issued] {
        let received = with(
            wire(&db, "work_permits", &wp.entity_sync_id).await,
            &[("permit_type_sync_id", json!(loto))],
        );
        let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &received).await;
        assert_eq!(code(&outcome), Some("PERMIT_TYPE_CHANGED"), "{outcome:?}");
        let type_code = text(
            &db,
            &format!(
                "SELECT pt.code FROM work_permits wp JOIN permit_types pt ON pt.id = wp.permit_type_id \
                 WHERE wp.id = {}",
                wp.id
            ),
        )
        .await;
        assert_eq!(type_code.as_deref(), Some("hot_work"));
    }
    // The same permit with its own type: passes.
    let same = wire(&db, "work_permits", &issued.entity_sync_id).await;
    let outcome = apply(&db, "work_permits", &issued.entity_sync_id, &same).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
}

/// A type received for the first time with its setting off is applied whoever last changed it (a PC
/// paired later is never stuck on a past change of an administrator who has since lost the right);
/// its record still needs a reason and a valid time. A new type that is on is judged in full.
#[tokio::test]
async fn a_new_type_received_off_is_applied_whoever_last_changed_it() {
    let db = setup().await;
    let held = wire(
        &db,
        "permit_types",
        &text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
            .await
            .expect("type sync id"),
    )
    .await;
    // VERIFIER does not hold the settings right here (an administrator demoted since the change).
    let demoted = json!(user_sync(&db, VERIFIER).await);
    let fresh = |code: &str, value: i64, at: Value, why: &str| {
        with(
            held.clone(),
            &[
                ("code", json!(code)),
                ("requires_three_person_removal", json!(value)),
                ("three_person_changed_by_sync_id", demoted.clone()),
                ("three_person_changed_at", at),
                ("three_person_change_reason", json!(why)),
            ],
        )
    };
    let reason = "Back to two persons after the audit";
    let stored = |sync: &'static str| {
        let db = &db;
        async move {
            text(
                db,
                &format!("SELECT code FROM permit_types WHERE entity_sync_id = '{sync}'"),
            )
            .await
        }
    };
    // The time and reason checks stay.
    for (row, why) in [
        (fresh("fresh_a", 0, json!("2999-01-01T00:00:00Z"), reason), "future"),
        (fresh("fresh_a", 0, json!("not a time"), reason), "unreadable"),
        (fresh("fresh_a", 0, json!(now()), "short"), "short reason"),
    ] {
        let outcome = apply(&db, "permit_types", "type-fresh-a", &row).await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_TYPE_THREE_PERSON_UNAUDITED"),
            "{why}: {outcome:?}"
        );
        assert_eq!(stored("type-fresh-a").await, None);
    }
    let outcome = apply(
        &db,
        "permit_types",
        "type-fresh-a",
        &fresh("fresh_a", 0, json!(now()), reason),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(stored("type-fresh-a").await.as_deref(), Some("fresh_a"));
    // On: the author's right is judged.
    let outcome = apply(
        &db,
        "permit_types",
        "type-fresh-b",
        &fresh("fresh_b", 1, json!(now()), "Third person for the press line"),
    )
    .await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUTHORISED"),
        "{outcome:?}"
    );
    assert_eq!(stored("type-fresh-b").await, None);
}

/// A type that was on before records existed (no record) passes a plain edit; a change recorded
/// here at a time that cannot be read refuses every received change of the type (fails closed).
#[tokio::test]
async fn a_type_without_a_record_passes_and_an_unreadable_held_time_fails_closed() {
    let db = setup().await;
    let ty = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
        .await
        .expect("type sync id");
    exec(
        &db,
        &format!("UPDATE permit_types SET requires_three_person_removal = 1 WHERE entity_sync_id = '{ty}'"),
    )
    .await;
    let renamed = with(
        wire(&db, "permit_types", &ty).await,
        &[("name", json!("Hot work (renamed)"))],
    );
    let outcome = apply(&db, "permit_types", &ty, &renamed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    exec(
        &db,
        &format!(
            "UPDATE permit_types SET three_person_changed_by_id = {APPLIER}, three_person_changed_at = 'garbled', \
             three_person_change_reason = 'Site rule after the 2026 LOTO audit' WHERE entity_sync_id = '{ty}'"
        ),
    )
    .await;
    let off = with(
        wire(&db, "permit_types", &ty).await,
        &[
            ("requires_three_person_removal", json!(0)),
            ("three_person_changed_by_sync_id", json!(user_sync(&db, APPLIER).await)),
            ("three_person_changed_at", json!(now())),
            (
                "three_person_change_reason",
                json!("Back to two persons for the shutdown"),
            ),
        ],
    );
    let outcome = apply(&db, "permit_types", &ty, &off).await;
    assert_eq!(code(&outcome), Some("PERMIT_TYPE_THREE_PERSON_STALE"), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT CAST(requires_three_person_removal AS TEXT) FROM permit_types WHERE entity_sync_id = '{ty}'"
            )
        )
        .await
        .as_deref(),
        Some("1")
    );
}

/// Owner decision, re-checked on arrival like here: a received start on a permit that expires
/// before the work order's planned end is a conflict, nothing written.
#[tokio::test]
async fn a_received_start_on_a_permit_expiring_before_the_planned_end_is_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-95").await;
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
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-95 breaker").await).await;
    set_status(&db, &wp, "active").await.expect("activate");
    let status_sql = format!(
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = {wo_id}"
    );
    let started = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[("status_sync_id", json!(in_progress)), ("actual_start", json!(now()))],
    );
    let later = (chrono::Utc::now() + chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(started.clone(), &[("planned_end", json!(later))]),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_EXPIRES_BEFORE_PLANNED_END"), "{outcome:?}");
    assert_eq!(text(&db, &status_sql).await.as_deref(), Some("ready"));
    // An unreadable planned end (text, or a bare day) is refused too, as locally, under its own code.
    for bad in ["someday", "2026-10-20"] {
        let outcome = apply(
            &db,
            "work_orders",
            &wo_sync,
            &with(started.clone(), &[("planned_end", json!(bad))]),
        )
        .await;
        assert_eq!(code(&outcome), Some("WO_PLANNED_END_UNREADABLE"), "{bad}: {outcome:?}");
    }
    // A planned end typed without a zone is read in this computer's time zone, as locally.
    let typed = (chrono::Local::now() + chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M")
        .to_string();
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(started.clone(), &[("planned_end", json!(typed))]),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_EXPIRES_BEFORE_PLANNED_END"), "{outcome:?}");

    let sooner = (chrono::Utc::now() + chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(started, &[("planned_end", json!(sooner))]),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(text(&db, &status_sql).await.as_deref(), Some("in_progress"));
}

/// A work order ready to start, with an active hot-work permit active since `active_since`, and its
/// received "started" row without a planned end. Returns (work order sync id, received row, permit
/// expiry, local work order id).
async fn ready_wo_with_permit_active_since(
    db: &DatabaseConnection,
    asset_code: &str,
    active_since: chrono::DateTime<chrono::Utc>,
) -> (String, Payload, chrono::DateTime<chrono::Utc>, i64) {
    let asset = insert_equipment(db, asset_code).await;
    let (wo_id, _) = assigned_wo_requiring_permit(db, asset).await;
    let wo_sync = text(
        db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let in_progress = text(
        db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'in_progress'",
    )
    .await
    .expect("status sync id");
    let wp = advance_to_issued(db, create_from_wo(db, "hot_work", wo_id).await.expect("permit")).await;
    let wp = set_status(db, &wp, "active").await.expect("activate");
    exec(
        db,
        &format!(
            "UPDATE work_permits SET activated_at = '{}' WHERE id = {}",
            active_since.format("%Y-%m-%dT%H:%M:%SZ"),
            wp.id
        ),
    )
    .await;
    let expiry = crate::settings::site::instant(wp.expires_at.as_deref().expect("expiry")).expect("expiry time");
    let row = with(
        wire(db, "work_orders", &wo_sync).await,
        &[
            ("status_sync_id", json!(in_progress)),
            ("planned_end", Value::Null),
            ("updated_at", json!(now())),
        ],
    );
    (wo_sync, row, expiry, wo_id)
}

/// Hours from `from` to `minutes` before (negative: after) the permit's expiry.
fn hours_until(from: chrono::DateTime<chrono::Utc>, expiry: chrono::DateTime<chrono::Utc>, minutes: i64) -> f64 {
    ((expiry - from).num_seconds() - minutes * 60) as f64 / 3600.0
}

/// CMMS Advisor decision (2026-10-19), re-checked on arrival like here: without a planned end, a
/// received start is judged against the start time the row records plus the expected duration,
/// never this PC's clock.
#[tokio::test]
async fn a_received_start_without_a_planned_end_is_judged_from_its_recorded_start() {
    let db = setup().await;
    let started_at = chrono::Utc::now() - chrono::Duration::hours(2);
    let (wo_sync, row, expiry, wo_id) =
        ready_wo_with_permit_active_since(&db, "EQ-SYNC-96", started_at - chrono::Duration::hours(1)).await;
    let started = with(
        row,
        &[(
            "actual_start",
            json!(started_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        )],
    );
    let status_sql = format!(
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = {wo_id}"
    );
    // The work, from its recorded start, ends 30 minutes after the expiry: refused, nothing written.
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(
            started.clone(),
            &[("expected_duration_hours", json!(hours_until(started_at, expiry, -30)))],
        ),
    )
    .await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_EXPIRES_BEFORE_ESTIMATED_END"),
        "{outcome:?}"
    );
    assert_eq!(text(&db, &status_sql).await.as_deref(), Some("ready"));
    // From its recorded start it ends 30 minutes before the expiry: applied (judged from this PC's
    // clock, two hours later, it would end after the expiry).
    let covered = hours_until(started_at, expiry, 30);
    assert!(chrono::Utc::now() + chrono::Duration::seconds((covered * 3600.0) as i64) > expiry);
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(started, &[("expected_duration_hours", json!(covered))]),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(text(&db, &status_sql).await.as_deref(), Some("in_progress"));
}

/// The same on a resume: the moment judged is the latest return to in progress the row's history
/// records (here a resume in the transition log), not its first start.
#[tokio::test]
async fn a_received_resume_without_a_planned_end_is_judged_from_its_recorded_resume() {
    let db = setup().await;
    let started_at = chrono::Utc::now() - chrono::Duration::hours(3);
    let resumed_at = chrono::Utc::now() - chrono::Duration::hours(1);
    let (wo_sync, row, expiry, wo_id) =
        ready_wo_with_permit_active_since(&db, "EQ-SYNC-97", started_at - chrono::Duration::hours(1)).await;
    exec(
        &db,
        &format!(
            "INSERT INTO wo_state_transition_log (wo_id, from_status, to_status, action, actor_id, acted_at, lifecycle_state) \
             VALUES ({wo_id}, 'on_hold', 'in_progress', 'resume', 1, '{}', 'in_progress')",
            resumed_at.format("%Y-%m-%dT%H:%M:%SZ")
        ),
    )
    .await;
    let started = with(
        row,
        &[(
            "actual_start",
            json!(started_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        )],
    );
    // Covered from the first start, not from the resume: refused.
    let hours = hours_until(started_at, expiry, 30);
    assert!(resumed_at + chrono::Duration::seconds((hours * 3600.0) as i64) > expiry);
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(started.clone(), &[("expected_duration_hours", json!(hours))]),
    )
    .await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_EXPIRES_BEFORE_ESTIMATED_END"),
        "{outcome:?}"
    );
    // Covered from the resume: applied.
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(
            started,
            &[("expected_duration_hours", json!(hours_until(resumed_at, expiry, 30)))],
        ),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// Neither a planned end nor an expected duration: a received start is not judged on the expiry
/// (the permit was valid when the work started); an expired permit is still refused.
#[tokio::test]
async fn a_received_start_without_any_end_is_not_judged_but_an_expired_permit_is() {
    let db = setup().await;
    let started_at = chrono::Utc::now() - chrono::Duration::hours(1);
    let (wo_sync, row, _, wo_id) =
        ready_wo_with_permit_active_since(&db, "EQ-SYNC-98", started_at - chrono::Duration::hours(1)).await;
    let started = with(
        row,
        &[
            ("expected_duration_hours", Value::Null),
            (
                "actual_start",
                json!(started_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            ),
        ],
    );
    let set_expiry = |at: chrono::DateTime<chrono::Utc>| {
        let db = &db;
        async move {
            exec(
                db,
                &format!(
                    "UPDATE work_permits SET expires_at = '{}' WHERE linked_work_order_id = {wo_id}",
                    at.format("%Y-%m-%dT%H:%M:%SZ")
                ),
            )
            .await;
        }
    };
    // The permit had expired when the work started: refused.
    set_expiry(started_at - chrono::Duration::minutes(5)).await;
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert_eq!(code(&outcome), Some("PERMIT_EXPIRED"), "{outcome:?}");
    // Valid when the work started (expired since, ten minutes later): with no end to judge, applied.
    set_expiry(started_at + chrono::Duration::minutes(10)).await;
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// Owner decision: switching a permit type's energy-isolation rule off on another PC is re-checked
/// on arrival like the three-person setting (who, when, why, the settings and permit-issue rights
/// here); switching it on is a plain edit; the shipped LOTO type's rule is never switched off.
#[tokio::test]
async fn a_received_isolation_switch_off_is_rechecked_and_loto_stays_locked() {
    let db = setup().await;
    let sync_of = |code: &'static str| {
        let db = &db;
        async move {
            text(
                db,
                &format!("SELECT entity_sync_id FROM permit_types WHERE code = '{code}'"),
            )
            .await
            .expect("type sync id")
        }
    };
    let (hot, loto) = (sync_of("hot_work").await, sync_of("loto").await);
    exec(
        &db,
        &format!("UPDATE permit_types SET requires_energy_isolation = 1 WHERE entity_sync_id = '{hot}'"),
    )
    .await;
    let isolation = |sync: String| {
        let db = &db;
        async move {
            text(
                db,
                &format!(
                    "SELECT CAST(requires_energy_isolation AS TEXT) FROM permit_types WHERE entity_sync_id = '{sync}'"
                ),
            )
            .await
        }
    };
    let admin = json!(user_sync(&db, APPLIER).await);
    let reason = "Hot work on isolated lines only";
    let off = |base: &Payload, by: Value, at: Value, why: Value| {
        with(
            base.clone(),
            &[
                ("requires_energy_isolation", json!(0)),
                ("isolation_changed_by_sync_id", by),
                ("isolation_changed_at", at),
                ("isolation_change_reason", why),
            ],
        )
    };
    let held = wire(&db, "permit_types", &hot).await;
    for (row, expected) in [
        (
            off(&held, Value::Null, Value::Null, Value::Null),
            "PERMIT_TYPE_ISOLATION_UNAUDITED",
        ),
        (
            off(&held, admin.clone(), json!(now()), json!("short")),
            "PERMIT_TYPE_ISOLATION_UNAUDITED",
        ),
        (
            off(&held, admin.clone(), json!("2999-01-01T00:00:00Z"), json!(reason)),
            "PERMIT_TYPE_ISOLATION_UNAUDITED",
        ),
        (
            off(&held, Value::Null, json!(now()), json!(reason)),
            "PERMIT_TYPE_ISOLATION_UNAUTHORISED",
        ),
        (
            off(
                &held,
                json!(user_sync(&db, VERIFIER).await),
                json!(now()),
                json!(reason),
            ),
            "PERMIT_TYPE_ISOLATION_UNAUTHORISED",
        ),
    ] {
        let outcome = apply(&db, "permit_types", &hot, &row).await;
        assert_eq!(code(&outcome), Some(expected), "{row:?} -> {outcome:?}");
        assert_eq!(isolation(hot.clone()).await.as_deref(), Some("1"), "nothing written");
    }
    // By an administrator, with a reason and a time: applied, with its record.
    let first = (chrono::Utc::now() - chrono::Duration::minutes(10))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let switched = off(&held, admin.clone(), json!(first), json!(reason));
    let outcome = apply(&db, "permit_types", &hot, &switched).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(isolation(hot.clone()).await.as_deref(), Some("0"));
    // On again: a plain edit, applied as received (the record kept).
    let on = with(
        wire(&db, "permit_types", &hot).await,
        &[("requires_energy_isolation", json!(1))],
    );
    let outcome = apply(&db, "permit_types", &hot, &on).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // Off again with the record already held here: an earlier change replayed.
    let replayed = with(
        wire(&db, "permit_types", &hot).await,
        &[("requires_energy_isolation", json!(0))],
    );
    let outcome = apply(&db, "permit_types", &hot, &replayed).await;
    assert_eq!(code(&outcome), Some("PERMIT_TYPE_ISOLATION_STALE"), "{outcome:?}");
    assert_eq!(isolation(hot.clone()).await.as_deref(), Some("1"));

    // The shipped LOTO type: refused whoever switched it off.
    let loto_off = off(
        &wire(&db, "permit_types", &loto).await,
        admin.clone(),
        json!(now()),
        json!(reason),
    );
    let outcome = apply(&db, "permit_types", &loto, &loto_off).await;
    assert_eq!(code(&outcome), Some("PERMIT_TYPE_LOTO_ISOLATION_LOCKED"), "{outcome:?}");
    assert_eq!(isolation(loto.clone()).await.as_deref(), Some("1"));

    // A type new here: off is how a type is created; its record is checked for its reason and time
    // only, whoever made it.
    let new_off = with(
        off(
            &held,
            json!(user_sync(&db, VERIFIER).await),
            json!(now()),
            json!(reason),
        ),
        &[("code", json!("new_iso_off"))],
    );
    let outcome = apply(&db, "permit_types", "type-new-iso-off", &new_off).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// The update's restore of the shipped LOTO type's isolation rule (migration 241, made by each PC
/// itself: nobody, a fixed reason) passes on arrival both ways: from a PC that needed it to one that
/// did not, and back. The same record on another type, or naming a person, is judged as usual.
#[tokio::test]
async fn the_updates_loto_isolation_restore_passes_both_ways() {
    let db = setup().await;
    let loto = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'loto'")
        .await
        .expect("loto");
    let hot = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
        .await
        .expect("hot work");
    let restore = |base: &Payload, by: Value, at: &str| {
        with(
            base.clone(),
            &[
                ("requires_energy_isolation", json!(1)),
                ("isolation_changed_by_sync_id", by),
                ("isolation_changed_at", json!(at)),
                (
                    "isolation_change_reason",
                    json!(crate::permit::queries::LOTO_ISOLATION_RESTORE_REASON),
                ),
            ],
        )
    };
    let record = |sync: String| {
        let db = &db;
        async move {
            text(
                db,
                &format!(
                    "SELECT COALESCE(isolation_changed_at, '-') || '|' || COALESCE(isolation_change_reason, '-') \
                     FROM permit_types WHERE entity_sync_id = '{sync}'"
                ),
            )
            .await
        }
    };
    // This PC kept LOTO on (no record); a PC that needed the restore sends its LOTO row: applied.
    let held = wire(&db, "permit_types", &loto).await;
    let restored_there = restore(&held, Value::Null, &now());
    let outcome = apply(
        &db,
        "permit_types",
        &loto,
        &with(restored_there.clone(), &[("name", json!("LOTO (A)"))]),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(
        record(loto.clone())
            .await
            .is_some_and(|r| r.ends_with(crate::permit::queries::LOTO_ISOLATION_RESTORE_REASON)),
        "the update's record travels with the row"
    );
    // Its own restore made later on another PC (another time): applied too.
    let later = (chrono::Utc::now() + chrono::Duration::minutes(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let outcome = apply(&db, "permit_types", &loto, &restore(&held, Value::Null, &later)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // And back: a PC that did not need it sends its LOTO row without a record: applied.
    let unrecorded = with(
        wire(&db, "permit_types", &loto).await,
        &[
            ("isolation_changed_by_sync_id", Value::Null),
            ("isolation_changed_at", Value::Null),
            ("isolation_change_reason", Value::Null),
            ("name", json!("LOTO (B)")),
        ],
    );
    let outcome = apply(&db, "permit_types", &loto, &unrecorded).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // Switching LOTO off stays refused, whatever record it carries.
    let off = with(
        restore(&wire(&db, "permit_types", &loto).await, Value::Null, &now()),
        &[("requires_energy_isolation", json!(0))],
    );
    let outcome = apply(&db, "permit_types", &loto, &off).await;
    assert_eq!(code(&outcome), Some("PERMIT_TYPE_LOTO_ISOLATION_LOCKED"), "{outcome:?}");

    // The same record on another type is no restore by the update: judged (nobody named).
    let outcome = apply(
        &db,
        "permit_types",
        &hot,
        &with(
            restore(&wire(&db, "permit_types", &hot).await, Value::Null, &now()),
            &[("requires_energy_isolation", json!(0))],
        ),
    )
    .await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_ISOLATION_UNAUTHORISED"),
        "{outcome:?}"
    );
}

/// Owner decision 5: a received change of either safety setting needs an author who holds both the
/// settings right and the permit-issue right here; the settings right alone is refused.
#[tokio::test]
async fn a_received_setting_change_by_a_settings_holder_without_the_permit_issue_right_is_refused() {
    let db = setup().await;
    user_with(&db, "settings_only", &[crate::rbac::permissions::ADM_SETTINGS]).await;
    let hot = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
        .await
        .expect("hot work");
    exec(
        &db,
        &format!(
            "UPDATE permit_types SET requires_energy_isolation = 1, requires_three_person_removal = 1 \
             WHERE entity_sync_id = '{hot}'"
        ),
    )
    .await;
    let held = wire(&db, "permit_types", &hot).await;
    let author = json!("settings_only-sync");
    let reason = json!("Changed by the settings holder only");
    let isolation_off = with(
        held.clone(),
        &[
            ("requires_energy_isolation", json!(0)),
            ("isolation_changed_by_sync_id", author.clone()),
            ("isolation_changed_at", json!(now())),
            ("isolation_change_reason", reason.clone()),
        ],
    );
    let outcome = apply(&db, "permit_types", &hot, &isolation_off).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_ISOLATION_UNAUTHORISED"),
        "{outcome:?}"
    );
    let three_person_off = with(
        held,
        &[
            ("requires_three_person_removal", json!(0)),
            ("three_person_changed_by_sync_id", author),
            ("three_person_changed_at", json!(now())),
            ("three_person_change_reason", reason),
        ],
    );
    let outcome = apply(&db, "permit_types", &hot, &three_person_off).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_THREE_PERSON_UNAUTHORISED"),
        "{outcome:?}"
    );
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT CAST(requires_energy_isolation AS TEXT) || CAST(requires_three_person_removal AS TEXT) \
                 FROM permit_types WHERE entity_sync_id = '{hot}'"
            )
        )
        .await
        .as_deref(),
        Some("11"),
        "nothing written"
    );
}

/// A permit type received from another PC never changes its code (compared exactly, as stored, and
/// only text can match: a number, true/false or null is refused too); its other fields are applied
/// as received.
#[tokio::test]
async fn a_received_permit_type_never_changes_its_code() {
    let db = setup().await;
    let ty = text(&db, "SELECT entity_sync_id FROM permit_types WHERE code = 'hot_work'")
        .await
        .expect("type sync id");
    let held = wire(&db, "permit_types", &ty).await;
    for changed in ["hot_work_2", "hot_work ", "HOT_WORK"] {
        let outcome = apply(
            &db,
            "permit_types",
            &ty,
            &with(held.clone(), &[("code", json!(changed))]),
        )
        .await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_TYPE_CODE_LOCKED"),
            "{changed}: {outcome:?}"
        );
    }
    for changed in [json!(123), json!(0), json!(1.5), json!(true), json!(false), Value::Null] {
        let outcome = apply(
            &db,
            "permit_types",
            &ty,
            &with(held.clone(), &[("code", changed.clone())]),
        )
        .await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_TYPE_CODE_LOCKED"),
            "{changed}: {outcome:?}"
        );
    }
    assert_eq!(
        text(
            &db,
            &format!("SELECT code FROM permit_types WHERE entity_sync_id = '{ty}'")
        )
        .await
        .as_deref(),
        Some("hot_work")
    );
    let renamed = with(held, &[("name", json!("Hot work (renamed)"))]);
    let outcome = apply(&db, "permit_types", &ty, &renamed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A clearing of a rule permit flag is valid only before the work started: a clearing held here is
/// judged again when a received row moves the start before it, and a work order past "ready" with
/// no start time counts as started (a clearing received with it is refused).
#[tokio::test]
async fn a_clearing_is_judged_against_a_received_start_and_a_start_without_time() {
    let db = setup().await;
    let status_sync = |code: &'static str| {
        let db = &db;
        async move {
            text(
                db,
                &format!("SELECT entity_sync_id FROM work_order_statuses WHERE code = '{code}'"),
            )
            .await
            .expect("status sync id")
        }
    };
    let (in_progress, on_hold) = (status_sync("in_progress").await, status_sync("on_hold").await);
    let admin = json!(user_sync(&db, APPLIER).await);
    let reason = json!("Consignation faite par l'exploitant");
    let at = |minutes_ago: i64| {
        (chrono::Utc::now() - chrono::Duration::minutes(minutes_ago))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    let flagged_wo = |asset: &'static str| {
        let db = &db;
        async move {
            let asset = insert_equipment(db, asset).await;
            let (wo_id, _) = assigned_wo_requiring_permit(db, asset).await;
            exec(
                db,
                &format!(
                    "UPDATE work_orders SET permit_rule_code = 'rule-elec', requires_permit = 1 WHERE id = {wo_id}"
                ),
            )
            .await;
            text(
                db,
                &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
            )
            .await
            .expect("wo sync id")
        }
    };
    let clearing = |base: Payload, cleared_at: String| {
        with(
            base,
            &[
                ("requires_permit", json!(0)),
                ("permit_rule_cleared_by_sync_id", admin.clone()),
                ("permit_rule_cleared_at", json!(cleared_at)),
                ("permit_rule_clear_reason", reason.clone()),
            ],
        )
    };

    // A clearing recorded here an hour ago, then a received start half an hour before it: refused.
    let wo = flagged_wo("EQ-SYNC-48A").await;
    let outcome = apply(
        &db,
        "work_orders",
        &wo,
        &clearing(wire(&db, "work_orders", &wo).await, at(60)),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let held = wire(&db, "work_orders", &wo).await;
    let early = with(
        held.clone(),
        &[("status_sync_id", json!(in_progress)), ("actual_start", json!(at(90)))],
    );
    let outcome = apply(&db, "work_orders", &wo, &early).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    // Started after the clearing: applied.
    let later = with(
        held,
        &[("status_sync_id", json!(in_progress)), ("actual_start", json!(at(30)))],
    );
    let outcome = apply(&db, "work_orders", &wo, &later).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    // A clearing received with the work order on hold and no start time: it counts as started.
    let wo = flagged_wo("EQ-SYNC-48B").await;
    let base = with(
        wire(&db, "work_orders", &wo).await,
        &[("status_sync_id", json!(on_hold))],
    );
    let outcome = apply(&db, "work_orders", &wo, &clearing(base, at(5))).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            &format!("SELECT CAST(requires_permit AS TEXT) FROM work_orders WHERE entity_sync_id = '{wo}'")
        )
        .await
        .as_deref(),
        Some("1")
    );
}

/// A hand-over log received from a PC that has not upgraded, with free-text roles, adds those roles
/// to the plant's list (flagged "à revoir") when it arrives, not at the next start of this PC.
#[tokio::test]
async fn a_received_free_text_handover_role_joins_the_list_on_arrival() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-90").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    let log = json!({
        "permit_sync_id": wp.entity_sync_id,
        "handed_from_role": "Équipe nuit",
        "handed_to_role": "Maintenance",
        "confirmation_note": "Zone left clean",
        "signed_at": now(),
        "row_version": 1,
        "_sync_format": "row.v1",
    })
    .as_object()
    .cloned()
    .expect("object");
    let outcome = apply(&db, "permit_handover_logs", "ho-from-old-pc", &log).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let roles = crate::permit::handover_roles::list_handover_roles(&db)
        .await
        .expect("roles");
    let night = roles
        .iter()
        .find(|r| r.code == "Équipe nuit")
        .expect("adopted on arrival");
    assert!(night.review_required);
    assert_eq!(
        roles.iter().filter(|r| r.code == "Maintenance").count(),
        1,
        "already in the list"
    );
}

/// A received hand-over log adopts only its own two roles (not every stored text, which the
/// start-up integrity pass handles), and writes nothing when both are already in the list.
#[tokio::test]
async fn a_received_handover_log_adopts_only_its_own_roles() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-91").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    // A free text stored here earlier (an older version wrote it), not adopted yet.
    exec(
        &db,
        &format!(
            "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
             confirmation_note, signed_at, row_version) VALUES ('ho-stored-text', {}, 'Ancien texte', 'HSE', \
             'Zone left clean', '2026-10-03T00:00:00Z', 1)",
            wp.id
        ),
    )
    .await;
    let log = |from: &str, to: &str| {
        json!({
            "permit_sync_id": wp.entity_sync_id,
            "handed_from_role": from,
            "handed_to_role": to,
            "confirmation_note": "Zone left clean",
            "signed_at": now(),
            "row_version": 1,
            "_sync_format": "row.v1",
        })
        .as_object()
        .cloned()
        .expect("object")
    };
    let values = || text(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM reference_values");
    // Both roles already in the list: nothing is written to the list.
    let before = values().await;
    let outcome = apply(&db, "permit_handover_logs", "ho-listed", &log("Maintenance", "HSE")).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(values().await, before);

    // One free text: that one only joins the list (the older stored text waits for the integrity pass).
    let outcome = apply(
        &db,
        "permit_handover_logs",
        "ho-free",
        &log("Chef de quart", "Maintenance"),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let codes: Vec<String> = crate::permit::handover_roles::list_handover_roles(&db)
        .await
        .expect("roles")
        .into_iter()
        .map(|r| r.code)
        .collect();
    assert!(codes.contains(&"Chef de quart".to_string()), "{codes:?}");
    assert!(!codes.contains(&"Ancien texte".to_string()), "{codes:?}");
    // The integrity pass at start-up stays the catch-all.
    crate::permit::handover_roles::adopt_stored_roles(&db)
        .await
        .expect("integrity");
    let codes: Vec<String> = crate::permit::handover_roles::list_handover_roles(&db)
        .await
        .expect("roles")
        .into_iter()
        .map(|r| r.code)
        .collect();
    assert!(codes.contains(&"Ancien texte".to_string()), "{codes:?}");
}

/// A problem with the list itself (here: no published version left) never turns the received
/// hand-over logs into sync conflicts: the log is applied, the list is not created or changed on
/// arrival, and the role waits for the integrity pass.
#[tokio::test]
async fn a_list_problem_does_not_refuse_received_handover_logs() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-92").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    exec(
        &db,
        "UPDATE reference_sets SET status = 'superseded' WHERE domain_id = \
         (SELECT id FROM reference_domains WHERE code = 'PERMIT.HANDOVER_ROLES')",
    )
    .await;
    let sets = || {
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM reference_sets WHERE domain_id = \
             (SELECT id FROM reference_domains WHERE code = 'PERMIT.HANDOVER_ROLES')",
        )
    };
    let before = sets().await;
    for (n, role) in ["Équipe A", "Équipe B"].iter().enumerate() {
        let log = json!({
            "permit_sync_id": wp.entity_sync_id,
            "handed_from_role": role,
            "handed_to_role": "Maintenance",
            "confirmation_note": "Zone left clean",
            "signed_at": now(),
            "row_version": 1,
            "_sync_format": "row.v1",
        })
        .as_object()
        .cloned()
        .expect("object");
        let outcome = apply(&db, "permit_handover_logs", &format!("ho-no-list-{n}"), &log).await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{role}: {outcome:?}");
    }
    assert_eq!(sets().await, before, "no list version created on arrival");
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM permit_handover_logs WHERE entity_sync_id LIKE 'ho-no-list-%'"
        )
        .await
        .as_deref(),
        Some("2")
    );
}
