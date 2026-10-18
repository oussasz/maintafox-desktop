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

async fn apply(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    crate::sync::row_codec::apply_row(
        db,
        &layout,
        sync_id,
        "upsert",
        &serde_json::to_string(payload).expect("json"),
    )
    .await
    .expect("apply")
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
    let late = with(active.clone(), &[("activated_at", json!("2999-01-01T00:00:00Z"))]);
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &late).await;
    assert_eq!(code(&outcome), Some("PERMIT_EXPIRED"), "{outcome:?}");
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
        crate::sync::materialize::materialize_inbox_batch(&db, None)
            .await
            .expect("materialize");
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

    // Requires a permit, none linked: refused, nothing written.
    let outcome = apply(&db, "work_orders", &wo_sync, &started).await;
    assert_eq!(code(&outcome), Some("PERMIT_REQUIRED_TO_START"), "{outcome:?}");
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

    // Active, not expired at the start, isolations verified: applied.
    set_status(&db, &wp, "active").await.expect("activate");
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
