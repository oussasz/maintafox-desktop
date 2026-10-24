//! A work order and its permits end in the right order (AU-02, AU-03, AU-101; owner decisions D4,
//! D5, D9): a work order is neither closed nor cancelled while one of its permits is open or a lock
//! is still fitted under them; a permit is neither closed nor cancelled while its work runs, and its
//! suspension puts the running work on hold; a permit ended on another PC before (or while) the work
//! ran here is applied and raised for review. Local commands and the arrival of received rows.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Map, Value};

use super::tests::{
    action, add_isolation, admin_id, advance_to_issued, apply_and_verify, assigned_wo_requiring_permit, create_from_wo,
    insert_equipment, scalar_i64, set_status, setup, start, take_off, VERIFIER,
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
