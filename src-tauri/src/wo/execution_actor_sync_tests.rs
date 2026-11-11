//! L5-08c: work-order execution changes received by synchronisation, re-checked against their
//! author with the rights held here now (`wo::execution_actor_sync`), through the real apply path
//! (`sync::row_codec::apply_row`), on the fixtures of the close-out re-checks
//! (`wo::closeout_sync_tests`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Map, Value};

use super::closeout_gates_tests::add_task;
use super::closeout_tests::tests::{admin_id, create_wo_in_progress, setup};
use crate::auth::scope_tests::{assign, role, user};
use crate::rbac::permissions::OT_EDIT;
use crate::sync::materialize::ApplyOutcome;
use crate::wo::execution_actor_sync::{codes, AUTHOR_NOT_RECEIVED};

type Payload = Map<String, Value>;

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn text(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect(sql)
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten())
}

async fn count(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect(sql)
        .and_then(|r| r.try_get_by_index::<i64>(0).ok())
        .unwrap_or(0)
}

async fn wire(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    crate::permit::sync_check::local_wire(db, entity_type, sync_id)
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

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
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

fn code(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

async fn wo_status(db: &DatabaseConnection, wo_id: i64) -> String {
    text(
        db,
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("status")
}

async fn wo_sync(db: &DatabaseConnection, wo_id: i64) -> String {
    text(
        db,
        "SELECT entity_sync_id FROM work_orders WHERE id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("wo sync id")
}

/// A colleague holding `ot.edit` plant-wide, who is not named on the work order.
async fn colleague(db: &DatabaseConnection) -> i64 {
    let id = user(db, "colleague").await;
    let editor = role(db, "l5_08c_editor", &[OT_EDIT]).await;
    assign(db, id, editor, "tenant", None).await;
    i64::from(id)
}

/// The work order as another PC sends it once put on hold there by `actor` (`None`: the history row
/// names nobody), with the reason `reason`; its status-history row waits in the inbox with it.
async fn received_hold(db: &DatabaseConnection, wo_id: i64, n: u32, actor: Option<i64>, reason: &str) -> Payload {
    let tid = format!("l5-08c-hold-{wo_id}-{n}");
    exec(
        db,
        "INSERT INTO wo_state_transition_log (entity_sync_id, wo_id, from_status, to_status, action, actor_id, \
         reason_code, acted_at) VALUES (?, ?, 'in_progress', 'on_hold', 'hold', ?, ?, ?)",
        vec![
            tid.clone().into(),
            wo_id.into(),
            actor.into(),
            reason.into(),
            now().into(),
        ],
    )
    .await;
    let transition = wire(db, "wo_state_transition_log", &tid).await;
    exec(
        db,
        "DELETE FROM wo_state_transition_log WHERE entity_sync_id = ?",
        vec![tid.clone().into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) \
         VALUES ('b-l5-08c', 'c1', 'wo_state_transition_log', ?, 'upsert', 1, ?, ?, 'pending')",
        vec![
            tid.clone().into(),
            serde_json::to_string(&transition).expect("json").into(),
            format!("h-{tid}").into(),
        ],
    )
    .await;
    received_status(db, wo_id, "on_hold").await
}

/// The work order as another PC sends it in `status`, its other columns as held here.
async fn received_status(db: &DatabaseConnection, wo_id: i64, status: &str) -> Payload {
    let sync = wo_sync(db, wo_id).await;
    let status_sync = text(
        db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = ?",
        vec![status.into()],
    )
    .await
    .expect("status sync id");
    with(
        wire(db, "work_orders", &sync).await,
        &[("status_sync_id", json!(status_sync)), ("updated_at", json!(now()))],
    )
}

/// Only the people who carry the work order out (L5-08): a colleague holding `ot.edit` at the site
/// but neither its assignee, a member of its team, its planner nor an approver is refused; named
/// its assignee, the same hold is applied.
#[tokio::test]
async fn a_hold_received_from_someone_who_does_not_carry_out_the_work_order_is_refused() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let sync = wo_sync(&db, wo_id).await;
    let colleague = colleague(&db).await;

    let payload = received_hold(&db, wo_id, 1, Some(colleague), "waiting_parts").await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert_eq!(code(&outcome), Some(codes::UNAUTHORISED), "{outcome:?}");
    assert_eq!(wo_status(&db, wo_id).await, "in_progress", "nothing written");

    exec(
        &db,
        "UPDATE work_orders SET primary_responsible_id = ? WHERE id = ?",
        vec![colleague.into(), wo_id.into()],
    )
    .await;
    let payload = received_hold(&db, wo_id, 2, Some(colleague), "waiting_parts").await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(wo_status(&db, wo_id).await, "on_hold");
}

/// The rights judged are the ones held here now: the assignee without `ot.edit` is refused, and so
/// is one whose account is disabled here since.
#[tokio::test]
async fn the_rights_are_judged_as_held_now() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let sync = wo_sync(&db, wo_id).await;
    let assignee = i64::from(user(&db, "assignee_no_right").await);
    exec(
        &db,
        "UPDATE work_orders SET primary_responsible_id = ? WHERE id = ?",
        vec![assignee.into(), wo_id.into()],
    )
    .await;
    let payload = received_hold(&db, wo_id, 1, Some(assignee), "waiting_parts").await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert_eq!(
        code(&outcome),
        Some(codes::UNAUTHORISED),
        "right withdrawn: {outcome:?}"
    );

    let editor = role(&db, "l5_08c_assignee_editor", &[OT_EDIT]).await;
    assign(&db, i32::try_from(assignee).expect("id"), editor, "tenant", None).await;
    exec(
        &db,
        "UPDATE user_accounts SET is_active = 0 WHERE id = ?",
        vec![assignee.into()],
    )
    .await;
    let payload = received_hold(&db, wo_id, 2, Some(assignee), "waiting_parts").await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert_eq!(
        code(&outcome),
        Some(codes::UNAUTHORISED),
        "account disabled: {outcome:?}"
    );
    assert_eq!(wo_status(&db, wo_id).await, "in_progress");

    exec(
        &db,
        "UPDATE user_accounts SET is_active = 1 WHERE id = ?",
        vec![assignee.into()],
    )
    .await;
    let payload = received_hold(&db, wo_id, 3, Some(assignee), "waiting_parts").await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A history row naming nobody for a hold a person makes is refused (fail closed); the hold the
/// application makes for an ended permit names nobody and is applied.
#[tokio::test]
async fn a_change_whose_history_names_nobody_is_refused_unless_the_permit_made_it() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let sync = wo_sync(&db, wo_id).await;
    let payload = received_hold(&db, wo_id, 1, None, "waiting_parts").await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert_eq!(code(&outcome), Some(codes::AUTHOR_UNKNOWN), "{outcome:?}");

    exec(
        &db,
        "DELETE FROM sync_inbox WHERE entity_type = 'wo_state_transition_log'",
        vec![],
    )
    .await;
    let payload = received_hold(&db, wo_id, 2, None, crate::wo::execution::PERMIT_ENDED_DELAY_REASON).await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// A status change received without its history row (who made it is not here) is applied and
/// recorded in the audit trail, never silently.
#[tokio::test]
async fn a_change_without_its_history_row_is_applied_and_recorded() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let sync = wo_sync(&db, wo_id).await;
    let payload = received_status(&db, wo_id, "on_hold").await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = ? AND target_id = ?",
            vec![AUTHOR_NOT_RECEIVED.into(), wo_id.to_string().into()],
        )
        .await,
        1
    );
}

/// A task result received from a colleague who does not carry out the work order is refused; the
/// same result by the assignee is applied.
#[tokio::test]
async fn a_task_result_is_judged_against_its_author() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let colleague = colleague(&db).await;
    let task = add_task(&db, wo_id, "Check the guard", true).await;
    let tsync = text(
        &db,
        "SELECT entity_sync_id FROM work_order_tasks WHERE id = ?",
        vec![task.id.into()],
    )
    .await
    .expect("task sync id");
    let result_by = |by: String, base: Payload| {
        with(
            base,
            &[
                ("is_completed", json!(1)),
                ("result_code", json!("ok")),
                ("completed_at", json!(now())),
                ("completed_by_sync_id", json!(by)),
            ],
        )
    };
    let colleague_sync = text(
        &db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![colleague.into()],
    )
    .await
    .expect("colleague sync id");
    let base = wire(&db, "work_order_tasks", &tsync).await;
    let outcome = apply(
        &db,
        "work_order_tasks",
        &tsync,
        &result_by(colleague_sync, base.clone()),
    )
    .await;
    assert_eq!(code(&outcome), Some(codes::UNAUTHORISED), "{outcome:?}");

    let admin = admin_id(&db).await;
    let admin_sync = text(
        &db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![admin.into()],
    )
    .await
    .expect("admin sync id");
    let outcome = apply(&db, "work_order_tasks", &tsync, &result_by(admin_sync, base)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}
