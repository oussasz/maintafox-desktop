//! Work-order completion and close re-checked on received rows (`wo::closeout_sync`), through the
//! real apply path (`sync::row_codec::apply_row`, and the inbox pass for rows that arrive together).
//! Two seeded databases copied from one template share every reference row (statuses, users,
//! types, the test equipment), so one plays the PC that sends and the other the PC that receives.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Map, Value};

use super::closeout_gates_tests::{add_task, set_wo_status};
use super::closeout_tests::tests::{
    admin_id, advance_to_completed_verified, create_wo_in_progress, seed_rams_for_complete, seed_test_equipment, setup,
};
use crate::sync::materialize::ApplyOutcome;
use crate::wo::closeout_sync::codes;

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

/// Applies a received row as the inbox pass does: kept only when applied.
async fn apply(db: &DatabaseConnection, entity_type: &str, sync_id: &str, payload: &Payload) -> ApplyOutcome {
    apply_op(db, entity_type, sync_id, "upsert", payload).await
}

async fn apply_op(
    db: &DatabaseConnection,
    entity_type: &str,
    sync_id: &str,
    operation: &str,
    payload: &Payload,
) -> ApplyOutcome {
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    let tx = db.begin().await.expect("begin");
    let outcome = crate::sync::row_codec::apply_row(
        &tx,
        &layout,
        sync_id,
        operation,
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

async fn delete(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> ApplyOutcome {
    let layout = crate::sync::row_layout::layout(db, entity_type).await.expect("layout");
    let payload = crate::sync::row_codec::delete_payload(&layout, None);
    apply_op(db, entity_type, sync_id, "delete", &payload).await
}

fn code(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

/// A received row waiting in the inbox (`batch`: the server batch, `snapshot-…` for a snapshot).
async fn inbox(
    db: &DatabaseConnection,
    batch: &str,
    entity_type: &str,
    sync_id: &str,
    version: i64,
    payload: &Payload,
) {
    exec(
        db,
        "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
         row_version, payload_json, payload_hash, apply_status) VALUES (?, 'c1', ?, ?, 'upsert', ?, ?, ?, 'pending')",
        vec![
            batch.into(),
            entity_type.into(),
            sync_id.into(),
            version.into(),
            serde_json::to_string(payload).expect("json").into(),
            format!("h-{sync_id}-{version}").into(),
        ],
    )
    .await;
}

async fn materialize(db: &DatabaseConnection) -> crate::sync::materialize::MaterializeInboxResult {
    let result = crate::sync::materialize::materialize_inbox(db, 200, true)
        .await
        .expect("materialize");
    crate::sync::row_codec::clear_ref_cache();
    result
}

async fn inbox_status(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Option<String> {
    text(
        db,
        "SELECT apply_status FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ? ORDER BY id DESC LIMIT 1",
        vec![entity_type.into(), sync_id.into()],
    )
    .await
}

async fn conflict_codes(db: &DatabaseConnection) -> Vec<String> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT conflict_type FROM sync_conflicts ORDER BY id".to_string(),
    ))
    .await
    .expect("conflicts")
    .iter()
    .filter_map(|r| r.try_get_by_index::<String>(0).ok())
    .collect()
}

async fn status_sync(db: &DatabaseConnection, status: &str) -> String {
    text(
        db,
        "SELECT entity_sync_id FROM work_order_statuses WHERE code = ?",
        vec![status.into()],
    )
    .await
    .expect("status sync id")
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

async fn wo_status(db: &DatabaseConnection, wo_id: i64) -> String {
    text(
        db,
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("status")
}

async fn task_sync(db: &DatabaseConnection, task_id: i64) -> String {
    text(
        db,
        "SELECT entity_sync_id FROM work_order_tasks WHERE id = ?",
        vec![task_id.into()],
    )
    .await
    .expect("task sync id")
}

async fn admin_sync(db: &DatabaseConnection) -> String {
    let id = admin_id(db).await;
    text(
        db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![id.into()],
    )
    .await
    .expect("admin sync id")
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A WO in progress with every completion gate met but its tasks: labour closed, no parts, RAMS.
async fn wo_ready_to_complete(db: &DatabaseConnection) -> (i64, String) {
    let actor = admin_id(db).await;
    let (wo_id, _) = create_wo_in_progress(db).await;
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
    seed_rams_for_complete(db, wo_id).await;
    let sync = wo_sync(db, wo_id).await;
    (wo_id, sync)
}

/// The WO as another PC sends it once completed there.
async fn received_completed(db: &DatabaseConnection, sync: &str) -> Payload {
    let completed = status_sync(db, "completed").await;
    with(
        wire(db, "work_orders", sync).await,
        &[
            ("status_sync_id", json!(completed)),
            ("mechanically_completed_at", json!(now())),
            ("updated_at", json!(now())),
        ],
    )
}

/// A task as another PC sends it once recorded there.
async fn received_task_result(db: &DatabaseConnection, sync: &str, result: &str) -> Payload {
    with(
        wire(db, "work_order_tasks", sync).await,
        &[
            ("is_completed", json!(1)),
            ("result_code", json!(result)),
            ("completed_at", json!(now())),
            ("completed_by_sync_id", json!(admin_sync(db).await)),
        ],
    )
}

#[tokio::test]
async fn a_completion_received_with_an_open_mandatory_task_is_a_conflict() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let task = add_task(&db, wo_id, "Lock out and try", true).await;
    let payload = received_completed(&db, &sync).await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert_eq!(code(&outcome), Some(codes::COMPLETION_GATE), "{outcome:?}");
    assert_eq!(wo_status(&db, wo_id).await, "in_progress", "nothing written");

    // Same arrival once the task is closed OK here: applied.
    tasks_ok(&db, task.id).await;
    let outcome = apply(&db, "work_orders", &sync, &payload).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(wo_status(&db, wo_id).await, "completed");
}

async fn tasks_ok(db: &DatabaseConnection, task_id: i64) {
    let actor = admin_id(db).await;
    crate::wo::tasks::complete_task(db, task_id, actor, "ok".into(), None)
        .await
        .expect("task ok");
}

#[tokio::test]
async fn a_completion_received_with_its_waiting_task_completions_is_applied_together() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let task = add_task(&db, wo_id, "Check the guard", true).await;
    let tsync = task_sync(&db, task.id).await;
    let task_row = received_task_result(&db, &tsync, "ok").await;
    let wo_row = received_completed(&db, &sync).await;
    // The task arrives first in the inbox; the work order is applied first (parents first).
    inbox(&db, "b-1", "work_order_tasks", &tsync, 50, &task_row).await;
    inbox(&db, "b-1", "work_orders", &sync, 50, &wo_row).await;
    let result = materialize(&db).await;
    assert_eq!(result.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(wo_status(&db, wo_id).await, "completed");
    assert_eq!(
        inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
        Some("applied")
    );
}

#[tokio::test]
async fn a_close_whose_waiting_verification_is_refused_applies_nothing() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let actor = admin_id(&db).await;
    // Completed here; the close arrives with a verification by the WO's own responsible (admin).
    let wo = crate::wo::execution::complete_wo_mechanically(
        &db,
        crate::wo::execution::WoMechCompleteInput {
            wo_id,
            actor_id: actor,
            expected_row_version: crate::wo::queries::get_work_order(&db, wo_id)
                .await
                .expect("wo")
                .expect("wo")
                .row_version,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .expect("complete");
    assert_eq!(wo.status_code.as_deref(), Some("completed"));
    let closed = status_sync(&db, "closed").await;
    let wo_row = with(
        wire(&db, "work_orders", &sync).await,
        &[("status_sync_id", json!(closed)), ("closed_at", json!(now()))],
    );
    let verification = json!({
        "_sync_format": "row.v1",
        "work_order_sync_id": sync,
        "verified_by_sync_id": admin_sync(&db).await,
        "verified_at": now(),
        "result": "pass",
        "return_to_service_confirmed": 1,
    });
    let Value::Object(verification) = verification else {
        unreachable!()
    };
    inbox(&db, "b-2", "work_order_verifications", "ver-self", 1, &verification).await;
    inbox(&db, "b-2", "work_orders", &sync, 60, &wo_row).await;
    materialize(&db).await;
    assert_eq!(wo_status(&db, wo_id).await, "completed", "nothing of the group written");
    let codes_found = conflict_codes(&db).await;
    assert!(codes_found.iter().any(|c| c == "SYNC_GROUP_REFUSED"), "{codes_found:?}");
    assert!(
        codes_found.iter().any(|c| c == codes::SELF_VERIFICATION),
        "{codes_found:?}"
    );
}

#[tokio::test]
async fn a_completion_waits_for_a_blocked_child() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let task = add_task(&db, wo_id, "Bleed the line", true).await;
    let tsync = task_sync(&db, task.id).await;
    let task_row = with(
        received_task_result(&db, &tsync, "ok").await,
        &[("completed_by_sync_id", json!("user-not-here-yet"))],
    );
    inbox(&db, "b-3", "work_order_tasks", &tsync, 50, &task_row).await;
    inbox(
        &db,
        "b-3",
        "work_orders",
        &sync,
        50,
        &received_completed(&db, &sync).await,
    )
    .await;
    let result = materialize(&db).await;
    assert_eq!(result.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(
        inbox_status(&db, "work_orders", &sync).await.as_deref(),
        Some("deferred")
    );
    assert_eq!(wo_status(&db, wo_id).await, "in_progress");
}

#[tokio::test]
async fn a_received_cancelled_mandatory_task_keeps_the_completion_refused() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let task = add_task(&db, wo_id, "Replace the filter", true).await;
    let tsync = task_sync(&db, task.id).await;
    // From a PC that has not upgraded, while the WO is open here: the row is the technician's record.
    let outcome = apply(
        &db,
        "work_order_tasks",
        &tsync,
        &received_task_result(&db, &tsync, "cancelled").await,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    let outcome = apply(&db, "work_orders", &sync, &received_completed(&db, &sync).await).await;
    assert_eq!(code(&outcome), Some(codes::COMPLETION_GATE), "{outcome:?}");
    match outcome {
        ApplyOutcome::Conflict { message, .. } => assert!(message.contains("Replace the filter"), "{message}"),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn a_task_changed_after_completion_here_is_a_conflict() {
    let db = setup().await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    let task = add_task(&db, wo_id, "Grease the bearing", true).await;
    tasks_ok(&db, task.id).await;
    advance_to_completed_verified(&db, wo_id, rv).await;
    let tsync = task_sync(&db, task.id).await;
    // Re-resulted on another PC.
    let changed = with(
        wire(&db, "work_order_tasks", &tsync).await,
        &[("result_code", json!("nok"))],
    );
    let outcome = apply(&db, "work_order_tasks", &tsync, &changed).await;
    assert_eq!(code(&outcome), Some(codes::TASK_AFTER_COMPLETION), "{outcome:?}");
    // Reopened on another PC.
    let reopened = with(
        wire(&db, "work_order_tasks", &tsync).await,
        &[
            ("is_completed", json!(0)),
            ("result_code", Value::Null),
            ("completed_at", Value::Null),
            ("completed_by_sync_id", Value::Null),
        ],
    );
    let outcome = apply(&db, "work_order_tasks", &tsync, &reopened).await;
    assert_eq!(code(&outcome), Some(codes::TASK_AFTER_COMPLETION), "{outcome:?}");
    // A mandatory task added on another PC.
    let added = with(
        wire(&db, "work_order_tasks", &tsync).await,
        &[
            ("task_description", json!("Added elsewhere")),
            ("is_completed", json!(0)),
            ("result_code", Value::Null),
        ],
    );
    let outcome = apply(&db, "work_order_tasks", "task-added-elsewhere", &added).await;
    assert_eq!(code(&outcome), Some(codes::TASK_AFTER_COMPLETION), "{outcome:?}");
    // Deleted on another PC.
    let outcome = delete(&db, "work_order_tasks", &tsync).await;
    assert_eq!(code(&outcome), Some(codes::TASK_AFTER_COMPLETION), "{outcome:?}");
    assert_eq!(
        text(
            &db,
            "SELECT result_code FROM work_order_tasks WHERE id = ?",
            vec![task.id.into()]
        )
        .await
        .as_deref(),
        Some("ok")
    );
    // The same state sent again: passes.
    let same = wire(&db, "work_order_tasks", &tsync).await;
    let outcome = apply(&db, "work_order_tasks", &tsync, &same).await;
    assert!(
        matches!(outcome, ApplyOutcome::Applied | ApplyOutcome::Unchanged),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_close_received_without_failure_mode_here_is_a_conflict() {
    let db = setup().await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    advance_to_completed_verified(&db, wo_id, rv).await;
    let sync = wo_sync(&db, wo_id).await;
    // Cleared here while the other PC closed it.
    exec(
        &db,
        "DELETE FROM work_order_failure_details WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await;
    let closed = status_sync(&db, "closed").await;
    let wo_row = with(
        wire(&db, "work_orders", &sync).await,
        &[("status_sync_id", json!(closed)), ("closed_at", json!(now()))],
    );
    let outcome = apply(&db, "work_orders", &sync, &wo_row).await;
    assert_eq!(code(&outcome), Some(codes::CLOSE_GATE), "{outcome:?}");
    assert_eq!(wo_status(&db, wo_id).await, "completed");
}

#[tokio::test]
async fn a_change_after_close_here_is_a_conflict() {
    use crate::wo::closeout::{self, WoCloseInput};
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    let tool = crate::wo::tools::add_tool(
        &db,
        crate::wo::tools::AddToolInput {
            wo_id,
            tool_label: "Puller".into(),
            tool_code: None,
            origin: None,
            notes: None,
        },
    )
    .await
    .expect("tool");
    let rv = advance_to_completed_verified(&db, wo_id, rv).await;
    closeout::close_wo(
        &db,
        WoCloseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            ..Default::default()
        },
    )
    .await
    .expect("closed");
    // Rows written by these commands get their sync identity from capture, not installed in this
    // test database.
    for table in [
        "work_order_failure_details",
        "work_order_interveners",
        "work_order_tools",
    ] {
        exec(
            &db,
            &format!("UPDATE {table} SET entity_sync_id = '{table}-' || id WHERE entity_sync_id IS NULL"),
            vec![],
        )
        .await;
    }
    let fd_sync = text(
        &db,
        "SELECT entity_sync_id FROM work_order_failure_details WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("failure detail");
    let fd = with(
        wire(&db, "work_order_failure_details", &fd_sync).await,
        &[
            ("failure_mode_sync_id", Value::Null),
            ("notes", json!("changed elsewhere")),
        ],
    );
    let outcome = apply(&db, "work_order_failure_details", &fd_sync, &fd).await;
    assert_eq!(code(&outcome), Some(codes::FAILURE_DETAIL_AFTER_CLOSE), "{outcome:?}");
    let outcome = delete(&db, "work_order_failure_details", &fd_sync).await;
    assert_eq!(code(&outcome), Some(codes::FAILURE_DETAIL_AFTER_CLOSE), "{outcome:?}");

    let labour_sync = text(
        &db,
        "SELECT entity_sync_id FROM work_order_interveners WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("labour");
    let labour = with(
        wire(&db, "work_order_interveners", &labour_sync).await,
        &[("hours_worked", json!(9.5))],
    );
    let outcome = apply(&db, "work_order_interveners", "labour-added-elsewhere", &labour).await;
    assert_eq!(code(&outcome), Some(codes::LABOUR_AFTER_CLOSE), "{outcome:?}");

    let tool_sync = text(
        &db,
        "SELECT entity_sync_id FROM work_order_tools WHERE id = ?",
        vec![tool.id.into()],
    )
    .await
    .expect("tool sync id");
    let changed = with(
        wire(&db, "work_order_tools", &tool_sync).await,
        &[("usage_status", json!("not_used"))],
    );
    let outcome = apply(&db, "work_order_tools", &tool_sync, &changed).await;
    assert_eq!(code(&outcome), Some(codes::TOOL_AFTER_CLOSE), "{outcome:?}");
}

#[tokio::test]
async fn a_received_verification_by_the_labour_person_is_a_conflict() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let verification = |wo: &str, by: &str| {
        let Value::Object(p) = json!({
            "_sync_format": "row.v1",
            "work_order_sync_id": wo,
            "verified_by_sync_id": by,
            "verified_at": now(),
            "result": "pass",
            "return_to_service_confirmed": 1,
        }) else {
            unreachable!()
        };
        p
    };
    let admin = admin_sync(&db).await;
    // Not completed here yet: waits for the completion (review B3); closed or cancelled here: refused.
    let outcome = apply(
        &db,
        "work_order_verifications",
        "ver-early",
        &verification(&sync, &admin),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    for status in ["closed", "cancelled"] {
        set_wo_status(&db, wo_id, status).await;
        let outcome = apply(
            &db,
            "work_order_verifications",
            "ver-late",
            &verification(&sync, &admin),
        )
        .await;
        assert_eq!(
            code(&outcome),
            Some(codes::VERIFICATION_WRONG_STATUS),
            "{status}: {outcome:?}"
        );
    }
    set_wo_status(&db, wo_id, "completed").await;
    // By the responsible (admin).
    let outcome = apply(
        &db,
        "work_order_verifications",
        "ver-resp",
        &verification(&sync, &admin),
    )
    .await;
    assert_eq!(code(&outcome), Some(codes::SELF_VERIFICATION), "{outcome:?}");
    // No responsible: by the person who recorded the labour (D7).
    exec(
        &db,
        "UPDATE work_orders SET primary_responsible_id = NULL WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    let outcome = apply(
        &db,
        "work_order_verifications",
        "ver-labour",
        &verification(&sync, &admin),
    )
    .await;
    assert_eq!(code(&outcome), Some(codes::VERIFIER_RECORDED_LABOUR), "{outcome:?}");
    // By someone else: applied.
    let other = super::closeout_tests::tests::create_second_user(&db).await;
    let other_sync = text(
        &db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![other.into()],
    )
    .await
    .expect("sync");
    let outcome = apply(
        &db,
        "work_order_verifications",
        "ver-ok",
        &verification(&sync, &other_sync),
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn take_theirs_reruns_the_closeout_gate() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let task = add_task(&db, wo_id, "Re-torque", true).await;
    inbox(
        &db,
        "b-4",
        "work_orders",
        &sync,
        70,
        &received_completed(&db, &sync).await,
    )
    .await;
    materialize(&db).await;
    let conflicts = crate::sync::queries::list_sync_conflicts(&db, Default::default())
        .await
        .expect("list");
    let conflict = conflicts
        .iter()
        .find(|c| c.entity_type == "work_orders" && c.entity_sync_id == sync)
        .expect("completion conflict");
    let take = |db: &DatabaseConnection| {
        let conflict = conflict.clone();
        let db = db.clone();
        async move {
            let tx = db.begin().await.expect("begin");
            let r = crate::sync::conflicts::take_remote_version(&tx, &conflict).await;
            if r.is_ok() {
                tx.commit().await.expect("commit");
            } else {
                tx.rollback().await.expect("rollback");
            }
            r
        }
    };
    assert!(take(&db).await.is_err(), "the same rule refuses the server version");
    assert_eq!(wo_status(&db, wo_id).await, "in_progress");
    tasks_ok(&db, task.id).await;
    take(&db).await.expect("passes once the task is closed here");
    assert_eq!(wo_status(&db, wo_id).await, "completed");
}

/// A PC joining the plant receives by snapshot a work order closed long ago, with a mandatory task
/// closed cancelled before the rule: applied with its children (pages are stored before any is
/// applied, the children in any order), never refused. The same rows received as changes (not a
/// snapshot) are judged.
#[tokio::test]
async fn a_snapshot_arrival_applies_history_without_false_conflicts() {
    use crate::wo::closeout::{self, WoCloseInput};
    let source = setup().await;
    let actor = admin_id(&source).await;
    let (wo_id, rv) = create_wo_in_progress(&source).await;
    let rv = advance_to_completed_verified(&source, wo_id, rv).await;
    closeout::close_wo(
        &source,
        WoCloseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            ..Default::default()
        },
    )
    .await
    .expect("closed");
    exec(
        &source,
        "INSERT INTO work_order_tasks (work_order_id, task_description, sequence_order, is_mandatory, is_completed, \
         result_code, origin, entity_sync_id) VALUES (?, 'Legacy step', 1, 1, 1, 'cancelled', 'planned', 'task-legacy')",
        vec![wo_id.into()],
    )
    .await;
    // Sync identities as capture would give them (not installed in this test database).
    for table in [
        "wo_state_transition_log",
        "work_order_failure_details",
        "work_order_interveners",
        "work_order_tasks",
        "work_order_verifications",
    ] {
        exec(
            &source,
            &format!("UPDATE {table} SET entity_sync_id = '{table}-' || id WHERE entity_sync_id IS NULL"),
            vec![],
        )
        .await;
    }
    let sync = wo_sync(&source, wo_id).await;
    let mut rows: Vec<(String, String, Payload)> = Vec::new();
    for (table, col) in [
        ("wo_state_transition_log", "wo_id"),
        ("work_order_failure_details", "work_order_id"),
        ("work_order_interveners", "work_order_id"),
        ("work_order_tasks", "work_order_id"),
        ("work_order_verifications", "work_order_id"),
    ] {
        let sids = source
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT entity_sync_id FROM {table} WHERE {col} = ? AND entity_sync_id IS NOT NULL"),
                [wo_id.into()],
            ))
            .await
            .expect("children");
        for r in sids {
            let sid: String = r.try_get_by_index(0).expect("sid");
            let p = wire(&source, table, &sid).await;
            rows.push((table.to_string(), sid, p));
        }
    }
    let wo_row = wire(&source, "work_orders", &sync).await;

    for (batch, children_first) in [("snapshot-all-1", true), ("snapshot-all-2", false), ("b-delta", true)] {
        let target = setup().await;
        seed_test_equipment(&target).await;
        super::closeout_tests::tests::create_second_user(&target).await;
        if !children_first {
            inbox(&target, batch, "work_orders", &sync, 5, &wo_row).await;
        }
        for (table, sid, p) in &rows {
            inbox(&target, batch, table, sid, 5, p).await;
        }
        if children_first {
            inbox(&target, batch, "work_orders", &sync, 5, &wo_row).await;
        }
        let result = materialize(&target).await;
        let found = conflict_codes(&target).await;
        let applied = text(
            &target,
            "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.entity_sync_id = ?",
            vec![sync.clone().into()],
        )
        .await;
        if batch.starts_with("snapshot-") {
            assert_eq!(result.conflicts, 0, "{batch}: {found:?}");
            let waiting: Vec<String> = target
                .query_all(Statement::from_string(
                    DbBackend::Sqlite,
                    "SELECT entity_type || ' ' || apply_status || ' ' || COALESCE(rejection_message, '') \
                     FROM sync_inbox WHERE apply_status <> 'applied'"
                        .to_string(),
                ))
                .await
                .expect("inbox")
                .iter()
                .filter_map(|r| r.try_get_by_index::<String>(0).ok())
                .collect();
            assert_eq!(applied.as_deref(), Some("closed"), "{batch}: {waiting:?}");
            assert_eq!(
                text(
                    &target,
                    "SELECT result_code FROM work_order_tasks WHERE entity_sync_id = 'task-legacy'",
                    vec![]
                )
                .await
                .as_deref(),
                Some("cancelled"),
                "{batch}"
            );
        } else {
            assert!(
                found
                    .iter()
                    .any(|c| c == codes::COMPLETION_GATE || c == codes::CLOSE_GATE),
                "{batch}: {found:?}"
            );
            assert_eq!(applied, None, "{batch}: not written");
        }
    }

    // Received as changes, without the legacy task, but verified by the WO's own responsible: the
    // verifier rule holds inside the group of a work order new here too.
    let admin = admin_sync(&source).await;
    let target = setup().await;
    seed_test_equipment(&target).await;
    super::closeout_tests::tests::create_second_user(&target).await;
    for (table, sid, p) in rows.iter().filter(|(_, sid, _)| sid != "task-legacy") {
        let p = if table == "work_order_verifications" {
            with(p.clone(), &[("verified_by_sync_id", json!(admin))])
        } else {
            p.clone()
        };
        inbox(&target, "b-self", table, sid, 5, &p).await;
    }
    inbox(&target, "b-self", "work_orders", &sync, 5, &wo_row).await;
    materialize(&target).await;
    let found = conflict_codes(&target).await;
    assert!(found.iter().any(|c| c == "SYNC_GROUP_REFUSED"), "{found:?}");
    assert_eq!(
        text(
            &target,
            "SELECT entity_sync_id FROM work_orders WHERE entity_sync_id = ?",
            vec![sync.clone().into()]
        )
        .await,
        None,
        "not written"
    );
}

// ── Review fixes (B1, B2, N2, S1-S3, S7, N5) ────────────────────────────────────────────────────

/// Gives every child row of these tables a sync identity (capture is not installed in these test
/// databases) and returns the rows, as another PC would send them.
async fn rows_of(source: &DatabaseConnection, wo_id: i64, tables: &[(&str, &str)]) -> Vec<(String, String, Payload)> {
    let mut rows = Vec::new();
    for (table, col) in tables {
        exec(
            source,
            &format!("UPDATE {table} SET entity_sync_id = '{table}-' || id WHERE entity_sync_id IS NULL"),
            vec![],
        )
        .await;
        let sids = source
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT entity_sync_id FROM {table} WHERE {col} = ? ORDER BY id"),
                [wo_id.into()],
            ))
            .await
            .expect("children");
        for r in sids {
            let sid: String = r.try_get_by_index(0).expect("sid");
            let p = wire(source, table, &sid).await;
            rows.push((table.to_string(), sid, p));
        }
    }
    rows
}

const CHILD_TABLES: &[(&str, &str)] = &[
    ("wo_state_transition_log", "wo_id"),
    ("work_order_failure_details", "work_order_id"),
    ("work_order_interveners", "work_order_id"),
    ("work_order_tasks", "work_order_id"),
    ("work_order_tools", "work_order_id"),
    ("work_order_verifications", "work_order_id"),
];

/// A PC holding the same reference rows as `setup()` (equipment, second user).
async fn target_pc() -> DatabaseConnection {
    let target = setup().await;
    seed_test_equipment(&target).await;
    super::closeout_tests::tests::create_second_user(&target).await;
    target
}

/// Puts a work order's children then the work order in `target`'s inbox under `batch`, then
/// applies the inbox.
async fn receive(
    target: &DatabaseConnection,
    batch: &str,
    wo: (&str, &Payload),
    rows: &[(String, String, Payload)],
    version: i64,
) -> crate::sync::materialize::MaterializeInboxResult {
    for (table, sid, p) in rows {
        inbox(target, batch, table, sid, version, p).await;
    }
    inbox(target, batch, "work_orders", wo.0, version, wo.1).await;
    materialize(target).await
}

async fn add_tool_to(db: &DatabaseConnection, wo_id: i64) {
    crate::wo::tools::add_tool(
        db,
        crate::wo::tools::AddToolInput {
            wo_id,
            tool_label: "Torque wrench".into(),
            tool_code: None,
            origin: None,
            notes: None,
        },
    )
    .await
    .expect("tool");
}

async fn status_by_sync(db: &DatabaseConnection, sync: &str) -> Option<String> {
    text(
        db,
        "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.entity_sync_id = ?",
        vec![sync.into()],
    )
    .await
}

async fn user_sync_of(db: &DatabaseConnection, id: i64) -> String {
    text(
        db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![id.into()],
    )
    .await
    .expect("user sync id")
}

/// B1: a work order received cancelled (new here, by a snapshot or as changes from a PC that
/// created and cancelled it offline) brings its task, labour and tool with it, never refused
/// against the status it has just been given.
#[tokio::test]
async fn a_cancelled_work_order_arrives_with_its_children() {
    let source = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&source).await;
    add_task(&source, wo_id, "Inspect the seal", true).await;
    add_tool_to(&source, wo_id).await;
    set_wo_status(&source, wo_id, "cancelled").await;
    let rows = rows_of(&source, wo_id, CHILD_TABLES).await;
    let wo_row = wire(&source, "work_orders", &sync).await;
    for batch in ["snapshot-all-join", "b-offline-cancel"] {
        let target = target_pc().await;
        let result = receive(&target, batch, (&sync, &wo_row), &rows, 5).await;
        assert_eq!(result.conflicts, 0, "{batch}: {:?}", conflict_codes(&target).await);
        assert_eq!(
            status_by_sync(&target, &sync).await.as_deref(),
            Some("cancelled"),
            "{batch}"
        );
        for (table, sid, _) in &rows {
            assert_eq!(
                inbox_status(&target, table, sid).await.as_deref(),
                Some("applied"),
                "{batch} {table}"
            );
        }
    }
}

/// B1: verified on another PC, then reopened there: this PC (holding it completed) receives the
/// reopening with the verification, both applied.
#[tokio::test]
async fn a_verification_received_with_the_reopening_is_applied() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let actor = admin_id(&db).await;
    let rv = crate::wo::queries::get_work_order(&db, wo_id)
        .await
        .expect("wo")
        .expect("wo")
        .row_version;
    crate::wo::execution::complete_wo_mechanically(
        &db,
        crate::wo::execution::WoMechCompleteInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .expect("completed here");
    let other = super::closeout_tests::tests::create_second_user(&db).await;
    let Value::Object(verification) = json!({
        "_sync_format": "row.v1",
        "work_order_sync_id": sync,
        "verified_by_sync_id": user_sync_of(&db, other).await,
        "verified_at": now(),
        "result": "fail",
        "return_to_service_confirmed": 0,
    }) else {
        unreachable!()
    };
    let in_progress = status_sync(&db, "in_progress").await;
    let reopened = with(
        wire(&db, "work_orders", &sync).await,
        &[("status_sync_id", json!(in_progress)), ("updated_at", json!(now()))],
    );
    inbox(
        &db,
        "b-reopen",
        "work_order_verifications",
        "ver-before-reopen",
        1,
        &verification,
    )
    .await;
    inbox(&db, "b-reopen", "work_orders", &sync, 80, &reopened).await;
    let result = materialize(&db).await;
    assert_eq!(result.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(wo_status(&db, wo_id).await, "in_progress");
    assert_eq!(
        inbox_status(&db, "work_order_verifications", "ver-before-reopen")
            .await
            .as_deref(),
        Some("applied")
    );
}

/// B2 / N2: a completed work order whose verifier recorded the labour (no responsible), from before
/// D7: applied by a snapshot this PC asked for; the same rows as changes are refused, also when the
/// server names the exchange batch like a snapshot.
#[tokio::test]
async fn snapshot_history_skips_the_verifier_rules_but_changes_do_not() {
    let source = setup().await;
    let (wo_id, rv) = create_wo_in_progress(&source).await;
    advance_to_completed_verified(&source, wo_id, rv).await;
    let actor = admin_id(&source).await;
    exec(
        &source,
        "UPDATE work_orders SET primary_responsible_id = NULL WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    exec(
        &source,
        "UPDATE work_order_verifications SET verified_by_id = ? WHERE work_order_id = ?",
        vec![actor.into(), wo_id.into()],
    )
    .await;
    let sync = wo_sync(&source, wo_id).await;
    let rows = rows_of(&source, wo_id, CHILD_TABLES).await;
    let wo_row = wire(&source, "work_orders", &sync).await;

    let target = target_pc().await;
    let result = receive(&target, "snapshot-all-history", (&sync, &wo_row), &rows, 5).await;
    assert_eq!(result.conflicts, 0, "{:?}", conflict_codes(&target).await);
    assert_eq!(status_by_sync(&target, &sync).await.as_deref(), Some("completed"));

    let disguised = crate::sync::materialize::exchange_batch_id("snapshot-all-evil");
    assert!(!disguised.starts_with(crate::sync::materialize::SNAPSHOT_BATCH_PREFIX));
    for batch in ["b-delta-history", disguised.as_str()] {
        let target = target_pc().await;
        receive(&target, batch, (&sync, &wo_row), &rows, 5).await;
        let found = conflict_codes(&target).await;
        assert!(found.iter().any(|c| c == "SYNC_GROUP_REFUSED"), "{batch}: {found:?}");
        assert_eq!(status_by_sync(&target, &sync).await, None, "{batch}: not written");
    }
}

/// S7 / N5: a mandatory task deleted or made optional past planning, and a task result received
/// alone while its work order is not executed here, are refused.
#[tokio::test]
async fn mandatory_tasks_are_not_removed_and_results_wait_for_execution() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let task = add_task(&db, wo_id, "Isolate the drive", true).await;
    let tsync = task_sync(&db, task.id).await;
    let optional = with(
        wire(&db, "work_order_tasks", &tsync).await,
        &[("is_mandatory", json!(0))],
    );
    let outcome = apply(&db, "work_order_tasks", &tsync, &optional).await;
    assert_eq!(code(&outcome), Some(codes::MANDATORY_TASK_REMOVED), "{outcome:?}");
    let outcome = delete(&db, "work_order_tasks", &tsync).await;
    assert_eq!(code(&outcome), Some(codes::MANDATORY_TASK_REMOVED), "{outcome:?}");

    // While planning here: allowed.
    set_wo_status(&db, wo_id, "planning").await;
    let outcome = apply(&db, "work_order_tasks", &tsync, &optional).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    // A result received alone while the work order is ready here: waits for its start (review B3);
    // on hold: applied.
    set_wo_status(&db, wo_id, "ready").await;
    let result = received_task_result(&db, &tsync, "ok").await;
    let outcome = apply(&db, "work_order_tasks", &tsync, &result).await;
    assert!(matches!(outcome, ApplyOutcome::Deferred { .. }), "{outcome:?}");
    set_wo_status(&db, wo_id, "on_hold").await;
    let outcome = apply(&db, "work_order_tasks", &tsync, &result).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

/// S1: the closer of a received close is the person of the transition of this close.
#[tokio::test]
async fn the_closer_is_read_from_the_close_being_judged() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let actor = admin_id(&db).await;
    let other = super::closeout_tests::tests::create_second_user(&db).await;
    for (by, at) in [(actor, "2026-05-01T10:00:00Z"), (other, "2026-06-01T10:00:00Z")] {
        exec(
            &db,
            "INSERT INTO wo_state_transition_log (wo_id, from_status, to_status, action, actor_id, acted_at) \
             VALUES (?, 'completed', 'closed', 'close', ?, ?)",
            vec![wo_id.into(), by.into(), at.into()],
        )
        .await;
    }
    exec(
        &db,
        "UPDATE work_orders SET closed_at = '2026-05-01T10:00:00Z' WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    assert_eq!(
        crate::wo::closeout_sync::closer_of(&db, wo_id).await.expect("closer"),
        Some(actor)
    );
    exec(
        &db,
        "UPDATE work_orders SET closed_at = '2026-07-01T10:00:00Z' WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    assert_eq!(
        crate::wo::closeout_sync::closer_of(&db, wo_id).await.expect("closer"),
        None
    );
}

/// S2: at close, a verification counts only while its verifier is still independent (not the
/// responsible; without one, nobody who recorded labour): local close and received close.
#[tokio::test]
async fn close_counts_only_an_independent_verification() {
    use crate::wo::closeout::{self, WoCloseInput};
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    let rv = advance_to_completed_verified(&db, wo_id, rv).await;
    let verifier = super::closeout_tests::tests::create_second_user(&db).await;
    // The verifier became the responsible after verifying.
    exec(
        &db,
        "UPDATE work_orders SET primary_responsible_id = ? WHERE id = ?",
        vec![verifier.into(), wo_id.into()],
    )
    .await;
    let r = closeout::close_wo(
        &db,
        WoCloseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            ..Default::default()
        },
    )
    .await;
    match r {
        Err(crate::errors::AppError::ValidationFailed(e)) => assert!(
            e.iter()
                .any(|m| m.starts_with("GATE_CLOSEOUT_VERIFIER_NOT_INDEPENDENT")),
            "{e:?}"
        ),
        other => panic!("refused expected, got {other:?}"),
    }
    let sync = wo_sync(&db, wo_id).await;
    let closed = status_sync(&db, "closed").await;
    let wo_row = with(
        wire(&db, "work_orders", &sync).await,
        &[("status_sync_id", json!(closed)), ("closed_at", json!(now()))],
    );
    let outcome = apply(&db, "work_orders", &sync, &wo_row).await;
    assert_eq!(code(&outcome), Some(codes::CLOSE_GATE), "{outcome:?}");
}

/// S3: a received close using the FMECA parts override (critical suggested parts not consumed):
/// the signer must be active, hold the role and differ from the closer of this close; the
/// password is not checked again here.
#[tokio::test]
async fn a_received_close_checks_the_stored_fmeca_override_signer() {
    let db = setup().await;
    let admin = admin_id(&db).await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    advance_to_completed_verified(&db, wo_id, rv).await;
    let verifier = super::closeout_tests::tests::create_second_user(&db).await;
    // A critical FMECA line with a suggested part for the WO's equipment and failure mode.
    exec(&db, "PRAGMA foreign_keys = OFF", vec![]).await;
    exec(
        &db,
        "INSERT INTO fmeca_analyses (entity_sync_id, equipment_id, title, created_at, updated_at) \
         VALUES ('fmeca-s3', 1, 'S3', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        vec![],
    )
    .await;
    exec(
        &db,
        "INSERT INTO fmeca_items (entity_sync_id, analysis_id, failure_mode_id, severity, occurrence, detectability, \
         rpn, review_required, updated_at) \
         VALUES ('fmeca-item-s3', (SELECT id FROM fmeca_analyses WHERE entity_sync_id = 'fmeca-s3'), 7777, 10, 10, 10, \
         1000, 0, '2026-01-01T00:00:00Z')",
        vec![],
    )
    .await;
    exec(
        &db,
        "INSERT INTO fmeca_item_parts (fmeca_item_id, article_id) \
         VALUES ((SELECT id FROM fmeca_items WHERE entity_sync_id = 'fmeca-item-s3'), 8888)",
        vec![],
    )
    .await;
    exec(
        &db,
        "UPDATE work_order_failure_details SET failure_mode_id = 7777 WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await;
    exec(&db, "PRAGMA foreign_keys = ON", vec![]).await;
    exec(
        &db,
        "UPDATE closeout_validation_policies SET require_fmeca_parts_for_critical = 1, \
         fmeca_parts_override_require_distinct_signer = 1",
        vec![],
    )
    .await;
    let closed_at = "2026-06-02T10:00:00Z";
    let sync = wo_sync(&db, wo_id).await;
    let closed = status_sync(&db, "closed").await;
    let admin_sync = user_sync_of(&db, admin).await;
    let verifier_sync = user_sync_of(&db, verifier).await;
    // The close transition of this close (received earlier), by the verifier user.
    exec(
        &db,
        "INSERT INTO wo_state_transition_log (wo_id, from_status, to_status, action, actor_id, acted_at) \
         VALUES (?, 'completed', 'closed', 'close', ?, ?)",
        vec![wo_id.into(), verifier.into(), closed_at.into()],
    )
    .await;
    let base = wire(&db, "work_orders", &sync).await;
    let close = |signer: &str| {
        with(
            base.clone(),
            &[
                ("status_sync_id", json!(closed)),
                ("closed_at", json!(closed_at)),
                (
                    "fmeca_parts_override_reason",
                    json!("Critical bearing not in stock; temporary repair approved by supervisor."),
                ),
                ("fmeca_parts_override_signed_by_sync_id", json!(signer)),
            ],
        )
    };
    let set_closer = |by: i64| {
        exec(
            &db,
            "UPDATE wo_state_transition_log SET actor_id = ? WHERE wo_id = ? AND to_status = 'closed'",
            vec![by.into(), wo_id.into()],
        )
    };
    // The administrator signs the override of the close they made themselves: refused.
    set_closer(admin).await;
    let outcome = apply(&db, "work_orders", &sync, &close(&admin_sync)).await;
    assert_eq!(
        code(&outcome),
        Some(codes::CLOSE_GATE),
        "signer is the closer: {outcome:?}"
    );
    // Signer without the role (the verifier user holds none), another closer: refused.
    set_closer(admin).await;
    let outcome = apply(&db, "work_orders", &sync, &close(&verifier_sync)).await;
    assert_eq!(
        code(&outcome),
        Some(codes::CLOSE_GATE),
        "signer without the role: {outcome:?}"
    );
    // An administrator signs, the verifier user closed — but the signer is inactive: refused.
    set_closer(verifier).await;
    exec(
        &db,
        "UPDATE user_accounts SET is_active = 0 WHERE id = ?",
        vec![admin.into()],
    )
    .await;
    let outcome = apply(&db, "work_orders", &sync, &close(&admin_sync)).await;
    assert_eq!(code(&outcome), Some(codes::CLOSE_GATE), "inactive signer: {outcome:?}");
    exec(
        &db,
        "UPDATE user_accounts SET is_active = 1 WHERE id = ?",
        vec![admin.into()],
    )
    .await;
    // No transition of this close here: the signer cannot be shown distinct from the closer.
    exec(
        &db,
        "UPDATE wo_state_transition_log SET acted_at = '2026-06-01T00:00:00Z' WHERE wo_id = ? AND to_status = 'closed'",
        vec![wo_id.into()],
    )
    .await;
    let outcome = apply(&db, "work_orders", &sync, &close(&admin_sync)).await;
    assert_eq!(
        code(&outcome),
        Some(codes::CLOSE_GATE),
        "no close transition: {outcome:?}"
    );
    exec(
        &db,
        "UPDATE wo_state_transition_log SET acted_at = ? WHERE wo_id = ? AND to_status = 'closed'",
        vec![closed_at.into(), wo_id.into()],
    )
    .await;
    // Valid: applied (the signer's password was checked where the close was made).
    let outcome = apply(&db, "work_orders", &sync, &close(&admin_sync)).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert_eq!(wo_status(&db, wo_id).await, "closed");
}

/// S8 (design test 26): PC1 adds a mandatory task while PC2 completes the WO; both refuse the
/// other's change; PC1 keeps its version, PC2 receives it, then takes PC1's task: both PCs end with
/// the WO in progress and the task open, no conflict left open.
#[tokio::test]
async fn keep_mine_on_one_side_converges() {
    use crate::sync::domain::ResolveSyncConflictInput;
    let pc1 = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&pc1).await;
    let pc2 = target_pc().await;
    let rows = rows_of(&pc1, wo_id, CHILD_TABLES).await;
    let wo_row = wire(&pc1, "work_orders", &sync).await;
    let first = receive(&pc2, "b-1", (&sync, &wo_row), &rows, 10).await;
    assert_eq!(first.conflicts, 0, "{:?}", conflict_codes(&pc2).await);
    let wo2 = scalar(&pc2, &sync).await;

    // PC2 completes; PC1 adds a mandatory task meanwhile.
    let admin2 = admin_id(&pc2).await;
    let rv2 = crate::wo::queries::get_work_order(&pc2, wo2)
        .await
        .expect("wo")
        .expect("wo")
        .row_version;
    crate::wo::execution::complete_wo_mechanically(
        &pc2,
        crate::wo::execution::WoMechCompleteInput {
            wo_id: wo2,
            actor_id: admin2,
            expected_row_version: rv2,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .expect("PC2 completes");
    let task = add_task(&pc1, wo_id, "Re-check the alignment", true).await;
    let tsync = task_sync(&pc1, task.id).await;

    // The exchange: each PC receives the other's change and refuses it.
    inbox(
        &pc1,
        "b-2",
        "work_orders",
        &sync,
        20,
        &wire(&pc2, "work_orders", &sync).await,
    )
    .await;
    materialize(&pc1).await;
    assert_eq!(conflict_codes(&pc1).await, vec![codes::COMPLETION_GATE.to_string()]);
    inbox(
        &pc2,
        "b-2",
        "work_order_tasks",
        &tsync,
        20,
        &wire(&pc1, "work_order_tasks", &tsync).await,
    )
    .await;
    materialize(&pc2).await;
    assert_eq!(
        conflict_codes(&pc2).await,
        vec![codes::TASK_AFTER_COMPLETION.to_string()]
    );
    assert_eq!(wo_status(&pc1, wo_id).await, "in_progress");
    assert_eq!(wo_status(&pc2, wo2).await, "completed");

    let resolve = |db: DatabaseConnection, action: &'static str| async move {
        let conflict = crate::sync::queries::list_sync_conflicts(&db, Default::default())
            .await
            .expect("conflicts")
            .into_iter()
            .find(|c| c.status == "new")
            .expect("open conflict");
        let user = admin_id(&db).await;
        crate::sync::queries::resolve_sync_conflict(
            &db,
            user,
            ResolveSyncConflictInput {
                conflict_id: conflict.id,
                expected_row_version: conflict.row_version,
                action: action.into(),
                resolution_note: Some("Task added on site must be done first".into()),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{action}: {e:?}"));
    };
    // PC1 keeps its version (the WO in progress) and sends it again.
    resolve(pc1.clone(), "accept_local").await;
    inbox(
        &pc2,
        "b-3",
        "work_orders",
        &sync,
        30,
        &wire(&pc1, "work_orders", &sync).await,
    )
    .await;
    let third = materialize(&pc2).await;
    assert_eq!(third.conflicts, 0, "{:?}", conflict_codes(&pc2).await);
    assert_eq!(wo_status(&pc2, wo2).await, "in_progress");
    // PC2 takes PC1's task: it now passes.
    resolve(pc2.clone(), "accept_remote").await;

    for (db, wo) in [(&pc1, wo_id), (&pc2, wo2)] {
        assert_eq!(wo_status(db, wo).await, "in_progress");
        assert_eq!(
            text(
                db,
                "SELECT task_description FROM work_order_tasks WHERE entity_sync_id = ? AND is_completed = 0",
                vec![tsync.clone().into()],
            )
            .await
            .as_deref(),
            Some("Re-check the alignment")
        );
        let open = crate::sync::queries::list_sync_conflicts(db, Default::default())
            .await
            .expect("conflicts")
            .into_iter()
            .filter(|c| c.status == "new")
            .count();
        assert_eq!(open, 0, "no conflict left open");
    }
}

async fn scalar(db: &DatabaseConnection, wo_sync: &str) -> i64 {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM work_orders WHERE entity_sync_id = ?",
        [wo_sync.into()],
    ))
    .await
    .expect("query")
    .expect("wo here")
    .try_get_by_index::<i64>(0)
    .expect("id")
}

// ── Review round 2 (B3, S1, S2, N1) ─────────────────────────────────────────────────────────────

fn verification_row(wo_sync: &str, by: &str) -> Payload {
    let Value::Object(p) = json!({
        "_sync_format": "row.v1",
        "work_order_sync_id": wo_sync,
        "verified_by_sync_id": by,
        "verified_at": now(),
        "result": "pass",
        "return_to_service_confirmed": 1,
    }) else {
        unreachable!()
    };
    p
}

/// B3: the task result arrives in one pass while the work order is still ready here; the
/// completion made on the same PC arrives in the next pass: the task waits, then comes with it.
#[tokio::test]
async fn a_task_result_received_before_its_completion_waits_for_it() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let task = add_task(&db, wo_id, "Lock out and try", true).await;
    let tsync = task_sync(&db, task.id).await;
    let task_row = received_task_result(&db, &tsync, "ok").await;
    let wo_row = received_completed(&db, &sync).await;
    set_wo_status(&db, wo_id, "ready").await;

    inbox(&db, "b-page-1", "work_order_tasks", &tsync, 50, &task_row).await;
    let first = materialize(&db).await;
    assert_eq!(first.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(
        inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
        Some("deferred")
    );
    inbox(&db, "b-page-2", "work_orders", &sync, 50, &wo_row).await;
    let second = materialize(&db).await;
    assert_eq!(second.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(wo_status(&db, wo_id).await, "completed");
    assert_eq!(
        inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
        Some("applied")
    );
    assert!(crate::wo::tasks::open_mandatory_tasks(&db, wo_id)
        .await
        .expect("open")
        .is_empty());
}

/// B3: the verification arrives in one pass while the work order is in progress here; the close
/// (completed, verified and closed on another PC) in the next: the verification waits, then the
/// close is judged with it and applied.
#[tokio::test]
async fn a_verification_received_before_its_close_waits_for_it() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    let other = super::closeout_tests::tests::create_second_user(&db).await;
    let verification = verification_row(&sync, &user_sync_of(&db, other).await);
    let closed = status_sync(&db, "closed").await;
    let wo_row = with(
        wire(&db, "work_orders", &sync).await,
        &[
            ("status_sync_id", json!(closed)),
            ("mechanically_completed_at", json!(now())),
            ("closed_at", json!(now())),
            ("updated_at", json!(now())),
        ],
    );
    inbox(
        &db,
        "b-page-1",
        "work_order_verifications",
        "ver-before-close",
        1,
        &verification,
    )
    .await;
    let first = materialize(&db).await;
    assert_eq!(first.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(
        inbox_status(&db, "work_order_verifications", "ver-before-close")
            .await
            .as_deref(),
        Some("deferred")
    );
    inbox(&db, "b-page-2", "work_orders", &sync, 60, &wo_row).await;
    let second = materialize(&db).await;
    assert_eq!(second.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(wo_status(&db, wo_id).await, "closed");
    assert_eq!(
        inbox_status(&db, "work_order_verifications", "ver-before-close")
            .await
            .as_deref(),
        Some("applied")
    );
}

/// S1: labour and tool use received while the work order is not executed here wait for its next
/// status change; closed here, labour is refused (R7).
#[tokio::test]
async fn labour_and_tool_use_received_outside_execution_wait() {
    let db = setup().await;
    let (wo_id, sync) = wo_ready_to_complete(&db).await;
    add_tool_to(&db, wo_id).await;
    exec(
        &db,
        "UPDATE work_order_interveners SET entity_sync_id = 'labour-s1-' || id WHERE entity_sync_id IS NULL",
        vec![],
    )
    .await;
    exec(
        &db,
        "UPDATE work_order_tools SET entity_sync_id = 'tool-s1-' || id, usage_status = 'planned' \
         WHERE entity_sync_id IS NULL",
        vec![],
    )
    .await;
    let labour_sync = text(
        &db,
        "SELECT entity_sync_id FROM work_order_interveners WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("labour");
    let tool_sync = text(
        &db,
        "SELECT entity_sync_id FROM work_order_tools WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("tool");
    let labour = with(
        wire(&db, "work_order_interveners", &labour_sync).await,
        &[("hours_worked", json!(3.5))],
    );
    let tool = with(
        wire(&db, "work_order_tools", &tool_sync).await,
        &[("usage_status", json!("used"))],
    );
    for status in ["ready", "completed"] {
        set_wo_status(&db, wo_id, status).await;
        let outcome = apply(&db, "work_order_interveners", &labour_sync, &labour).await;
        assert!(
            matches!(outcome, ApplyOutcome::Deferred { .. }),
            "{status} labour: {outcome:?}"
        );
        let outcome = apply(&db, "work_order_tools", &tool_sync, &tool).await;
        assert!(
            matches!(outcome, ApplyOutcome::Deferred { .. }),
            "{status} tool: {outcome:?}"
        );
    }
    set_wo_status(&db, wo_id, "closed").await;
    let outcome = apply(&db, "work_order_interveners", &labour_sync, &labour).await;
    assert_eq!(code(&outcome), Some(codes::LABOUR_AFTER_CLOSE), "{outcome:?}");

    // Ready here, the labour waits; the start received next brings it.
    set_wo_status(&db, wo_id, "ready").await;
    inbox(&db, "b-s1-1", "work_order_interveners", &labour_sync, 50, &labour).await;
    let first = materialize(&db).await;
    assert_eq!(first.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(
        inbox_status(&db, "work_order_interveners", &labour_sync)
            .await
            .as_deref(),
        Some("deferred")
    );
    let in_progress = status_sync(&db, "in_progress").await;
    let started = with(
        wire(&db, "work_orders", &sync).await,
        &[("status_sync_id", json!(in_progress)), ("updated_at", json!(now()))],
    );
    inbox(&db, "b-s1-2", "work_orders", &sync, 50, &started).await;
    let second = materialize(&db).await;
    assert_eq!(second.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(
        inbox_status(&db, "work_order_interveners", &labour_sync)
            .await
            .as_deref(),
        Some("applied")
    );
}

/// S2: completed here; reopened elsewhere, the task reopened and recorded again, completed again:
/// received with the same status, it is judged as coming back from execution — applied with its
/// re-recorded task, or refused by the completion checks when that task is left open.
#[tokio::test]
async fn a_completion_made_again_elsewhere_is_judged_with_its_re_recorded_children() {
    for (result, refused) in [("ok", false), ("cancelled", true)] {
        let db = setup().await;
        let (wo_id, sync) = wo_ready_to_complete(&db).await;
        let actor = admin_id(&db).await;
        let task = add_task(&db, wo_id, "Measure the vibration", true).await;
        tasks_ok(&db, task.id).await;
        let rv = crate::wo::queries::get_work_order(&db, wo_id)
            .await
            .expect("wo")
            .expect("wo")
            .row_version;
        crate::wo::execution::complete_wo_mechanically(
            &db,
            crate::wo::execution::WoMechCompleteInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await
        .expect("completed here");
        let tsync = task_sync(&db, task.id).await;
        let task_row = with(
            wire(&db, "work_order_tasks", &tsync).await,
            &[
                ("result_code", json!(result)),
                ("completed_at", json!("2099-01-01T00:00:00Z")),
            ],
        );
        let again = with(
            wire(&db, "work_orders", &sync).await,
            &[
                ("mechanically_completed_at", json!("2026-01-02T03:04:05Z")),
                ("updated_at", json!(now())),
            ],
        );
        let task_row = with(task_row, &[("completed_at", json!(now()))]);
        inbox(&db, "b-again", "work_order_tasks", &tsync, 70, &task_row).await;
        inbox(&db, "b-again", "work_orders", &sync, 70, &again).await;
        materialize(&db).await;
        let found = conflict_codes(&db).await;
        if refused {
            assert!(found.iter().any(|c| c == codes::COMPLETION_GATE), "{result}: {found:?}");
        } else {
            assert!(found.is_empty(), "{result}: {found:?}");
            assert_eq!(
                inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
                Some("applied")
            );
        }
        assert_eq!(wo_status(&db, wo_id).await, "completed");
    }
}

/// N1: in the group of a snapshot work order new here, a verification received as a change (not
/// with the snapshot) is judged: by the labour person of a WO without responsible, refused.
#[tokio::test]
async fn a_change_applied_with_snapshot_history_is_judged() {
    let source = setup().await;
    let (wo_id, rv) = create_wo_in_progress(&source).await;
    advance_to_completed_verified(&source, wo_id, rv).await;
    let actor = admin_id(&source).await;
    exec(
        &source,
        "UPDATE work_orders SET primary_responsible_id = NULL WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    let sync = wo_sync(&source, wo_id).await;
    let rows: Vec<(String, String, Payload)> = rows_of(&source, wo_id, CHILD_TABLES)
        .await
        .into_iter()
        .filter(|(table, _, _)| table != "work_order_verifications")
        .collect();
    let wo_row = wire(&source, "work_orders", &sync).await;
    let target = target_pc().await;
    for (table, sid, p) in &rows {
        inbox(&target, "snapshot-all-n1", table, sid, 5, p).await;
    }
    let by_labour_person = verification_row(&sync, &user_sync_of(&source, actor).await);
    inbox(
        &target,
        "b-delta-n1",
        "work_order_verifications",
        "ver-n1",
        1,
        &by_labour_person,
    )
    .await;
    inbox(&target, "snapshot-all-n1", "work_orders", &sync, 5, &wo_row).await;
    materialize(&target).await;
    let found = conflict_codes(&target).await;
    assert!(found.iter().any(|c| c == "SYNC_GROUP_REFUSED"), "{found:?}");
}

// ── Review round 3 (SF1, SF2) ───────────────────────────────────────────────────────────────────

fn hours_from_now(h: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::hours(h))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

async fn rejection_message(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Option<String> {
    text(
        db,
        "SELECT rejection_message FROM sync_inbox WHERE entity_type = ? AND entity_sync_id = ? ORDER BY id DESC LIMIT 1",
        vec![entity_type.into(), sync_id.into()],
    )
    .await
}

/// A WO completed here with one mandatory task recorded OK.
async fn completed_here_with_task(db: &DatabaseConnection) -> (i64, String, String) {
    let (wo_id, sync) = wo_ready_to_complete(db).await;
    let actor = admin_id(db).await;
    let task = add_task(db, wo_id, "Measure the vibration", true).await;
    tasks_ok(db, task.id).await;
    let rv = crate::wo::queries::get_work_order(db, wo_id)
        .await
        .expect("wo")
        .expect("wo")
        .row_version;
    crate::wo::execution::complete_wo_mechanically(
        db,
        crate::wo::execution::WoMechCompleteInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .expect("completed here");
    let tsync = task_sync(db, task.id).await;
    (wo_id, sync, tsync)
}

/// SF1: reopened elsewhere, the task recorded again, completed again; the task arrives in one pass,
/// the work order in the next. The task waits (recorded after this PC's completion), then comes
/// with the return from execution and is judged with it (R3 refuses a task left open).
#[tokio::test]
async fn a_reopening_split_across_passes_is_judged_with_its_return() {
    for (result, refused) in [("ok", false), ("cancelled", true)] {
        let db = setup().await;
        let (wo_id, sync, tsync) = completed_here_with_task(&db).await;
        let task_row = with(
            wire(&db, "work_order_tasks", &tsync).await,
            &[
                ("result_code", json!(result)),
                ("completed_at", json!(hours_from_now(1))),
            ],
        );
        inbox(&db, "b-reopen-1", "work_order_tasks", &tsync, 70, &task_row).await;
        let first = materialize(&db).await;
        assert_eq!(first.conflicts, 0, "{result}: {:?}", conflict_codes(&db).await);
        assert_eq!(
            inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
            Some("deferred")
        );
        assert!(
            rejection_message(&db, "work_order_tasks", &tsync)
                .await
                .is_some_and(|m| m.contains("recorded after this PC's completion or close")),
            "the reason is shown"
        );

        let again = with(
            wire(&db, "work_orders", &sync).await,
            &[
                ("mechanically_completed_at", json!(hours_from_now(2))),
                ("updated_at", json!(now())),
            ],
        );
        inbox(&db, "b-reopen-2", "work_orders", &sync, 70, &again).await;
        materialize(&db).await;
        let found = conflict_codes(&db).await;
        if refused {
            assert!(found.iter().any(|c| c == codes::COMPLETION_GATE), "{result}: {found:?}");
        } else {
            assert!(found.is_empty(), "{result}: {found:?}");
            assert_eq!(
                inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
                Some("applied")
            );
        }
        assert_eq!(wo_status(&db, wo_id).await, "completed");
    }
}

/// SF1: the reopen counter alone (same stamps) marks the return from execution.
#[tokio::test]
async fn a_higher_reopen_count_marks_a_return_from_execution() {
    let db = setup().await;
    let (_, sync, tsync) = completed_here_with_task(&db).await;
    let task_row = with(
        wire(&db, "work_order_tasks", &tsync).await,
        &[("completed_at", json!(hours_from_now(1)))],
    );
    let reopen_count = wire(&db, "work_orders", &sync)
        .await
        .get("reopen_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let again = with(
        wire(&db, "work_orders", &sync).await,
        &[("reopen_count", json!(reopen_count + 1)), ("updated_at", json!(now()))],
    );
    inbox(&db, "b-count-1", "work_order_tasks", &tsync, 70, &task_row).await;
    materialize(&db).await;
    assert_eq!(
        inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
        Some("deferred")
    );
    inbox(&db, "b-count-2", "work_orders", &sync, 70, &again).await;
    let second = materialize(&db).await;
    assert_eq!(second.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(
        inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
        Some("applied")
    );
}

/// SF1: a WO closed here, reopened and closed again elsewhere; its new verification arrives in one
/// pass, the close in the next: the verification waits, then the close is judged with it.
#[tokio::test]
async fn a_reclose_split_across_passes_is_judged_with_its_return() {
    use crate::wo::closeout::{self, WoCloseInput};
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    let rv = advance_to_completed_verified(&db, wo_id, rv).await;
    closeout::close_wo(
        &db,
        WoCloseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            ..Default::default()
        },
    )
    .await
    .expect("closed here");
    let sync = wo_sync(&db, wo_id).await;
    let verifier = super::closeout_tests::tests::create_second_user(&db).await;
    let verification = with(
        verification_row(&sync, &user_sync_of(&db, verifier).await),
        &[("verified_at", json!(hours_from_now(1)))],
    );
    inbox(
        &db,
        "b-reclose-1",
        "work_order_verifications",
        "ver-reclose",
        1,
        &verification,
    )
    .await;
    let first = materialize(&db).await;
    assert_eq!(first.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(
        inbox_status(&db, "work_order_verifications", "ver-reclose")
            .await
            .as_deref(),
        Some("deferred")
    );
    let again = with(
        wire(&db, "work_orders", &sync).await,
        &[("closed_at", json!(hours_from_now(2))), ("updated_at", json!(now()))],
    );
    inbox(&db, "b-reclose-2", "work_orders", &sync, 80, &again).await;
    let second = materialize(&db).await;
    assert_eq!(second.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(wo_status(&db, wo_id).await, "closed");
    assert_eq!(
        inbox_status(&db, "work_order_verifications", "ver-reclose")
            .await
            .as_deref(),
        Some("applied")
    );
}

/// SF1: a row recorded after this PC's completion does not turn an ordinary close (completed →
/// closed, no return from execution) into a refused group: the close is judged without it, and the
/// row keeps waiting, visibly, never applied unjudged.
#[tokio::test]
async fn a_row_recorded_after_completion_does_not_block_the_close() {
    let db = setup().await;
    let (wo_id, sync, tsync) = completed_here_with_task(&db).await;
    let verifier = super::closeout_tests::tests::create_second_user(&db).await;
    exec(
        &db,
        "INSERT INTO work_order_verifications (work_order_id, verified_by_id, verified_at, result, \
         return_to_service_confirmed) VALUES (?, ?, ?, 'pass', 1)",
        vec![wo_id.into(), verifier.into(), now().into()],
    )
    .await;
    let task_row = with(
        wire(&db, "work_order_tasks", &tsync).await,
        &[("result_code", json!("na")), ("completed_at", json!(hours_from_now(1)))],
    );
    inbox(&db, "b-after-1", "work_order_tasks", &tsync, 70, &task_row).await;
    materialize(&db).await;
    assert_eq!(
        inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
        Some("deferred")
    );
    let closed = status_sync(&db, "closed").await;
    let close = with(
        wire(&db, "work_orders", &sync).await,
        &[
            ("status_sync_id", json!(closed)),
            ("closed_at", json!(now())),
            ("updated_at", json!(now())),
        ],
    );
    inbox(&db, "b-after-2", "work_orders", &sync, 70, &close).await;
    let second = materialize(&db).await;
    assert_eq!(second.conflicts, 0, "{:?}", conflict_codes(&db).await);
    assert_eq!(wo_status(&db, wo_id).await, "closed");
    assert_eq!(
        inbox_status(&db, "work_order_tasks", &tsync).await.as_deref(),
        Some("deferred")
    );
    assert_eq!(
        text(
            &db,
            "SELECT result_code FROM work_order_tasks WHERE entity_sync_id = ?",
            vec![tsync.clone().into()]
        )
        .await
        .as_deref(),
        Some("ok"),
        "never applied unjudged"
    );
}

/// SF2: a WO cancelled on another PC with labour still open: the labour closure is written before
/// the cancel, so it arrives first; in two passes, nothing is refused and the labour is closed here.
#[tokio::test]
async fn labour_closed_by_a_cancel_arrives_before_it() {
    let source = setup().await;
    let actor = admin_id(&source).await;
    let (wo_id, rv) = create_wo_in_progress(&source).await;
    crate::wo::labor::add_labor_entry(
        &source,
        crate::wo::labor::AddLaborInput {
            wo_id,
            intervener_id: actor,
            skill_id: None,
            started_at: Some("2026-04-10T08:00:00Z".into()),
            ended_at: None,
            hours_worked: None,
            hourly_rate: None,
            notes: None,
        },
    )
    .await
    .expect("open labour");
    let sync = wo_sync(&source, wo_id).await;
    let target = target_pc().await;
    let rows = rows_of(&source, wo_id, CHILD_TABLES).await;
    let first = receive(
        &target,
        "b-cancel-0",
        (&sync, &wire(&source, "work_orders", &sync).await),
        &rows,
        5,
    )
    .await;
    assert_eq!(first.conflicts, 0, "{:?}", conflict_codes(&target).await);

    crate::wo::queries::cancel_work_order(
        &source,
        crate::wo::domain::WoCancelInput {
            id: wo_id,
            expected_row_version: rv,
            actor_id: actor,
            cancel_reason: "Equipment replaced instead".into(),
        },
    )
    .await
    .expect("cancelled on the other PC");
    let labour_sync = text(
        &source,
        "SELECT entity_sync_id FROM work_order_interveners WHERE work_order_id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("labour sync id");
    // Pass 1: the labour closure (written first); pass 2: the cancel.
    inbox(
        &target,
        "b-cancel-1",
        "work_order_interveners",
        &labour_sync,
        10,
        &wire(&source, "work_order_interveners", &labour_sync).await,
    )
    .await;
    let pass1 = materialize(&target).await;
    assert_eq!(pass1.conflicts, 0, "{:?}", conflict_codes(&target).await);
    inbox(
        &target,
        "b-cancel-2",
        "work_orders",
        &sync,
        10,
        &wire(&source, "work_orders", &sync).await,
    )
    .await;
    let pass2 = materialize(&target).await;
    assert_eq!(pass2.conflicts, 0, "{:?}", conflict_codes(&target).await);
    assert_eq!(status_by_sync(&target, &sync).await.as_deref(), Some("cancelled"));
    assert!(
        text(
            &target,
            "SELECT ended_at FROM work_order_interveners WHERE entity_sync_id = ?",
            vec![labour_sync.into()],
        )
        .await
        .is_some(),
        "labour closed here too"
    );
}
