//! Close-out gates (AU-05, AU-15, AU-99, CMMS Advisor D3 / D7 / O2 / O3), local rules:
//! - a mandatory task closes only with OK, N/A, or NOK with a finding note or a follow-up work
//!   order; cancelled / deferred refused on it; optional tasks take every result;
//! - task results, reopenings and tool marks only while the WO is in progress or on hold;
//! - a recorded result is changed only by reopening the task, with a comment, its previous result
//!   kept in the history;
//! - an open mandatory task (legacy row included) blocks the completion and the close.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::closeout_tests::tests::{admin_id, create_wo_in_progress, setup};
use crate::errors::AppError;
use crate::wo::tasks::{self, task_codes, AddTaskInput, CompleteTaskInput};
use crate::wo::tools::{self, tool_codes, AddToolInput};

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect(sql);
}

async fn text(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect(sql)
        .and_then(|r| r.try_get_by_index::<Option<String>>(0).ok().flatten())
}

pub(crate) async fn set_wo_status(db: &DatabaseConnection, wo_id: i64, code: &str) {
    exec(
        db,
        "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = ?) WHERE id = ?",
        vec![code.into(), wo_id.into()],
    )
    .await;
}

pub(crate) async fn add_task(db: &DatabaseConnection, wo_id: i64, label: &str, mandatory: bool) -> tasks::WoTask {
    tasks::add_task(
        db,
        AddTaskInput {
            wo_id,
            task_description: label.into(),
            sequence_order: 1,
            is_mandatory: mandatory,
            estimated_minutes: None,
            origin: None,
        },
    )
    .await
    .expect("add task")
}

fn result(code: &str) -> CompleteTaskInput {
    CompleteTaskInput {
        result_code: code.into(),
        ..Default::default()
    }
}

/// The refusal code a result carries (first reason).
fn refused<T: std::fmt::Debug>(r: Result<T, AppError>) -> String {
    match r {
        Err(AppError::ValidationFailed(msgs)) => msgs
            .first()
            .and_then(|m| m.split([':', ' ']).next())
            .unwrap_or_default()
            .to_string(),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A second work order of the test plant (the follow-up of a NOK).
async fn second_wo(db: &DatabaseConnection) -> i64 {
    create_wo_in_progress(db).await.0
}

#[tokio::test]
async fn mandatory_task_refuses_cancelled_and_deferred() {
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let task = add_task(&db, wo_id, "Lock out the motor", true).await;
    for code in ["cancelled", "deferred"] {
        let r = tasks::complete_task_with(&db, task.id, actor, result(code)).await;
        assert_eq!(refused(r), task_codes::MANDATORY_RESULT_REFUSED, "{code}");
    }
    assert_eq!(
        text(
            &db,
            "SELECT result_code FROM work_order_tasks WHERE id = ?",
            vec![task.id.into()]
        )
        .await,
        None
    );
}

#[tokio::test]
async fn mandatory_nok_needs_finding_or_follow_up() {
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let follow_up = second_wo(&db).await;
    let bare = add_task(&db, wo_id, "Check the coupling", true).await;
    let r = tasks::complete_task_with(&db, bare.id, actor, result("nok")).await;
    assert_eq!(refused(r), task_codes::NOK_NEEDS_EVIDENCE);
    // A blank note is no finding.
    let r = tasks::complete_task_with(
        &db,
        bare.id,
        actor,
        CompleteTaskInput {
            finding_note: Some("   ".into()),
            ..result("nok")
        },
    )
    .await;
    assert_eq!(refused(r), task_codes::NOK_NEEDS_EVIDENCE);

    let noted = tasks::complete_task_with(
        &db,
        bare.id,
        actor,
        CompleteTaskInput {
            finding_note: Some("Coupling worn, 2 mm play".into()),
            ..result("nok")
        },
    )
    .await
    .expect("NOK with a finding");
    assert_eq!(noted.finding_note.as_deref(), Some("Coupling worn, 2 mm play"));

    let other = add_task(&db, wo_id, "Check the seal", true).await;
    let linked = tasks::complete_task_with(
        &db,
        other.id,
        actor,
        CompleteTaskInput {
            follow_up_work_order_id: Some(follow_up),
            ..result("nok")
        },
    )
    .await
    .expect("NOK with a follow-up");
    assert_eq!(linked.follow_up_work_order_id, Some(follow_up));
    assert!(tasks::open_mandatory_tasks(&db, wo_id).await.expect("open").is_empty());
}

#[tokio::test]
async fn follow_up_must_be_another_existing_work_order() {
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let task = add_task(&db, wo_id, "Inspect the bearing", true).await;
    let with_follow_up = |id: i64| CompleteTaskInput {
        follow_up_work_order_id: Some(id),
        ..result("nok")
    };
    let r = tasks::complete_task_with(&db, task.id, actor, with_follow_up(wo_id)).await;
    assert_eq!(refused(r), task_codes::FOLLOW_UP_INVALID, "same work order");
    let r = tasks::complete_task_with(&db, task.id, actor, with_follow_up(9_999_999)).await;
    assert_eq!(refused(r), task_codes::FOLLOW_UP_INVALID, "missing work order");
    let cancelled = second_wo(&db).await;
    set_wo_status(&db, cancelled, "cancelled").await;
    let r = tasks::complete_task_with(&db, task.id, actor, with_follow_up(cancelled)).await;
    assert_eq!(refused(r), task_codes::FOLLOW_UP_INVALID, "cancelled work order");
}

#[tokio::test]
async fn optional_task_keeps_all_results() {
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    for code in ["ok", "nok", "na", "deferred", "cancelled"] {
        let task = add_task(&db, wo_id, &format!("Optional {code}"), false).await;
        let done = tasks::complete_task_with(&db, task.id, actor, result(code))
            .await
            .expect(code);
        assert_eq!(done.result_code.as_deref(), Some(code));
    }
}

#[tokio::test]
async fn task_result_refused_outside_execution() {
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let task = add_task(&db, wo_id, "Torque the bolts", true).await;
    for status in ["planning", "ready", "completed", "closed", "cancelled"] {
        set_wo_status(&db, wo_id, status).await;
        let r = tasks::complete_task_with(&db, task.id, actor, result("ok")).await;
        assert_eq!(refused(r), task_codes::WRONG_STATUS, "{status}");
    }
    // On hold: recorded, and the WO stays on hold.
    set_wo_status(&db, wo_id, "on_hold").await;
    tasks::complete_task_with(&db, task.id, actor, result("ok"))
        .await
        .expect("recorded during a hold");
    assert_eq!(
        text(
            &db,
            "SELECT s.code FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id WHERE w.id = ?",
            vec![wo_id.into()],
        )
        .await
        .as_deref(),
        Some("on_hold")
    );
}

#[tokio::test]
async fn completed_task_is_not_completed_again_but_reopened_with_its_history() {
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let task = add_task(&db, wo_id, "Measure the vibration", true).await;
    let first = tasks::complete_task_with(
        &db,
        task.id,
        actor,
        CompleteTaskInput {
            notes: Some("4.1 mm/s".into()),
            finding_note: Some("Above the limit".into()),
            ..result("nok")
        },
    )
    .await
    .expect("first result");
    let r = tasks::complete_task_with(&db, task.id, actor, result("ok")).await;
    assert_eq!(refused(r), task_codes::ALREADY_COMPLETED);

    // Reopening needs a comment, and only while the WO is executed.
    let r = tasks::reopen_task(&db, task.id, actor, "  ".into()).await;
    assert_eq!(refused(r), task_codes::REOPEN_COMMENT_REQUIRED);
    set_wo_status(&db, wo_id, "completed").await;
    let r = tasks::reopen_task(&db, task.id, actor, "Measured again".into()).await;
    assert_eq!(refused(r), task_codes::WRONG_STATUS);
    set_wo_status(&db, wo_id, "in_progress").await;

    let reopened = tasks::reopen_task(&db, task.id, actor, "Measured again after balancing".into())
        .await
        .expect("reopen");
    assert!(!reopened.is_completed);
    assert_eq!(reopened.result_code, None);
    assert_eq!(reopened.finding_note, None);
    let r = tasks::reopen_task(&db, task.id, actor, "again".into()).await;
    assert_eq!(refused(r), task_codes::NOT_COMPLETED);

    // The previous result stays in the timeline and the audit trail.
    let params = text(
        &db,
        "SELECT summary_params_json FROM work_order_execution_events \
         WHERE work_order_id = ? AND event_type = 'task_reopened'",
        vec![wo_id.into()],
    )
    .await
    .expect("timeline event");
    let params: serde_json::Value = serde_json::from_str(&params).expect("json");
    assert_eq!(params["previous_result"], "nok");
    assert_eq!(params["previous_notes"], "4.1 mm/s");
    assert_eq!(params["previous_finding_note"], "Above the limit");
    assert_eq!(
        params["previous_completed_at"],
        first.completed_at.clone().expect("time")
    );
    assert_eq!(params["comment"], "Measured again after balancing");
    assert!(params["previous_completed_by"].is_string());
    let audit = text(
        &db,
        "SELECT details_json FROM wo_change_events WHERE wo_id = ? AND action = 'task_reopened'",
        vec![wo_id.into()],
    )
    .await
    .expect("audit row");
    assert!(audit.contains("Above the limit"), "{audit}");

    let again = tasks::complete_task_with(&db, task.id, actor, result("ok"))
        .await
        .expect("recorded again");
    assert_eq!(again.result_code.as_deref(), Some("ok"));
}

#[tokio::test]
async fn tool_marks_refused_outside_execution() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let tool = tools::add_tool(
        &db,
        AddToolInput {
            wo_id,
            tool_label: "Torque wrench".into(),
            tool_code: None,
            origin: Some("planned".into()),
            notes: None,
        },
    )
    .await
    .expect("tool");
    for status in ["planning", "completed", "closed", "cancelled"] {
        set_wo_status(&db, wo_id, status).await;
        assert_eq!(
            refused(tools::mark_tool_used(&db, tool.id).await),
            tool_codes::WRONG_STATUS,
            "{status}"
        );
        assert_eq!(
            refused(tools::mark_tool_not_used(&db, tool.id).await),
            tool_codes::WRONG_STATUS,
            "{status}"
        );
    }
    set_wo_status(&db, wo_id, "on_hold").await;
    assert_eq!(
        tools::mark_tool_used(&db, tool.id).await.expect("on hold").usage_status,
        "used"
    );
}

#[tokio::test]
async fn completion_gate_counts_legacy_cancelled_mandatory_as_open() {
    let db = setup().await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let task = add_task(&db, wo_id, "Legacy mandatory step", true).await;
    // Closed cancelled before the rule (or received from a PC that has not upgraded).
    exec(
        &db,
        "UPDATE work_order_tasks SET is_completed = 1, result_code = 'cancelled' WHERE id = ?",
        vec![task.id.into()],
    )
    .await;
    let gates = crate::wo::execution::collect_completion_gates(&db, wo_id)
        .await
        .expect("gates");
    let tasks_gate = gates.iter().find(|g| g.code == "TASKS").expect("TASKS gate");
    assert!(!tasks_gate.passed);
    assert_eq!(tasks_gate.detail.as_deref(), Some("Legacy mandatory step"));
    // NOK without evidence: open too; with a finding: closed.
    exec(
        &db,
        "UPDATE work_order_tasks SET result_code = 'nok' WHERE id = ?",
        vec![task.id.into()],
    )
    .await;
    assert_eq!(tasks::open_mandatory_tasks(&db, wo_id).await.expect("open").len(), 1);
    exec(
        &db,
        "UPDATE work_order_tasks SET finding_note = 'Seal leaking' WHERE id = ?",
        vec![task.id.into()],
    )
    .await;
    assert!(tasks::open_mandatory_tasks(&db, wo_id).await.expect("open").is_empty());
}

#[tokio::test]
async fn close_refuses_open_mandatory_task() {
    use super::closeout_tests::tests::advance_to_completed_verified;
    use crate::wo::closeout::{self, WoCloseInput};
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    let rv = advance_to_completed_verified(&db, wo_id, rv).await;
    // A mandatory task left open on the completed WO (received from another PC, or legacy).
    exec(
        &db,
        "INSERT INTO work_order_tasks (work_order_id, task_description, sequence_order, is_mandatory, is_completed, \
         result_code, origin, entity_sync_id) VALUES (?, 'Test the interlock', 9, 1, 1, 'deferred', 'planned', 'task-open-close')",
        vec![wo_id.into()],
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
        Err(AppError::ValidationFailed(errors)) => {
            assert!(
                errors
                    .iter()
                    .any(|e| e.starts_with("GATE_CLOSEOUT_TASKS_OPEN") && e.contains("Test the interlock")),
                "{errors:?}"
            );
        }
        other => panic!("close must be refused, got {other:?}"),
    }
    exec(
        &db,
        "UPDATE work_order_tasks SET result_code = 'ok' WHERE entity_sync_id = 'task-open-close'",
        vec![],
    )
    .await;
    let closed = closeout::close_wo(
        &db,
        WoCloseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            ..Default::default()
        },
    )
    .await
    .expect("closed once the task is closed OK");
    assert_eq!(closed.status_code.as_deref(), Some("closed"));
}

#[tokio::test]
async fn verifier_without_responsible_must_not_have_recorded_labour() {
    use super::closeout_tests::tests::{create_second_user, seed_rams_for_complete};
    use crate::wo::closeout::{self, SaveVerificationInput};
    use crate::wo::execution::{self, WoMechCompleteInput};
    use crate::wo::labor::{self, AddLaborInput};
    let db = setup().await;
    let actor = admin_id(&db).await;
    let other = create_second_user(&db).await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    labor::add_labor_entry(
        &db,
        AddLaborInput {
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
    crate::wo::parts::confirm_no_parts_used(&db, wo_id, actor)
        .await
        .expect("no parts");
    let rv = seed_rams_for_complete(&db, wo_id).await;
    let wo = execution::complete_wo_mechanically(
        &db,
        WoMechCompleteInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            actual_end: None,
            actual_duration_hours: None,
            conclusion: None,
        },
    )
    .await
    .expect("completed");
    exec(
        &db,
        "UPDATE work_orders SET primary_responsible_id = NULL WHERE id = ?",
        vec![wo_id.into()],
    )
    .await;
    let verify = |by: i64| SaveVerificationInput {
        wo_id,
        verified_by_id: by,
        result: "pass".into(),
        return_to_service_confirmed: true,
        recurrence_risk_level: None,
        notes: None,
        expected_row_version: wo.row_version,
    };
    // The user who recorded labour.
    let r = closeout::save_verification(&db, verify(actor)).await;
    assert_eq!(refused(r), "GATE_CLOSEOUT_VERIFIER_RECORDED_LABOUR");
    // The person (personnel register) who recorded labour, through the verifier's account.
    exec(
        &db,
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('T-D7', 'Samia Haddad', 'available')",
        vec![],
    )
    .await;
    exec(
        &db,
        "INSERT INTO work_order_interveners (work_order_id, personnel_id, started_at, ended_at, hours_worked) \
         VALUES (?, (SELECT id FROM personnel WHERE employee_code = 'T-D7'), '2026-04-10T08:00:00Z', \
                 '2026-04-10T09:00:00Z', 1.0)",
        vec![wo_id.into()],
    )
    .await;
    exec(
        &db,
        "UPDATE user_accounts SET personnel_id = (SELECT id FROM personnel WHERE employee_code = 'T-D7') WHERE id = ?",
        vec![other.into()],
    )
    .await;
    let r = closeout::save_verification(&db, verify(other)).await;
    assert_eq!(refused(r), "GATE_CLOSEOUT_VERIFIER_RECORDED_LABOUR");
    // Someone who did not work on it: accepted.
    exec(
        &db,
        "UPDATE user_accounts SET personnel_id = NULL WHERE id = ?",
        vec![other.into()],
    )
    .await;
    closeout::save_verification(&db, verify(other))
        .await
        .expect("an independent verifier");
}

#[tokio::test]
async fn labour_is_booked_and_closed_only_during_execution() {
    use crate::wo::labor::{self, labor_codes, AddLaborInput};
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, _) = create_wo_in_progress(&db).await;
    let booking = || AddLaborInput {
        wo_id,
        intervener_id: actor,
        skill_id: None,
        started_at: Some("2026-04-10T08:00:00Z".into()),
        ended_at: None,
        hours_worked: None,
        hourly_rate: None,
        notes: None,
    };
    let open = labor::add_labor_entry(&db, booking())
        .await
        .expect("booked in progress");
    for status in ["planning", "ready", "completed", "closed", "cancelled"] {
        set_wo_status(&db, wo_id, status).await;
        assert_eq!(
            refused(labor::add_labor_entry(&db, booking()).await),
            labor_codes::WRONG_STATUS,
            "{status}"
        );
        assert_eq!(
            refused(labor::close_labor_entry(&db, open.id, "2026-04-10T10:00:00Z".into(), actor).await),
            labor_codes::WRONG_STATUS,
            "{status}"
        );
    }
    set_wo_status(&db, wo_id, "on_hold").await;
    labor::close_labor_entry(&db, open.id, "2026-04-10T10:00:00Z".into(), actor)
        .await
        .expect("closed during a hold");
}

/// N3 (lead decision): cancelling a WO closes its labour still open at the cancel time, audited.
#[tokio::test]
async fn cancel_closes_open_labour_at_the_cancel_time() {
    use crate::wo::labor::{self, AddLaborInput};
    let db = setup().await;
    let actor = admin_id(&db).await;
    let (wo_id, rv) = create_wo_in_progress(&db).await;
    let open = labor::add_labor_entry(
        &db,
        AddLaborInput {
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
    let wo = crate::wo::queries::cancel_work_order(
        &db,
        crate::wo::domain::WoCancelInput {
            id: wo_id,
            expected_row_version: rv,
            actor_id: actor,
            cancel_reason: "Equipment replaced instead".into(),
        },
    )
    .await
    .expect("cancelled");
    let cancelled_at = text(
        &db,
        "SELECT cancelled_at FROM work_orders WHERE id = ?",
        vec![wo_id.into()],
    )
    .await
    .expect("cancelled_at");
    assert_eq!(wo.status_code.as_deref(), Some("cancelled"));
    assert_eq!(
        text(
            &db,
            "SELECT ended_at FROM work_order_interveners WHERE id = ?",
            vec![open.id.into()]
        )
        .await,
        Some(cancelled_at)
    );
    let notes = text(
        &db,
        "SELECT notes FROM work_order_interveners WHERE id = ?",
        vec![open.id.into()],
    )
    .await
    .expect("note");
    assert!(notes.contains("annulation"), "{notes}");
    assert!(
        text(
            &db,
            "SELECT details_json FROM wo_change_events WHERE wo_id = ? AND action = 'labor_closed_by_cancellation'",
            vec![wo_id.into()],
        )
        .await
        .is_some(),
        "audited"
    );
}
