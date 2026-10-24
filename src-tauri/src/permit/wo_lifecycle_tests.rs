//! A work order and its permits end in the right order (AU-02, AU-03, AU-101; owner decisions D4,
//! D5, D9): a work order is neither closed nor cancelled while one of its permits is open or a lock
//! is still fitted under them; a permit is neither closed nor cancelled while its work runs, and its
//! suspension puts the running work on hold; a permit ended on another PC before (or while) the work
//! ran here is applied and raised for review. Local commands and the arrival of received rows.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Map, Value};

use super::tests::{
    action, add_handover_role, add_isolation, admin_id, advance_to_issued, apply_and_verify,
    assigned_wo_requiring_permit, create_from_wo, insert_equipment, scalar_i64, set_status, setup, start, take_off,
    APPLIER, VERIFIER,
};
use crate::errors::AppError;
use crate::permit::queries;
use crate::sync::materialize::ApplyOutcome;

type Payload = Map<String, Value>;

/// Every message of a refusal (plain or coded).
fn messages(err: AppError) -> Vec<String> {
    match err {
        AppError::ValidationFailed(msgs) => msgs,
        AppError::OrgValidationFailed(issues) => issues.into_iter().map(|i| i.code).collect(),
        other => vec![format!("{other:?}")],
    }
}

fn has(msgs: &[String], code: &str) -> bool {
    msgs.iter().any(|m| m.starts_with(code))
}

async fn wo_status(db: &DatabaseConnection, wo_id: i64) -> String {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("query")
        .expect("work order");
    row.try_get_by_index::<String>(0).expect("status")
}

async fn wo_row_version(db: &DatabaseConnection, wo_id: i64) -> i64 {
    scalar_i64(db, &format!("SELECT row_version FROM work_orders WHERE id = {wo_id}")).await
}

async fn cancel_wo(db: &DatabaseConnection, wo_id: i64) -> Result<(), AppError> {
    crate::wo::queries::cancel_work_order(
        db,
        crate::wo::domain::WoCancelInput {
            id: wo_id,
            expected_row_version: wo_row_version(db, wo_id).await,
            actor_id: admin_id(db).await,
            cancel_reason: "Travaux reportés".into(),
        },
    )
    .await
    .map(|_| ())
}

async fn close_refusals(db: &DatabaseConnection, wo_id: i64) -> Vec<String> {
    let evidence = crate::wo::closeout::CloseEvidence {
        closer_user_id: Some(admin_id(db).await),
        no_downtime_attestation: None,
        no_downtime_attestation_reason: None,
        fmeca_override_reason: None,
        fmeca_override_signer_id: None,
        fmeca_override_signer_password: crate::wo::closeout::SignerPassword::Given(None),
    };
    let rpn = crate::reliability::advanced_rams::queries::fmeca_rpn_critical_threshold_i64_in(db)
        .await
        .expect("rpn");
    crate::wo::closeout::close_preflight(db, wo_id, &evidence, rpn)
        .await
        .expect("preflight")
        .0
}

/// The row as this PC would send it.
async fn wire(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    super::sync_check::local_wire(db, entity_type, sync_id)
        .await
        .expect("payload")
        .expect("row")
}

fn with(mut p: Payload, changes: &[(&str, Value)]) -> Payload {
    for (k, v) in changes {
        p.insert((*k).to_string(), v.clone());
    }
    p
}

/// Applies a received row as the inbox pass does: kept only when applied.
async fn apply(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
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

fn conflict_code(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

async fn wo_sync(db: &DatabaseConnection, wo_id: i64) -> String {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("query")
        .expect("work order");
    row.try_get_by_index::<String>(0).expect("sync id")
}

async fn status_sync(db: &DatabaseConnection, code: &str) -> String {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM work_order_statuses WHERE code = ?",
            [code.into()],
        ))
        .await
        .expect("query")
        .expect("status");
    row.try_get_by_index::<String>(0).expect("sync id")
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ── AU-02: the end of a work order waits for its permits (D4) ──────────────────

#[tokio::test]
async fn a_work_order_is_not_cancelled_while_a_permit_is_open_or_a_lock_fitted() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-AU02-1").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = create_from_wo(&db, "loto", wo_id).await.expect("permit");

    // A draft permit is open: refused before anything is written.
    let msgs = messages(cancel_wo(&db, wo_id).await.expect_err("open permit"));
    assert!(has(&msgs, "GATE_WO_CANCEL_PERMIT_OPEN"), "{msgs:?}");
    assert!(!has(&msgs, "GATE_WO_CANCEL_LOCKS_FITTED"), "{msgs:?}");
    assert_eq!(wo_status(&db, wo_id).await, "ready");

    // Its lock fitted, then the permit cancelled: no permit open, but the lock still holds the plant.
    let wp = advance_to_issued(&db, wp).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-AU02-1").await).await;
    set_status(&db, &wp, "cancelled").await.expect("permit cancelled");
    let msgs = messages(cancel_wo(&db, wo_id).await.expect_err("lock fitted"));
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(has(&msgs, "GATE_WO_CANCEL_LOCKS_FITTED"), "{msgs:?}");
    assert_eq!(wo_status(&db, wo_id).await, "ready");

    // Lock taken off and its removal verified: the work order is cancelled.
    let iso = take_off(&db, &iso).await;
    queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("removal verified");
    cancel_wo(&db, wo_id).await.expect("cancelled");
    assert_eq!(wo_status(&db, wo_id).await, "cancelled");
}

#[tokio::test]
async fn the_close_gate_lists_an_open_permit_and_a_fitted_lock() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-AU02-2").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("permit")).await;
    let iso = apply_and_verify(&db, &add_isolation(&db, wp.id, "MCC-AU02-2").await).await;
    let wp = set_status(&db, &wp, "active").await.expect("active");

    let refusals = close_refusals(&db, wo_id).await;
    assert!(has(&refusals, "GATE_CLOSEOUT_PERMIT_OPEN"), "{refusals:?}");
    assert!(has(&refusals, "GATE_CLOSEOUT_LOCKS_FITTED"), "{refusals:?}");

    // Lock removal verified: only the permit, still active, holds the close.
    let iso = take_off(&db, &iso).await;
    queries::verify_permit_isolation_removal(&db, action(&iso), VERIFIER)
        .await
        .expect("removal verified");
    let refusals = close_refusals(&db, wo_id).await;
    assert!(has(&refusals, "GATE_CLOSEOUT_PERMIT_OPEN"), "{refusals:?}");
    assert!(!has(&refusals, "GATE_CLOSEOUT_LOCKS_FITTED"), "{refusals:?}");

    // Closed: nothing of the permit holds the close any more.
    let wp = queries::get_work_permit(&db, wp.id)
        .await
        .expect("read")
        .expect("permit");
    set_status(&db, &wp, "closed").await.expect("permit closed");
    let refusals = close_refusals(&db, wo_id).await;
    assert!(
        !refusals
            .iter()
            .any(|r| r.starts_with("GATE_CLOSEOUT_PERMIT_OPEN") || r.starts_with("GATE_CLOSEOUT_LOCKS_FITTED")),
        "{refusals:?}"
    );
}

#[tokio::test]
async fn a_cancellation_received_while_a_permit_is_open_here_is_a_conflict() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-AU02-3").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = create_from_wo(&db, "hot_work", wo_id).await.expect("permit");
    let sync = wo_sync(&db, wo_id).await;
    let cancelled = with(
        wire(&db, "work_orders", &sync).await,
        &[
            ("status_sync_id", json!(status_sync(&db, "cancelled").await)),
            ("cancelled_at", json!(now())),
            ("cancel_reason", json!("Annulé sur un autre poste")),
        ],
    );

    let outcome = apply(&db, "work_orders", &sync, &cancelled).await;
    assert_eq!(
        conflict_code(&outcome),
        Some(crate::wo::closeout_sync::codes::CANCEL_GATE),
        "{outcome:?}"
    );
    assert_eq!(wo_status(&db, wo_id).await, "ready", "nothing written");

    // Once the permit is cancelled here too, the same row applies.
    set_status(&db, &wp, "cancelled").await.expect("permit cancelled");
    let outcome = apply(&db, "work_orders", &sync, &cancelled).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(wo_status(&db, wo_id).await, "cancelled");
}

/// Mechanical completion is not refused by an open permit (the permit-to-work order: completion,
/// then lock removal, permit close and hand-back); only close and cancellation wait for them.
#[tokio::test]
async fn mechanical_completion_is_not_held_by_an_open_permit() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-AU02-5").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "hot_work", wo_id).await.expect("permit")).await;
    set_status(&db, &wp, "active").await.expect("active");
    start(&db, wo_id, rv).await.expect("started");
    let gates = crate::wo::execution::collect_completion_gates(&db, wo_id)
        .await
        .expect("gates");
    assert!(
        gates.iter().all(|g| !g.code.contains("PERMIT")),
        "no permit gate on completion: {gates:?}"
    );
}

// ── AU-03 / D5: a permit's end waits for its running work; a suspension holds it ────

/// An active hot-work permit (no isolation) linked to a started work order.
async fn running_work(db: &DatabaseConnection, asset_code: &str) -> (i64, crate::permit::domain::WorkPermit) {
    let asset = insert_equipment(db, asset_code).await;
    let (wo_id, rv) = assigned_wo_requiring_permit(db, asset).await;
    let wp = advance_to_issued(db, create_from_wo(db, "hot_work", wo_id).await.expect("permit")).await;
    let wp = set_status(db, &wp, "active").await.expect("active");
    start(db, wo_id, rv).await.expect("started");
    (wo_id, wp)
}

async fn reread(db: &DatabaseConnection, wp: &crate::permit::domain::WorkPermit) -> crate::permit::domain::WorkPermit {
    queries::get_work_permit(db, wp.id)
        .await
        .expect("read")
        .expect("permit")
}

async fn pause(db: &DatabaseConnection, wo_id: i64) {
    let reason = scalar_i64(
        db,
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id \
         JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS' AND rv.code = 'other' AND rs.status = 'published' LIMIT 1",
    )
    .await;
    crate::wo::execution::pause_wo(
        db,
        crate::wo::execution::WoPauseInput {
            wo_id,
            actor_id: admin_id(db).await,
            expected_row_version: wo_row_version(db, wo_id).await,
            delay_reason_id: reason,
            comment: None,
        },
    )
    .await
    .expect("pause");
}

async fn suspend(db: &DatabaseConnection, wp: &crate::permit::domain::WorkPermit, reason: &str) {
    queries::suspend_work_permit(
        db,
        crate::permit::domain::PermitSuspendInput {
            permit_id: wp.id,
            expected_row_version: reread(db, wp).await.row_version,
            reason: reason.into(),
            reactivation_conditions: None,
        },
        admin_id(db).await,
    )
    .await
    .expect("suspend");
}

async fn notices(db: &DatabaseConnection, category: &str) -> i64 {
    scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.category_code = '{category}'"
        ),
    )
    .await
}

async fn notices_to(db: &DatabaseConnection, category: &str, user_id: i64) -> i64 {
    scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.category_code = '{category}' AND n.recipient_user_id = {user_id}"
        ),
    )
    .await
}

async fn resume(db: &DatabaseConnection, wo_id: i64) -> Result<(), AppError> {
    crate::wo::execution::resume_wo(
        db,
        crate::wo::execution::WoResumeInput {
            wo_id,
            actor_id: admin_id(db).await,
            expected_row_version: wo_row_version(db, wo_id).await,
            estimated_remaining_hours: None,
        },
    )
    .await
    .map(|_| ())
}

#[tokio::test]
async fn a_permit_is_neither_closed_nor_cancelled_while_its_work_runs() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-AU03-1").await;

    let msgs = messages(set_status(&db, &wp, "closed").await.expect_err("close refused"));
    assert_eq!(msgs, vec!["GATE_PERMIT_CLOSE_WORK_IN_PROGRESS".to_string()]);
    let msgs = messages(set_status(&db, &wp, "cancelled").await.expect_err("cancel refused"));
    assert_eq!(msgs, vec!["GATE_PERMIT_CANCEL_WORK_IN_PROGRESS".to_string()]);
    assert_eq!(reread(&db, &wp).await.status, "active", "nothing written");
    assert_eq!(wo_status(&db, wo_id).await, "in_progress");

    // The work paused first: the permit closes.
    pause(&db, wo_id).await;
    set_status(&db, &reread(&db, &wp).await, "closed")
        .await
        .expect("closed once the work is paused");
}

#[tokio::test]
async fn suspending_a_permit_holds_its_running_work_until_the_permit_is_reinstated() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-D5-1").await;
    // Hidden from the hold screen by the plant: the suspension still holds the work with it.
    db.execute_unprepared("UPDATE reference_values SET is_active = 0 WHERE code = 'permit_suspended'")
        .await
        .expect("hide the reason");

    suspend(&db, &wp, "Détection de gaz").await;
    assert_eq!(reread(&db, &wp).await.status, "suspended");
    assert_eq!(wo_status(&db, wo_id).await, "on_hold", "held in the same transaction");
    let segment = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, s.hold_kind, s.comment FROM work_order_delay_segments s \
             JOIN reference_values rv ON rv.id = s.delay_reason_id WHERE s.work_order_id = ? AND s.ended_at IS NULL",
            [wo_id.into()],
        ))
        .await
        .expect("query")
        .expect("open delay segment");
    assert_eq!(
        segment.try_get_by_index::<String>(0).expect("code"),
        crate::wo::execution::PERMIT_SUSPENDED_DELAY_REASON
    );
    assert_eq!(segment.try_get_by_index::<String>(1).expect("kind"), "prerequisite");
    assert!(segment
        .try_get_by_index::<String>(2)
        .expect("comment")
        .contains("Détection de gaz"));
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM wo_state_transition_log WHERE wo_id = {wo_id} AND to_status = 'on_hold' \
                 AND reason_code = 'permit_suspended'"
            )
        )
        .await,
        1
    );
    // Its responsible (the work order's) is told.
    let responsible = admin_id(&db).await;
    assert_eq!(notices_to(&db, "permit_suspended_work_held", responsible).await, 1);

    // Resume refused while the permit is suspended.
    let msgs = messages(resume(&db, wo_id).await.expect_err("resume refused"));
    assert_eq!(msgs, vec!["PERMIT_SUSPENDED".to_string()]);

    // Reinstated: the work resumes.
    let wp = set_status(&db, &reread(&db, &wp).await, "revalidation_required")
        .await
        .expect("revalidation");
    set_status(&db, &wp, "active").await.expect("reinstated");
    resume(&db, wo_id).await.expect("resumed");
    assert_eq!(wo_status(&db, wo_id).await, "in_progress");
}

#[tokio::test]
async fn suspending_a_permit_without_running_work_changes_no_work_order() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-D5-2").await;
    let (wo_id, _) = assigned_wo_requiring_permit(&db, asset).await;
    let wp = advance_to_issued(&db, create_from_wo(&db, "hot_work", wo_id).await.expect("permit")).await;
    let wp = set_status(&db, &wp, "active").await.expect("active");
    suspend(&db, &wp, "Vent fort").await;
    assert_eq!(wo_status(&db, wo_id).await, "ready");
    assert_eq!(notices(&db, "permit_suspended_work_held").await, 0);
}

// ── AU-101 / D9: a permit ended elsewhere under work held here ─────────────────

fn ago(minutes: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::minutes(minutes))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

async fn review_conflicts(db: &DatabaseConnection, permit_sync: &str) -> Vec<(String, String)> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT conflict_type, status FROM sync_conflicts WHERE entity_type = 'work_permits' AND entity_sync_id = ? \
         ORDER BY id",
        [permit_sync.into()],
    ))
    .await
    .expect("conflicts")
    .iter()
    .map(|r| {
        (
            r.try_get_by_index::<String>(0).expect("type"),
            r.try_get_by_index::<String>(1).expect("status"),
        )
    })
    .collect()
}

/// The open suspension of `wp` as the other PC sends it (`suspended_at` moved to `at`), and the
/// permit's suspended row; then taken back here, so this PC holds the permit active again.
async fn suspension_as_received(
    db: &DatabaseConnection,
    wp: &crate::permit::domain::WorkPermit,
    at: Option<String>,
) -> (String, Payload, Payload) {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM permit_suspensions WHERE permit_id = ?",
            [wp.id.into()],
        ))
        .await
        .expect("query")
        .expect("suspension");
    let sync: String = row.try_get_by_index(0).expect("sync id");
    let suspended = wire(db, "work_permits", &wp.entity_sync_id).await;
    let mut suspension = wire(db, "permit_suspensions", &sync).await;
    if let Some(at) = at {
        suspension = with(suspension, &[("suspended_at", json!(at))]);
    }
    for sql in [
        format!("DELETE FROM permit_suspensions WHERE permit_id = {}", wp.id),
        format!("UPDATE work_permits SET status = 'active' WHERE id = {}", wp.id),
    ] {
        db.execute_unprepared(&sql).await.expect("undo");
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) VALUES (?, 'c1', 'permit_suspensions', ?, \
         'upsert', 2, ?, ?, 'pending')",
        [
            format!("b-{sync}").into(),
            sync.clone().into(),
            serde_json::to_string(&suspension).expect("json").into(),
            format!("h-{sync}").into(),
        ],
    ))
    .await
    .expect("inbox");
    (sync, suspended, suspension)
}

#[tokio::test]
async fn a_permit_closed_elsewhere_before_the_work_started_here_is_applied_and_raised() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-D9-1").await;
    // Closed on another PC an hour before the start recorded here (it did not have the start).
    let closed = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("closed")), ("closed_at", json!(ago(60)))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied),
        "the fact converges: {outcome:?}"
    );
    assert_eq!(reread(&db, &wp).await.status, "closed");
    assert_eq!(
        wo_status(&db, wo_id).await,
        "in_progress",
        "the work order is not changed here"
    );
    assert_eq!(
        review_conflicts(&db, &wp.entity_sync_id).await,
        vec![(
            crate::permit::ended_under_work::codes::ENDED_BEFORE_START.to_string(),
            "new".to_string()
        )]
    );
    // The work order's responsible is told (the permit close-right holders of the site too).
    let responsible = admin_id(&db).await;
    assert_eq!(notices_to(&db, "permit_ended_under_work", responsible).await, 1);

    // The same row again: already applied, nothing more raised.
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
    assert_eq!(review_conflicts(&db, &wp.entity_sync_id).await.len(), 1);
    assert_eq!(notices_to(&db, "permit_ended_under_work", responsible).await, 1);
}

#[tokio::test]
async fn a_permit_cancelled_elsewhere_while_the_work_runs_here_is_applied_and_raised() {
    let db = setup().await;
    let (_, wp) = running_work(&db, "EQ-D9-2").await;
    let cancelled = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("cancelled"))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &cancelled).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(reread(&db, &wp).await.status, "cancelled");
    assert_eq!(
        review_conflicts(&db, &wp.entity_sync_id).await,
        vec![(
            crate::permit::ended_under_work::codes::ENDED_DURING_WORK.to_string(),
            "new".to_string()
        )]
    );
}

#[tokio::test]
async fn a_permit_suspended_elsewhere_before_the_work_started_here_is_applied_and_raised() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-D9-3").await;
    // Made here to get its rows (it holds the work), then taken back: this PC holds the permit
    // active and the work running, and receives the suspension dated an hour before the start.
    suspend(&db, &wp, "Alarme gaz").await;
    let (_, suspended, _) = suspension_as_received(&db, &wp, Some(ago(60))).await;
    for sql in [
        format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress') \
             WHERE id = {wo_id}"
        ),
        format!("DELETE FROM work_order_delay_segments WHERE work_order_id = {wo_id}"),
    ] {
        db.execute_unprepared(&sql).await.expect("undo the hold");
    }

    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &suspended).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(reread(&db, &wp).await.status, "suspended");
    assert_eq!(
        review_conflicts(&db, &wp.entity_sync_id).await,
        vec![(
            crate::permit::ended_under_work::codes::ENDED_BEFORE_START.to_string(),
            "new".to_string()
        )]
    );
}

/// The normal order: the other PC suspended the permit with the work running there, so the work
/// order arrives on hold first; the suspension then arrives over held work: no review, the
/// responsible is told the work is held (D5).
#[tokio::test]
async fn a_suspension_received_over_held_work_is_not_a_conflict() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-D9-4").await;
    pause(&db, wo_id).await;
    suspend(&db, &wp, "Alarme incendie").await;
    let (_, suspended, _) = suspension_as_received(&db, &wp, None).await;
    let before = notices(&db, "permit_suspended_work_held").await;
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &suspended).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(review_conflicts(&db, &wp.entity_sync_id).await.is_empty());
    assert_eq!(notices(&db, "permit_suspended_work_held").await, before + 1);
}

// ── Review follow-ups: notice audiences, review-only conflicts, arrival cases ──────────

/// Permit close-right holders: one at the permit's site (node 1 of `setup`), one on another site,
/// and an account without the right. Returns (here, elsewhere, without).
async fn close_right_holders(db: &DatabaseConnection, tag: &str) -> (i64, i64, i64) {
    use crate::auth::scope_tests::{assign, node, role, user};
    let other_site = node(db, &format!("{tag}-OTHER"), None).await;
    let closer = role(db, &format!("{tag} closer"), &[crate::rbac::permissions::PTW_CLOSE]).await;
    let here = user(db, &format!("{tag}_closer_here")).await;
    let elsewhere = user(db, &format!("{tag}_closer_elsewhere")).await;
    let without = user(db, &format!("{tag}_no_right")).await;
    assign(db, here, closer, "org_node", Some(1)).await;
    assign(db, elsewhere, closer, "org_node", Some(other_site)).await;
    (i64::from(here), i64::from(elsewhere), i64::from(without))
}

/// S1: a running work order without a responsible, held by its permit's suspension: the notice goes
/// to the permit close-right holders of the permit's site (never to nobody).
#[tokio::test]
async fn a_suspension_holding_work_without_a_responsible_tells_the_close_right_holders_of_the_site() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-S1-1").await;
    let (here, elsewhere, without) = close_right_holders(&db, "s1").await;
    db.execute_unprepared(&format!(
        "UPDATE work_orders SET primary_responsible_id = NULL WHERE id = {wo_id}"
    ))
    .await
    .expect("no responsible");

    suspend(&db, &wp, "Fuite détectée").await;
    assert_eq!(wo_status(&db, wo_id).await, "on_hold");
    assert_eq!(notices_to(&db, "permit_suspended_work_held", here).await, 1);
    assert_eq!(notices_to(&db, "permit_suspended_work_held", elsewhere).await, 0);
    assert_eq!(notices_to(&db, "permit_suspended_work_held", without).await, 0);
}

/// S1: with a responsible, the notice stays theirs (the fallback is not added).
#[tokio::test]
async fn a_suspension_holding_work_with_a_responsible_does_not_use_the_fallback() {
    let db = setup().await;
    let (_, wp) = running_work(&db, "EQ-S1-2").await;
    let (here, _, _) = close_right_holders(&db, "s1b").await;
    suspend(&db, &wp, "Fuite détectée").await;
    assert_eq!(
        notices_to(&db, "permit_suspended_work_held", admin_id(&db).await).await,
        1
    );
    assert_eq!(notices_to(&db, "permit_suspended_work_held", here).await, 0);
}

/// N7: the hold made with a suspension is staged for the other PCs before the permit's change.
#[tokio::test]
async fn a_suspension_stages_the_work_order_hold_before_the_permit() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-N7-1").await;
    crate::sync::capture::install_capture(&db).await.expect("capture");
    for sql in ["DELETE FROM sync_capture_journal", "DELETE FROM sync_outbox"] {
        db.execute_unprepared(sql).await.expect("clear");
    }
    suspend(&db, &wp, "Alarme gaz").await;
    crate::sync::capture::drain_capture_journal(&db).await.expect("drain");
    let first = |entity: &str, sync: &str| {
        format!(
            "SELECT COALESCE(MIN(id), 0) FROM sync_outbox WHERE entity_type = '{entity}' AND entity_sync_id = '{sync}'"
        )
    };
    let wo_row = scalar_i64(&db, &first("work_orders", &wo_sync(&db, wo_id).await)).await;
    let permit_row = scalar_i64(&db, &first("work_permits", &wp.entity_sync_id)).await;
    assert!(wo_row > 0 && permit_row > 0, "both staged: {wo_row} {permit_row}");
    assert!(wo_row < permit_row, "the hold goes first: {wo_row} {permit_row}");
}

async fn resolve(db: &DatabaseConnection, conflict_id: i64, action: &str) -> Result<(), AppError> {
    let version = scalar_i64(
        db,
        &format!("SELECT row_version FROM sync_conflicts WHERE id = {conflict_id}"),
    )
    .await;
    crate::sync::queries::resolve_sync_conflict(
        db,
        admin_id(db).await,
        crate::sync::domain::ResolveSyncConflictInput {
            conflict_id,
            expected_row_version: version,
            action: action.into(),
            resolution_note: Some("Travail arrêté et vérifié sur place".into()),
        },
    )
    .await
    .map(|_| ())
}

async fn text_of(db: &DatabaseConnection, sql: &str) -> String {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<String>(0)
        .expect("text")
}

/// S2: a permit-ended review conflict accepts only "dismiss" and "escalate", refused before anything
/// is changed; dismissing is the recommended choice.
#[tokio::test]
async fn a_permit_ended_review_conflict_is_only_dismissed_or_escalated() {
    let db = setup().await;
    let (_, wp) = running_work(&db, "EQ-S2-1").await;
    let cancelled = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("cancelled"))],
    );
    assert!(matches!(
        apply(&db, "work_permits", &wp.entity_sync_id, &cancelled).await,
        ApplyOutcome::Applied
    ));
    let conflict_id = scalar_i64(
        &db,
        &format!(
            "SELECT id FROM sync_conflicts WHERE entity_type = 'work_permits' AND entity_sync_id = '{}'",
            wp.entity_sync_id
        ),
    )
    .await;
    assert_eq!(
        text_of(
            &db,
            &format!("SELECT recommended_action FROM sync_conflicts WHERE id = {conflict_id}")
        )
        .await,
        "dismiss"
    );
    let outbox_before = scalar_i64(&db, "SELECT COUNT(*) FROM sync_outbox").await;
    for action in ["accept_local", "accept_remote", "renumber_local", "retry_later"] {
        let msgs = messages(resolve(&db, conflict_id, action).await.expect_err(action));
        assert!(has(&msgs, "GATE_SYNC_CONFLICT_REVIEW_ONLY"), "{action}: {msgs:?}");
    }
    assert_eq!(review_conflicts(&db, &wp.entity_sync_id).await[0].1, "new", "unchanged");
    assert_eq!(reread(&db, &wp).await.status, "cancelled");
    assert_eq!(scalar_i64(&db, "SELECT COUNT(*) FROM sync_outbox").await, outbox_before);

    resolve(&db, conflict_id, "escalate").await.expect("escalated");
    resolve(&db, conflict_id, "dismiss").await.expect("dismissed");
    assert_eq!(review_conflicts(&db, &wp.entity_sync_id).await[0].1, "dismissed");
}

/// S5.1: the permit-ended notice goes to the responsible and to the permit close-right holders of
/// the permit's site only.
#[tokio::test]
async fn a_permit_ended_notice_reaches_the_close_right_holders_of_the_permit_site_only() {
    let db = setup().await;
    let (_, wp) = running_work(&db, "EQ-S5-1").await;
    let (here, elsewhere, without) = close_right_holders(&db, "s5").await;
    let closed = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("closed")), ("closed_at", json!(ago(60)))],
    );
    assert!(matches!(
        apply(&db, "work_permits", &wp.entity_sync_id, &closed).await,
        ApplyOutcome::Applied
    ));
    assert_eq!(notices_to(&db, "permit_ended_under_work", admin_id(&db).await).await, 1);
    assert_eq!(notices_to(&db, "permit_ended_under_work", here).await, 1);
    assert_eq!(notices_to(&db, "permit_ended_under_work", elsewhere).await, 0);
    assert_eq!(notices_to(&db, "permit_ended_under_work", without).await, 0);
}

/// S5.2: a closure dated after the start here, received while the work runs: ENDED_DURING_WORK.
#[tokio::test]
async fn a_timed_closure_after_the_start_received_while_the_work_runs_is_raised_during_work() {
    let db = setup().await;
    let (_, wp) = running_work(&db, "EQ-S5-2").await;
    let later = (chrono::Utc::now() + chrono::Duration::minutes(5))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let closed = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("closed")), ("closed_at", json!(later))],
    );
    assert!(matches!(
        apply(&db, "work_permits", &wp.entity_sync_id, &closed).await,
        ApplyOutcome::Applied
    ));
    assert_eq!(
        review_conflicts(&db, &wp.entity_sync_id).await,
        vec![(
            crate::permit::ended_under_work::codes::ENDED_DURING_WORK.to_string(),
            "new".to_string()
        )]
    );
    assert_eq!(notices_to(&db, "permit_ended_under_work", admin_id(&db).await).await, 1);
}

/// S5.3: a received end then left waiting (a parent not here yet) leaves no conflict and no notice;
/// once it applies, both come.
#[tokio::test]
async fn a_received_end_left_waiting_raises_nothing_until_it_applies() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-S5-3").await;
    // A suspension received with the closure, waiting for its author (not received yet): the
    // closure is judged with it (its group) and waits with it.
    suspend(&db, &wp, "Alarme gaz").await;
    suspension_as_received(&db, &wp, None).await;
    for sql in [
        format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress') \
             WHERE id = {wo_id}"
        ),
        format!("DELETE FROM work_order_delay_segments WHERE work_order_id = {wo_id}"),
        "UPDATE sync_inbox SET payload_json = json_set(payload_json, '$.suspended_by_sync_id', 'user-not-received-yet') \
         WHERE entity_type = 'permit_suspensions'"
            .to_string(),
    ] {
        db.execute_unprepared(&sql).await.expect("as received");
    }
    let closed = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("closed")), ("closed_at", json!(ago(60)))],
    );
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(reread(&db, &wp).await.status, "active");
    assert!(review_conflicts(&db, &wp.entity_sync_id).await.is_empty());
    assert_eq!(notices(&db, "permit_ended_under_work").await, 0);

    // Once nothing waits, the same row applies: raised and told.
    db.execute_unprepared("DELETE FROM sync_inbox WHERE entity_type = 'permit_suspensions'")
        .await
        .expect("received elsewhere");
    let outcome = apply(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(review_conflicts(&db, &wp.entity_sync_id).await.len(), 1);
    assert_eq!(notices(&db, "permit_ended_under_work").await, 1);
}

// ── D4 conditions (CMMS Advisor, 2026-10-24): "Terminé" with locks fitted ──────────────

/// A LOTO permit with `points` locks applied and verified, active, its work order started.
async fn running_loto_work(
    db: &DatabaseConnection,
    asset_code: &str,
    points: &[&str],
) -> (i64, Vec<crate::permit::domain::PermitIsolation>) {
    let asset = insert_equipment(db, asset_code).await;
    let (wo_id, rv) = assigned_wo_requiring_permit(db, asset).await;
    let wp = advance_to_issued(db, create_from_wo(db, "loto", wo_id).await.expect("permit")).await;
    let mut locks = Vec::new();
    for p in points {
        locks.push(apply_and_verify(db, &add_isolation(db, wp.id, p).await).await);
    }
    set_status(db, &wp, "active").await.expect("active");
    start(db, wo_id, rv).await.expect("started");
    (wo_id, locks)
}

async fn open_downtime(db: &DatabaseConnection, wo_id: i64) -> i64 {
    crate::wo::delay::open_downtime_segment(
        db,
        crate::wo::delay::OpenDowntimeInput {
            wo_id,
            downtime_type: "full".into(),
            comment: None,
            actor_id: admin_id(db).await,
            classification_code: None,
        },
    )
    .await
    .expect("downtime opened")
    .id
}

/// Every completion gate met but the downtime: labour closed, no parts, failure coding.
async fn complete(db: &DatabaseConnection, wo_id: i64) -> Result<(), AppError> {
    let actor = admin_id(db).await;
    crate::wo::labor::add_labor_entry(
        db,
        crate::wo::labor::AddLaborInput {
            wo_id,
            intervener_id: actor,
            skill_id: None,
            started_at: Some("2026-04-10T08:00:00Z".into()),
            ended_at: Some("2026-04-10T10:00:00Z".into()),
            hours_worked: None,
            hourly_rate: None,
            notes: None,
        },
    )
    .await
    .expect("labour");
    crate::wo::parts::confirm_no_parts_used(db, wo_id, actor)
        .await
        .expect("no parts");
    let symptom = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
        .await
        .expect("lookup symptom")
        .expect("seeded symptom");
    crate::wo::closeout::save_failure_detail(
        db,
        crate::wo::closeout::SaveFailureDetailInput {
            wo_id,
            symptom_id: Some(symptom),
            failure_mode_id: None,
            failure_cause_id: None,
            failure_effect_id: None,
            is_temporary_repair: false,
            is_permanent_repair: true,
            cause_not_determined: true,
            notes: Some("Disjoncteur remplacé".into()),
        },
    )
    .await
    .expect("failure detail");
    crate::wo::closeout::update_wo_rca(
        db,
        crate::wo::closeout::UpdateWoRcaInput {
            wo_id,
            root_cause_summary: Some("Usure".into()),
            corrective_action_summary: Some("Remplacement".into()),
        },
    )
    .await
    .expect("rca");
    crate::wo::execution::complete_wo_mechanically(
        db,
        crate::wo::execution::WoMechCompleteInput {
            wo_id,
            actor_id: actor,
            expected_row_version: wo_row_version(db, wo_id).await,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .map(|_| ())
}

async fn segment_end(db: &DatabaseConnection, segment_id: i64) -> Option<String> {
    db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT ended_at FROM work_order_downtime_segments WHERE id = {segment_id}"),
    ))
    .await
    .expect("query")
    .expect("segment")
    .try_get_by_index::<Option<String>>(0)
    .expect("ended_at")
}

async fn deisolation_pending(db: &DatabaseConnection, wo_id: i64) -> bool {
    crate::wo::queries::get_work_order(db, wo_id)
        .await
        .expect("read")
        .expect("work order")
        .deisolation_pending
}

async fn verify_removal(db: &DatabaseConnection, iso: &crate::permit::domain::PermitIsolation) -> String {
    let off = take_off(db, iso).await;
    queries::verify_permit_isolation_removal(db, action(&off), VERIFIER)
        .await
        .expect("removal verified")
        .removal_verified_at
        .expect("verified at")
}

/// D4 conditions 1 and 2: "Terminé" is allowed with locks fitted and the downtime still open; the
/// work order shows "Déconsignation à faire"; the downtime cannot be ended by hand, nor the work
/// order closed; lock removal is allowed from "Terminé"; the downtime ends at the moment the removal
/// of the LAST fitted lock is verified, audited; then the close is no longer held by them.
#[tokio::test]
async fn completion_with_locks_fitted_keeps_the_equipment_down_until_the_last_removal_is_verified() {
    let db = setup().await;
    let (wo_id, locks) = running_loto_work(&db, "EQ-D4C-1", &["MCC-D4C-1", "MCC-D4C-2"]).await;
    let segment = open_downtime(&db, wo_id).await;
    assert!(
        !deisolation_pending(&db, wo_id).await,
        "running work is not de-isolation"
    );

    complete(&db, wo_id).await.expect("Terminé with locks fitted");
    assert_eq!(wo_status(&db, wo_id).await, "completed");
    assert!(deisolation_pending(&db, wo_id).await);
    assert_eq!(segment_end(&db, segment).await, None, "the equipment stays down");

    let msgs = messages(
        crate::wo::delay::close_downtime_segment(&db, segment, None)
            .await
            .expect_err("not by hand"),
    );
    assert!(has(&msgs, "GATE_DOWNTIME_HELD_BY_LOCKS"), "{msgs:?}");
    let refusals = close_refusals(&db, wo_id).await;
    assert!(has(&refusals, "GATE_CLOSEOUT_LOCKS_FITTED"), "{refusals:?}");
    assert!(has(&refusals, "GATE_CLOSEOUT_OPEN_DOWNTIME"), "{refusals:?}");

    // First lock: one is still fitted, nothing ends.
    verify_removal(&db, &locks[0]).await;
    assert_eq!(segment_end(&db, segment).await, None);
    assert!(deisolation_pending(&db, wo_id).await);

    // Last lock: the downtime ends at that verification, in its transaction, audited.
    let verified_at = verify_removal(&db, &locks[1]).await;
    assert_eq!(segment_end(&db, segment).await.as_deref(), Some(verified_at.as_str()));
    assert!(!deisolation_pending(&db, wo_id).await);
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = '{}' AND target_id = '{wo_id}'",
                crate::wo::execution::AUDIT_DOWNTIME_ENDED_BY_LOCK_REMOVAL
            )
        )
        .await,
        1
    );
    let refusals = close_refusals(&db, wo_id).await;
    assert!(!has(&refusals, "GATE_CLOSEOUT_LOCKS_FITTED"), "{refusals:?}");
    assert!(!has(&refusals, "GATE_CLOSEOUT_OPEN_DOWNTIME"), "{refusals:?}");
}

/// Without a fitted lock, open downtime still refuses "Terminé" (the gate as it was).
#[tokio::test]
async fn without_a_fitted_lock_open_downtime_still_refuses_completion() {
    let db = setup().await;
    let (wo_id, _) = running_work(&db, "EQ-D4C-2").await;
    let segment = open_downtime(&db, wo_id).await;
    let msgs = messages(complete(&db, wo_id).await.expect_err("open downtime"));
    assert!(has(&msgs, "GATE_COMPLETE_OPEN_DOWNTIME"), "{msgs:?}");
    assert_eq!(wo_status(&db, wo_id).await, "in_progress");
    // Ended by hand (no lock holds it): completion goes through.
    crate::wo::delay::close_downtime_segment(&db, segment, None)
        .await
        .expect("closed by hand");
    complete(&db, wo_id).await.expect("completed");
}

/// Lock removal stays refused while the work runs, and is open once the work is "Terminé".
#[tokio::test]
async fn lock_removal_is_refused_under_running_work_and_open_once_terminated() {
    let db = setup().await;
    let (wo_id, locks) = running_loto_work(&db, "EQ-D4C-3", &["MCC-D4C-3"]).await;
    let refused = queries::record_permit_isolation_removed(&db, action(&locks[0]), APPLIER)
        .await
        .expect_err("work running");
    assert!(
        format!("{refused:?}").contains("PERMIT_REMOVAL_WORK_IN_PROGRESS"),
        "{refused:?}"
    );
    complete(&db, wo_id).await.expect("completed");
    verify_removal(&db, &locks[0]).await;
    assert!(!deisolation_pending(&db, wo_id).await);
}

/// The permit holders keep the work order on their list: its responsible, and whoever fitted one of
/// its locks; nobody else; until the last removal is verified.
#[tokio::test]
async fn a_terminated_work_order_with_a_lock_fitted_stays_on_its_permit_holders_list() {
    let db = setup().await;
    let (wo_id, locks) = running_loto_work(&db, "EQ-D4C-4", &["MCC-D4C-4"]).await;
    let other = i64::from(crate::auth::scope_tests::user(&db, "d4c_other").await);
    complete(&db, wo_id).await.expect("completed");
    let now = chrono::Utc::now();
    let mine = crate::home::my_work::my_work_orders(&db, admin_id(&db).await, now)
        .await
        .expect("my work");
    assert_eq!(mine.deisolation_pending, 1);
    assert_eq!(mine.deisolation_items[0].id, wo_id);
    assert!(
        mine.items.iter().all(|i| i.id != wo_id),
        "work done: not in the open list"
    );
    let theirs = crate::home::my_work::my_work_orders(&db, other, now)
        .await
        .expect("their work");
    assert_eq!(theirs.deisolation_pending, 0);

    // The lock's fitter, not responsible, sees it too.
    db.execute_unprepared(&format!(
        "UPDATE permit_isolations SET applied_by_id = {other} WHERE id = {}",
        locks[0].id
    ))
    .await
    .expect("fitted by another person");
    db.execute_unprepared(&format!(
        "UPDATE work_orders SET primary_responsible_id = NULL WHERE id = {wo_id}"
    ))
    .await
    .expect("no responsible");
    let theirs = crate::home::my_work::my_work_orders(&db, other, now)
        .await
        .expect("their work");
    assert_eq!(theirs.deisolation_pending, 1);
}

// ── CMMS Advisor answer to AU-101: hold on arrival (C1), untimed cancellation (C2, S4),
//    suspension lifted around a start (C3, S3) ─────────────────────────────────────────

/// Receives `payload` through the inbox and applies it as the sync worker does (the after-pass
/// consequences included). This PC's own changes are taken as sent (nothing unsent to cross).
async fn receive(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) {
    for sql in ["DELETE FROM sync_capture_journal", "DELETE FROM sync_outbox"] {
        db.execute_unprepared(sql).await.expect("taken as sent");
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) VALUES (?, 'c1', ?, ?, 'upsert', 9000, ?, ?, 'pending')",
        [
            format!("b-recv-{sync_id}").into(),
            entity_type.into(),
            sync_id.into(),
            serde_json::to_string(payload).expect("json").into(),
            format!("h-recv-{sync_id}").into(),
        ],
    ))
    .await
    .expect("inbox");
    let result = crate::sync::materialize::materialize_inbox_batch(db, None)
        .await
        .expect("materialize");
    crate::sync::row_codec::clear_ref_cache();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

async fn open_hold(db: &DatabaseConnection, wo_id: i64) -> (String, Option<i64>, String) {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, s.entered_by_id, s.entity_sync_id FROM work_order_delay_segments s \
             JOIN reference_values rv ON rv.id = s.delay_reason_id WHERE s.work_order_id = ? AND s.ended_at IS NULL",
            [wo_id.into()],
        ))
        .await
        .expect("query")
        .expect("open delay segment");
    (
        row.try_get_by_index(0).expect("code"),
        row.try_get_by_index(1).expect("by"),
        row.try_get_by_index(2).expect("sync id"),
    )
}

async fn notice_keys(db: &DatabaseConnection, category: &str) -> Vec<String> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT message_key FROM notification_events WHERE category_code = '{category}' ORDER BY id"),
    ))
    .await
    .expect("events")
    .iter()
    .map(|r| r.try_get_by_index::<String>(0).expect("key"))
    .collect()
}

/// C1: a closure received while the work runs here: the permit row, its review conflict and its
/// notices are applied; then the work order is put on hold in its own transaction, as the system,
/// with the reason "permit_ended", audited with the permit's sync id, and staged for the other PCs.
#[tokio::test]
async fn a_closure_received_over_running_work_puts_it_on_hold_after_the_apply() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-C1-1").await;
    crate::sync::capture::install_capture(&db).await.expect("capture");
    let closed = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("closed")), ("closed_at", json!(ago(60)))],
    );
    receive(&db, "work_permits", &wp.entity_sync_id, &closed).await;
    assert_eq!(reread(&db, &wp).await.status, "closed");
    assert_eq!(review_conflicts(&db, &wp.entity_sync_id).await.len(), 1);
    assert_eq!(notices(&db, "permit_ended_under_work").await, 1);

    assert_eq!(wo_status(&db, wo_id).await, "on_hold");
    let (reason, by, _) = open_hold(&db, wo_id).await;
    assert_eq!(reason, crate::wo::execution::PERMIT_ENDED_DELAY_REASON);
    assert_eq!(by, None, "recorded as the system");
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM wo_state_transition_log WHERE wo_id = {wo_id} AND to_status = 'on_hold' \
                 AND actor_id IS NULL AND reason_code = 'permit_ended'"
            )
        )
        .await,
        1
    );
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = '{}' AND actor_id IS NULL \
                 AND details_json LIKE '%{}%'",
                crate::wo::execution::AUDIT_HELD_PERMIT_ENDED_ELSEWHERE,
                wp.entity_sync_id
            )
        )
        .await,
        1
    );
    // A local change like any other: sent to the other PCs.
    crate::sync::capture::drain_capture_journal(&db).await.expect("drain");
    let staged = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM sync_outbox WHERE entity_type = 'work_orders' AND entity_sync_id = '{}'",
            wo_sync(&db, wo_id).await
        ),
    )
    .await;
    assert_eq!(staged, 1);
    // Resume stays refused by the permit gates.
    assert!(resume(&db, wo_id).await.is_err());

    // Idempotent: the same end looked at again writes nothing more.
    let version = wo_row_version(&db, wo_id).await;
    let again = crate::permit::ended_hold::sweep(&db).await;
    assert_eq!(again.work_orders_held, 0);
    assert_eq!(wo_row_version(&db, wo_id).await, version);
    assert_eq!(
        scalar_i64(
            &db,
            &format!("SELECT COUNT(*) FROM work_order_delay_segments WHERE work_order_id = {wo_id}")
        )
        .await,
        1
    );
}

/// C1: a suspension received over running work holds it with the reason "permit_suspended".
#[tokio::test]
async fn a_suspension_received_over_running_work_puts_it_on_hold_with_the_suspension_reason() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-C1-2").await;
    suspend(&db, &wp, "Alarme gaz").await;
    let (_, suspended, _) = suspension_as_received(&db, &wp, Some(ago(60))).await;
    for sql in [
        format!(
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'in_progress') \
             WHERE id = {wo_id}"
        ),
        format!("DELETE FROM work_order_delay_segments WHERE work_order_id = {wo_id}"),
    ] {
        db.execute_unprepared(&sql).await.expect("undo the hold");
    }
    assert!(matches!(
        apply(&db, "work_permits", &wp.entity_sync_id, &suspended).await,
        ApplyOutcome::Applied
    ));
    assert_eq!(crate::permit::ended_hold::sweep(&db).await.work_orders_held, 1);
    assert_eq!(wo_status(&db, wo_id).await, "on_hold");
    assert_eq!(
        open_hold(&db, wo_id).await.0,
        crate::wo::execution::PERMIT_SUSPENDED_DELAY_REASON
    );
}

/// C1: a hold that cannot be written never refuses the received permit: the row, its review
/// conflict and its notices are kept; the work order stays as it was.
#[tokio::test]
async fn a_hold_that_fails_after_the_apply_keeps_the_received_permit_and_its_review() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-C1-3").await;
    db.execute_unprepared(
        "CREATE TRIGGER test_refuse_delay BEFORE INSERT ON work_order_delay_segments \
         BEGIN SELECT RAISE(ABORT, 'test: hold refused'); END",
    )
    .await
    .expect("trigger");
    let cancelled = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("cancelled"))],
    );
    receive(&db, "work_permits", &wp.entity_sync_id, &cancelled).await;
    assert_eq!(reread(&db, &wp).await.status, "cancelled");
    assert_eq!(review_conflicts(&db, &wp.entity_sync_id).await.len(), 1);
    assert_eq!(notices(&db, "permit_ended_under_work").await, 1);
    assert_eq!(wo_status(&db, wo_id).await, "in_progress", "nothing half-written");
}

/// C2 (S4): a cancellation (no time) over work started here and not running — on hold, or
/// completed — is raised for review and told, with a text that gives no time.
#[tokio::test]
async fn an_untimed_cancellation_over_held_or_completed_work_is_raised_for_review() {
    for completed in [false, true] {
        let db = setup().await;
        let (wo_id, wp) = running_work(&db, if completed { "EQ-C2-2" } else { "EQ-C2-1" }).await;
        if completed {
            complete(&db, wo_id).await.expect("completed");
        } else {
            pause(&db, wo_id).await;
        }
        let cancelled = with(
            wire(&db, "work_permits", &wp.entity_sync_id).await,
            &[("status", json!("cancelled"))],
        );
        let version = wo_row_version(&db, wo_id).await;
        receive(&db, "work_permits", &wp.entity_sync_id, &cancelled).await;
        assert_eq!(
            review_conflicts(&db, &wp.entity_sync_id).await,
            vec![(
                crate::permit::ended_under_work::codes::ENDED_BEFORE_START.to_string(),
                "new".to_string()
            )],
            "completed: {completed}"
        );
        assert_eq!(
            notice_keys(&db, "permit_ended_under_work").await,
            vec![crate::permit::ended_under_work::codes::NOTICE_BEFORE_START_UNTIMED.to_string()]
        );
        assert_eq!(wo_row_version(&db, wo_id).await, version, "not running: nothing held");
    }
}

/// C2: the same cancellation over running work: during the work, untimed text, and the hold.
#[tokio::test]
async fn an_untimed_cancellation_over_running_work_is_raised_and_holds_it() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-C2-3").await;
    let cancelled = with(
        wire(&db, "work_permits", &wp.entity_sync_id).await,
        &[("status", json!("cancelled"))],
    );
    receive(&db, "work_permits", &wp.entity_sync_id, &cancelled).await;
    assert_eq!(
        notice_keys(&db, "permit_ended_under_work").await,
        vec![crate::permit::ended_under_work::codes::NOTICE_DURING_WORK_UNTIMED.to_string()]
    );
    let body = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT n.body FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE e.category_code = 'permit_ended_under_work' LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("query")
        .expect("notice")
        .try_get_by_index::<String>(0)
        .expect("body");
    assert!(!body.contains('?'), "no empty time shown: {body}");
    assert_eq!(wo_status(&db, wo_id).await, "on_hold");
}

/// A suspension of `target` made and lifted on another PC from `from` to `until`, as received:
/// its sync id and its row. Built from a suspension made here on another permit whose work does not
/// run (so nothing of `target`'s work order is touched).
async fn lifted_suspension(
    db: &DatabaseConnection,
    target: &crate::permit::domain::WorkPermit,
    tag: &str,
    from: &str,
    until: &str,
) -> (String, Payload) {
    let asset = insert_equipment(db, &format!("EQ-{tag}-OTHER")).await;
    let (other_wo, _) = assigned_wo_requiring_permit(db, asset).await;
    let other = advance_to_issued(db, create_from_wo(db, "hot_work", other_wo).await.expect("permit")).await;
    let other = set_status(db, &other, "active").await.expect("active");
    suspend(db, &other, "Vent fort").await;
    let (_, _, suspension) = suspension_as_received(db, &other, None).await;
    db.execute_unprepared("DELETE FROM sync_inbox WHERE entity_type = 'permit_suspensions'")
        .await
        .expect("not waiting");
    let sync = crate::sync::identity::new_entity_sync_id();
    let mut changes = vec![
        ("permit_sync_id", json!(target.entity_sync_id)),
        ("suspended_at", json!(from)),
        ("reinstated_at", json!(until)),
    ];
    if suspension.contains_key("entity_sync_id") {
        changes.push(("entity_sync_id", json!(sync)));
    }
    (sync.clone(), with(suspension, &changes))
}

/// The review conflicts recorded on a received suspension (one per window, G4).
async fn suspension_conflicts(db: &DatabaseConnection, suspension_sync: &str) -> Vec<(String, String)> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT conflict_type, status FROM sync_conflicts WHERE entity_type = 'permit_suspensions' \
         AND entity_sync_id = ? ORDER BY id",
        [suspension_sync.into()],
    ))
    .await
    .expect("conflicts")
    .iter()
    .map(|r| {
        (
            r.try_get_by_index::<String>(0).expect("type"),
            r.try_get_by_index::<String>(1).expect("status"),
        )
    })
    .collect()
}

fn in_minutes(minutes: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::minutes(minutes))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// C3 (S3): a suspension made and lifted on another PC whose window covers the start here: raised
/// for review (on the suspension) and told; the permit is active again, nothing is held. A window
/// before the work: nothing.
#[tokio::test]
async fn a_suspension_lifted_elsewhere_around_a_start_here_is_raised_for_review() {
    for covers in [true, false] {
        let db = setup().await;
        let (wo_id, wp) = running_work(&db, if covers { "EQ-C3-1" } else { "EQ-C3-2" }).await;
        let (from, until) = if covers {
            (ago(10), in_minutes(5))
        } else {
            (ago(120), ago(60))
        };
        let (sync, lifted) = lifted_suspension(&db, &wp, if covers { "C3-1" } else { "C3-2" }, &from, &until).await;
        let outcome = apply(&db, "permit_suspensions", &sync, &lifted).await;
        assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
        let expected = if covers {
            vec![(
                crate::permit::ended_under_work::codes::ENDED_BEFORE_START.to_string(),
                "new".to_string(),
            )]
        } else {
            vec![]
        };
        assert_eq!(suspension_conflicts(&db, &sync).await, expected, "covers: {covers}");
        assert_eq!(
            notice_keys(&db, "permit_ended_under_work").await,
            if covers {
                vec![crate::permit::ended_under_work::codes::NOTICE_SUSPENDED_AROUND_START.to_string()]
            } else {
                vec![]
            }
        );
        crate::permit::ended_hold::sweep(&db).await;
        assert_eq!(wo_status(&db, wo_id).await, "in_progress", "the permit is active again");
        // The same row again: known here now, nothing more.
        apply(&db, "permit_suspensions", &sync, &lifted).await;
        assert_eq!(suspension_conflicts(&db, &sync).await.len(), usize::from(covers));
    }
}

/// G3: work already running here when the window began (started before it, not held since):
/// raised as during the work, with its own notice; nothing held.
#[tokio::test]
async fn a_suspension_lifted_elsewhere_while_the_work_ran_here_is_raised_for_review() {
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-G3-1").await;
    let (sync, lifted) = lifted_suspension(&db, &wp, "G3-1", &in_minutes(1), &in_minutes(5)).await;
    let outcome = apply(&db, "permit_suspensions", &sync, &lifted).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        suspension_conflicts(&db, &sync).await,
        vec![(
            crate::permit::ended_under_work::codes::ENDED_DURING_WORK.to_string(),
            "new".to_string()
        )]
    );
    assert_eq!(
        notice_keys(&db, "permit_ended_under_work").await,
        vec![crate::permit::ended_under_work::codes::NOTICE_SUSPENDED_DURING_WORK.to_string()]
    );
    crate::permit::ended_hold::sweep(&db).await;
    assert_eq!(wo_status(&db, wo_id).await, "in_progress", "the permit is active again");

    // Paused before the window: the work did not run through it.
    let db = setup().await;
    let (wo_id, wp) = running_work(&db, "EQ-G3-2").await;
    pause(&db, wo_id).await;
    let (sync, lifted) = lifted_suspension(&db, &wp, "G3-2", &in_minutes(1), &in_minutes(5)).await;
    apply(&db, "permit_suspensions", &sync, &lifted).await;
    assert!(suspension_conflicts(&db, &sync).await.is_empty());
}

/// G4: each lifted window has its own review: a second window, received after the first was
/// dismissed, is a new open conflict.
#[tokio::test]
async fn each_lifted_suspension_window_has_its_own_review() {
    let db = setup().await;
    let (_, wp) = running_work(&db, "EQ-G4-1").await;
    let (first, row) = lifted_suspension(&db, &wp, "G4-1", &ago(10), &in_minutes(5)).await;
    apply(&db, "permit_suspensions", &first, &row).await;
    let first_id = scalar_i64(
        &db,
        &format!("SELECT id FROM sync_conflicts WHERE entity_sync_id = '{first}'"),
    )
    .await;
    resolve(&db, first_id, "dismiss").await.expect("dismissed");

    let (second, row) = lifted_suspension(&db, &wp, "G4-2", &ago(9), &in_minutes(6)).await;
    apply(&db, "permit_suspensions", &second, &row).await;
    assert_eq!(suspension_conflicts(&db, &first).await[0].1, "dismissed");
    assert_eq!(
        suspension_conflicts(&db, &second).await,
        vec![(
            crate::permit::ended_under_work::codes::ENDED_BEFORE_START.to_string(),
            "new".to_string()
        )]
    );
}

/// D4 condition 2 on arrival: a lock removal verified on another PC ends the downtime of a work
/// order completed here when it was the last fitted lock, at the last verification's time.
#[tokio::test]
async fn a_received_last_removal_ends_the_downtime_of_a_completed_work_order() {
    let db = setup().await;
    let (wo_id, locks) = running_loto_work(&db, "EQ-D4C-5", &["MCC-D4C-5"]).await;
    let segment = open_downtime(&db, wo_id).await;
    complete(&db, wo_id).await.expect("completed");
    let verified_at = verify_removal(&db, &locks[0]).await;
    // As received: the verification is here, the downtime not ended by it yet.
    db.execute_unprepared(&format!(
        "UPDATE work_order_downtime_segments SET ended_at = NULL WHERE id = {segment}"
    ))
    .await
    .expect("as received");
    assert_eq!(crate::permit::ended_hold::sweep(&db).await.downtimes_ended, 1);
    assert_eq!(segment_end(&db, segment).await.as_deref(), Some(verified_at.as_str()));
    // Again: nothing more.
    assert_eq!(crate::permit::ended_hold::sweep(&db).await.downtimes_ended, 0);
    assert_eq!(
        scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM audit_events WHERE action_code = '{}' AND target_id = '{wo_id}'",
                crate::wo::execution::AUDIT_DOWNTIME_ENDED_BY_LOCK_REMOVAL
            )
        )
        .await,
        2,
        "the local verification's, then the one after sync"
    );
}

// ── Two PCs holding the same work order for the same permit end (C1) ─────────────────────

async fn device_with_verifier() -> DatabaseConnection {
    let db = crate::sync::test_support::started_device_db().await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, identity_mode, is_active, \
         is_admin, force_password_change, created_at, updated_at) VALUES (?, 'user-sync-ptw-verifier', \
         'ptw_verifier', 'PTW Verifier', 'local', 1, 0, 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
         strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [VERIFIER.into()],
    ))
    .await
    .expect("verifier");
    db
}

async fn id_of(db: &DatabaseConnection, table: &str, sync_id: &str) -> i64 {
    scalar_i64(
        db,
        &format!("SELECT id FROM {table} WHERE entity_sync_id = '{sync_id}'"),
    )
    .await
}

async fn open_conflict_kinds(db: &DatabaseConnection) -> Vec<String> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT conflict_type FROM sync_conflicts WHERE status IN ('new', 'triaged', 'escalated') ORDER BY id"
            .to_string(),
    ))
    .await
    .expect("conflicts")
    .iter()
    .map(|r| r.try_get_by_index::<String>(0).expect("type"))
    .collect()
}

/// PC3 closes the permit while PC1 starts the work (PC2 follows PC1). PC1 and PC2 both receive the
/// closure and both hold the work order; their holds cross on the server. The first one kept, the
/// other PC's own hold is settled without a person: both converge on one hold (one delay segment),
/// with nothing left to send and no conflict but the permit's review; no ping-pong afterwards.
#[tokio::test]
async fn two_pcs_holding_the_same_work_order_for_the_same_permit_end_converge() {
    use crate::sync::test_support::TestServer;
    let pc1 = device_with_verifier().await;
    let pc2 = crate::sync::test_support::started_device_db().await;
    let pc3 = crate::sync::test_support::started_device_db().await;
    let mut server = TestServer::new();

    let asset = insert_equipment(&pc1, "EQ-C1-X").await;
    let (wo1, rv) = assigned_wo_requiring_permit(&pc1, asset).await;
    let wp1 = advance_to_issued(&pc1, create_from_wo(&pc1, "hot_work", wo1).await.expect("permit")).await;
    let wp1 = set_status(&pc1, &wp1, "active").await.expect("active");
    let wo_sync_id = wo_sync(&pc1, wo1).await;
    for (name, db) in [("PC1", &pc1), ("PC2", &pc2), ("PC3", &pc3)] {
        server.exchange(name, db).await;
    }

    // PC3 closes the permit (the work is not running there); PC1 then starts the work.
    let wp3 = queries::get_work_permit(&pc3, id_of(&pc3, "work_permits", &wp1.entity_sync_id).await)
        .await
        .expect("read")
        .expect("permit on PC3");
    set_status(&pc3, &wp3, "closed").await.expect("closed on PC3");
    start(&pc1, wo1, rv).await.expect("started on PC1");
    server.exchange("PC1", &pc1).await;
    server.exchange("PC2", &pc2).await;
    let wo2 = id_of(&pc2, "work_orders", &wo_sync_id).await;
    assert_eq!(wo_status(&pc2, wo2).await, "in_progress");
    server.exchange("PC3", &pc3).await;

    // Both receive the closure and hold the work order, PC1 some minutes before PC2 (its own
    // clock: the two holds differ by their times).
    server.exchange("PC1", &pc1).await;
    for sql in [
        format!("UPDATE work_orders SET updated_at = '{}' WHERE id = {wo1}", ago(7)),
        format!(
            "UPDATE work_order_delay_segments SET started_at = '{}' WHERE work_order_id = {wo1} AND ended_at IS NULL",
            ago(7)
        ),
    ] {
        pc1.execute_unprepared(&sql).await.expect("PC1's clock");
    }
    server.exchange("PC2", &pc2).await;
    assert_eq!(wo_status(&pc1, wo1).await, "on_hold");
    assert_eq!(wo_status(&pc2, wo2).await, "on_hold");

    // The holds cross: PC1's reaches the server first, PC2's is settled.
    for _ in 0..2 {
        server.exchange("PC1", &pc1).await;
        server.exchange("PC2", &pc2).await;
    }
    for (name, db, wo_id) in [("PC1", &pc1, wo1), ("PC2", &pc2, wo2)] {
        assert_eq!(wo_status(db, wo_id).await, "on_hold", "{name}");
        assert_eq!(
            scalar_i64(
                db,
                &format!(
                    "SELECT COUNT(*) FROM work_order_delay_segments WHERE work_order_id = {wo_id} AND ended_at IS NULL"
                )
            )
            .await,
            1,
            "{name}: one hold"
        );
        // Closed and started within the same second here: before the start or during the work.
        let kinds = open_conflict_kinds(db).await;
        assert!(
            kinds.len() == 1 && kinds[0].starts_with("PERMIT_ENDED_"),
            "{name}: only the permit's review: {kinds:?}"
        );
        assert_eq!(
            scalar_i64(
                db,
                "SELECT COUNT(*) FROM sync_outbox WHERE status IN ('pending', 'in_flight', 'conflict')"
            )
            .await,
            0,
            "{name}: nothing left to send"
        );
    }
    assert_eq!(
        open_hold(&pc1, wo1).await.2,
        open_hold(&pc2, wo2).await.2,
        "the same segment"
    );
    // PC2's own hold crossed PC1's on the server and was closed without a person.
    for entity in ["work_orders", "work_order_delay_segments"] {
        assert_eq!(
            scalar_i64(
                &pc2,
                &format!(
                    "SELECT COUNT(*) FROM sync_outbox WHERE entity_type = '{entity}' AND status = 'superseded' \
                     AND rejection_code = '{}'",
                    crate::permit::ended_hold::CROSSED_HOLD_KEPT
                )
            )
            .await,
            1,
            "{entity}"
        );
    }
    // No ping-pong.
    for (name, db) in [("PC1", &pc1), ("PC2", &pc2)] {
        let report = server.exchange(name, db).await;
        assert_eq!((report.sent, report.received), (0, 0), "{name}");
    }
}

/// Puts `payload` in the inbox (this PC's unsent changes kept) and applies it.
async fn receive_keeping_own(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) VALUES (?, 'c2', ?, ?, 'upsert', 9500, ?, ?, 'pending')",
        [
            format!("b-own-{sync_id}").into(),
            entity_type.into(),
            sync_id.into(),
            serde_json::to_string(payload).expect("json").into(),
            format!("h-own-{sync_id}").into(),
        ],
    ))
    .await
    .expect("inbox");
    crate::sync::materialize::materialize_inbox_batch(db, None)
        .await
        .expect("materialize");
    crate::sync::row_codec::clear_ref_cache();
}

/// The other PC's same hold received while this PC's own is still unsent: this PC's change is
/// closed and the received version applied, no conflict for a person; a received work order that
/// differs by more than its time is left to a person.
#[tokio::test]
async fn the_same_hold_received_while_this_pcs_own_is_unsent_is_settled_without_a_person() {
    for same in [true, false] {
        let db = setup().await;
        let (wo_id, wp) = running_work(&db, if same { "EQ-C1-4" } else { "EQ-C1-5" }).await;
        crate::sync::capture::install_capture(&db).await.expect("capture");
        let closed = with(
            wire(&db, "work_permits", &wp.entity_sync_id).await,
            &[("status", json!("closed")), ("closed_at", json!(ago(60)))],
        );
        receive(&db, "work_permits", &wp.entity_sync_id, &closed).await;
        assert_eq!(wo_status(&db, wo_id).await, "on_hold");
        crate::sync::capture::drain_capture_journal(&db).await.expect("drain");

        let wo_sync_id = wo_sync(&db, wo_id).await;
        let (_, _, segment_sync) = open_hold(&db, wo_id).await;
        let mut theirs = with(
            wire(&db, "work_orders", &wo_sync_id).await,
            &[("updated_at", json!(ago(7)))],
        );
        if !same {
            theirs = with(theirs, &[("title", json!("Titre changé sur l'autre poste"))]);
        }
        let their_segment = with(
            wire(&db, "work_order_delay_segments", &segment_sync).await,
            &[("started_at", json!(ago(7)))],
        );
        receive_keeping_own(&db, "work_order_delay_segments", &segment_sync, &their_segment).await;
        receive_keeping_own(&db, "work_orders", &wo_sync_id, &theirs).await;

        let concurrent = scalar_i64(
            &db,
            "SELECT COUNT(*) FROM sync_conflicts WHERE conflict_type = 'CONCURRENT_EDIT' AND status = 'new'",
        )
        .await;
        let segment_started = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT started_at FROM work_order_delay_segments WHERE entity_sync_id = ?",
                [segment_sync.clone().into()],
            ))
            .await
            .expect("query")
            .expect("segment")
            .try_get_by_index::<String>(0)
            .expect("started_at");
        assert_eq!(
            Some(segment_started.as_str()),
            their_segment.get("started_at").and_then(Value::as_str),
            "the other PC's segment is kept"
        );
        if same {
            assert_eq!(concurrent, 0, "settled without a person");
            assert_eq!(
                scalar_i64(
                    &db,
                    "SELECT COUNT(*) FROM sync_outbox WHERE status IN ('pending', 'in_flight', 'conflict') \
                     AND entity_type IN ('work_orders', 'work_order_delay_segments')"
                )
                .await,
                0
            );
        } else {
            assert_eq!(concurrent, 1, "a person decides on the other change");
        }
    }
}

// ── Review round 2: F1, F2, F5, G1, G2 ────────────────────────────────────────────────

/// G1: permit P1 closed while the work was on hold, the work resumed under P2, then P1 handed back
/// on another PC: no review (P1 ended before), and no hold (never on a permit's status alone).
#[tokio::test]
async fn a_hand_back_received_for_a_permit_closed_under_held_work_holds_nothing() {
    let db = setup().await;
    let (wo_id, p1) = running_work(&db, "EQ-G1-1").await;
    pause(&db, wo_id).await;
    let p1 = set_status(&db, &reread(&db, &p1).await, "closed")
        .await
        .expect("closed while the work is on hold");
    let p2 = advance_to_issued(
        &db,
        create_from_wo(&db, "hot_work", wo_id).await.expect("second permit"),
    )
    .await;
    set_status(&db, &p2, "active").await.expect("P2 active");
    resume(&db, wo_id).await.expect("resumed under P2");

    // P1's hand-back as the other PC sends it (made here to get its row, then taken back).
    add_handover_role(&db, "Maintenance").await;
    add_handover_role(&db, "Operations").await;
    queries::append_permit_handover_log(
        &db,
        crate::permit::domain::PermitHandoverLogInput {
            permit_id: p1.id,
            handed_from_role: "Maintenance".into(),
            handed_to_role: "Operations".into(),
            confirmation_note: "Zone propre".into(),
            received_by_id: None,
        },
        APPLIER,
    )
    .await
    .expect("handover log");
    set_status(&db, &p1, "handed_back").await.expect("handed back");
    let handed_back = wire(&db, "work_permits", &p1.entity_sync_id).await;
    db.execute_unprepared(&format!(
        "UPDATE work_permits SET status = 'closed', handed_back_at = NULL WHERE id = {}",
        p1.id
    ))
    .await
    .expect("closed here");

    receive(&db, "work_permits", &p1.entity_sync_id, &handed_back).await;
    assert_eq!(reread(&db, &p1).await.status, "handed_back");
    assert!(review_conflicts(&db, &p1.entity_sync_id).await.is_empty());
    assert_eq!(wo_status(&db, wo_id).await, "in_progress", "no false hold");
}

/// G2: the pass that applied a permit end committed and the PC stopped before its sweep: the next
/// pass (even with nothing received) or the start-up catch-up holds the work (stored state).
#[tokio::test]
async fn a_hold_not_written_before_a_stop_is_caught_up() {
    for at_start_up in [false, true] {
        let db = setup().await;
        let (wo_id, wp) = running_work(&db, if at_start_up { "EQ-G2-1" } else { "EQ-G2-2" }).await;
        let closed = with(
            wire(&db, "work_permits", &wp.entity_sync_id).await,
            &[("status", json!("closed")), ("closed_at", json!(ago(60)))],
        );
        // Applied and committed (row, review, notice); the sweep never ran.
        assert!(matches!(
            apply(&db, "work_permits", &wp.entity_sync_id, &closed).await,
            ApplyOutcome::Applied
        ));
        assert_eq!(wo_status(&db, wo_id).await, "in_progress");
        if at_start_up {
            // What the start-up runs (`startup`, phase 4a permits).
            assert_eq!(crate::permit::ended_hold::sweep(&db).await.work_orders_held, 1);
        } else {
            crate::sync::materialize::materialize_inbox_batch(&db, None)
                .await
                .expect("an empty pass");
        }
        assert_eq!(wo_status(&db, wo_id).await, "on_hold", "start-up: {at_start_up}");
    }
}

/// F1: while a lock is fitted under the work order's permits, its downtime is not ended by hand
/// (work running), is not an integrity defect, and the integrity repair refuses to end it.
#[tokio::test]
async fn downtime_is_not_ended_by_hand_or_by_the_repair_while_a_lock_is_fitted() {
    let db = setup().await;
    let (wo_id, locks) = running_loto_work(&db, "EQ-F1-1", &["MCC-F1-1"]).await;
    let segment = open_downtime(&db, wo_id).await;
    let msgs = messages(
        crate::wo::delay::close_downtime_segment(&db, segment, None)
            .await
            .expect_err("work running, lock fitted"),
    );
    assert!(has(&msgs, "GATE_DOWNTIME_HELD_BY_LOCKS"), "{msgs:?}");

    let unclosed = format!(
        "SELECT COUNT(*) FROM data_integrity_findings WHERE finding_code = 'WO_DOWNTIME_UNCLOSED' \
         AND record_id = {segment} AND status = 'open'"
    );
    crate::data_integrity::run_data_integrity_detectors(&db)
        .await
        .expect("detectors");
    assert_eq!(scalar_i64(&db, &unclosed).await, 0, "not a defect while isolated");

    // A finding raised while no lock was fitted, the lock fitted again since: the repair refuses.
    let lock = locks[0].id;
    for sql in [
        format!("UPDATE permit_isolations SET removal_verified_at = '2026-01-01T00:00:00Z' WHERE id = {lock}"),
        String::new(),
        format!("UPDATE permit_isolations SET removal_verified_at = NULL WHERE id = {lock}"),
    ] {
        if sql.is_empty() {
            crate::data_integrity::run_data_integrity_detectors(&db)
                .await
                .expect("detectors");
        } else {
            db.execute_unprepared(&sql).await.expect("lock state");
        }
    }
    assert_eq!(scalar_i64(&db, &unclosed).await, 1);
    let finding = scalar_i64(
        &db,
        &format!(
            "SELECT id FROM data_integrity_findings WHERE finding_code = 'WO_DOWNTIME_UNCLOSED' \
             AND record_id = {segment}"
        ),
    )
    .await;
    let version = scalar_i64(
        &db,
        &format!("SELECT row_version FROM data_integrity_findings WHERE id = {finding}"),
    )
    .await;
    let msgs = messages(
        crate::data_integrity::apply_data_integrity_repair(
            &db,
            crate::data_integrity::ApplyDataIntegrityRepairInput {
                finding_id: finding,
                expected_row_version: version,
                repair_kind: "close_downtime_at_start".into(),
            },
            admin_id(&db).await,
        )
        .await
        .expect_err("repair refused"),
    );
    assert!(has(&msgs, "GATE_DOWNTIME_HELD_BY_LOCKS"), "{msgs:?}");
    assert_eq!(segment_end(&db, segment).await, None);
}

/// F2: a downtime end received while a lock is fitted here is a conflict; it waits when the
/// removal verification of that lock is received with it and still to apply.
#[tokio::test]
async fn a_downtime_end_received_while_a_lock_is_fitted_here_is_not_applied() {
    let db = setup().await;
    let (wo_id, locks) = running_loto_work(&db, "EQ-F2-1", &["MCC-F2-1"]).await;
    // Rows get their sync identity as on a device (capture).
    crate::sync::capture::install_capture(&db).await.expect("capture");
    let segment = open_downtime(&db, wo_id).await;
    let segment_sync = text_of(
        &db,
        &format!("SELECT entity_sync_id FROM work_order_downtime_segments WHERE id = {segment}"),
    )
    .await;
    let ended = with(
        wire(&db, "work_order_downtime_segments", &segment_sync).await,
        &[("ended_at", json!(now()))],
    );
    let outcome = apply(&db, "work_order_downtime_segments", &segment_sync, &ended).await;
    assert_eq!(
        conflict_code(&outcome),
        Some(crate::permit::ended_hold::DOWNTIME_END_LOCKS_FITTED),
        "{outcome:?}"
    );
    assert_eq!(segment_end(&db, segment).await, None);

    // The removal verification of that lock received with it, still to apply: the end waits.
    let verified = with(
        wire(&db, "permit_isolations", &locks[0].entity_sync_id).await,
        &[("removal_verified_at", json!(now()))],
    );
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) VALUES ('b-f2', 'c-f2', 'permit_isolations', ?, \
         'upsert', 9000, ?, 'h-f2', 'deferred')",
        [
            locks[0].entity_sync_id.clone().into(),
            serde_json::to_string(&verified).expect("json").into(),
        ],
    ))
    .await
    .expect("inbox");
    let outcome = apply(&db, "work_order_downtime_segments", &segment_sync, &ended).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(segment_end(&db, segment).await, None);
}

/// F5: two permits on the work order; the downtime ends only when the last lock of BOTH is verified
/// off (here the last one is under the first, cancelled, permit).
#[tokio::test]
async fn the_downtime_waits_for_the_last_lock_of_every_permit_of_the_work_order() {
    let db = setup().await;
    let asset = insert_equipment(&db, "EQ-F5-1").await;
    let (wo_id, rv) = assigned_wo_requiring_permit(&db, asset).await;
    let p1 = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("P1")).await;
    let lock1 = apply_and_verify(&db, &add_isolation(&db, p1.id, "MCC-F5-1").await).await;
    // P1 cancelled with its lock still fitted (the lock stays on the plant).
    set_status(&db, &p1, "cancelled").await.expect("P1 cancelled");
    let p2 = advance_to_issued(&db, create_from_wo(&db, "loto", wo_id).await.expect("P2")).await;
    let lock2 = apply_and_verify(&db, &add_isolation(&db, p2.id, "MCC-F5-2").await).await;
    set_status(&db, &p2, "active").await.expect("P2 active");
    start(&db, wo_id, rv).await.expect("started");
    let segment = open_downtime(&db, wo_id).await;
    complete(&db, wo_id).await.expect("completed with locks fitted");

    verify_removal(&db, &lock2).await;
    assert_eq!(segment_end(&db, segment).await, None, "P1's lock is still fitted");
    assert!(deisolation_pending(&db, wo_id).await);
    let last = verify_removal(&db, &lock1).await;
    assert_eq!(segment_end(&db, segment).await.as_deref(), Some(last.as_str()));
    assert!(!deisolation_pending(&db, wo_id).await);
}
