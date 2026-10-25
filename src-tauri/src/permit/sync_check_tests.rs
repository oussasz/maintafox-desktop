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
    // A draft permit is not deleted either: no screen deletes a permit (it is cancelled), and its
    // lines would be left without it.
    let draft = create_permit(&db, "loto", asset).await;
    let outcome = delete(&db, "work_permits", &draft.entity_sync_id).await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
    assert!(count("work_permits", draft.entity_sync_id.clone()).await, "draft kept");
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
            ("removed_by_sync_id", json!(applier)),
            ("removed_at", json!(now())),
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

    // Lock taken off by its applier, its removal verified by a second person while the permit is
    // active: applied.
    let removal = with(
        wire(&db, "permit_isolations", &first.entity_sync_id).await,
        &[
            ("removal_verified_by_sync_id", json!(verifier)),
            ("removal_verified_at", json!(now())),
            ("removed_by_sync_id", json!(applier)),
            ("removed_at", json!(now())),
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
            ("removed_by_sync_id", json!(user_sync(&db, APPLIER).await)),
            ("removed_at", json!(now())),
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

    // Another rule, or another permit type for the rule, once the work order has started: refused
    // (before, the PCs converge: `two_rules_derived_on_two_lists_converge_without_a_conflict`).
    let on_hold = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'on_hold'",
    )
    .await
    .expect("status sync id");
    for changes in [
        vec![
            ("permit_rule_code", json!("rule-other")),
            ("status_sync_id", json!(on_hold)),
        ],
        vec![
            ("permit_rule_permit_type", json!("hot_work")),
            ("status_sync_id", json!(on_hold)),
        ],
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
    // No rule at all, from a PC that has not applied the rule list (AU-04 B2): applied, never
    // refused, and the rule held here is kept with its permit type and requirement (R2-B1).
    let without = with(
        flagged.clone(),
        &[
            ("permit_rule_code", Value::Null),
            ("permit_rule_permit_type", Value::Null),
            ("requires_permit", json!(0)),
        ],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &without).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT permit_rule_code || '/' || permit_rule_permit_type || '/' || requires_permit \
                 FROM work_orders WHERE id = {wo_id}"
            )
        )
        .await
        .as_deref(),
        Some("rule-elec/loto/1")
    );
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
    super::tests::grant_tenant_right(&db, VERIFIER, crate::rbac::permissions::PTW_CLOSE).await;
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
                ("removed_by_sync_id", json!(user_sync(&db, APPLIER).await)),
                ("removed_at", json!(now())),
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
    // That re-tick (an older build keeps the clearing) ends protected here: the clearing is withdrawn
    // and recorded.
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT CAST(requires_permit AS TEXT) || '|' || COALESCE(permit_rule_cleared_at, '') \
                 FROM work_orders WHERE id = {wo_id}"
            ),
        )
        .await
        .as_deref(),
        Some("1|")
    );
    assert!(
        text(
            &db,
            &format!(
                "SELECT payload_json FROM wo_action_events WHERE wo_id = {wo_id} \
                 AND action_code = 'permit_clearing_withdrawn'"
            ),
        )
        .await
        .is_some_and(|p| p.contains("received_retick")),
        "recorded"
    );
    // No local action removes a part of a recorded clearing, or removes it without requiring the
    // permit again: a received row doing so is refused.
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
            "removed, not required",
            vec![
                ("requires_permit", json!(0)),
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
    // Required again on a PC that withdraws the clearing (`withdraw_clearing_on_retick`): the received
    // re-tick, the whole clearing removed, is applied and the flag is protected again here.
    let reticked = with(
        cleared.clone(),
        &[
            ("requires_permit", json!(1)),
            ("permit_rule_cleared_by_sync_id", json!(null)),
            ("permit_rule_cleared_at", json!(null)),
            ("permit_rule_clear_reason", json!(null)),
        ],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &reticked).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "re-tick: {outcome:?}");
    assert_eq!(
        text(
            &db,
            &format!(
                "SELECT CAST(requires_permit AS TEXT) || '|' || COALESCE(permit_rule_cleared_at, '') \
                 FROM work_orders WHERE id = {wo_id}"
            ),
        )
        .await
        .as_deref(),
        Some("1|")
    );
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(reticked, &[("requires_permit", json!(0))]),
    )
    .await;
    assert_eq!(
        code(&outcome),
        Some("WO_PERMIT_RULE_CLEAR_INVALID"),
        "unticked again without a clearing: {outcome:?}"
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

/// A second site and equipment installed on it (sync ids `permit-test-org-002`, `eq-sync-<code>`).
async fn equipment_on_second_site(db: &DatabaseConnection, code: &str) -> i64 {
    // The first site and its scaffolding.
    let _ = insert_equipment(db, &format!("{code}-FIRST")).await;
    exec(
        db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, \
         structure_model_id) VALUES (2, 'permit-test-org-002', 'SITE-PTW-2', 'Permit Test Site 2', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('eq-sync-{code}', '{code}', 'Second site asset', 'active_in_service', 2, \
             datetime('now'), datetime('now'))"
        ),
    )
    .await;
    super::tests::scalar_i64(db, &format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'")).await
}

/// A permit's equipment or work order changed on another PC is judged on arrival against the copy
/// held here: approved here, or with a lock fitted or an isolation planned here, it is a conflict
/// with nothing written; the work order is on the equipment, and the site is theirs.
#[tokio::test]
async fn a_received_equipment_change_is_rechecked_against_the_permit_held_here() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-EQ1").await;
    let same_site = insert_equipment(&db, "EQ-SYNC-EQ2").await;
    let far = equipment_on_second_site(&db, "EQ-SYNC-EQ3").await;
    let asset_sync = |code: &str| json!(format!("eq-sync-{code}"));

    // Approved here: approved for its equipment, the change is refused.
    let approved = create_permit(&db, "hot_work", asset).await;
    let approved = set_status(&db, &approved, "pending_review").await.expect("submit");
    let approved = set_status(&db, &approved, "approved").await.expect("approve");
    let moved = with(
        wire(&db, "work_permits", &approved.entity_sync_id).await,
        &[("asset_sync_id", asset_sync("EQ-SYNC-EQ2"))],
    );
    let outcome = apply(&db, "work_permits", &approved.entity_sync_id, &moved).await;
    assert_eq!(code(&outcome), Some("PERMIT_EQUIPMENT_LOCKED"), "{outcome:?}");

    // A draft here with an isolation point planned, or with a lock fitted: refused.
    let planned = create_permit(&db, "loto", asset).await;
    add_isolation(&db, planned.id, "MCC-7 breaker").await;
    let moved = with(
        wire(&db, "work_permits", &planned.entity_sync_id).await,
        &[("asset_sync_id", asset_sync("EQ-SYNC-EQ2"))],
    );
    let outcome = apply(&db, "work_permits", &planned.entity_sync_id, &moved).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_EQUIPMENT_ISOLATIONS_PLANNED"),
        "{outcome:?}"
    );
    let fitted = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, fitted.id, "MCC-8 breaker").await;
    queries::apply_permit_isolation(&db, super::tests::action(&iso), APPLIER)
        .await
        .expect("fit the lock");
    let moved = with(
        wire(&db, "work_permits", &fitted.entity_sync_id).await,
        &[("asset_sync_id", asset_sync("EQ-SYNC-EQ2"))],
    );
    let outcome = apply(&db, "work_permits", &fitted.entity_sync_id, &moved).await;
    assert_eq!(code(&outcome), Some("PERMIT_EQUIPMENT_LOCK_FITTED"), "{outcome:?}");

    // A draft without isolations: the site must be the new equipment's.
    let draft = create_permit(&db, "hot_work", asset).await;
    let held = wire(&db, "work_permits", &draft.entity_sync_id).await;
    let stale_site = with(held.clone(), &[("asset_sync_id", asset_sync("EQ-SYNC-EQ3"))]);
    let outcome = apply(&db, "work_permits", &draft.entity_sync_id, &stale_site).await;
    assert_eq!(code(&outcome), Some("PERMIT_SITE_NOT_EQUIPMENT_SITE"), "{outcome:?}");
    let draft_now = queries::get_work_permit(&db, draft.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!(draft_now.asset_id, asset, "nothing written");
    // Moved with its site, and sent back to draft from review on the other PC: applied.
    let followed = with(
        held.clone(),
        &[
            ("asset_sync_id", asset_sync("EQ-SYNC-EQ3")),
            ("entity_sync_id", json!("permit-test-org-002")),
        ],
    );
    let outcome = apply(&db, "work_permits", &draft.entity_sync_id, &followed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let draft_now = queries::get_work_permit(&db, draft.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!((draft_now.asset_id, draft_now.entity_id), (far, 2));

    // Same site, same node: applied. Waiting for review here, sent back to draft there: applied.
    let review = create_permit(&db, "hot_work", asset).await;
    let review = set_status(&db, &review, "pending_review").await.expect("submit");
    let back = with(
        wire(&db, "work_permits", &review.entity_sync_id).await,
        &[
            ("asset_sync_id", asset_sync("EQ-SYNC-EQ2")),
            ("status", json!("draft")),
            // Back to draft there: its submission was cleared (four-eyes).
            ("submitted_by_sync_id", Value::Null),
            ("submitted_at", Value::Null),
        ],
    );
    let outcome = apply(&db, "work_permits", &review.entity_sync_id, &back).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let review_now = queries::get_work_permit(&db, review.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!((review_now.asset_id, review_now.status.as_str()), (same_site, "draft"));

    // Linked to a work order on other equipment: refused.
    let (wo_id, _) = assigned_wo_requiring_permit(&db, same_site).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("work order sync id");
    let free = create_permit(&db, "hot_work", asset).await;
    let linked = with(
        wire(&db, "work_permits", &free.entity_sync_id).await,
        &[("linked_work_order_sync_id", json!(wo_sync))],
    );
    let outcome = apply(&db, "work_permits", &free.entity_sync_id, &linked).await;
    assert_eq!(code(&outcome), Some("PERMIT_WO_EQUIPMENT_MISMATCH"), "{outcome:?}");
    // With the work order's equipment: applied.
    let aligned = with(linked, &[("asset_sync_id", asset_sync("EQ-SYNC-EQ2"))]);
    let outcome = apply(&db, "work_permits", &free.entity_sync_id, &aligned).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A line under the first site (sync id `permit-test-org-003`, node 3) and equipment installed on it.
async fn equipment_on_line_of_first_site(db: &DatabaseConnection, code: &str) -> i64 {
    let _ = insert_equipment(db, &format!("{code}-FIRST")).await;
    exec(
        db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, parent_id, status, created_at, \
         updated_at, structure_model_id) VALUES (3, 'permit-test-org-003', 'LINE-PTW-3', 'Permit Test Line 3', 1, 1, \
         'active', datetime('now'), datetime('now'), 1)",
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
             created_at, updated_at) VALUES ('eq-sync-{code}', '{code}', 'Line asset', 'active_in_service', 3, \
             datetime('now'), datetime('now'))"
        ),
    )
    .await;
    super::tests::scalar_i64(db, &format!("SELECT id FROM equipment WHERE asset_id_code = '{code}'")).await
}

/// The version the server holds for a permit, as recorded when it was last synced here.
async fn synced_at(db: &DatabaseConnection, wp: &WorkPermit, version: i64) {
    crate::sync::queries::record_synced_state(db, "work_permits", &wp.entity_sync_id, version, "test-hash", "test")
        .await
        .expect("synced state");
}

/// The site of a permit received alone (same equipment and work order) is judged as on this PC:
/// approved here, it is fixed; before, it must be the equipment's site or a level above it.
#[tokio::test]
async fn a_received_site_change_is_rechecked_against_the_permit_held_here() {
    let db = setup().await;
    let on_line = equipment_on_line_of_first_site(&db, "EQ-SYNC-SITE1").await;
    let _ = equipment_on_second_site(&db, "EQ-SYNC-SITE2").await;
    let node = |id: i64| json!(format!("permit-test-org-00{id}"));

    // Approved here: any other site is a conflict, nothing written.
    let approved = create_permit(&db, "hot_work", on_line).await;
    assert_eq!(approved.entity_id, 1, "recorded on the site above its line");
    let approved = set_status(&db, &approved, "pending_review").await.expect("submit");
    let approved = set_status(&db, &approved, "approved").await.expect("approve");
    for to in [2, 3] {
        let moved = with(
            wire(&db, "work_permits", &approved.entity_sync_id).await,
            &[("entity_sync_id", node(to))],
        );
        let outcome = apply(&db, "work_permits", &approved.entity_sync_id, &moved).await;
        assert_eq!(code(&outcome), Some("PERMIT_SITE_LOCKED"), "to node {to}: {outcome:?}");
    }
    let kept = queries::get_work_permit(&db, approved.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!(kept.entity_id, 1, "nothing written");

    // A draft here: another site is a conflict; its equipment's line, then the site above: applied.
    let draft = create_permit(&db, "hot_work", on_line).await;
    let held = wire(&db, "work_permits", &draft.entity_sync_id).await;
    let outcome = apply(
        &db,
        "work_permits",
        &draft.entity_sync_id,
        &with(held.clone(), &[("entity_sync_id", node(2))]),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_SITE_NOT_EQUIPMENT_SITE"), "{outcome:?}");
    for to in [3, 1] {
        let outcome = apply(
            &db,
            "work_permits",
            &draft.entity_sync_id,
            &with(
                wire(&db, "work_permits", &draft.entity_sync_id).await,
                &[("entity_sync_id", node(to))],
            ),
        )
        .await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "to node {to}: {outcome:?}");
        let now = queries::get_work_permit(&db, draft.id)
            .await
            .expect("read")
            .expect("permit");
        assert_eq!(now.entity_id, to);
    }
    // A level below the equipment's site: a permit on equipment of site 1 moved to line 3.
    let on_site = create_permit(&db, "hot_work", insert_equipment(&db, "EQ-SYNC-SITE3").await).await;
    let outcome = apply(
        &db,
        "work_permits",
        &on_site.entity_sync_id,
        &with(
            wire(&db, "work_permits", &on_site.entity_sync_id).await,
            &[("entity_sync_id", node(3))],
        ),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_SITE_NOT_EQUIPMENT_SITE"), "{outcome:?}");
}

/// A new organisation structure as received here from the PC that published it: node 1 (the
/// first site) deleted, and in structure model 90 (active, superseding model 1 at the moment it
/// was activated, as a publish records it) its copy (node 90, sync id `permit-test-org-090`: same
/// code and node type) under a new top node (node 91, `permit-test-org-091`, another code).
/// Returns the copy's id.
async fn receive_structure_replacing_first_site(db: &DatabaseConnection) -> i64 {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, activated_at, \
         created_at, updated_at) VALUES (90, 'permit-test-model-090', 90, 'active', '2026-10-01T00:00:00+00:00', \
         datetime('now'), datetime('now'))",
        "UPDATE org_structure_models SET status = 'superseded', superseded_at = '2026-10-01T00:00:00+00:00' \
         WHERE id = 1",
        // The node types of the new structure, matched by their stable code.
        "INSERT INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, \
         updated_at) SELECT 90, 'permit-test-type-090', 90, t.code, t.label, 1, datetime('now'), datetime('now') \
         FROM org_node_types t WHERE t.id = (SELECT node_type_id FROM org_nodes WHERE id = 1)",
        "INSERT INTO org_nodes (id, entity_sync_id, code, name, node_type_id, parent_id, status, created_at, \
         updated_at, structure_model_id) VALUES (91, 'permit-test-org-091', 'PLANT-PTW-NEW', 'New plant', 90, NULL, \
         'active', datetime('now'), datetime('now'), 90)",
        "INSERT INTO org_nodes (id, entity_sync_id, code, name, node_type_id, parent_id, status, created_at, \
         updated_at, structure_model_id) SELECT 90, 'permit-test-org-090', n.code, n.name, 90, 91, 'active', \
         datetime('now'), datetime('now'), 90 FROM org_nodes n WHERE n.id = 1",
        "UPDATE org_nodes SET deleted_at = datetime('now') WHERE id = 1",
    ] {
        exec(db, sql).await;
    }
    90
}

/// A new organisation structure published on another PC deletes the former nodes and moves every
/// permit (approved ones included) to the copy of its own node (`org::validation`): received here
/// once its former node is deleted here, the move to that copy is judged on the equipment's chain
/// whatever the permit's status. An equipment change stays locked, and an approved permit moved to
/// any other node is refused as locked, even a live node above its equipment (the new plant): the
/// received node must be the copy of the one held here (same code and node type).
#[tokio::test]
async fn a_received_site_move_off_a_deleted_node_is_accepted_only_to_its_copy() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-REORG1").await;
    let far = equipment_on_second_site(&db, "EQ-SYNC-REORG2").await;
    let _ = equipment_on_line_of_first_site(&db, "EQ-SYNC-REORG3").await;
    let node = |id: i64| json!(format!("permit-test-org-{id:03}"));
    let approved = create_permit(&db, "hot_work", asset).await;
    assert_eq!(approved.entity_id, 1);
    let approved = set_status(&db, &approved, "pending_review").await.expect("submit");
    let approved = set_status(&db, &approved, "approved").await.expect("approve");

    // The publish as received here: node 1 deleted, the equipment moved to the copy of node 1.
    let copy = receive_structure_replacing_first_site(&db).await;
    exec(
        &db,
        &format!("UPDATE equipment SET installed_at_node_id = {copy} WHERE id = {asset}"),
    )
    .await;
    let moved_to = |to: i64| {
        let db = &db;
        let sync_id = approved.entity_sync_id.clone();
        async move {
            apply(
                db,
                "work_permits",
                &sync_id,
                &with(
                    wire(db, "work_permits", &sync_id).await,
                    &[("entity_sync_id", node(to))],
                ),
            )
            .await
        }
    };

    // Its equipment changed too: still locked.
    let far_sync = text(&db, &format!("SELECT entity_sync_id FROM equipment WHERE id = {far}"))
        .await
        .expect("far equipment sync id");
    let outcome = apply(
        &db,
        "work_permits",
        &approved.entity_sync_id,
        &with(
            wire(&db, "work_permits", &approved.entity_sync_id).await,
            &[("asset_sync_id", json!(far_sync)), ("entity_sync_id", node(copy))],
        ),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_EQUIPMENT_LOCKED"), "{outcome:?}");
    // Not the copy of node 1: the new plant above its equipment (on its chain), line 3 (under the
    // deleted node) and the second site are all refused as locked; nothing written.
    for to in [91, 3, 2] {
        let outcome = moved_to(to).await;
        assert_eq!(code(&outcome), Some("PERMIT_SITE_LOCKED"), "to node {to}: {outcome:?}");
    }
    // A site not applied here yet: the row waits.
    let outcome = moved_to(99).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    // The copy while its structure is still a draft here (the publish arrived in part: model 1
    // still in use, its node deleted): no copy yet, the row waits for the activation.
    exec(
        &db,
        "UPDATE org_structure_models SET status = CASE id WHEN 90 THEN 'draft' ELSE 'active' END WHERE id IN (1, 90)",
    )
    .await;
    let outcome = moved_to(copy).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    exec(
        &db,
        "UPDATE org_structure_models SET status = CASE id WHEN 90 THEN 'active' ELSE 'superseded' END \
         WHERE id IN (1, 90)",
    )
    .await;
    let kept = queries::get_work_permit(&db, approved.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!(kept.entity_id, 1, "nothing written");
    // The copy of its own node, its equipment's node: applied.
    let outcome = moved_to(copy).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let moved = queries::get_work_permit(&db, approved.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!(moved.entity_id, copy);
    assert_eq!(moved.status, "approved");
}

/// The copy must keep the node type of the node held here, matched by its stable code: a node of
/// the new structure with the held node's code but another type is not its copy.
#[tokio::test]
async fn a_received_site_move_to_a_node_of_another_type_is_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-REORG4").await;
    let approved = create_permit(&db, "hot_work", asset).await;
    let approved = set_status(&db, &approved, "pending_review").await.expect("submit");
    let approved = set_status(&db, &approved, "approved").await.expect("approve");
    let copy = receive_structure_replacing_first_site(&db).await;
    exec(
        &db,
        &format!("UPDATE equipment SET installed_at_node_id = {copy} WHERE id = {asset}"),
    )
    .await;
    exec(
        &db,
        "INSERT INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, \
         updated_at) VALUES (91, 'permit-test-type-091', 90, 'PTW-OTHER-LEVEL', 'Other level', 1, datetime('now'), \
         datetime('now'))",
    )
    .await;
    exec(
        &db,
        &format!("UPDATE org_nodes SET node_type_id = 91 WHERE id = {copy}"),
    )
    .await;
    let outcome = apply(
        &db,
        "work_permits",
        &approved.entity_sync_id,
        &with(
            wire(&db, "work_permits", &approved.entity_sync_id).await,
            &[("entity_sync_id", json!("permit-test-org-090"))],
        ),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_SITE_LOCKED"), "{outcome:?}");
}

/// A permit held here waiting for review, as last synced at `version`, with nothing of this PC still
/// to send (its own saves are on the server).
async fn held_in_review_synced_at(db: &DatabaseConnection, asset: i64, version: i64) -> WorkPermit {
    let wp = create_permit(db, "hot_work", asset).await;
    let wp = set_status(db, &wp, "pending_review").await.expect("submit");
    exec(db, "DELETE FROM sync_capture_journal").await;
    exec(db, "DELETE FROM sync_outbox").await;
    synced_at(db, &wp, version).await;
    wp
}

/// Runs the inbox pass on the rows waiting; returns (applied, conflicts).
async fn inbox_pass(db: &DatabaseConnection) -> (i64, i64) {
    let result = crate::sync::materialize::materialize_inbox_batch(db, None)
        .await
        .expect("materialize");
    crate::sync::row_codec::clear_ref_cache();
    (result.applied, result.conflicts)
}

/// The normal offline flow on another PC: a permit waiting for review has its equipment (or only
/// its site) changed, goes back to draft there, is submitted again and even approved, then syncs.
/// The capture journal folds those saves into ONE row, one version above the one held here: it is
/// accepted, with no rule on how many versions it carries.
#[tokio::test]
async fn a_folded_offline_equipment_change_resubmitted_or_approved_is_accepted() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-RV1").await;
    let other = insert_equipment(&db, "EQ-SYNC-RV2").await;
    for status in ["pending_review", "approved"] {
        let wp = held_in_review_synced_at(&db, asset, 5).await;
        let changed = with(
            wire(&db, "work_permits", &wp.entity_sync_id).await,
            &[
                ("asset_sync_id", json!("eq-sync-EQ-SYNC-RV2")),
                ("status", json!(status)),
            ],
        );
        // Approved there by a second person (four-eyes): it says who.
        let changed = if status == "approved" {
            approved_by(&changed, &user_sync(&db, VERIFIER).await, 0)
        } else {
            changed
        };
        inbox(&db, "work_permits", &wp.entity_sync_id, 6, "pending", &changed).await;
        assert_eq!(inbox_pass(&db).await, (1, 0), "{status} at held + 1");
        let now = queries::get_work_permit(&db, wp.id)
            .await
            .expect("read")
            .expect("permit");
        assert_eq!((now.asset_id, now.status.as_str()), (other, status));
        assert_eq!(
            text(
                &db,
                &format!(
                    "SELECT apply_status FROM sync_inbox WHERE entity_sync_id = '{}'",
                    wp.entity_sync_id
                )
            )
            .await
            .as_deref(),
            Some("applied")
        );
    }

    // The site alone (to its equipment's line, on the chain), resubmitted there: accepted too.
    let on_line = equipment_on_line_of_first_site(&db, "EQ-SYNC-RV3").await;
    let wp = held_in_review_synced_at(&db, on_line, 5).await;
    assert_eq!(wp.entity_id, 1);
    let moved = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("entity_sync_id", json!("permit-test-org-003"))],
    );
    inbox(&db, "work_permits", &wp.entity_sync_id, 6, "pending", &moved).await;
    assert_eq!(inbox_pass(&db).await, (1, 0));
    let now = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!((now.entity_id, now.status.as_str()), (3, "pending_review"));

    // The other arrival rules still hold for a folded row: a site off the new equipment's chain is
    // a conflict, nothing written.
    let far = equipment_on_second_site(&db, "EQ-SYNC-RV4").await;
    let wp = held_in_review_synced_at(&db, asset, 5).await;
    let off_chain = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[
            ("asset_sync_id", json!("eq-sync-EQ-SYNC-RV4")),
            ("status", json!("approved")),
        ],
    );
    let off_chain = approved_by(&off_chain, &user_sync(&db, VERIFIER).await, 0);
    inbox(&db, "work_permits", &wp.entity_sync_id, 6, "pending", &off_chain).await;
    assert_eq!(inbox_pass(&db).await, (0, 1));
    let now = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    assert_ne!(now.asset_id, far, "nothing written");
    assert_eq!(now.status, "pending_review");
}

/// A work order is never removed from a permit by a received row, and a work order recording no
/// usable node (`0`) gives the permit no site: a conflict, as the edit on this PC refuses it.
#[tokio::test]
async fn a_received_permit_never_drops_its_work_order_nor_takes_one_without_a_site() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-WO1").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let linked = create_from_wo(&db, "hot_work", wo_id)
        .await
        .expect("permit on the work order");
    let unlinked = with(
        wire(&db, "work_permits", &linked.entity_sync_id).await,
        &[("linked_work_order_sync_id", Value::Null)],
    );
    let outcome = apply(&db, "work_permits", &linked.entity_sync_id, &unlinked).await;
    assert_eq!(code(&outcome), Some("PERMIT_WO_UNLINKED"), "{outcome:?}");

    let (wo_zero, _) = assigned_wo_requiring_permit(&db, asset).await;
    exec(
        &db,
        &format!("UPDATE work_orders SET entity_id = 0 WHERE id = {wo_zero}"),
    )
    .await;
    let wo_zero_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_zero}"),
    )
    .await
    .expect("work order sync id");
    let free = create_permit(&db, "hot_work", asset).await;
    let onto = with(
        wire(&db, "work_permits", &free.entity_sync_id).await,
        &[("linked_work_order_sync_id", json!(wo_zero_sync))],
    );
    let outcome = apply(&db, "work_permits", &free.entity_sync_id, &onto).await;
    assert_eq!(code(&outcome), Some("PERMIT_SITE_NOT_EQUIPMENT_SITE"), "{outcome:?}");
    // The same edit on this PC: refused too (no usable site).
    let err = queries::update_work_permit(
        &db,
        crate::permit::domain::WorkPermitUpdateInput {
            id: free.id,
            linked_work_order_id: Some(wo_zero),
            asset_id: None,
            entity_id: None,
            expires_at: None,
            expected_row_version: free.row_version,
        },
        APPLIER,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, crate::errors::AppError::ValidationFailed(_)), "{err:?}");
}

/// Known limit (fail-safe): isolation rows received just before the permit row that changes its
/// equipment (points planned on the other PC for the new equipment) are held here when the permit
/// row is judged; nothing tells which PC planned them or for which version of the permit, so the
/// change is a conflict for a person to decide, with nothing written (`docs/KNOWN_LIMITS.md`).
#[tokio::test]
async fn isolations_received_before_the_equipment_change_make_it_a_conflict() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-ISO1").await;
    let _ = insert_equipment(&db, "EQ-SYNC-ISO2").await;
    let wp = create_permit(&db, "loto", asset).await;
    let held = wire(&db, "work_permits", &wp.entity_sync_id).await;
    // The isolation planned on the other PC for the new equipment, received first.
    let iso = add_isolation(&db, wp.id, "New pump breaker").await;
    let iso_wire = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", iso.id)).await;
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &iso_wire).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // Then the permit row with the new equipment.
    let changed = with(held, &[("asset_sync_id", json!("eq-sync-EQ-SYNC-ISO2"))]);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &changed).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_EQUIPMENT_ISOLATIONS_PLANNED"),
        "{outcome:?}"
    );
    let kept = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    assert_eq!(kept.asset_id, asset, "nothing written");
}

// ── Cancelled planned isolation lines (CMMS Advisor decision) ────────

/// The line as received cancelled by `by` now.
async fn cancelled_wire(db: &DatabaseConnection, sync_id: &str, by: i64) -> Payload {
    with(
        wire(db, "permit_isolations", sync_id).await,
        &[
            ("cancelled_at", json!(now())),
            ("cancelled_by_sync_id", json!(user_sync(db, by).await)),
        ],
    )
}

async fn held_cancelled_at(db: &DatabaseConnection, sync_id: &str) -> Option<String> {
    text(
        db,
        &format!("SELECT cancelled_at FROM permit_isolations WHERE entity_sync_id = '{sync_id}'"),
    )
    .await
}

fn cancel_input(iso: &crate::permit::domain::PermitIsolation) -> crate::permit::domain::PermitIsolationCancelInput {
    crate::permit::domain::PermitIsolationCancelInput {
        isolation_id: iso.id,
        expected_row_version: iso.row_version,
    }
}

/// A cancellation received for a line never applied, on a permit held here as a draft or waiting
/// for review, is applied (who and when kept); the line held cancelled is then kept as it was: never
/// un-cancelled, applied or changed by a received row.
#[tokio::test]
async fn a_received_cancellation_of_a_planned_line_is_applied_and_never_undone() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-CX1").await;
    let wp = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, wp.id, "MCC-21 breaker").await;
    let planned = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    assert!(planned.contains_key("cancelled_at") && planned.contains_key("cancelled_by_sync_id"));

    let cancelled = cancelled_wire(&db, &iso.entity_sync_id, VERIFIER).await;
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &cancelled).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(held_cancelled_at(&db, &iso.entity_sync_id).await.is_some());
    assert_eq!(
        super::tests::scalar_i64(
            &db,
            &format!("SELECT cancelled_by_id FROM permit_isolations WHERE id = {}", iso.id)
        )
        .await,
        VERIFIER
    );

    // The same cancellation again: nothing changes.
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &cancelled).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
    // Un-cancelled, applied or renamed: refused, the line stays cancelled.
    let applied = with(
        cancelled.clone(),
        &[
            ("applied_at", json!(now())),
            ("applied_by_sync_id", json!(user_sync(&db, APPLIER).await)),
        ],
    );
    for received in [
        planned.clone(),
        applied,
        with(cancelled.clone(), &[("isolation_point", json!("Another point"))]),
    ] {
        let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &received).await;
        assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_CANCELLED"), "{outcome:?}");
    }
    assert!(held_cancelled_at(&db, &iso.entity_sync_id).await.is_some());

    // Waiting for review here: accepted too, and the permit's return to draft follows.
    let review = create_permit(&db, "loto", asset).await;
    let line = add_isolation(&db, review.id, "MCC-22 breaker").await;
    let review = set_status(&db, &review, "pending_review").await.expect("submit");
    let reset = with(
        wire(&db, "work_permits", &review.entity_sync_id).await,
        &[
            ("status", json!("draft")),
            // Back to draft there: its submission was cleared (four-eyes).
            ("submitted_by_sync_id", Value::Null),
            ("submitted_at", Value::Null),
        ],
    );
    let received = cancelled_wire(&db, &line.entity_sync_id, VERIFIER).await;
    let outcome = apply(&db, "permit_isolations", &line.entity_sync_id, &received).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let outcome = apply(&db, "work_permits", &review.entity_sync_id, &reset).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(permit_status(&db, &review).await, "draft");

    // A line planned and cancelled on the other PC, new here, on a draft: applied.
    let fresh = add_isolation(&db, wp.id, "MCC-23 breaker").await;
    let fresh_wire = cancelled_wire(&db, &fresh.entity_sync_id, VERIFIER).await;
    exec(&db, &format!("DELETE FROM permit_isolations WHERE id = {}", fresh.id)).await;
    let outcome = apply(&db, "permit_isolations", &fresh.entity_sync_id, &fresh_wire).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(held_cancelled_at(&db, &fresh.entity_sync_id).await.is_some());
}

/// A received cancellation of a line applied (in the row or here), on a permit approved or further
/// on here, or without who or when, is a conflict with nothing written; a delete stays refused.
#[tokio::test]
async fn a_received_cancellation_of_an_applied_line_or_an_approved_permit_is_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-CX2").await;

    // Applied here, while the other PC still had it planned.
    let draft = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, draft.id, "MCC-24 breaker").await;
    let planned_there = cancelled_wire(&db, &iso.entity_sync_id, VERIFIER).await;
    queries::apply_permit_isolation(&db, super::tests::action(&iso), APPLIER)
        .await
        .expect("fit the lock here");
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &planned_there).await;
    assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_CANCEL_APPLIED"), "{outcome:?}");
    // Applied in the row itself.
    let applied_row = cancelled_wire(&db, &iso.entity_sync_id, VERIFIER).await;
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &applied_row).await;
    assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_CANCEL_APPLIED"), "{outcome:?}");
    assert!(held_cancelled_at(&db, &iso.entity_sync_id).await.is_none());

    // Approved here.
    let approved = create_permit(&db, "loto", asset).await;
    let line = add_isolation(&db, approved.id, "MCC-25 breaker").await;
    let approved = set_status(&db, &approved, "pending_review").await.expect("submit");
    set_status(&db, &approved, "approved").await.expect("approve");
    let received = cancelled_wire(&db, &line.entity_sync_id, VERIFIER).await;
    let outcome = apply(&db, "permit_isolations", &line.entity_sync_id, &received).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_ISOLATION_CANCEL_WRONG_STATUS"),
        "{outcome:?}"
    );
    assert!(held_cancelled_at(&db, &line.entity_sync_id).await.is_none());

    // Without who, or dated in the future.
    let other = create_permit(&db, "loto", asset).await;
    let planned = add_isolation(&db, other.id, "MCC-26 breaker").await;
    let base = cancelled_wire(&db, &planned.entity_sync_id, VERIFIER).await;
    for received in [
        with(base.clone(), &[("cancelled_by_sync_id", Value::Null)]),
        with(base.clone(), &[("cancelled_at", json!("2999-01-01T00:00:00Z"))]),
    ] {
        let outcome = apply(&db, "permit_isolations", &planned.entity_sync_id, &received).await;
        assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_CANCEL_INVALID"), "{outcome:?}");
    }
    assert!(held_cancelled_at(&db, &planned.entity_sync_id).await.is_none());

    // A delete of the line stays refused once its permit is past draft (a cancellation is an update).
    let outcome = delete(&db, "permit_isolations", &line.entity_sync_id).await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
}

/// A planned line cancelled here no longer blocks a received change of the permit's equipment, and
/// a cancelled line is no isolation of a permit received active.
#[tokio::test]
async fn a_cancelled_line_does_not_block_a_received_equipment_change_or_activation() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-CX3").await;
    let _ = insert_equipment(&db, "EQ-SYNC-CX4").await;
    let wp = create_permit(&db, "loto", asset).await;
    let iso = add_isolation(&db, wp.id, "Old pump breaker").await;
    let held = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let moved = with(held, &[("asset_sync_id", json!("eq-sync-EQ-SYNC-CX4"))]);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &moved).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_EQUIPMENT_ISOLATIONS_PLANNED"),
        "{outcome:?}"
    );
    queries::cancel_permit_isolation(&db, cancel_input(&iso), VERIFIER)
        .await
        .expect("cancel here");
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &moved).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");

    // Received active with one verified line and one cancelled line: the cancelled one is not
    // counted as an isolation left unverified.
    let other = create_permit(&db, "loto", asset).await;
    let dropped = add_isolation(&db, other.id, "Valve not needed").await;
    queries::cancel_permit_isolation(&db, cancel_input(&dropped), VERIFIER)
        .await
        .expect("cancel in draft");
    let other = queries::get_work_permit(&db, other.id)
        .await
        .expect("read")
        .expect("permit");
    let other = advance_to_issued(&db, other).await;
    apply_and_verify(&db, &add_isolation(&db, other.id, "MCC-27 breaker").await).await;
    let active = with(
        wire(&db, "work_permits", &other.entity_sync_id).await,
        &[("status", json!("active")), ("activated_at", json!(now()))],
    );
    let outcome = apply(&db, "work_permits", &other.entity_sync_id, &active).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A cancelled line is the only trace of its cancellation on this PC: a received delete of it is
/// refused whatever its permit's status (a planned line of a draft may still go), and a received
/// row cannot change any of its content (energy and method texts, the mapping review mark).
#[tokio::test]
async fn a_cancelled_line_is_never_deleted_or_rewritten_by_sync() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-CX5").await;
    let wp = create_permit(&db, "loto", asset).await;
    let cancelled = add_isolation(&db, wp.id, "Line to cancel").await;
    let planned = add_isolation(&db, wp.id, "Line still planned").await;
    queries::cancel_permit_isolation(&db, cancel_input(&cancelled), VERIFIER)
        .await
        .expect("cancel in draft");
    assert_eq!(permit_status(&db, &wp).await, "draft");

    let held = wire(&db, "permit_isolations", &cancelled.entity_sync_id).await;
    for (key, value) in [
        ("energy_type", json!("Pneumatic (changed elsewhere)")),
        ("isolation_method", json!("Valve (changed elsewhere)")),
        ("mapping_review_required", flipped(&held, "mapping_review_required")),
    ] {
        let changed = with(held.clone(), &[(key, value)]);
        let outcome = apply(&db, "permit_isolations", &cancelled.entity_sync_id, &changed).await;
        assert_eq!(code(&outcome), Some("PERMIT_ISOLATION_CANCELLED"), "{key}: {outcome:?}");
    }

    for operation in ["delete", "archive"] {
        let outcome = delete_as(&db, "permit_isolations", &cancelled.entity_sync_id, operation).await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_DELETE_REFUSED"),
            "{operation}: {outcome:?}"
        );
    }
    assert!(
        text(
            &db,
            &format!(
                "SELECT cancelled_at FROM permit_isolations WHERE entity_sync_id = '{}'",
                cancelled.entity_sync_id
            )
        )
        .await
        .is_some(),
        "the cancelled line is kept"
    );
    let outcome = delete(&db, "permit_isolations", &planned.entity_sync_id).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A received delete of a draft permit is refused (its lines have no foreign key to it and would be
/// left without it), and a cancelled line is still refused when its permit is not held here (a row
/// left from before permit deletes were refused): the cancellation's trace cannot be removed in two
/// steps.
#[tokio::test]
async fn a_permit_delete_then_a_cancelled_line_delete_are_both_refused() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-CX6").await;
    let wp = create_permit(&db, "loto", asset).await;
    let cancelled = add_isolation(&db, wp.id, "Line cancelled in draft").await;
    queries::cancel_permit_isolation(&db, cancel_input(&cancelled), VERIFIER)
        .await
        .expect("cancel in draft");
    assert_eq!(permit_status(&db, &wp).await, "draft");

    let outcome = delete(&db, "work_permits", &wp.entity_sync_id).await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
    assert!(
        text(
            &db,
            &format!("SELECT entity_sync_id FROM work_permits WHERE id = {}", wp.id)
        )
        .await
        .is_some(),
        "the draft permit is kept"
    );

    // Its permit gone all the same (as an older received delete left it): the line still stays.
    exec(&db, &format!("DELETE FROM work_permits WHERE id = {}", wp.id)).await;
    let outcome = delete(&db, "permit_isolations", &cancelled.entity_sync_id).await;
    assert_eq!(code(&outcome), Some("PERMIT_DELETE_REFUSED"), "{outcome:?}");
    assert!(
        text(
            &db,
            &format!(
                "SELECT cancelled_at FROM permit_isolations WHERE entity_sync_id = '{}'",
                cancelled.entity_sync_id
            )
        )
        .await
        .is_some(),
        "the cancelled line is kept"
    );
}

/// The flag `key` of `p` set the other way, in the form the row carries it (number or boolean).
fn flipped(p: &Payload, key: &str) -> Value {
    match p.get(key) {
        Some(Value::Bool(b)) => json!(!b),
        Some(Value::Number(n)) if n.as_i64().is_some_and(|v| v != 0) => json!(0),
        _ => json!(1),
    }
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

/// An instant in the stored form (UTC, `Z`, whole seconds).
fn stored_at(at: chrono::DateTime<chrono::Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A late work order (planned "yesterday 08:00-12:00", started then, on hold overnight) received as
/// resumed at `resumed_at`, with its hot-work permit expiring five minutes after the resume.
/// Returns (work order sync id, received row, local work order id).
async fn late_wo_resumed(
    db: &DatabaseConnection,
    asset_code: &str,
    resumed_at: chrono::DateTime<chrono::Utc>,
) -> (String, Payload, i64) {
    let started_at = resumed_at - chrono::Duration::hours(25);
    let (wo_sync, row, _, wo_id) =
        ready_wo_with_permit_active_since(db, asset_code, started_at - chrono::Duration::hours(1)).await;
    exec(
        db,
        &format!(
            "UPDATE work_permits SET expires_at = '{}' WHERE linked_work_order_id = {wo_id}",
            stored_at(resumed_at + chrono::Duration::minutes(5))
        ),
    )
    .await;
    let row = with(
        row,
        &[
            ("actual_start", json!(stored_at(started_at))),
            ("planned_end", json!(stored_at(started_at + chrono::Duration::hours(4)))),
            ("expected_duration_hours", json!(4.0)),
        ],
    );
    (wo_sync, row, wo_id)
}

/// The history row of an entry into in progress at `at` (`action`, with `remaining` as the
/// estimated remaining time): received, waiting in the inbox with the work order (`received`), or
/// already on this PC with no sync identity (`!received`, read directly). Replaces any earlier one.
async fn entry_into_progress(
    db: &DatabaseConnection,
    wo_id: i64,
    at: &str,
    action: &str,
    remaining: Value,
    received: bool,
) {
    exec(
        db,
        "DELETE FROM sync_inbox WHERE entity_type = 'wo_state_transition_log'",
    )
    .await;
    exec(
        db,
        &format!("DELETE FROM wo_state_transition_log WHERE wo_id = {wo_id} AND to_status = 'in_progress'"),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO wo_state_transition_log (entity_sync_id, wo_id, from_status, to_status, action, actor_id, \
             acted_at, lifecycle_state) VALUES ('entry-{wo_id}', {wo_id}, 'on_hold', 'in_progress', '{action}', 1, \
             '{at}', 'in_progress')"
        ),
    )
    .await;
    if received {
        let payload = with(
            wire(db, "wo_state_transition_log", &format!("entry-{wo_id}")).await,
            &[("estimated_remaining_hours", remaining)],
        );
        exec(
            db,
            &format!("DELETE FROM wo_state_transition_log WHERE entity_sync_id = 'entry-{wo_id}'"),
        )
        .await;
        inbox(
            db,
            "wo_state_transition_log",
            &format!("entry-{wo_id}"),
            1,
            "pending",
            &payload,
        )
        .await;
    } else {
        let value = match remaining {
            Value::Null => "NULL".to_string(),
            Value::String(s) => format!("'{s}'"),
            other => other.to_string(),
        };
        exec(
            db,
            &format!(
                "UPDATE wo_state_transition_log SET entity_sync_id = NULL, estimated_remaining_hours = {value} \
                 WHERE entity_sync_id = 'entry-{wo_id}'"
            ),
        )
        .await;
    }
}

/// Review SF1 and NOTE 9, re-checked on arrival exactly as on the PC that resumed: a late resume is
/// judged on the resume time the row records plus the expected duration (the planned end, already
/// past, no longer hides it), or plus the estimated remaining time recorded on the resume's history
/// row. A missing value means the full duration; one that is not a number of hours greater than 0
/// is refused (fails safe); one recorded on a start is ignored (a start always uses the full
/// duration, as here). The same through a received history row and one already on this PC.
#[tokio::test]
async fn a_received_late_resume_is_judged_on_the_duration_or_its_recorded_remaining_time() {
    for received in [true, false] {
        let db = setup().await;
        let resumed_at = chrono::Utc::now() - chrono::Duration::minutes(1);
        let at = stored_at(resumed_at);
        let (wo_sync, row, wo_id) = late_wo_resumed(&db, "EQ-SYNC-99", resumed_at).await;
        let status_sql = format!(
            "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = {wo_id}"
        );
        let judged = |remaining: Value, action: &'static str| {
            let (db, row, wo_sync, at) = (&db, row.clone(), wo_sync.clone(), at.clone());
            async move {
                entry_into_progress(db, wo_id, &at, action, remaining, received).await;
                apply(db, "work_orders", &wo_sync, &row).await
            }
        };
        // No remaining time: the full 4 h from the resume outlive the permit (the old rule judged
        // the past planned end and let it through).
        let outcome = judged(Value::Null, "resume").await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_EXPIRES_BEFORE_ESTIMATED_END"),
            "received {received}: {outcome:?}"
        );
        assert_eq!(text(&db, &status_sql).await.as_deref(), Some("ready"));
        // Two hours remaining still outlive it.
        let outcome = judged(json!(2.0), "resume").await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_EXPIRES_BEFORE_ESTIMATED_END"),
            "{outcome:?}"
        );
        // A value no PC records (0, negative, not a number): refused, never read as "no value".
        for bad in [json!(0), json!(-1.5), json!("soon")] {
            let outcome = judged(bad.clone(), "resume").await;
            assert_eq!(
                code(&outcome),
                Some("WO_REMAINING_TIME_INVALID"),
                "received {received}, {bad}: {outcome:?}"
            );
        }
        // Recorded on a start (no PC does that): ignored, the full duration is judged.
        let outcome = judged(json!(0.05), "start").await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_EXPIRES_BEFORE_ESTIMATED_END"),
            "{outcome:?}"
        );
        // A planned end already past and no duration: unknown end, refused, unless a remaining
        // time was recorded.
        let no_duration = with(row.clone(), &[("expected_duration_hours", Value::Null)]);
        entry_into_progress(&db, wo_id, &at, "resume", Value::Null, received).await;
        let outcome = apply(&db, "work_orders", &wo_sync, &no_duration).await;
        assert_eq!(code(&outcome), Some("WO_PLANNED_END_PASSED"), "{outcome:?}");
        // Three minutes remaining fit before the expiry: applied, as on the PC that resumed (or
        // reopened). A number sent as text reads as that number (SQLite stores it so).
        let (fits, action) = if received {
            (json!("0.05"), "resume")
        } else {
            (json!(0.05), "reopen")
        };
        let outcome = judged(fits, action).await;
        assert!(
            matches!(outcome, ApplyOutcome::Applied),
            "received {received}: {outcome:?}"
        );
        assert_eq!(text(&db, &status_sql).await.as_deref(), Some("in_progress"));
    }
}

/// Review NOTE 5: an expected duration that is not a number of hours gets the same coded refusal
/// on this PC and on arrival (it used to be an uncoded error here and "not judged" on arrival); a
/// number sent as text is that number on both sides.
#[tokio::test]
async fn an_expected_duration_that_is_not_a_number_is_refused_here_and_on_arrival() {
    let db = setup().await;
    let started_at = chrono::Utc::now() - chrono::Duration::minutes(10);
    let (wo_sync, row, expiry, wo_id) =
        ready_wo_with_permit_active_since(&db, "EQ-SYNC-100", started_at - chrono::Duration::hours(1)).await;
    let started = with(row, &[("actual_start", json!(stored_at(started_at)))]);
    // This PC: a duration stored as text.
    exec(
        &db,
        &format!("UPDATE work_orders SET planned_end = NULL, expected_duration_hours = 'four' WHERE id = {wo_id}"),
    )
    .await;
    match crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await {
        Err(crate::errors::AppError::OrgValidationFailed(issues)) => {
            assert_eq!(issues[0].code, "WO_EXPECTED_DURATION_UNREADABLE");
            assert_eq!(issues[0].params.get("value").map(String::as_str), Some("four"));
        }
        other => panic!("expected WO_EXPECTED_DURATION_UNREADABLE, got {other:?}"),
    }
    // A blob is never a number either (review N5): the same refusal, never "no duration".
    exec(
        &db,
        &format!("UPDATE work_orders SET expected_duration_hours = X'34' WHERE id = {wo_id}"),
    )
    .await;
    match crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await {
        Err(crate::errors::AppError::OrgValidationFailed(issues)) => {
            assert_eq!(issues[0].code, "WO_EXPECTED_DURATION_UNREADABLE");
        }
        other => panic!("expected WO_EXPECTED_DURATION_UNREADABLE for a blob, got {other:?}"),
    }
    // Back to text before the arrival cases (the received row is laid over this row).
    exec(
        &db,
        &format!("UPDATE work_orders SET expected_duration_hours = 'four' WHERE id = {wo_id}"),
    )
    .await;
    // Arrival: the same value, the same refusal.
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(started.clone(), &[("expected_duration_hours", json!("four"))]),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_EXPECTED_DURATION_UNREADABLE"), "{outcome:?}");
    // A number sent as text: that number. Ending 30 minutes after the expiry: refused as an
    // estimated end, exactly as the number itself.
    let late = hours_until(started_at, expiry, -30);
    for sent in [json!(late), json!(late.to_string())] {
        let outcome = apply(
            &db,
            "work_orders",
            &wo_sync,
            &with(started.clone(), &[("expected_duration_hours", sent.clone())]),
        )
        .await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_EXPIRES_BEFORE_ESTIMATED_END"),
            "{sent}: {outcome:?}"
        );
    }
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
    // The fixed reason naming a person is no restore by the update: judged as usual (here: an
    // author without the settings right, refused), nothing written.
    let held = wire(&db, "permit_types", &loto).await;
    user_with(&db, "no_settings_right", &[crate::rbac::permissions::PTW_ISSUE]).await;
    let named = restore(&held, json!("no_settings_right-sync"), &now());
    let outcome = apply(&db, "permit_types", &loto, &named).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_TYPE_ISOLATION_UNAUTHORISED"),
        "{outcome:?}"
    );
    // Nor is a restore record with a time in the future (beyond the tolerated clock difference) or
    // a time that cannot be read (review N1): judged, and refused for its time.
    let future = (chrono::Utc::now() + chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    for at in [future.as_str(), "9999-01-01T00:00:00Z", "someday"] {
        let outcome = apply(&db, "permit_types", &loto, &restore(&held, Value::Null, at)).await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_TYPE_ISOLATION_UNAUDITED"),
            "{at}: {outcome:?}"
        );
    }
    assert_eq!(record(loto.clone()).await.as_deref(), Some("-|-"), "nothing written");
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
    // Refused for the missing permit-issue right, not the settings right (review N6).
    let refused_for = |outcome: &ApplyOutcome| match outcome {
        ApplyOutcome::Conflict { message, .. } => message.clone(),
        other => panic!("expected a conflict, got {other:?}"),
    };
    assert!(
        refused_for(&outcome).contains("without the permit-issue right"),
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
    assert!(
        refused_for(&outcome).contains("without the permit-issue right"),
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

/// Review NOTE 2a: a hand-over role that only Rust would call blank (a tab) is judged on arrival as
/// the start-up integrity pass judges it (SQLite `TRIM`: spaces only): adopted by both, once; a
/// role of spaces only is skipped by both.
#[tokio::test]
async fn arrival_and_the_integrity_pass_agree_on_blank_handover_roles() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-92").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    exec(
        &db,
        &format!(
            "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
             confirmation_note, signed_at, row_version) VALUES ('ho-blank-roles', {}, char(9), '   ', \
             'Zone left clean', '2026-10-03T00:00:00Z', 1)",
            wp.id
        ),
    )
    .await;
    let log_id = text(
        &db,
        "SELECT CAST(id AS TEXT) FROM permit_handover_logs WHERE entity_sync_id = 'ho-blank-roles'",
    )
    .await
    .and_then(|id| id.parse::<i64>().ok())
    .expect("log");
    let adopted = |code_sql: &'static str| {
        let db = &db;
        async move {
            text(
                db,
                &format!("SELECT CAST(COUNT(*) AS TEXT) FROM reference_values WHERE code = {code_sql}"),
            )
            .await
        }
    };
    crate::permit::handover_roles::adopt_received_roles(&db, log_id)
        .await
        .expect("arrival");
    assert_eq!(
        adopted("char(9)").await.as_deref(),
        Some("1"),
        "the tab role is adopted on arrival"
    );
    assert_eq!(adopted("'   '").await.as_deref(), Some("0"), "spaces only: skipped");
    // The integrity pass finds the tab role already listed and skips the spaces, as arrival did.
    crate::permit::handover_roles::adopt_stored_roles(&db)
        .await
        .expect("integrity");
    assert_eq!(adopted("char(9)").await.as_deref(), Some("1"));
    assert_eq!(adopted("'   '").await.as_deref(), Some("0"));
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

// ── Four-eyes on permit approval (owner decision 2026-10-17, migration 244) ────────────────

/// A permit written and submitted here by APPLIER, held waiting for review, as this PC sends it.
async fn submitted_by_applier(db: &DatabaseConnection, code: &str) -> (WorkPermit, Payload) {
    let asset = insert_equipment(db, code).await;
    let wp = create_permit(db, "hot_work", asset).await;
    let wp = set_status(db, &wp, "pending_review").await.expect("submit");
    let held = wire(db, "work_permits", &wp.entity_sync_id).await;
    (wp, held)
}

fn approved_by(held: &Payload, who: &str, self_approved: i64) -> Payload {
    with(
        held.clone(),
        &[
            ("status", json!("approved")),
            ("approved_by_sync_id", json!(who)),
            ("approved_at", json!(now())),
            ("self_approved", json!(self_approved)),
        ],
    )
}

async fn read_permit(db: &DatabaseConnection, wp: &WorkPermit) -> WorkPermit {
    queries::get_work_permit(db, wp.id)
        .await
        .expect("read")
        .expect("permit")
}

/// On a four-eyes site, an approval received from another PC by the permit's author or submitter
/// is a conflict (flagged as a self-approval or not), as is one that names nobody or flags a second
/// person as a self-approval; nothing is written. An approval by a second person is applied and
/// recorded.
#[tokio::test]
async fn a_received_self_approval_is_refused_on_a_four_eyes_site() {
    let db = setup().await;
    let (wp, held) = submitted_by_applier(&db, "EQ-SYNC-4E1").await;
    // A real planning site that keeps four-eyes (no per-site rule).
    super::tests::make_test_site_a_planning_site(&db).await;
    let applier = user_sync(&db, APPLIER).await;
    let verifier = user_sync(&db, VERIFIER).await;

    for flag in [1, 0] {
        let outcome = apply(
            &db,
            "work_permits",
            &wp.entity_sync_id,
            &approved_by(&held, &applier, flag),
        )
        .await;
        assert_eq!(code(&outcome), Some("PERMIT_SELF_APPROVAL"), "flag {flag}: {outcome:?}");
        if flag == 1 {
            // The conflict names the approval date and the site's rule here now.
            let ApplyOutcome::Conflict { message, .. } = &outcome else {
                unreachable!()
            };
            assert!(message.contains(" on 20") && message.contains("four-eyes"), "{message}");
        }
        assert_eq!(permit_status(&db, &wp).await, "pending_review");
    }
    let nobody = with(held.clone(), &[("status", json!("approved"))]);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &nobody).await;
    assert_eq!(code(&outcome), Some("PERMIT_APPROVER_MISSING"), "{outcome:?}");
    let mislabelled = approved_by(&held, &verifier, 1);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &mislabelled).await;
    assert_eq!(code(&outcome), Some("PERMIT_APPROVAL_RECORD_INVALID"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "pending_review");

    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &approved_by(&held, &verifier, 0),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let stored = read_permit(&db, &wp).await;
    assert_eq!(stored.status, "approved");
    assert_eq!(stored.approved_by_id, Some(VERIFIER));
    assert!(!stored.self_approved);
}

/// A self-approval received from another PC is accepted only flagged, and only where the permit's
/// site opted out of four-eyes on this PC.
#[tokio::test]
async fn a_received_self_approval_is_accepted_where_the_site_opted_out() {
    let db = setup().await;
    let applier = user_sync(&db, APPLIER).await;
    let (wp, held) = submitted_by_applier(&db, "EQ-SYNC-4E2").await;
    super::tests::opt_site_out_of_four_eyes(&db).await;
    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &approved_by(&held, &applier, 0),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_SELF_APPROVAL"), "not flagged: {outcome:?}");
    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &approved_by(&held, &applier, 1),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let stored = read_permit(&db, &wp).await;
    assert_eq!(
        (stored.status.as_str(), stored.approved_by_id),
        ("approved", Some(APPLIER))
    );
    assert!(stored.self_approved, "shown as a self-approval");
}

/// An approver not applied on this PC yet does not change the verdict (identities are compared by
/// sync id): a second person's approval waits for that person, a self-approval is refused anyway.
#[tokio::test]
async fn a_received_approval_by_a_person_not_here_yet_waits_for_that_person() {
    let db = setup().await;
    let (wp, held) = submitted_by_applier(&db, "EQ-SYNC-4E3").await;
    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &approved_by(&held, "user-sync-still-arriving", 0),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "pending_review");
    // Submitted and approved by that same person, not here yet: refused anyway (four-eyes here).
    let own = with(
        approved_by(&held, "user-sync-still-arriving", 1),
        &[("submitted_by_sync_id", json!("user-sync-still-arriving"))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &own).await;
    assert_eq!(code(&outcome), Some("PERMIT_SELF_APPROVAL"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "pending_review");
}

/// Who wrote, submitted and approved a permit is never erased or rewritten by a received row once
/// recorded here (past review); a permit approved before approvers were recorded is not given one
/// by a received row either. A change of something else applies.
#[tokio::test]
async fn a_received_row_cannot_erase_or_rewrite_a_recorded_approver() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-4E4").await;
    let wp = advance_to_issued(&db, create_permit(&db, "hot_work", asset).await).await;
    let approver = super::tests::second_approver(&db).await;
    assert_eq!(wp.approved_by_id, Some(approver));
    let held = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let applier = user_sync(&db, APPLIER).await;
    let verifier = user_sync(&db, VERIFIER).await;
    for (what, change) in [
        ("approver rewritten", ("approved_by_sync_id", json!(applier))),
        ("approver erased", ("approved_by_sync_id", Value::Null)),
        (
            "approval time rewritten",
            ("approved_at", json!("2026-01-01T00:00:00Z")),
        ),
        ("approval erased", ("approved_at", Value::Null)),
        ("flagged a self-approval", ("self_approved", json!(1))),
        ("submitter rewritten", ("submitted_by_sync_id", json!(verifier))),
        ("submission erased", ("submitted_at", Value::Null)),
        ("author rewritten", ("created_by_sync_id", json!(verifier))),
        ("author erased", ("created_by_sync_id", Value::Null)),
    ] {
        let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &with(held.clone(), &[change])).await;
        assert_eq!(
            code(&outcome),
            Some("PERMIT_APPROVAL_RECORD_CHANGED"),
            "{what}: {outcome:?}"
        );
    }
    let stored = read_permit(&db, &wp).await;
    assert_eq!(
        (stored.approved_by_id, stored.submitted_by_id, stored.created_by_id),
        (Some(approver), Some(APPLIER), Some(APPLIER))
    );
    let later = (chrono::Utc::now() + chrono::Duration::hours(6)).to_rfc3339();
    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &with(held.clone(), &[("expires_at", json!(later))]),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(read_permit(&db, &wp).await.approved_by_id, Some(approver));

    // Approved before approvers were recorded: shown "approver not recorded", never given one.
    exec(
        &db,
        &format!(
            "UPDATE work_permits SET created_by_id = NULL, submitted_by_id = NULL, submitted_at = NULL, \
             approved_by_id = NULL, approved_at = NULL, self_approved = NULL WHERE id = {}",
            wp.id
        ),
    )
    .await;
    let old = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let invented = with(
        old.clone(),
        &[
            ("approved_by_sync_id", json!(verifier)),
            ("approved_at", json!(now())),
            ("self_approved", json!(0)),
        ],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &invented).await;
    assert_eq!(code(&outcome), Some("PERMIT_APPROVAL_RECORD_CHANGED"), "{outcome:?}");
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &old).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "an old permit unchanged passes: {outcome:?}"
    );
}

/// A per-site rule change for the test site (org node 1) waiting in the inbox, as the server sends
/// it (`settings::site`): `value_json` is the rule's JSON text.
async fn site_rule_waiting(db: &DatabaseConnection, value_json: &str) {
    let payload: Payload = serde_json::from_value(json!({
        "site_node_sync_id": "permit-test-org-001",
        "setting_key": crate::reliability::study::decisions::SELF_APPROVAL_SETTING,
        "value_json": value_json,
    }))
    .expect("payload");
    inbox(db, "site_settings", "site-setting-waiting", 1, "deferred", &payload).await;
}

/// The author held here, none included, is never replaced by a received row: a permit written
/// before authors were recorded is not given one (it would then pass as approved by a second
/// person).
#[tokio::test]
async fn a_received_row_never_gives_an_author_to_a_permit_without_one() {
    let db = setup().await;
    let (wp, _) = submitted_by_applier(&db, "EQ-SYNC-4E5").await;
    exec(
        &db,
        &format!("UPDATE work_permits SET created_by_id = NULL WHERE id = {}", wp.id),
    )
    .await;
    let held = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let given = with(
        held.clone(),
        &[("created_by_sync_id", json!(user_sync(&db, VERIFIER).await))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &given).await;
    assert_eq!(code(&outcome), Some("PERMIT_APPROVAL_RECORD_CHANGED"), "{outcome:?}");
    assert_eq!(read_permit(&db, &wp).await.created_by_id, None);
    let later = (chrono::Utc::now() + chrono::Duration::hours(6)).to_rfc3339();
    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &with(held, &[("expires_at", json!(later))]),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A self-approval made under the site's opt-out waits while that opt-out is still in the inbox
/// here; a waiting change that keeps four-eyes does not hold it (conflict). Under the opt-out held
/// here, a self-approval waits while the site's return to four-eyes is still in the inbox.
#[tokio::test]
async fn a_received_self_approval_waits_for_its_sites_rule_still_arriving() {
    let db = setup().await;
    let applier = user_sync(&db, APPLIER).await;
    let (wp, held) = submitted_by_applier(&db, "EQ-SYNC-4E6").await;
    super::tests::make_test_site_a_planning_site(&db).await;
    let own = approved_by(&held, &applier, 1);

    site_rule_waiting(&db, "false").await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &own).await;
    assert_eq!(
        code(&outcome),
        Some("PERMIT_SELF_APPROVAL"),
        "four-eyes kept: {outcome:?}"
    );

    exec(&db, "DELETE FROM sync_inbox WHERE entity_type = 'site_settings'").await;
    site_rule_waiting(&db, " true ").await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &own).await;
    assert!(
        matches!(outcome, ApplyOutcome::Deferred { .. }),
        "opt-out arriving: {outcome:?}"
    );
    assert_eq!(permit_status(&db, &wp).await, "pending_review");

    // Opted out here, the return to four-eyes on its way: waits too.
    exec(&db, "DELETE FROM sync_inbox WHERE entity_type = 'site_settings'").await;
    super::tests::opt_site_out_of_four_eyes(&db).await;
    site_rule_waiting(&db, "false").await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &own).await;
    assert!(
        matches!(outcome, ApplyOutcome::Deferred { .. }),
        "return arriving: {outcome:?}"
    );
    exec(&db, "DELETE FROM sync_inbox WHERE entity_type = 'site_settings'").await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &own).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A permit received for the first time, already approved, is judged like any approval: on a
/// planning site keeping four-eyes its author's approval is refused, a second person's applied; a
/// self-approval naming a site not applied here yet waits for it.
#[tokio::test]
async fn a_new_permit_received_already_approved_is_judged_on_four_eyes() {
    let db = setup().await;
    let applier = user_sync(&db, APPLIER).await;
    let verifier = user_sync(&db, VERIFIER).await;
    let (_, held) = submitted_by_applier(&db, "EQ-SYNC-4E7").await;
    super::tests::make_test_site_a_planning_site(&db).await;
    let fresh = |code: &str| with(held.clone(), &[("code", json!(code))]);

    let own = approved_by(&fresh("PTW-NEW-4E-1"), &applier, 1);
    let outcome = apply(&db, "work_permits", "wp-new-4e-1", &own).await;
    assert_eq!(code(&outcome), Some("PERMIT_SELF_APPROVAL"), "{outcome:?}");

    let elsewhere = with(own, &[("entity_sync_id", json!("org-node-not-here-yet"))]);
    let outcome = apply(&db, "work_permits", "wp-new-4e-1", &elsewhere).await;
    assert!(
        matches!(outcome, ApplyOutcome::Deferred { .. }),
        "site not here: {outcome:?}"
    );

    let second = approved_by(&fresh("PTW-NEW-4E-2"), &verifier, 0);
    let outcome = apply(&db, "work_permits", "wp-new-4e-2", &second).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            "SELECT status FROM work_permits WHERE entity_sync_id = 'wp-new-4e-2'"
        )
        .await
        .as_deref(),
        Some("approved")
    );
}

/// A draft held here may arrive submitted and approved in one row (an offline PC folds its saves):
/// judged as a submission then an approval.
#[tokio::test]
async fn a_held_draft_received_approved_in_one_row_is_judged_as_submitted_then_approved() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-4E8").await;
    super::tests::make_test_site_a_planning_site(&db).await;
    let wp = create_permit(&db, "hot_work", asset).await;
    let applier = user_sync(&db, APPLIER).await;
    let draft = wire(&db, "work_permits", &wp.entity_sync_id).await;
    let submitted = with(
        draft,
        &[("submitted_by_sync_id", json!(applier)), ("submitted_at", json!(now()))],
    );
    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &approved_by(&submitted, &applier, 0),
    )
    .await;
    assert_eq!(code(&outcome), Some("PERMIT_SELF_APPROVAL"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "draft");
    let verifier = user_sync(&db, VERIFIER).await;
    let outcome = apply(
        &db,
        "work_permits",
        &wp.entity_sync_id,
        &approved_by(&submitted, &verifier, 0),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let stored = read_permit(&db, &wp).await;
    assert_eq!(
        (stored.status.as_str(), stored.submitted_by_id, stored.approved_by_id),
        ("approved", Some(APPLIER), Some(VERIFIER))
    );
}

/// An approval dated before its submission is an inconsistent record; one dated in the future is
/// refused like any received time beyond the clock tolerance.
#[tokio::test]
async fn a_received_approval_out_of_time_order_is_refused() {
    let db = setup().await;
    let (wp, held) = submitted_by_applier(&db, "EQ-SYNC-4E9").await;
    let verifier = user_sync(&db, VERIFIER).await;
    let before = with(
        approved_by(&held, &verifier, 0),
        &[("approved_at", json!("2020-01-01T00:00:00Z"))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &before).await;
    assert_eq!(code(&outcome), Some("PERMIT_APPROVAL_RECORD_INVALID"), "{outcome:?}");
    let future = with(
        approved_by(&held, &verifier, 0),
        &[("approved_at", json!("2999-01-01T00:00:00Z"))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &future).await;
    assert_eq!(code(&outcome), Some("SYNC_TIME_INCONSISTENT"), "{outcome:?}");
    assert_eq!(permit_status(&db, &wp).await, "pending_review");
}

// ── Lock taken off, forced removal and the worker told (migration 249) ───────────────────────────

/// An active LOTO permit here with one lock fitted by APPLIER and verified by VERIFIER.
async fn active_lock_here(
    db: &DatabaseConnection,
    asset_code: &str,
) -> (WorkPermit, crate::permit::domain::PermitIsolation) {
    let asset = insert_equipment(db, asset_code).await;
    let wp = advance_to_issued(db, create_permit(db, "loto", asset).await).await;
    let iso = apply_and_verify(db, &add_isolation(db, wp.id, &format!("{asset_code} breaker")).await).await;
    let wp = set_status(db, &wp, "active").await.expect("activate");
    (wp, iso)
}

fn attempts_json(at: &str) -> Value {
    json!(
        serde_json::to_string(&json!([{ "at": at, "method": "Téléphone", "result": "Pas de réponse" }])).expect("json")
    )
}

fn minutes_ago(m: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::minutes(m))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Each lock-removal rule refused on a received row, then a valid forced removal applied (and
/// announced here), then its record and the worker told never rewritten.
#[tokio::test]
async fn a_received_lock_removal_keeps_the_applier_and_forced_removal_rules() {
    let db = setup().await;
    let (_, iso) = active_lock_here(&db, "EQ-SYNC-LR1").await;
    let applier = user_sync(&db, APPLIER).await;
    let verifier = user_sync(&db, VERIFIER).await;
    user_with(&db, "sx-chef", &[crate::rbac::permissions::PTW_CLOSE]).await;
    let chef = "sx-chef-sync".to_string();
    user_with(&db, "sx-remover", &[]).await;
    let remover = "sx-remover-sync".to_string();
    user_with(&db, "sx-outsider", &[]).await;
    let outsider = "sx-outsider-sync".to_string();
    let base = wire(&db, "permit_isolations", &iso.entity_sync_id).await;
    let sid = iso.entity_sync_id.clone();
    let check = |p: Payload, expected: &'static str| {
        let db = &db;
        let sid = sid.clone();
        async move {
            let outcome = apply(db, "permit_isolations", &sid, &p).await;
            assert_eq!(code(&outcome), Some(expected), "{outcome:?}");
        }
    };
    let removed_at = minutes_ago(5);

    // Normal removal recorded by someone other than the applier; without its time.
    check(
        with(
            base.clone(),
            &[
                ("removed_by_sync_id", json!(verifier)),
                ("removed_at", json!(removed_at)),
            ],
        ),
        "PERMIT_LOCK_REMOVAL_NOT_APPLIER",
    )
    .await;
    check(
        with(base.clone(), &[("removed_by_sync_id", json!(applier))]),
        "PERMIT_LOCK_REMOVAL_INVALID",
    )
    .await;
    // Removal verified with no trace of the lock taken off (a PC before the update), or before it.
    check(
        with(
            base.clone(),
            &[
                ("removal_verified_by_sync_id", json!(verifier)),
                ("removal_verified_at", json!(now())),
            ],
        ),
        "PERMIT_REMOVAL_PRE_UPGRADE",
    )
    .await;
    check(
        with(
            base.clone(),
            &[
                ("removed_by_sync_id", json!(applier)),
                ("removed_at", json!(now())),
                ("removal_verified_by_sync_id", json!(verifier)),
                ("removal_verified_at", json!(minutes_ago(30))),
            ],
        ),
        "PERMIT_REMOVAL_LOCK_NOT_REMOVED",
    )
    .await;

    // A forced removal: its whole record, the people apart.
    let forced = with(
        base.clone(),
        &[
            ("removed_by_sync_id", json!(remover)),
            ("removed_at", json!(removed_at)),
            ("forced_removal", json!(1)),
            ("forced_authorized_by_sync_id", json!(chef)),
            (
                "forced_reason",
                json!("Fin de poste, travailleur parti sans retirer son cadenas"),
            ),
            ("forced_worker_absent", json!(1)),
            ("forced_contact_attempts_json", attempts_json(&minutes_ago(20))),
        ],
    );
    for (key, value) in [
        ("forced_reason", json!("Absent")),
        ("forced_worker_absent", json!(0)),
        ("forced_contact_attempts_json", json!("[]")),
        ("forced_contact_attempts_json", json!("not a list")),
        ("forced_contact_attempts_json", attempts_json("2999-01-01T00:00:00Z")),
        ("forced_authorized_by_sync_id", Value::Null),
    ] {
        check(with(forced.clone(), &[(key, value)]), "PERMIT_FORCED_REMOVAL_INVALID").await;
    }
    // A forced-removal record on a removal not flagged forced.
    check(
        with(
            forced.clone(),
            &[("forced_removal", json!(0)), ("removed_by_sync_id", json!(applier))],
        ),
        "PERMIT_FORCED_REMOVAL_INVALID",
    )
    .await;
    check(
        with(forced.clone(), &[("forced_authorized_by_sync_id", json!(applier))]),
        "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_APPLIER",
    )
    .await;
    check(
        with(forced.clone(), &[("forced_authorized_by_sync_id", json!(remover))]),
        "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_REMOVER",
    )
    .await;
    check(
        with(forced.clone(), &[("removed_by_sync_id", json!(applier))]),
        "PERMIT_FORCED_REMOVAL_REMOVER_IS_APPLIER",
    )
    .await;
    // Authorised by someone without the close right at the permit's site here.
    check(
        with(forced.clone(), &[("forced_authorized_by_sync_id", json!(outsider))]),
        "PERMIT_FORCED_REMOVAL_UNAUTHORISED",
    )
    .await;
    // Its removal verified by the person who took it off.
    check(
        with(
            forced.clone(),
            &[
                ("removal_verified_by_sync_id", json!(remover)),
                ("removal_verified_at", json!(now())),
            ],
        ),
        "PERMIT_REMOVAL_VERIFIER_IS_REMOVER",
    )
    .await;
    assert_eq!(
        text(
            &db,
            &format!("SELECT removed_at FROM permit_isolations WHERE id = {}", iso.id)
        )
        .await,
        None,
        "nothing written"
    );

    // Valid: applied, and the close-right holders of the site here are told.
    let outcome = apply(&db, "permit_isolations", &sid, &forced).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let notices = text(
        &db,
        "SELECT CAST(COUNT(*) AS TEXT) FROM notification_events WHERE category_code = 'loto_forced_removal'",
    )
    .await;
    assert_eq!(notices.as_deref(), Some("1"));
    // Its record is never rewritten.
    check(
        with(
            forced.clone(),
            &[("forced_reason", json!("Un autre motif de plus de vingt caractères"))],
        ),
        "PERMIT_LOCK_REMOVAL_RECORD_CHANGED",
    )
    .await;

    // The worker told: in the app by the worker only; by a close-right holder with a method,
    // after the removal, never the worker themselves.
    let held = wire(&db, "permit_isolations", &sid).await;
    let told_by = |who: &str, at: String, in_app: bool| {
        let mut changes = vec![("informed_by_sync_id", json!(who)), ("informed_at", json!(at))];
        if in_app {
            changes.push(("informed_in_app", json!(1)));
        } else {
            changes.push(("informed_method", json!("Téléphone")));
        }
        with(held.clone(), &changes)
    };
    check(told_by(&verifier, now(), true), "PERMIT_WORKER_ACK_NOT_YOURS").await;
    check(told_by(&applier, now(), false), "PERMIT_WORKER_INFORMED_INVALID").await;
    check(told_by(&chef, minutes_ago(60), false), "PERMIT_WORKER_INFORMED_INVALID").await;
    check(told_by(&outsider, now(), false), "PERMIT_WORKER_INFORMED_UNAUTHORISED").await;
    let told = told_by(&chef, now(), false);
    let outcome = apply(&db, "permit_isolations", &sid, &told).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    check(
        with(told, &[("informed_method", json!("SMS"))]),
        "PERMIT_LOCK_REMOVAL_RECORD_CHANGED",
    )
    .await;
}

/// A lock received as applied by a worker with a forced removal they were not told of (taken off
/// before it was applied) is refused; it waits while the worker's acknowledgement is in the inbox;
/// once they are told here it is applied.
#[tokio::test]
async fn a_received_lock_of_a_worker_not_told_of_a_forced_removal_is_refused() {
    let db = setup().await;
    let (_, iso) = active_lock_here(&db, "EQ-SYNC-LR2").await;
    let chef = user_with(&db, "sx2-chef", &[crate::rbac::permissions::PTW_CLOSE]).await;
    let remover = user_with(&db, "sx2-remover", &[]).await;
    let method = crate::permit::contact_methods::list_contact_methods(&db)
        .await
        .expect("methods")[0]
        .code
        .clone();
    let forced = queries::force_permit_isolation_removal(
        &db,
        crate::permit::domain::PermitForcedRemovalInput {
            isolation_id: iso.id,
            expected_row_version: iso.row_version,
            removed_by_id: remover,
            reason: "Fin de poste, travailleur parti sans retirer son cadenas".into(),
            worker_absent_attested: true,
            contact_attempts: vec![crate::permit::domain::ContactAttempt {
                at: minutes_ago(30),
                method,
                result: "Pas de réponse".into(),
            }],
        },
        chef,
    )
    .await
    .expect("forced removal here");

    let asset = insert_equipment(&db, "EQ-SYNC-LR2b").await;
    let other = advance_to_issued(&db, create_permit(&db, "loto", asset).await).await;
    let line = add_isolation(&db, other.id, "Valve V-LR2").await;
    let applier = user_sync(&db, APPLIER).await;
    let fitted_now = with(
        wire(&db, "permit_isolations", &line.entity_sync_id).await,
        &[("applied_by_sync_id", json!(applier)), ("applied_at", json!(now()))],
    );
    let outcome = apply(&db, "permit_isolations", &line.entity_sync_id, &fitted_now).await;
    assert_eq!(code(&outcome), Some("PERMIT_WORKER_NOT_INFORMED"), "{outcome:?}");
    // Fitted before the forced removal: the worker could not have been told yet, accepted.
    let fitted_before = with(fitted_now.clone(), &[("applied_at", json!(minutes_ago(90)))]);
    let outcome = apply(&db, "permit_isolations", &line.entity_sync_id, &fitted_before).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    exec(
        &db,
        &format!(
            "UPDATE permit_isolations SET applied_by_id = NULL, applied_at = NULL WHERE id = {}",
            line.id
        ),
    )
    .await;

    // The worker's acknowledgement is waiting in the inbox: the lock waits for it.
    let acknowledged = with(
        wire(&db, "permit_isolations", &forced.entity_sync_id).await,
        &[
            ("informed_by_sync_id", json!(applier)),
            ("informed_at", json!(now())),
            ("informed_in_app", json!(1)),
        ],
    );
    inbox(
        &db,
        "permit_isolations",
        &forced.entity_sync_id,
        9,
        "pending",
        &acknowledged,
    )
    .await;
    let outcome = apply(&db, "permit_isolations", &line.entity_sync_id, &fitted_now).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    exec(&db, "DELETE FROM sync_inbox").await;

    // Told here (acknowledged at sign-in): applied.
    queries::acknowledge_forced_removal(&db, forced.id, APPLIER)
        .await
        .expect("acknowledged");
    let outcome = apply(&db, "permit_isolations", &line.entity_sync_id, &fitted_now).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A lock removal verified on a PC before the update (no trace of the lock taken off by its
/// applier) arriving on a PC that has the update: its own conflict for a person to check on site
/// (CMMS Advisor, S2 option a); meanwhile the lock stays fitted here — counted among the locks in
/// place, and the permit does not close.
#[tokio::test]
async fn a_removal_verified_before_the_update_is_held_for_review_and_the_lock_stays_fitted() {
    let db = setup().await;
    let (wp, iso) = active_lock_here(&db, "EQ-SYNC-LR3").await;
    let (_, locks_before) = queries::isolation_locks_in_place(&db).await.expect("locks");
    assert!(locks_before >= 1, "the fitted lock is counted before the row arrives");
    let verifier = user_sync(&db, VERIFIER).await;
    let pre_upgrade = with(
        wire(&db, "permit_isolations", &iso.entity_sync_id).await,
        &[
            ("removal_verified_by_sync_id", json!(verifier)),
            ("removal_verified_at", json!(now())),
        ],
    );
    let outcome = apply(&db, "permit_isolations", &iso.entity_sync_id, &pre_upgrade).await;
    assert_eq!(code(&outcome), Some("PERMIT_REMOVAL_PRE_UPGRADE"), "{outcome:?}");
    if let ApplyOutcome::Conflict { message, .. } = &outcome {
        assert!(message.contains("retrait vérifié avant la mise à jour"), "{message}");
    }
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
    let (_, locks_after) = queries::isolation_locks_in_place(&db).await.expect("locks");
    assert!(locks_after >= 1, "the lock is still counted in place");
    assert_eq!(locks_after, locks_before, "still a lock in place");
    let err = set_status(&db, &wp, "closed").await.expect_err("lock still fitted");
    assert!(format!("{err:?}").contains("PERMIT_REMOVAL_PENDING"), "{err:?}");
}

// ── A permit requirement carried from a request flagged "sécurité" (migration 255, AU-01, D1) ──

/// An account holding `ptw.issue` on the org node `site` only (a permanent role); its sync id.
async fn site_permit_issuer(db: &DatabaseConnection, username: &str, site: i64) -> String {
    exec(
        db,
        &format!(
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
             force_password_change, created_at, updated_at) VALUES ('{username}-sync', '{username}', '{username}', \
             'local', 1, 0, 0, datetime('now'), datetime('now'))"
        ),
    )
    .await;
    let user: i64 = text(
        db,
        &format!("SELECT CAST(id AS TEXT) FROM user_accounts WHERE username = '{username}'"),
    )
    .await
    .expect("user")
    .parse()
    .expect("id");
    let role =
        crate::auth::scope_tests::role(db, &format!("SO {username}"), &[crate::rbac::permissions::PTW_ISSUE]).await;
    crate::auth::scope_tests::assign(db, i32::try_from(user).expect("id"), role, "org_node", Some(site)).await;
    format!("{username}-sync")
}

/// A work order whose permit requirement comes from a safety request, as this PC holds it, and its
/// sync id.
async fn safety_origin_wo(db: &DatabaseConnection, equipment_code: &str) -> (i64, String) {
    let asset = insert_equipment(db, equipment_code).await;
    let (wo_id, _) = assigned_wo_requiring_permit(db, asset).await;
    exec(
        db,
        &format!("UPDATE work_orders SET permit_from_safety_di = 1, requires_permit = 1 WHERE id = {wo_id}"),
    )
    .await;
    let wo_sync = text(
        db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    (wo_id, wo_sync)
}

async fn requires_permit(db: &DatabaseConnection, wo_id: i64) -> Option<String> {
    text(
        db,
        &format!("SELECT CAST(requires_permit AS TEXT) FROM work_orders WHERE id = {wo_id}"),
    )
    .await
}

/// A received work order that clears a requirement carried from a safety request says who cleared
/// it (with the permit-issue right at its site here), when and why — as a plant rule's flag.
#[tokio::test]
async fn a_cleared_safety_request_requirement_is_rechecked() {
    let db = setup().await;
    let (wo_id, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-SO-1").await;
    let held = wire(&db, "work_orders", &wo_sync).await;
    assert_eq!(
        held.get("permit_from_safety_di"),
        Some(&json!(1)),
        "the origin travels with the row"
    );
    let cleared = |by: Value, at: Value, reason: Value| {
        with(
            held.clone(),
            &[
                ("requires_permit", json!(0)),
                ("permit_rule_cleared_by_sync_id", by),
                ("permit_rule_cleared_at", at),
                ("permit_rule_clear_reason", reason),
            ],
        )
    };
    let reason = json!("Inspection visuelle seulement");
    let admin = json!(user_sync(&db, APPLIER).await);

    // Cleared by the general edit of another PC: no who, when or why.
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &with(held.clone(), &[("requires_permit", json!(0))]),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    for (what, payload) in [
        ("no reason", cleared(admin.clone(), json!(now()), Value::Null)),
        ("short reason", cleared(admin.clone(), json!(now()), json!("court"))),
        ("no time", cleared(admin.clone(), Value::Null, reason.clone())),
        ("no person", cleared(Value::Null, json!(now()), reason.clone())),
    ] {
        let outcome = apply(&db, "work_orders", &wo_sync, &payload).await;
        assert_eq!(
            code(&outcome),
            Some("WO_PERMIT_RULE_CLEAR_INVALID"),
            "{what}: {outcome:?}"
        );
    }
    // Someone without the permit-issue right; someone holding it at another site only.
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(json!(user_sync(&db, VERIFIER).await), json!(now()), reason.clone()),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_UNAUTHORISED"), "{outcome:?}");
    exec(
        &db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (2, 'so-node-2', 'SO-SITE-2', 'Site 2', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
    )
    .await;
    let elsewhere = site_permit_issuer(&db, "so_issuer_2", 2).await;
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(json!(elsewhere), json!(now()), reason.clone()),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_UNAUTHORISED"), "{outcome:?}");
    // A person not applied here yet: waits.
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(json!("user-not-here"), json!(now()), reason.clone()),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(
        requires_permit(&db, wo_id).await.as_deref(),
        Some("1"),
        "nothing applied"
    );

    // The permit-issue right at the work order's site, who / when / why: applied.
    let issuer = site_permit_issuer(&db, "so_issuer_1", 1).await;
    let outcome = apply(
        &db,
        "work_orders",
        &wo_sync,
        &cleared(json!(issuer), json!(now()), reason.clone()),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(requires_permit(&db, wo_id).await.as_deref(), Some("0"));
    assert_eq!(
        text(
            &db,
            &format!("SELECT CAST(permit_from_safety_di AS TEXT) FROM work_orders WHERE id = {wo_id}")
        )
        .await
        .as_deref(),
        Some("1"),
        "the origin stays on record"
    );
}

/// The safety origin is never turned off by a received row; a row without the column (a PC without
/// migration 255) keeps the value held here, so its general-edit clearing is refused too.
#[tokio::test]
async fn the_safety_origin_is_never_removed_by_sync() {
    let db = setup().await;
    let (wo_id, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-SO-2").await;
    let held = wire(&db, "work_orders", &wo_sync).await;

    for payload in [
        with(held.clone(), &[("permit_from_safety_di", json!(0))]),
        with(
            held.clone(),
            &[("permit_from_safety_di", json!(0)), ("requires_permit", json!(0))],
        ),
        with(held.clone(), &[("permit_from_safety_di", Value::Null)]),
    ] {
        let outcome = apply(&db, "work_orders", &wo_sync, &payload).await;
        assert_eq!(code(&outcome), Some("WO_PERMIT_SAFETY_ORIGIN_CHANGED"), "{outcome:?}");
    }

    let mut old_pc = with(held.clone(), &[("requires_permit", json!(0))]);
    old_pc.remove("permit_from_safety_di");
    let outcome = apply(&db, "work_orders", &wo_sync, &old_pc).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    assert_eq!(requires_permit(&db, wo_id).await.as_deref(), Some("1"));

    // A row of that PC that leaves the requirement alone is applied, and the origin is kept.
    let mut old_pc = with(held, &[("title", json!("Replace breaker (updated)"))]);
    old_pc.remove("permit_from_safety_di");
    let outcome = apply(&db, "work_orders", &wo_sync, &old_pc).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            &format!("SELECT CAST(permit_from_safety_di AS TEXT) FROM work_orders WHERE id = {wo_id}")
        )
        .await
        .as_deref(),
        Some("1")
    );
}

/// A request on the equipment of site 1, flagged "sécurité"; its sync id.
async fn request_flagged_safety(db: &DatabaseConnection) -> String {
    let asset = insert_equipment(db, "EQ-SYNC-SO-DI").await;
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
        .await
        .expect("symptom")
        .expect("seeded symptom");
    let di = crate::di::queries::create_intervention_request(
        db,
        crate::di::queries::DiCreateInput {
            equipment_id: asset,
            org_node_id: 1,
            title: "Fuite".into(),
            description: "Fuite sur la pompe".into(),
            origin_type: "operator".into(),
            request_type: "repair".into(),
            symptom_code_id: Some(symptom),
            impact_level: "unknown".into(),
            production_impact: false,
            safety_flag: true,
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: "normal".into(),
            observed_at: None,
            source_inspection_anomaly_id: None,
            submitter_id: APPLIER,
        },
    )
    .await
    .expect("request");
    text(
        db,
        &format!("SELECT entity_sync_id FROM intervention_requests WHERE id = {}", di.id),
    )
    .await
    .expect("request sync id")
}

/// A work order new here, converted from a safety request on a PC without migration 255, judged with
/// the safety flag of its request here: not started, it is applied protected (origin, permit
/// required); past "ready" and unticked without who, when or why, it is refused; required, it is
/// applied with that origin.
#[tokio::test]
async fn a_new_work_order_from_an_older_pc_takes_the_safety_origin_of_its_request() {
    let db = setup().await;
    let (_, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-SO-3").await;
    let template = wire(&db, "work_orders", &wo_sync).await;
    let on_hold = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'on_hold'",
    )
    .await
    .expect("on hold status");
    let di = request_flagged_safety(&db).await;
    let new_row = |code: &str, requires: i64| {
        let mut p = with(
            template.clone(),
            &[
                ("code", json!(code)),
                ("source_di_sync_id", json!(di.clone())),
                ("requires_permit", json!(requires)),
            ],
        );
        p.remove("permit_from_safety_di");
        p
    };

    // Not started, unticked there by the general edit: applied, and protected here.
    let outcome = apply(&db, "work_orders", "wo-older-pc-unticked", &new_row("OT-OLD-PC-0", 0)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            "SELECT CAST(permit_from_safety_di AS TEXT) || '|' || CAST(requires_permit AS TEXT) \
             FROM work_orders WHERE entity_sync_id = 'wo-older-pc-unticked'"
        )
        .await
        .as_deref(),
        Some("1|1")
    );

    // Past "ready", unticked without who, when or why: refused, never applied silently.
    let outcome = apply(
        &db,
        "work_orders",
        "wo-older-pc-cleared",
        &with(new_row("OT-OLD-PC-1", 0), &[("status_sync_id", json!(on_hold))]),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");

    // Still required: applied, and protected here.
    let outcome = apply(&db, "work_orders", "wo-older-pc-flagged", &new_row("OT-OLD-PC-2", 1)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            "SELECT CAST(permit_from_safety_di AS TEXT) FROM work_orders WHERE entity_sync_id = 'wo-older-pc-flagged'"
        )
        .await
        .as_deref(),
        Some("1")
    );
}

// ── Review of AU-01: explicit origin 0, waits, site move ──────────────────────────────────────

/// A request on a new equipment of site 1 (flagged "sécurité" when `safety`); its sync id.
async fn request_on_site_1(db: &DatabaseConnection, equipment_code: &str, safety: bool) -> String {
    let asset = insert_equipment(db, equipment_code).await;
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
        .await
        .expect("symptom")
        .expect("seeded symptom");
    let di = crate::di::queries::create_intervention_request(
        db,
        crate::di::queries::DiCreateInput {
            equipment_id: asset,
            org_node_id: 1,
            title: "Fuite".into(),
            description: "Fuite sur la pompe".into(),
            origin_type: "operator".into(),
            request_type: "repair".into(),
            symptom_code_id: Some(symptom),
            impact_level: "unknown".into(),
            production_impact: false,
            safety_flag: safety,
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: "normal".into(),
            observed_at: None,
            source_inspection_anomaly_id: None,
            submitter_id: APPLIER,
        },
    )
    .await
    .expect("request");
    text(
        db,
        &format!("SELECT entity_sync_id FROM intervention_requests WHERE id = {}", di.id),
    )
    .await
    .expect("request sync id")
}

fn deferred_message(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Deferred { message, .. } => Some(message.as_str()),
        _ => None,
    }
}

/// A received work order past "ready" (its permit requirement frozen) that says origin 0 with a request
/// flagged here is stored as sent while it requires a permit: the start was judged where it started.
/// With a request not flagged, 0 is applied as sent, and so are its later rows.
#[tokio::test]
async fn an_explicit_origin_0_is_judged_the_same_on_every_row() {
    let db = setup().await;
    let (_, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-SO-4").await;
    let template = wire(&db, "work_orders", &wo_sync).await;
    let on_hold = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'on_hold'",
    )
    .await
    .expect("on hold status");
    let row = |code: &str, di: &str, title: &str| {
        with(
            template.clone(),
            &[
                ("code", json!(code)),
                ("status_sync_id", json!(on_hold.clone())),
                ("source_di_sync_id", json!(di)),
                ("requires_permit", json!(1)),
                ("permit_from_safety_di", json!(0)),
                ("title", json!(title)),
            ],
        )
    };
    let stored = |sync: &'static str| {
        let db = &db;
        async move {
            text(
                db,
                &format!(
                    "SELECT CAST(permit_from_safety_di AS TEXT) || '|' || CAST(requires_permit AS TEXT) \
                     FROM work_orders WHERE entity_sync_id = '{sync}'"
                ),
            )
            .await
        }
    };

    let flagged = request_on_site_1(&db, "EQ-SYNC-SO-DI-4", true).await;
    for title in ["Fuite pompe", "Fuite pompe (mise à jour)"] {
        let outcome = apply(
            &db,
            "work_orders",
            "wo-explicit-0-flagged",
            &row("OT-X0-1", &flagged, title),
        )
        .await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{title}: {outcome:?}");
        assert_eq!(
            stored("wo-explicit-0-flagged").await.as_deref(),
            Some("0|1"),
            "as sent, required"
        );
    }
    // Its later row unticking it without who, when or why: refused, although its stored origin is 0
    // (the request is flagged here, so it is judged as a safety work order on every row).
    let outcome = apply(
        &db,
        "work_orders",
        "wo-explicit-0-flagged",
        &with(
            row("OT-X0-1", &flagged, "Fuite pompe"),
            &[("requires_permit", json!(0))],
        ),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    assert_eq!(
        stored("wo-explicit-0-flagged").await.as_deref(),
        Some("0|1"),
        "still required"
    );
    // Past "ready", unticked without who, when or why: refused.
    let outcome = apply(
        &db,
        "work_orders",
        "wo-explicit-0-flagged-2",
        &with(row("OT-X0-3", &flagged, "Fuite"), &[("requires_permit", json!(0))]),
    )
    .await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");

    let plain = request_on_site_1(&db, "EQ-SYNC-SO-DI-5", false).await;
    for title in ["Fuite pompe", "Fuite pompe (mise à jour)"] {
        let outcome = apply(
            &db,
            "work_orders",
            "wo-explicit-0-plain",
            &row("OT-X0-2", &plain, title),
        )
        .await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{title}: {outcome:?}");
        assert_eq!(stored("wo-explicit-0-plain").await.as_deref(), Some("0|1"), "as sent");
    }
}

/// Two PCs offline: PC B flags the request while PC A links a work order to it (not flagged on A).
/// On B, A's work order (origin 0, no permit) and its later rows are applied and protected; A
/// protects its own copy when the request reaches it. Both end with the origin and a permit required.
#[tokio::test]
async fn a_request_flagged_on_one_pc_and_linked_on_another_converges_protected() {
    let db = setup().await;
    let (_, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-SO-8").await;
    let planning = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'planning'",
    )
    .await
    .expect("planning status");
    let template = wire(&db, "work_orders", &wo_sync).await;
    let state = |sync: &'static str| {
        let db = &db;
        async move {
            text(
                db,
                &format!(
                    "SELECT CAST(permit_from_safety_di AS TEXT) || '|' || CAST(requires_permit AS TEXT) \
                     FROM work_orders WHERE entity_sync_id = '{sync}'"
                ),
            )
            .await
        }
    };

    // On B: the request is flagged here; A's planning work order arrives without the origin.
    let flagged = request_on_site_1(&db, "EQ-SYNC-SO-DI-8", true).await;
    let from_a = |title: &str| {
        with(
            template.clone(),
            &[
                ("code", json!("OT-A-1")),
                ("source_di_sync_id", json!(flagged.clone())),
                ("status_sync_id", json!(planning.clone())),
                ("actual_start", Value::Null),
                ("requires_permit", json!(0)),
                ("permit_from_safety_di", json!(0)),
                ("title", json!(title)),
            ],
        )
    };
    let outcome = apply(&db, "work_orders", "wo-from-pc-a", &from_a("Fuite pompe")).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state("wo-from-pc-a").await.as_deref(), Some("1|1"), "protected here");
    // A edits it again before the request reaches it: applied, still protected here.
    let outcome = apply(&db, "work_orders", "wo-from-pc-a", &from_a("Fuite pompe (mise à jour)")).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state("wo-from-pc-a").await.as_deref(), Some("1|1"));
    // Once A received the request it sends the protected row: applied.
    let outcome = apply(
        &db,
        "work_orders",
        "wo-from-pc-a",
        &with(
            from_a("Fuite pompe"),
            &[("permit_from_safety_di", json!(1)), ("requires_permit", json!(1))],
        ),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state("wo-from-pc-a").await.as_deref(), Some("1|1"));

    // On A: its own work order linked to the request while not flagged, then the flagged request
    // arrives: A protects it in the same apply, recorded.
    let unflagged = request_on_site_1(&db, "EQ-SYNC-SO-DI-9", false).await;
    let outcome = apply(
        &db,
        "work_orders",
        "wo-on-pc-a",
        &with(
            template.clone(),
            &[
                ("code", json!("OT-A-2")),
                ("source_di_sync_id", json!(unflagged.clone())),
                ("status_sync_id", json!(planning.clone())),
                ("actual_start", Value::Null),
                ("requires_permit", json!(0)),
                ("permit_from_safety_di", json!(0)),
            ],
        ),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(state("wo-on-pc-a").await.as_deref(), Some("0|0"), "not flagged yet");
    let request = with(
        wire(&db, "intervention_requests", &unflagged).await,
        &[("safety_flag", json!(1))],
    );
    let outcome = apply(&db, "intervention_requests", &unflagged, &request).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        state("wo-on-pc-a").await.as_deref(),
        Some("1|1"),
        "protected when the flag arrives"
    );
    assert!(
        text(
            &db,
            "SELECT payload_json FROM wo_action_events e JOIN work_orders w ON w.id = e.wo_id \
             WHERE w.entity_sync_id = 'wo-on-pc-a' AND e.action_code = 'permit_safety_origin_raised'",
        )
        .await
        .is_some_and(|p| p.contains("safety_request_flagged")),
        "recorded"
    );
}

/// A new work order from an older PC waits for its request; a clearing judged at a site not applied
/// here yet waits for that site.
#[tokio::test]
async fn a_safety_check_waits_for_its_request_and_its_site() {
    let db = setup().await;
    let (_, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-SO-6").await;
    let held = wire(&db, "work_orders", &wo_sync).await;

    let mut older = with(
        held.clone(),
        &[
            ("code", json!("OT-WAIT-1")),
            ("source_di_sync_id", json!("di-not-here")),
        ],
    );
    older.remove("permit_from_safety_di");
    let outcome = apply(&db, "work_orders", "wo-waits-for-request", &older).await;
    let message = deferred_message(&outcome).unwrap_or_else(|| panic!("waits: {outcome:?}"));
    assert!(message.contains("request it was converted from"), "{message}");

    let moved_and_cleared = with(
        held,
        &[
            ("entity_sync_id", json!("node-not-here")),
            ("requires_permit", json!(0)),
            ("permit_rule_cleared_by_sync_id", json!(user_sync(&db, VERIFIER).await)),
            ("permit_rule_cleared_at", json!(now())),
            ("permit_rule_clear_reason", json!("Inspection visuelle seulement")),
        ],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &moved_and_cleared).await;
    let message = deferred_message(&outcome).unwrap_or_else(|| panic!("waits: {outcome:?}"));
    assert!(message.contains("its site node-not-here"), "{message}");
}

/// A cleared safety requirement moved to another site by a received row keeping the clearing is
/// refused; the same move with the clearing withdrawn (as the moving PC does) is applied.
#[tokio::test]
async fn a_received_site_move_does_not_carry_a_safety_clearing() {
    let db = setup().await;
    let (wo_id, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-SO-7").await;
    let issuer = site_permit_issuer(&db, "so_issuer_7", 1).await;
    let held = wire(&db, "work_orders", &wo_sync).await;
    let cleared = with(
        held,
        &[
            ("requires_permit", json!(0)),
            ("permit_rule_cleared_by_sync_id", json!(issuer)),
            ("permit_rule_cleared_at", json!(now())),
            ("permit_rule_clear_reason", json!("Inspection visuelle seulement")),
        ],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &cleared).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    exec(
        &db,
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (2, 'so-node-2', 'SO-SITE-2', 'Site 2', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
    )
    .await;

    let moved_kept = with(cleared.clone(), &[("entity_sync_id", json!("so-node-2"))]);
    let outcome = apply(&db, "work_orders", &wo_sync, &moved_kept).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_INVALID"), "{outcome:?}");
    assert_eq!(
        requires_permit(&db, wo_id).await.as_deref(),
        Some("0"),
        "nothing applied"
    );

    let moved_withdrawn = with(
        cleared,
        &[
            ("entity_sync_id", json!("so-node-2")),
            ("requires_permit", json!(1)),
            ("permit_rule_cleared_by_sync_id", Value::Null),
            ("permit_rule_cleared_at", Value::Null),
            ("permit_rule_clear_reason", Value::Null),
        ],
    );
    let outcome = apply(&db, "work_orders", &wo_sync, &moved_withdrawn).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        requires_permit(&db, wo_id).await.as_deref(),
        Some("1"),
        "in force again"
    );
}

/// N4 (AU-99 review): a work order this PC holds ready, received completed (its start made on
/// another PC on the way), is judged by the permit gate at its recorded start, as a received start
/// is; with a valid permit chain it is applied (the completion checks then pass on the state held
/// here).
#[tokio::test]
async fn a_completion_received_from_ready_passes_the_permit_gate() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-N4").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let completed = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'completed'",
    )
    .await
    .expect("status sync id");
    let status_sql = format!(
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = {wo_id}"
    );
    let status_of = || text(&db, &status_sql);
    let ready = status_of().await;
    // What the completion checks read here (corrective work): failure coding and no parts used.
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(&db, "vibration")
        .await
        .expect("symptom")
        .expect("seeded symptom");
    exec(
        &db,
        &format!(
            "INSERT INTO work_order_failure_details (work_order_id, symptom_id, cause_not_determined, notes, \
             is_temporary_repair, is_permanent_repair) VALUES ({wo_id}, {symptom}, 1, 'Cause not found on site', 0, 1)"
        ),
    )
    .await;
    let base = wire(&db, "work_orders", &wo_sync).await;
    let received = |start: String| {
        let row = with(
            base.clone(),
            &[
                ("status_sync_id", json!(completed)),
                ("actual_start", json!(start)),
                ("mechanically_completed_at", json!(now())),
                ("parts_actuals_confirmed", json!(1)),
                ("root_cause_summary", json!("Breaker contacts worn")),
            ],
        );
        async move { row }
    };

    // No permit received yet: waits (it may still be coming), nothing written.
    let outcome = apply(&db, "work_orders", &wo_sync, &received(now()).await).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    // A linked permit never active: refused with the start gate's code, nothing written.
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-N4 breaker").await).await;
    let outcome = apply(&db, "work_orders", &wo_sync, &received(now()).await).await;
    assert_eq!(code(&outcome), Some("PERMIT_NOT_ACTIVE"), "{outcome:?}");
    assert_eq!(status_of().await, ready);

    // Active with its isolation verified, started after the activation: applied.
    set_status(&db, &wp, "active").await.expect("activate");
    let outcome = apply(&db, "work_orders", &wo_sync, &received(now()).await).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(status_of().await.as_deref(), Some("completed"));
}

/// N4: a completed work order new here received by a snapshot (plant history) is not judged by
/// the permit gate; the same row received as a change waits for its permit.
#[tokio::test]
async fn a_snapshot_completed_work_order_new_here_is_not_judged_by_the_permit_gate() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-N4S").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let completed = text(
        &db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = 'completed'",
    )
    .await
    .expect("status sync id");
    let row = with(
        wire(&db, "work_orders", &wo_sync).await,
        &[
            ("status_sync_id", json!(completed)),
            ("actual_start", json!(now())),
            ("mechanically_completed_at", json!(now())),
        ],
    );
    // This PC never held it: removed here with what refers to it.
    exec(&db, "PRAGMA foreign_keys = OFF").await;
    for table in ["wo_state_transition_log WHERE wo_id", "wo_change_events WHERE wo_id"] {
        exec(&db, &format!("DELETE FROM {table} = {wo_id}")).await;
    }
    exec(&db, &format!("DELETE FROM work_orders WHERE id = {wo_id}")).await;
    exec(&db, "PRAGMA foreign_keys = ON").await;
    let insert = |batch: &str, version: i64| {
        format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
             row_version, payload_json, payload_hash, apply_status) VALUES ('{batch}', 'c1', 'work_orders', '{wo_sync}', \
             'upsert', {version}, '{}', 'h-{batch}', 'pending')",
            serde_json::to_string(&row).expect("json").replace('\'', "''")
        )
    };
    let applied_status = format!(
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id \
         WHERE w.entity_sync_id = '{wo_sync}'"
    );

    // As a change: waits for its permit, not written.
    exec(&db, &insert("b-n4-delta", 5)).await;
    let (_, conflicts) = inbox_pass(&db).await;
    assert_eq!(conflicts, 0);
    assert_eq!(text(&db, &applied_status).await, None, "waits for its permit");
    exec(&db, "DELETE FROM sync_inbox WHERE server_batch_id = 'b-n4-delta'").await;

    // By a snapshot this PC asked for: applied as the plant holds it.
    exec(&db, &insert("snapshot-all-n4", 6)).await;
    let (_, conflicts) = inbox_pass(&db).await;
    assert_eq!(conflicts, 0);
    assert_eq!(text(&db, &applied_status).await.as_deref(), Some("completed"));
}

// ── AU-04: plant permit rules on every work-order origin ──────────────────────────────────────

async fn wo_text(db: &DatabaseConnection, sync_id: &str, column: &str) -> Option<String> {
    text(
        db,
        &format!("SELECT CAST({column} AS TEXT) FROM work_orders WHERE entity_sync_id = '{sync_id}'"),
    )
    .await
}

async fn wo_events(db: &DatabaseConnection, sync_id: &str, action: &str) -> Option<String> {
    text(
        db,
        &format!(
            "SELECT CAST(COUNT(*) AS TEXT) FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
             WHERE wo.entity_sync_id = '{sync_id}' AND e.action_code = '{action}'"
        ),
    )
    .await
}

/// Owner decision D2 (a), audit finding AU-04 B2/S2/S3: a received work order without the rule that
/// matches it here (its sender has not applied that rule list) is applied, never refused. Not started
/// (by its status), it gets the rule here; the later rows of its sender, still without the rule, are
/// applied and the rule stays; a received change of its work type is judged the same way. Already
/// started, it is applied as received and recorded once as a safety finding.
#[tokio::test]
async fn a_received_work_order_without_the_rule_held_here_converges() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-RULE-1").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wo_sync = text(
        &db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    let template = wire(&db, "work_orders", &wo_sync).await;
    // The rule is held here; the PC that created the work orders below did not hold it.
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "CORRECTIVE_LOTO",
        json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    let mut statuses = std::collections::HashMap::new();
    for status in ["draft", "planning", "on_hold"] {
        let sync = text(
            &db,
            &format!("SELECT entity_sync_id FROM work_order_statuses WHERE code = '{status}'"),
        )
        .await
        .expect("status sync id");
        statuses.insert(status, sync);
    }
    let row = |code: &str, status: &str, requires: i64, changes: &[(&str, Value)]| {
        let p = with(
            template.clone(),
            &[
                ("code", json!(code)),
                ("status_sync_id", json!(statuses[status])),
                ("requires_permit", json!(requires)),
                ("permit_rule_code", Value::Null),
                ("permit_rule_permit_type", Value::Null),
                ("actual_start", Value::Null),
            ],
        );
        with(p, changes)
    };

    // Not started, without the permit: applied, and the rule recorded here.
    let outcome = apply(&db, "work_orders", "wo-rule-1", &row("OT-RM-1", "planning", 0, &[])).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(&db, "wo-rule-1", "permit_rule_code").await.as_deref(),
        Some("CORRECTIVE_LOTO")
    );
    assert_eq!(wo_text(&db, "wo-rule-1", "requires_permit").await.as_deref(), Some("1"));
    assert_eq!(
        wo_events(&db, "wo-rule-1", "permit_rule_applied").await.as_deref(),
        Some("1")
    );
    // A later row of its sender, still without the rule: applied, never WO_PERMIT_RULE_CHANGED, and
    // the rule stays.
    let later = row("OT-RM-1", "planning", 0, &[("title", json!("Replace breaker (2)"))]);
    let outcome = apply(&db, "work_orders", "wo-rule-1", &later).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(&db, "wo-rule-1", "permit_rule_code").await.as_deref(),
        Some("CORRECTIVE_LOTO")
    );
    assert_eq!(wo_text(&db, "wo-rule-1", "requires_permit").await.as_deref(), Some("1"));
    // The sender then sends it with the rule (it applied the list): the same trace, applied.
    let same = row(
        "OT-RM-1",
        "planning",
        1,
        &[
            ("permit_rule_code", json!("CORRECTIVE_LOTO")),
            ("permit_rule_permit_type", json!("loto")),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-rule-1", &same).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
    // Another rule over the one held here, not started: applied, the rule of the list here kept
    // (R2-S1; a started one is refused, `a_started_row_without_the_rule_keeps_the_rule_held_here`).
    let other = with(same, &[("permit_rule_code", json!("OTHER_RULE"))]);
    let outcome = apply(&db, "work_orders", "wo-rule-1", &other).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(&db, "wo-rule-1", "permit_rule_code").await.as_deref(),
        Some("CORRECTIVE_LOTO")
    );

    // S3: received with another work type (no rule), then changed to the ruled one.
    let preventive = template_type_other(&db).await;
    let outcome = apply(
        &db,
        "work_orders",
        "wo-rule-2",
        &row("OT-RM-2", "planning", 0, &[("type_sync_id", preventive.clone())]),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(wo_text(&db, "wo-rule-2", "permit_rule_code").await, None);
    let outcome = apply(&db, "work_orders", "wo-rule-2", &row("OT-RM-2", "planning", 0, &[])).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(&db, "wo-rule-2", "permit_rule_code").await.as_deref(),
        Some("CORRECTIVE_LOTO")
    );

    // S2: a draft with a start time is judged by its status (not started): it gets the rule.
    let outcome = apply(
        &db,
        "work_orders",
        "wo-rule-3",
        &row(
            "OT-RM-3",
            "draft",
            1,
            &[("actual_start", json!("2026-10-01T08:00:00Z"))],
        ),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(&db, "wo-rule-3", "permit_rule_code").await.as_deref(),
        Some("CORRECTIVE_LOTO")
    );

    // Started without the permit: applied as received (its history kept), recorded once as a finding.
    let started = row(
        "OT-RM-4",
        "on_hold",
        0,
        &[("actual_start", json!("2026-10-01T08:00:00Z"))],
    );
    let outcome = apply(&db, "work_orders", "wo-rule-4", &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(wo_text(&db, "wo-rule-4", "permit_rule_code").await, None);
    assert_eq!(
        wo_events(&db, "wo-rule-4", "permit_rule_missing_on_arrival")
            .await
            .as_deref(),
        Some("1")
    );
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM notification_events WHERE category_code = 'wo_permit_rule_missing' \
             AND json_extract(payload_json, '$.entity_sync_id') = 'wo-rule-4'"
        )
        .await
        .as_deref(),
        Some("1"),
        "the permit issuers of its site are told"
    );
    let again = with(started, &[("title", json!("Replace breaker (bis)"))]);
    let outcome = apply(&db, "work_orders", "wo-rule-4", &again).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_events(&db, "wo-rule-4", "permit_rule_missing_on_arrival")
            .await
            .as_deref(),
        Some("1"),
        "once"
    );
}

/// AU-04 B2: the rule list received here flags this PC's work orders not started yet that a rule
/// of it matches, as the PC that published it did; started ones are left as they are.
#[tokio::test]
async fn a_received_rule_list_flags_the_open_work_orders_here() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-SYNC-RULE-2").await;
    let actor = super::tests::admin_id(&db).await;
    let draft = crate::wo::queries::create_work_order(
        &db,
        crate::wo::domain::WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(asset),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: Some(1),
            planner_id: None,
            urgency_code: Some("normal".into()),
            subtype_code: None,
            title: "Pump seal".into(),
            description: None,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("draft");
    assert_eq!(draft.permit_rule_code, None);
    // A rule of the list as another PC sends it (here from a rule that matches nothing).
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "TEMPLATE",
        json!({ "work_type_code": "preventive", "permit_type_code": "loto" }),
    )
    .await;
    exec(
        &db,
        "UPDATE reference_values SET entity_sync_id = 'rv-template' WHERE code = 'TEMPLATE'",
    )
    .await;
    exec(
        &db,
        "UPDATE reference_sets SET entity_sync_id = 'rs-permit-rules' \
         WHERE entity_sync_id IS NULL AND id = (SELECT set_id FROM reference_values WHERE code = 'TEMPLATE')",
    )
    .await;
    let received = with(
        wire(&db, "reference_values", "rv-template").await,
        &[
            ("code", json!("CORRECTIVE_LOTO")),
            ("label", json!("CORRECTIVE_LOTO")),
            (
                "metadata_json",
                json!(json!({ "work_type_code": "corrective", "permit_type_code": "loto" }).to_string()),
            ),
        ],
    );
    let outcome = apply(&db, "reference_values", "rv-received-rule", &received).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let draft = crate::wo::queries::get_work_order(&db, draft.id)
        .await
        .expect("q")
        .expect("wo");
    assert!(draft.requires_permit);
    assert_eq!(draft.permit_rule_code.as_deref(), Some("CORRECTIVE_LOTO"));
    let cause = text(
        &db,
        &format!(
            "SELECT json_extract(payload_json, '$.cause') FROM wo_action_events WHERE wo_id = {} \
             AND action_code = 'permit_rule_applied'",
            draft.id
        ),
    )
    .await;
    assert_eq!(cause.as_deref(), Some("rules_received"));
}

/// The sync id of a work type other than "corrective".
async fn template_type_other(db: &DatabaseConnection) -> Value {
    json!(text(
        db,
        "SELECT v.entity_sync_id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
         JOIN reference_domains d ON d.id = s.domain_id \
         WHERE d.code = 'WORK.ORDER_TYPES' AND s.status = 'published' AND v.code = 'preventive'"
    )
    .await
    .expect("preventive type"))
}

// ── AU-04 round 2: a row without the rule never strips the rule held here (R2-B1), two derived
// rules converge (R2-S1), the finding reaches the permit issuers of the site (R2-S3) ──────────────

/// A received work-order row built from `template`, with these changes.
fn wo_row(template: &Payload, changes: &[(&str, Value)]) -> Payload {
    with(template.clone(), changes)
}

/// Applies a received row through the inbound path (`apply_inbound_payload`: capture suppressed,
/// as the inbox pass applies it), in a transaction kept only when the row is applied.
async fn apply_received(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    use sea_orm::TransactionTrait;
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::materialize::apply_inbound_payload(
        &tx,
        entity_type,
        sync_id,
        "upsert",
        1,
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

/// The sync id of a work-order status.
async fn status_sync(db: &DatabaseConnection, status: &str) -> Value {
    json!(text(
        db,
        &format!("SELECT entity_sync_id FROM work_order_statuses WHERE code = '{status}'")
    )
    .await
    .expect("status sync id"))
}

/// A work order not started (planning) received here, its rule `CORRECTIVE_LOTO` derived here: the
/// row as its sender sends it (without the rule), the template of later rows.
async fn ruled_work_order(db: &DatabaseConnection, equipment: &str, wo_sync: &str) -> Payload {
    let asset = insert_equipment(db, equipment).await;
    let (wo_id, _) = assigned_wo_requiring_permit(db, asset).await;
    let template_sync = text(
        db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    if text(db, "SELECT code FROM reference_values WHERE code = 'CORRECTIVE_LOTO'")
        .await
        .is_none()
    {
        crate::permit::requirement_rules::add_published_rule(
            db,
            "CORRECTIVE_LOTO",
            json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
        )
        .await;
    }
    let row = wo_row(
        &wire(db, "work_orders", &template_sync).await,
        &[
            ("code", json!(format!("OT-{wo_sync}"))),
            ("status_sync_id", status_sync(db, "planning").await),
            ("requires_permit", json!(0)),
            ("permit_rule_code", Value::Null),
            ("permit_rule_permit_type", Value::Null),
            ("actual_start", Value::Null),
        ],
    );
    let outcome = apply(db, "work_orders", wo_sync, &row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(db, wo_sync, "permit_rule_code").await.as_deref(),
        Some("CORRECTIVE_LOTO")
    );
    row
}

type RuleState = (Option<String>, Option<String>, Option<String>, Option<String>);

/// The rule, its permit type, the requirement and the time of a clearing of a work order here.
async fn rule_state(db: &DatabaseConnection, wo_sync: &str) -> RuleState {
    (
        wo_text(db, wo_sync, "permit_rule_code").await,
        wo_text(db, wo_sync, "permit_rule_permit_type").await,
        wo_text(db, wo_sync, "requires_permit").await,
        wo_text(db, wo_sync, "permit_rule_cleared_at").await,
    )
}

fn ruled(rule: &str, permit_type: &str) -> RuleState {
    (Some(rule.into()), Some(permit_type.into()), Some("1".into()), None)
}

/// R2-B1, started: a row without the rule (and without the permit) over a rule held here is applied
/// (its history is kept), the rule, its permit type and the requirement are kept here, the finding
/// is recorded once and told to the permit issuers of its site, and the resume gate still asks for
/// the permit. A row naming another rule on it is refused.
#[tokio::test]
async fn a_started_row_without_the_rule_keeps_the_rule_held_here() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R2B1-1", "wo-r2-1").await;
    let started = wo_row(
        &planning,
        &[
            ("status_sync_id", status_sync(&db, "on_hold").await),
            ("actual_start", json!("2026-10-01T08:00:00Z")),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r2-1", &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(&db, "wo-r2-1", "status_id").await,
        text(
            &db,
            "SELECT CAST(id AS TEXT) FROM work_order_statuses WHERE code = 'on_hold'"
        )
        .await,
        "its history is applied"
    );
    assert_eq!(rule_state(&db, "wo-r2-1").await, ruled("CORRECTIVE_LOTO", "loto"));
    assert_eq!(
        wo_events(&db, "wo-r2-1", "permit_rule_missing_on_arrival")
            .await
            .as_deref(),
        Some("1")
    );
    let finding = text(
        &db,
        "SELECT e.payload_json FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
         WHERE wo.entity_sync_id = 'wo-r2-1' AND e.action_code = 'permit_rule_missing_on_arrival'",
    )
    .await
    .expect("finding");
    let finding: Value = serde_json::from_str(&finding).expect("json");
    assert_eq!(finding["rule_code"], json!("CORRECTIVE_LOTO"));
    assert_eq!(finding["requires_permit"], json!(false), "what the row said");
    assert_eq!(finding["rule_held_here"], json!(true));
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM notification_events WHERE category_code = 'wo_permit_rule_missing' \
             AND json_extract(payload_json, '$.entity_sync_id') = 'wo-r2-1'"
        )
        .await
        .as_deref(),
        Some("1")
    );
    // The resume gate asks for the permit of the rule.
    let wo_id: i64 = text(
        &db,
        "SELECT CAST(id AS TEXT) FROM work_orders WHERE entity_sync_id = 'wo-r2-1'",
    )
    .await
    .expect("id")
    .parse()
    .expect("id");
    let gate = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await;
    assert!(
        format!("{gate:?}").contains("PERMIT_REQUIRED_TO_START"),
        "the permit is still required: {gate:?}"
    );
    // Again: still kept, the finding once.
    let again = wo_row(&started, &[("title", json!("Replace breaker (bis)"))]);
    let outcome = apply(&db, "work_orders", "wo-r2-1", &again).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r2-1").await, ruled("CORRECTIVE_LOTO", "loto"));
    assert_eq!(
        wo_events(&db, "wo-r2-1", "permit_rule_missing_on_arrival")
            .await
            .as_deref(),
        Some("1"),
        "once"
    );
    // A row that names another rule on the started work order: refused.
    let other = wo_row(
        &started,
        &[
            ("permit_rule_code", json!("OTHER_RULE")),
            ("permit_rule_permit_type", json!("loto")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r2-1", &other).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CHANGED"), "{outcome:?}");
}

/// R2-B1, not started: a row without the rule that also moves the work order to a work type no rule
/// matches, or that arrives after the rule was retired here, keeps the rule held here (a rule only
/// adds). Nothing of this is sent (derived in the apply).
#[tokio::test]
async fn a_not_started_row_without_the_rule_keeps_the_rule_held_here() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R2B1-2", "wo-r2-2").await;
    // Capture on, as on a PC that synchronises: only what the apply derives could be captured.
    crate::sync::capture::install_capture(&db).await.expect("capture");
    exec(&db, "DELETE FROM sync_capture_journal").await;
    let unruled = wo_row(&planning, &[("type_sync_id", template_type_other(&db).await)]);
    let outcome = apply_received(&db, "work_orders", "wo-r2-2", &unruled).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r2-2").await, ruled("CORRECTIVE_LOTO", "loto"));
    // The rule retired here since: kept.
    exec(
        &db,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'CORRECTIVE_LOTO'",
    )
    .await;
    let later = wo_row(&planning, &[("title", json!("Replace breaker (2)"))]);
    let outcome = apply_received(&db, "work_orders", "wo-r2-2", &later).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r2-2").await, ruled("CORRECTIVE_LOTO", "loto"));
    assert_eq!(
        wo_events(&db, "wo-r2-2", "permit_rule_missing_on_arrival")
            .await
            .as_deref(),
        Some("0"),
        "not started: no finding"
    );
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM sync_capture_journal WHERE entity_type = 'work_orders' \
             AND entity_sync_id = 'wo-r2-2'"
        )
        .await
        .as_deref(),
        Some("0"),
        "derived in the apply: never sent"
    );
}

/// R2-B1, clearings: a clearing received with a row without the rule is judged as a clearing of the
/// rule held here (refused from someone without the permit-issue right, accepted from a plant-wide
/// holder, the rule kept); a clearing held here is kept by a row that carries neither the rule nor
/// a clearing, and withdrawn when that row requires the permit.
#[tokio::test]
async fn a_clearing_with_a_row_without_the_rule_is_judged_against_the_rule_held_here() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R2B1-3", "wo-r2-3").await;
    let cleared_at = now();
    let cleared_by = |who: Value| {
        wo_row(
            &planning,
            &[
                ("permit_rule_cleared_by_sync_id", who),
                ("permit_rule_cleared_at", json!(cleared_at)),
                ("permit_rule_clear_reason", json!("Consignation faite par l'exploitant")),
            ],
        )
    };
    // From someone without the right: refused, nothing changes here.
    let verifier = json!(user_sync(&db, VERIFIER).await);
    let outcome = apply(&db, "work_orders", "wo-r2-3", &cleared_by(verifier)).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_UNAUTHORISED"), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r2-3").await, ruled("CORRECTIVE_LOTO", "loto"));
    // From the plant-wide permit issuer: accepted, the rule kept with the clearing.
    let admin = json!(user_sync(&db, APPLIER).await);
    let outcome = apply(&db, "work_orders", "wo-r2-3", &cleared_by(admin)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let (rule, permit_type, requires, at) = rule_state(&db, "wo-r2-3").await;
    assert_eq!(
        (rule.as_deref(), permit_type.as_deref(), requires.as_deref()),
        (Some("CORRECTIVE_LOTO"), Some("loto"), Some("0"))
    );
    assert!(at.is_some(), "the clearing is recorded");
    // A row of a PC that never had the rule nor the clearing: the clearing held here is kept.
    let stale = wo_row(&planning, &[("title", json!("Replace breaker (3)"))]);
    let outcome = apply(&db, "work_orders", "wo-r2-3", &stale).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        rule_state(&db, "wo-r2-3").await,
        (rule.clone(), permit_type.clone(), Some("0".into()), at)
    );
    assert_eq!(
        wo_text(&db, "wo-r2-3", "permit_rule_clear_reason").await.as_deref(),
        Some("Consignation faite par l'exploitant")
    );
    // The same PC requiring the permit: required, the clearing withdrawn.
    let required = wo_row(&planning, &[("requires_permit", json!(1))]);
    let outcome = apply(&db, "work_orders", "wo-r2-3", &required).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r2-3").await, ruled("CORRECTIVE_LOTO", "loto"));
}

/// R2-B1, safety origin: a work order from a request flagged "sécurité" that also holds a plant rule
/// here is not downgraded by a row without the rule: a clearing by a holder of the permit-issue right
/// at its site only (enough for a safety request) is refused, the rule needs it plant-wide.
#[tokio::test]
async fn a_row_without_the_rule_never_downgrades_a_ruled_safety_work_order() {
    use crate::auth::scope_tests::{assign, role, user};
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R2B1-4", "wo-r2-4").await;
    exec(
        &db,
        "UPDATE work_orders SET permit_from_safety_di = 1 WHERE entity_sync_id = 'wo-r2-4'",
    )
    .await;
    let issuer_role = role(&db, "r2 site issuer", &[crate::rbac::permissions::PTW_ISSUE]).await;
    let site_issuer = user(&db, "r2_site_issuer").await;
    assign(&db, site_issuer, issuer_role, "org_node", Some(1)).await;
    let site_cleared = wo_row(
        &planning,
        &[
            (super::sync_check::SAFETY_ORIGIN, json!(1)),
            ("permit_rule_cleared_by_sync_id", json!("scope-user-r2_site_issuer")),
            ("permit_rule_cleared_at", json!(now())),
            ("permit_rule_clear_reason", json!("Inspection visuelle seulement")),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r2-4", &site_cleared).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CLEAR_UNAUTHORISED"), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r2-4").await, ruled("CORRECTIVE_LOTO", "loto"));
}

/// R2-S1: two PCs that derived different rules for a work order not started (two versions of the
/// list) converge with no conflict: each keeps the rule that comes first in its list (a rule it
/// does not hold comes after its own); once the lists agree, both keep the same. Nothing is sent.
#[tokio::test]
async fn two_rules_derived_on_two_lists_converge_without_a_conflict() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R2S1-1", "wo-s1-1").await;
    // Capture on, as on a PC that synchronises: only what the apply derives could be captured.
    crate::sync::capture::install_capture(&db).await.expect("capture");
    exec(&db, "DELETE FROM sync_capture_journal").await;
    // The other PC derived A_FIRST from its list (v2), not held here yet: this PC keeps its own.
    let from_v2 = wo_row(
        &planning,
        &[
            ("permit_rule_code", json!("A_FIRST")),
            ("permit_rule_permit_type", json!("loto")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply_received(&db, "work_orders", "wo-s1-1", &from_v2).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-s1-1").await, ruled("CORRECTIVE_LOTO", "loto"));
    // The list here gets A_FIRST, first in its order: the next row converges on it.
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "A_FIRST",
        json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    exec(&db, "UPDATE reference_values SET sort_order = 0 WHERE code = 'A_FIRST'").await;
    let outcome = apply_received(&db, "work_orders", "wo-s1-1", &from_v2).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
    assert_eq!(rule_state(&db, "wo-s1-1").await, ruled("A_FIRST", "loto"));
    // A late row of a PC still on the old list (CORRECTIVE_LOTO): A_FIRST stays.
    let from_v1 = wo_row(
        &planning,
        &[
            ("permit_rule_code", json!("CORRECTIVE_LOTO")),
            ("permit_rule_permit_type", json!("loto")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply_received(&db, "work_orders", "wo-s1-1", &from_v1).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-s1-1").await, ruled("A_FIRST", "loto"));
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
             WHERE wo.entity_sync_id = 'wo-s1-1' AND e.action_code = 'permit_rule_applied' \
               AND json_extract(e.payload_json, '$.cause') = 'rules_converged'"
        )
        .await
        .as_deref(),
        Some("1"),
        "the one change of the rule held here is recorded; keeping it is not a change"
    );
    let change: Value = serde_json::from_str(
        &text(
            &db,
            "SELECT e.payload_json FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
             WHERE wo.entity_sync_id = 'wo-s1-1' AND e.action_code = 'permit_rule_applied' \
               AND json_extract(e.payload_json, '$.cause') = 'rules_converged'",
        )
        .await
        .expect("change"),
    )
    .expect("json");
    assert_eq!(
        (
            &change["previous_rule_code"],
            &change["previous_permit_type_code"],
            &change["rule_code"],
            &change["permit_type_code"],
        ),
        (
            &json!("CORRECTIVE_LOTO"),
            &json!("loto"),
            &json!("A_FIRST"),
            &json!("loto")
        )
    );
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM wo_change_events c JOIN work_orders wo ON wo.id = c.wo_id \
             WHERE wo.entity_sync_id = 'wo-s1-1' AND c.action = 'permit_rule_applied' \
               AND json_extract(c.details_json, '$.cause') = 'rules_converged'"
        )
        .await
        .as_deref(),
        Some("1"),
        "audited"
    );
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM sync_capture_journal WHERE entity_type = 'work_orders' \
             AND entity_sync_id = 'wo-s1-1'"
        )
        .await
        .as_deref(),
        Some("0"),
        "derived in the apply: never sent"
    );
}

/// R2-S3: the finding notice reaches the permit-issue right holders of the work order's site (and
/// the plant-wide ones), not those of another site; with nobody holding the right, the settings
/// administrators.
#[tokio::test]
async fn the_finding_notice_reaches_the_permit_issuers_of_the_site() {
    use crate::auth::scope_tests::{assign, node, role, user};
    let db = setup().await;
    let issuers = role(&db, "r2s3 issuer", &[crate::rbac::permissions::PTW_ISSUE]).await;
    let here = user(&db, "r2s3_here").await;
    let elsewhere = user(&db, "r2s3_elsewhere").await;
    let without = user(&db, "r2s3_without").await;
    let site = node(&db, "R2S3-SITE", None).await;
    let other_site = node(&db, "R2S3-OTHER", None).await;
    assert_ne!(site, other_site);
    assign(&db, here, issuers, "org_node", Some(site)).await;
    assign(&db, elsewhere, issuers, "org_node", Some(other_site)).await;
    async fn notices_to(db: &DatabaseConnection, user: i32, wo: &str) -> Option<String> {
        text(
            db,
            &format!(
                "SELECT CAST(COUNT(*) AS TEXT) FROM notifications n \
                   JOIN notification_events e ON e.id = n.notification_event_id \
                  WHERE e.category_code = 'wo_permit_rule_missing' AND n.recipient_user_id = {user} \
                    AND json_extract(e.payload_json, '$.entity_sync_id') = '{wo}'"
            ),
        )
        .await
    }
    let on_hold = status_sync(&db, "on_hold").await;
    let started = |planning: &Payload| {
        wo_row(
            planning,
            &[
                ("status_sync_id", on_hold.clone()),
                ("actual_start", json!("2026-10-01T08:00:00Z")),
            ],
        )
    };
    // Its site here: the work order and the rows its sender sends name it.
    let at_site = |wo: &'static str, planning: Payload| {
        let db = &db;
        async move {
            exec(
                db,
                &format!("UPDATE work_orders SET entity_id = {site} WHERE entity_sync_id = '{wo}'"),
            )
            .await;
            wo_row(&planning, &[("entity_sync_id", json!("scope-R2S3-SITE"))])
        }
    };
    let planning = at_site("wo-s3-1", ruled_work_order(&db, "EQ-R2S3-1", "wo-s3-1").await).await;
    let outcome = apply(&db, "work_orders", "wo-s3-1", &started(&planning)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(notices_to(&db, here, "wo-s3-1").await.as_deref(), Some("1"));
    assert_eq!(notices_to(&db, elsewhere, "wo-s3-1").await.as_deref(), Some("0"));
    assert_eq!(notices_to(&db, without, "wo-s3-1").await.as_deref(), Some("0"));
    // Nobody holds the right here (no role carries it, no elevated administrator is active): the
    // settings administrators.
    let planning = ruled_work_order(&db, "EQ-R2S3-2", "wo-s3-2").await;
    let settings = role(&db, "r2s3 settings", &[crate::rbac::permissions::ADM_SETTINGS]).await;
    let settings_admin = user(&db, "r2s3_settings").await;
    assign(&db, settings_admin, settings, "tenant", None).await;
    exec(
        &db,
        &format!(
            "DELETE FROM role_permissions WHERE permission_id = (SELECT id FROM permissions WHERE name = '{}')",
            crate::rbac::permissions::PTW_ISSUE
        ),
    )
    .await;
    exec(
        &db,
        &format!("UPDATE user_accounts SET is_active = 0 WHERE id = {APPLIER}"),
    )
    .await;
    let outcome = apply(&db, "work_orders", "wo-s3-2", &started(&planning)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        notices_to(&db, settings_admin, "wo-s3-2").await.as_deref(),
        Some("1"),
        "fallback"
    );
    assert_eq!(notices_to(&db, here, "wo-s3-2").await.as_deref(), Some("0"));
}

// ── AU-04 round 3: the received row never chooses the rule (R3-B1) ─────────────────────────────

/// The local id of a work order.
async fn wo_id_of(db: &DatabaseConnection, wo_sync: &str) -> i64 {
    text(
        db,
        &format!("SELECT CAST(id AS TEXT) FROM work_orders WHERE entity_sync_id = '{wo_sync}'"),
    )
    .await
    .expect("id")
    .parse()
    .expect("id")
}

/// R3-B1: a received rule listed here that sorts first but does not match the work order here never
/// replaces the rule held here: LOTO stays, nothing is recorded, and a live hot-work permit does not
/// satisfy the start gate.
#[tokio::test]
async fn a_received_rule_that_does_not_match_here_never_wins() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R3B1-1", "wo-r3-1").await;
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "R0_WELD",
        json!({ "work_type_code": "preventive", "permit_type_code": "hot_work" }),
    )
    .await;
    exec(&db, "UPDATE reference_values SET sort_order = 0 WHERE code = 'R0_WELD'").await;
    let swap = wo_row(
        &planning,
        &[
            ("permit_rule_code", json!("R0_WELD")),
            ("permit_rule_permit_type", json!("hot_work")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r3-1", &swap).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r3-1").await, ruled("CORRECTIVE_LOTO", "loto"));
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
             WHERE wo.entity_sync_id = 'wo-r3-1' AND json_extract(e.payload_json, '$.cause') = 'rules_converged'"
        )
        .await
        .as_deref(),
        Some("0"),
        "the rule held here is kept: no change"
    );
    // A live hot-work permit: the start gate still asks for a LOTO permit.
    let wo_id = wo_id_of(&db, "wo-r3-1").await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "hot_work", wo_id).await.expect("permit")).await;
    set_status(&db, &wp, "active").await.expect("activate");
    let gate = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await;
    assert!(
        format!("{gate:?}").contains("PERMIT_TYPE_REQUIRED_TO_START"),
        "a hot-work permit is not a LOTO: {gate:?}"
    );
}

/// R3-B1: with the rule held here retired (nothing matches here), a received made-up code, or the
/// same code with a made-up permit type, never wins: the rule held here and its type are kept.
#[tokio::test]
async fn a_made_up_rule_or_type_never_replaces_a_retired_rule_held_here() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R3B1-2", "wo-r3-2").await;
    exec(
        &db,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'CORRECTIVE_LOTO'",
    )
    .await;
    for (code, permit_type) in [("AAA", "a_forged"), ("CORRECTIVE_LOTO", "a_forged")] {
        let forged = wo_row(
            &planning,
            &[
                ("permit_rule_code", json!(code)),
                ("permit_rule_permit_type", json!(permit_type)),
                ("requires_permit", json!(1)),
            ],
        );
        let outcome = apply(&db, "work_orders", "wo-r3-2", &forged).await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{code}: {outcome:?}");
        assert_eq!(
            rule_state(&db, "wo-r3-2").await,
            ruled("CORRECTIVE_LOTO", "loto"),
            "{code} / {permit_type}"
        );
    }
}

/// R3-B1: a row that names another rule and moves a work order started here back to a status not
/// started is refused: started on either side counts.
#[tokio::test]
async fn a_status_moved_back_does_not_let_another_rule_through() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R3B1-3", "wo-r3-3").await;
    exec(
        &db,
        "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress'), \
         actual_start = '2026-10-01T08:00:00Z' WHERE entity_sync_id = 'wo-r3-3'",
    )
    .await;
    let back = wo_row(
        &planning,
        &[
            ("permit_rule_code", json!("A_OTHER")),
            ("permit_rule_permit_type", json!("hot_work")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r3-3", &back).await;
    assert_eq!(code(&outcome), Some("WO_PERMIT_RULE_CHANGED"), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r3-3").await, ruled("CORRECTIVE_LOTO", "loto"));
}

/// R3-N4: a row that says origin 0 for a work order not started whose request is flagged here, and
/// carries a clearing by someone without the right, is applied (the origin is derived here), then
/// protected in the same apply: origin 1, the permit required, the clearing withdrawn. Never stored
/// cleared.
#[tokio::test]
async fn an_origin_0_row_with_an_unauthorised_clearing_ends_protected() {
    let db = setup().await;
    let (_, wo_sync) = safety_origin_wo(&db, "EQ-SYNC-N4-1").await;
    let template = wire(&db, "work_orders", &wo_sync).await;
    let flagged = request_on_site_1(&db, "EQ-SYNC-N4-DI", true).await;
    let row = with(
        template,
        &[
            ("code", json!("OT-N4-1")),
            ("source_di_sync_id", json!(flagged)),
            ("status_sync_id", status_sync(&db, "planning").await),
            ("actual_start", Value::Null),
            ("permit_from_safety_di", json!(0)),
            ("requires_permit", json!(0)),
            ("permit_rule_code", Value::Null),
            ("permit_rule_permit_type", Value::Null),
            ("permit_rule_cleared_by_sync_id", json!(user_sync(&db, VERIFIER).await)),
            ("permit_rule_cleared_at", json!(now())),
            ("permit_rule_clear_reason", json!("Inspection visuelle seulement")),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-n4-origin-0", &row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            "SELECT CAST(permit_from_safety_di AS TEXT) || '|' || CAST(requires_permit AS TEXT) || '|' || \
             COALESCE(permit_rule_cleared_at, 'none') FROM work_orders WHERE entity_sync_id = 'wo-n4-origin-0'"
        )
        .await
        .as_deref(),
        Some("1|1|none"),
        "protected, the clearing withdrawn"
    );
}

// ── AU-04 round 4: a row naming a rule while none is held here never chooses it (R4-B1); a
// clearing withdrawn by the convergence is recorded (R4-S1) ─────────────────────────────────────

/// A work order new here as its sender sends it: not started (planning), naming `rule` / `permit_type`
/// and requiring the permit. The list here may hold `CORRECTIVE_LOTO` (`with_rule`).
async fn new_row_naming(
    db: &DatabaseConnection,
    equipment: &str,
    wo_sync: &str,
    rule: &str,
    permit_type: &str,
    with_rule: bool,
) -> Payload {
    let asset = insert_equipment(db, equipment).await;
    let (wo_id, _) = assigned_wo_requiring_permit(db, asset).await;
    let template_sync = text(
        db,
        &format!("SELECT entity_sync_id FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("wo sync id");
    if with_rule
        && text(db, "SELECT code FROM reference_values WHERE code = 'CORRECTIVE_LOTO'")
            .await
            .is_none()
    {
        crate::permit::requirement_rules::add_published_rule(
            db,
            "CORRECTIVE_LOTO",
            json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
        )
        .await;
    }
    wo_row(
        &wire(db, "work_orders", &template_sync).await,
        &[
            ("code", json!(format!("OT-{wo_sync}"))),
            ("status_sync_id", status_sync(db, "planning").await),
            ("requires_permit", json!(1)),
            ("permit_rule_code", json!(rule)),
            ("permit_rule_permit_type", json!(permit_type)),
            ("actual_start", Value::Null),
        ],
    )
}

/// The `rules_converged` events of a work order, as JSON.
async fn converged_events(db: &DatabaseConnection, wo_sync: &str) -> Vec<Value> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "SELECT e.payload_json FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
             WHERE wo.entity_sync_id = '{wo_sync}' AND e.action_code = 'permit_rule_applied' \
               AND json_extract(e.payload_json, '$.cause') = 'rules_converged' ORDER BY e.id"
        ),
    ))
    .await
    .expect("events")
    .iter()
    .map(|r| serde_json::from_str(&r.try_get_by_index::<String>(0).expect("json")).expect("json"))
    .collect()
}

/// R4-B1: a work order new here naming the rule that matches here with a weaker permit type gets
/// the rule's own type here, recorded; a live hot-work permit does not satisfy the start gate.
#[tokio::test]
async fn a_new_work_order_naming_a_weaker_permit_type_gets_the_rule_type_here() {
    let db = setup().await;
    let row = new_row_naming(&db, "EQ-R4B1-1", "wo-r4-1", "CORRECTIVE_LOTO", "hot_work", true).await;
    let outcome = apply(&db, "work_orders", "wo-r4-1", &row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r4-1").await, ruled("CORRECTIVE_LOTO", "loto"));
    let events = converged_events(&db, "wo-r4-1").await;
    assert_eq!(events.len(), 1, "recorded");
    assert_eq!(
        (&events[0]["previous_permit_type_code"], &events[0]["permit_type_code"]),
        (&json!("hot_work"), &json!("loto"))
    );
    let wo_id = wo_id_of(&db, "wo-r4-1").await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "hot_work", wo_id).await.expect("permit")).await;
    set_status(&db, &wp, "active").await.expect("activate");
    let gate = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await;
    assert!(
        format!("{gate:?}").contains("PERMIT_TYPE_REQUIRED_TO_START"),
        "a hot-work permit is not a LOTO: {gate:?}"
    );
}

/// R4-B1: a work order new here naming a rule that does not match it here gets the rule that does;
/// naming a rule while nothing matches here, it keeps the rule as sent (it only adds).
#[tokio::test]
async fn a_new_work_order_naming_a_rule_gets_this_pcs_rule_or_keeps_its_own_when_none_matches() {
    let db = setup().await;
    let row = new_row_naming(&db, "EQ-R4B1-2", "wo-r4-2", "R0_WELD", "hot_work", true).await;
    let outcome = apply(&db, "work_orders", "wo-r4-2", &row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r4-2").await, ruled("CORRECTIVE_LOTO", "loto"));
    assert_eq!(converged_events(&db, "wo-r4-2").await.len(), 1, "recorded");

    let db = setup().await;
    let row = new_row_naming(&db, "EQ-R4B1-3", "wo-r4-3", "SOME_RULE", "loto", false).await;
    let outcome = apply(&db, "work_orders", "wo-r4-3", &row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r4-3").await, ruled("SOME_RULE", "loto"));
    assert!(converged_events(&db, "wo-r4-3").await.is_empty(), "nothing changed");
}

/// R4-B1: a work order new here received started, naming the rule here with a weaker permit type, is
/// applied as received (its history) and recorded once as a finding naming both, the permit issuers
/// of its site told.
#[tokio::test]
async fn a_started_new_work_order_naming_a_weaker_type_raises_the_finding() {
    let db = setup().await;
    let row = new_row_naming(&db, "EQ-R4B1-4", "wo-r4-4", "CORRECTIVE_LOTO", "hot_work", true).await;
    let started = wo_row(
        &row,
        &[
            ("status_sync_id", status_sync(&db, "on_hold").await),
            ("actual_start", json!("2026-10-01T08:00:00Z")),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r4-4", &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        rule_state(&db, "wo-r4-4").await,
        ruled("CORRECTIVE_LOTO", "hot_work"),
        "applied as received"
    );
    let finding: Value = serde_json::from_str(
        &text(
            &db,
            "SELECT e.payload_json FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
             WHERE wo.entity_sync_id = 'wo-r4-4' AND e.action_code = 'permit_rule_missing_on_arrival'",
        )
        .await
        .expect("finding"),
    )
    .expect("json");
    assert_eq!(
        (
            &finding["rule_code"],
            &finding["permit_type_code"],
            &finding["received_permit_type_code"]
        ),
        (&json!("CORRECTIVE_LOTO"), &json!("loto"), &json!("hot_work"))
    );
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM notification_events WHERE category_code = 'wo_permit_rule_missing' \
             AND json_extract(payload_json, '$.entity_sync_id') = 'wo-r4-4'"
        )
        .await
        .as_deref(),
        Some("1")
    );
    // R5-S2: the notice and the audit name both rules and both types, never "without the rule".
    let (key, params) = (
        text(
            &db,
            "SELECT message_key FROM notification_events WHERE category_code = 'wo_permit_rule_missing' \
             AND json_extract(payload_json, '$.entity_sync_id') = 'wo-r4-4'",
        )
        .await,
        text(
            &db,
            "SELECT message_params_json FROM notification_events WHERE category_code = 'wo_permit_rule_missing' \
             AND json_extract(payload_json, '$.entity_sync_id') = 'wo-r4-4'",
        )
        .await
        .expect("params"),
    );
    assert_eq!(key.as_deref(), Some("wo_permit_rule_other_on_arrival"));
    let params: Value = serde_json::from_str(&params).expect("json");
    assert_eq!(
        (
            &params["rule"],
            &params["permit_type"],
            &params["received_rule"],
            &params["received_permit_type"]
        ),
        (
            &json!("CORRECTIVE_LOTO"),
            &json!("loto"),
            &json!("CORRECTIVE_LOTO"),
            &json!("hot_work")
        )
    );
    let stored = text(
        &db,
        "SELECT n.title || ' ' || COALESCE(n.body, '') FROM notifications n \
           JOIN notification_events e ON e.id = n.notification_event_id \
          WHERE e.category_code = 'wo_permit_rule_missing' \
            AND json_extract(e.payload_json, '$.entity_sync_id') = 'wo-r4-4' LIMIT 1",
    )
    .await
    .expect("notice");
    assert!(
        stored.contains("hot_work") && stored.contains("loto") && !stored.contains("without permit rule"),
        "{stored}"
    );
    let summary = text(
        &db,
        "SELECT c.summary FROM wo_change_events c JOIN work_orders wo ON wo.id = c.wo_id \
         WHERE wo.entity_sync_id = 'wo-r4-4' AND c.action = 'permit_rule_missing_on_arrival'",
    )
    .await
    .expect("audit");
    assert!(
        summary.contains("hot_work") && summary.contains("loto") && summary.contains("avec la règle"),
        "{summary}"
    );
}

/// R4-S1: a clearing held here, withdrawn by a row naming another rule while the convergence keeps
/// the rule held here, is recorded with who cleared it, when and why.
#[tokio::test]
async fn a_clearing_withdrawn_by_the_convergence_is_recorded() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-R4S1-1", "wo-r4s1").await;
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET requires_permit = 0, permit_rule_cleared_by_id = {APPLIER}, \
             permit_rule_cleared_at = '2026-10-01T07:00:00Z', permit_rule_clear_reason = 'Consignation faite par l''exploitant' \
             WHERE entity_sync_id = 'wo-r4s1'"
        ),
    )
    .await;
    let other = wo_row(
        &planning,
        &[
            ("permit_rule_code", json!("OTHER_RULE")),
            ("permit_rule_permit_type", json!("hot_work")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r4s1", &other).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r4s1").await, ruled("CORRECTIVE_LOTO", "loto"));
    let events = converged_events(&db, "wo-r4s1").await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        (
            &events[0]["withdrawn_cleared_at"],
            &events[0]["withdrawn_cleared_by_sync_id"],
            &events[0]["withdrawn_clear_reason"],
        ),
        (
            &json!("2026-10-01T07:00:00Z"),
            &json!(user_sync(&db, APPLIER).await),
            &json!("Consignation faite par l'exploitant")
        )
    );
}

// ── AU-04 round 5: a rule kept as received is recorded and re-checked (R5-S1); test gaps ────────

/// Events of a work order with this cause, as JSON.
async fn rule_events_with_cause(db: &DatabaseConnection, wo_sync: &str, cause: &str) -> Vec<Value> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "SELECT e.payload_json FROM wo_action_events e JOIN work_orders wo ON wo.id = e.wo_id \
             WHERE wo.entity_sync_id = '{wo_sync}' AND e.action_code = 'permit_rule_applied' \
               AND json_extract(e.payload_json, '$.cause') = '{cause}' ORDER BY e.id"
        ),
    ))
    .await
    .expect("events")
    .iter()
    .map(|r| serde_json::from_str(&r.try_get_by_index::<String>(0).expect("json")).expect("json"))
    .collect()
}

/// R5-S1: a rule kept as received because nothing here matched is recorded (once); when the list
/// here later matches the work order, the catch-up gives it this PC's rule and type, recorded.
#[tokio::test]
async fn a_rule_kept_as_received_is_recorded_and_rechecked_when_the_list_matches() {
    let db = setup().await;
    let row = new_row_naming(&db, "EQ-R5S1-1", "wo-r5-1", "CORRECTIVE_LOTO", "hot_work", false).await;
    for _ in 0..2 {
        let outcome = apply(&db, "work_orders", "wo-r5-1", &row).await;
        assert!(
            matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
            "{outcome:?}"
        );
    }
    assert_eq!(rule_state(&db, "wo-r5-1").await, ruled("CORRECTIVE_LOTO", "hot_work"));
    assert_eq!(
        rule_events_with_cause(&db, "wo-r5-1", "rule_received_unmatched")
            .await
            .len(),
        1,
        "recorded once"
    );
    // The list arrives here: the catch-up applies this PC's type.
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "CORRECTIVE_LOTO",
        json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    // This one, and the template work order of the fixture (planned here with no rule).
    assert_eq!(crate::permit::requirement_rules::catch_up(&db).await, 2);
    assert_eq!(rule_state(&db, "wo-r5-1").await, ruled("CORRECTIVE_LOTO", "loto"));
    let events = rule_events_with_cause(&db, "wo-r5-1", "catch_up").await;
    assert_eq!(events.len(), 1);
    assert_eq!(
        (&events[0]["previous_permit_type_code"], &events[0]["permit_type_code"]),
        (&json!("hot_work"), &json!("loto"))
    );
    assert_eq!(crate::permit::requirement_rules::catch_up(&db).await, 0, "once");
}

/// R5-S1: a work order received started with a weaker type (a finding), then received back in a
/// status not started, gets this PC's rule and type.
#[tokio::test]
async fn a_started_arrival_back_to_planning_gets_this_pcs_rule() {
    let db = setup().await;
    let planning = new_row_naming(&db, "EQ-R5S1-2", "wo-r5-2", "CORRECTIVE_LOTO", "hot_work", true).await;
    let started = wo_row(
        &planning,
        &[
            ("status_sync_id", status_sync(&db, "on_hold").await),
            ("actual_start", json!("2026-10-01T08:00:00Z")),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r5-2", &started).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r5-2").await, ruled("CORRECTIVE_LOTO", "hot_work"));
    let outcome = apply(&db, "work_orders", "wo-r5-2", &planning).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r5-2").await, ruled("CORRECTIVE_LOTO", "loto"));
    assert_eq!(
        rule_events_with_cause(&db, "wo-r5-2", "work_order_received")
            .await
            .len(),
        1,
        "recorded"
    );
}

/// R5-N5: a work order that exists here with no rule yet, received naming the rule here with a
/// weaker type, gets the rule's type here.
#[tokio::test]
async fn an_unstamped_work_order_here_naming_a_weaker_type_gets_the_rule_type_here() {
    let db = setup().await;
    let named = new_row_naming(&db, "EQ-R5N5-1", "wo-r5n5-1", "CORRECTIVE_LOTO", "hot_work", false).await;
    let unruled = wo_row(
        &named,
        &[
            ("permit_rule_code", Value::Null),
            ("permit_rule_permit_type", Value::Null),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r5n5-1", &unruled).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        wo_text(&db, "wo-r5n5-1", "permit_rule_code").await,
        None,
        "nothing here yet"
    );
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "CORRECTIVE_LOTO",
        json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    let outcome = apply(&db, "work_orders", "wo-r5n5-1", &named).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r5n5-1").await, ruled("CORRECTIVE_LOTO", "loto"));
}

/// R5-N5: a made-up permit type kept as received (nothing matches here) is recorded, asks for its
/// permit at the start, and a permit issuer can lift it before the start.
#[tokio::test]
async fn a_made_up_type_kept_as_received_is_recorded_and_can_be_lifted() {
    let db = setup().await;
    let row = new_row_naming(&db, "EQ-R5N5-2", "wo-r5n5-2", "SOME_RULE", "made_up", false).await;
    let outcome = apply(&db, "work_orders", "wo-r5n5-2", &row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r5n5-2").await, ruled("SOME_RULE", "made_up"));
    assert_eq!(
        rule_events_with_cause(&db, "wo-r5n5-2", "rule_received_unmatched")
            .await
            .len(),
        1
    );
    let wo_id = wo_id_of(&db, "wo-r5n5-2").await;
    let gate = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await;
    assert!(gate.is_err(), "its permit is asked for: {gate:?}");
    let row_version: i64 = text(
        &db,
        &format!("SELECT CAST(row_version AS TEXT) FROM work_orders WHERE id = {wo_id}"),
    )
    .await
    .expect("version")
    .parse()
    .expect("version");
    let cleared = crate::wo::workflow::actions::permit_requirement::clear_permit_rule_flag(
        &db,
        crate::wo::workflow::actions::permit_requirement::WoPermitRuleClearInput {
            wo_id,
            expected_row_version: row_version,
            reason: "Type de permis inconnu ici, travail sans risque".into(),
        },
        APPLIER,
    )
    .await
    .expect("lifted by a permit issuer");
    assert!(!cleared.requires_permit);
}

/// R5-N3: a clearing held here, replaced by a row naming the rule this PC's list now gives (kept as
/// received, no rewrite), is recorded; a clearing a new row carried for a rule this PC replaces is
/// recorded too.
#[tokio::test]
async fn both_clearing_withdrawals_of_the_convergence_are_recorded() {
    let db = setup().await;
    // Held clearing, the received rule kept (it is this PC's first match).
    let planning = ruled_work_order(&db, "EQ-R5N3-1", "wo-r5n3-1").await;
    exec(
        &db,
        &format!(
            "UPDATE work_orders SET requires_permit = 0, permit_rule_cleared_by_id = {APPLIER}, \
             permit_rule_cleared_at = '2026-10-01T07:00:00Z', permit_rule_clear_reason = 'Consignation faite par l''exploitant' \
             WHERE entity_sync_id = 'wo-r5n3-1'"
        ),
    )
    .await;
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "A_FIRST",
        json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    exec(&db, "UPDATE reference_values SET sort_order = 0 WHERE code = 'A_FIRST'").await;
    let first = wo_row(
        &planning,
        &[
            ("permit_rule_code", json!("A_FIRST")),
            ("permit_rule_permit_type", json!("loto")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r5n3-1", &first).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r5n3-1").await, ruled("A_FIRST", "loto"));
    let events = converged_events(&db, "wo-r5n3-1").await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["withdrawn_cleared_at"], json!("2026-10-01T07:00:00Z"));

    // A new row with a valid clearing for a weaker type: this PC's type, the clearing withdrawn.
    let db = setup().await;
    let named = new_row_naming(&db, "EQ-R5N3-2", "wo-r5n3-2", "CORRECTIVE_LOTO", "hot_work", true).await;
    let cleared_at = now();
    let cleared = wo_row(
        &named,
        &[
            ("requires_permit", json!(0)),
            ("permit_rule_cleared_by_sync_id", json!(user_sync(&db, APPLIER).await)),
            ("permit_rule_cleared_at", json!(cleared_at)),
            ("permit_rule_clear_reason", json!("Consignation faite par l'exploitant")),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-r5n3-2", &cleared).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-r5n3-2").await, ruled("CORRECTIVE_LOTO", "loto"));
    let events = converged_events(&db, "wo-r5n3-2").await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["received_withdrawn_cleared_at"], json!(cleared_at));
}

// ── CMMS Advisor decision on AU-04 R5-S3 (option A): permit types ranked by energy isolation ──

/// A held LOTO rule is never replaced by a hot-work rule the list here now gives first: kept, no
/// type-change notice.
#[tokio::test]
async fn a_recheck_never_replaces_a_held_isolation_type() {
    let db = setup().await;
    let planning = ruled_work_order(&db, "EQ-OPTA-1", "wo-opta-1").await;
    exec(
        &db,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'CORRECTIVE_LOTO'",
    )
    .await;
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "H_FIRST",
        json!({ "work_type_code": "corrective", "permit_type_code": "hot_work" }),
    )
    .await;
    let other = wo_row(
        &planning,
        &[
            ("permit_rule_code", json!("OTHER_RULE")),
            ("permit_rule_permit_type", json!("hot_work")),
            ("requires_permit", json!(1)),
        ],
    );
    let outcome = apply(&db, "work_orders", "wo-opta-1", &other).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-opta-1").await, ruled("CORRECTIVE_LOTO", "loto"));
    assert_eq!(
        text(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM notification_events WHERE message_key = 'wo_permit_rule_type_changed'"
        )
        .await
        .as_deref(),
        Some("0")
    );
}

/// A held hot-work rule replaced by the LOTO rule the list here now gives: the type change is
/// recorded and the permit issuers of the site are told.
#[tokio::test]
async fn a_held_rule_type_change_notifies_the_permit_issuers() {
    let db = setup().await;
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "H_RULE",
        json!({ "work_type_code": "corrective", "permit_type_code": "hot_work" }),
    )
    .await;
    let row = new_row_naming(&db, "EQ-OPTA-2", "wo-opta-2", "H_RULE", "hot_work", false).await;
    let outcome = apply(&db, "work_orders", "wo-opta-2", &row).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-opta-2").await, ruled("H_RULE", "hot_work"));
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "L_RULE",
        json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    let other = wo_row(&row, &[("permit_rule_code", json!("OTHER_RULE"))]);
    let outcome = apply(&db, "work_orders", "wo-opta-2", &other).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(rule_state(&db, "wo-opta-2").await, ruled("L_RULE", "loto"));
    let params: Value = serde_json::from_str(
        &text(
            &db,
            "SELECT message_params_json FROM notification_events WHERE message_key = 'wo_permit_rule_type_changed' \
             AND category_code = 'wo_permit_rule_missing'",
        )
        .await
        .expect("notice"),
    )
    .expect("json");
    assert_eq!(
        (&params["previous_permit_type"], &params["permit_type"]),
        (&json!("hot_work"), &json!("loto"))
    );
}
