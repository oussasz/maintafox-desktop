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
    // Not completed here: refused.
    let outcome = apply(
        &db,
        "work_order_verifications",
        "ver-early",
        &verification(&sync, &admin),
    )
    .await;
    assert_eq!(code(&outcome), Some(codes::VERIFICATION_WRONG_STATUS), "{outcome:?}");
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
}
