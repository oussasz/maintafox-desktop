//! Inspection rounds: thresholds, evidence rules, automatic anomalies, round lifecycle,
//! runner view and the visible offline queue (PRD §6.25, Phase 6.2); recurring schedules,
//! module figures, anomaly follow-up and checkpoint history (Phase 6.3).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::inspection::domain::{
    AddInspectionPhotoInput, CreateInspectionTemplateInput, GenerateInspectionRoundsInput, InspectionAnomalyRowFilter,
    InspectionCheckpoint, InspectionCheckpointDraft, InspectionKpisInput, InspectionRound, InspectionRoundActionInput,
    InspectionRoundSummaryFilter, PublishInspectionTemplateVersionInput, RecordInspectionResultInput,
    SaveInspectionScheduleInput, ScheduleInspectionRoundInput,
};
use crate::inspection::results::{eval_numeric_status, record_inspection_result};
use crate::inspection::{insights, queries, routing, runner, scheduling};
use crate::sync::row_state::{row_sync_states, ROW_SYNC_ERROR, ROW_SYNC_SYNCED, ROW_SYNC_WAITING};

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'insp-test-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'insp-test-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'insp-test-node', 'SITE-INS', 'Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('insp-eq-1', 'PMP-101', 'Feed pump 101', 'active_in_service', 1, \
         datetime('now'), datetime('now'))",
        "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES ('T-001', 'Amina Tech', 'available')",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("fixture");
    }
    db
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

async fn technician(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM personnel WHERE employee_code = 'T-001'").await
}

async fn admin_user(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id FROM user_accounts WHERE username = 'admin'").await
}

fn draft(seq: i64, code: &str, check_type: &str) -> InspectionCheckpointDraft {
    InspectionCheckpointDraft {
        sequence_order: seq,
        asset_id: None,
        component_id: None,
        checkpoint_code: code.into(),
        description: None,
        check_type: check_type.into(),
        measurement_unit: None,
        normal_min: None,
        normal_max: None,
        warning_min: None,
        warning_max: None,
        requires_photo: None,
        requires_comment_on_exception: None,
    }
}

fn numeric(
    seq: i64,
    code: &str,
    n: (Option<f64>, Option<f64>),
    w: (Option<f64>, Option<f64>),
) -> InspectionCheckpointDraft {
    InspectionCheckpointDraft {
        measurement_unit: Some("°C".into()),
        normal_min: n.0,
        normal_max: n.1,
        warning_min: w.0,
        warning_max: w.1,
        ..draft(seq, code, "numeric")
    }
}

fn cp_from(d: &InspectionCheckpointDraft) -> InspectionCheckpoint {
    InspectionCheckpoint {
        id: 1,
        entity_sync_id: "cp".into(),
        template_version_id: 1,
        sequence_order: d.sequence_order,
        asset_id: None,
        component_id: None,
        checkpoint_code: d.checkpoint_code.clone(),
        description: None,
        check_type: d.check_type.clone(),
        measurement_unit: d.measurement_unit.clone(),
        normal_min: d.normal_min,
        normal_max: d.normal_max,
        warning_min: d.warning_min,
        warning_max: d.warning_max,
        requires_photo: false,
        requires_comment_on_exception: false,
        row_version: 1,
    }
}

/// Template with: bearing temperature (≤ 70 normal, ≤ 85 warning, comment on exception), a
/// visual pass/fail with a mandatory photo, and a boolean guard check. Returns the round.
async fn scheduled_round(db: &DatabaseConnection) -> InspectionRound {
    let asset = scalar_i64(db, "SELECT id FROM equipment WHERE asset_id_code = 'PMP-101'").await;
    let mut temp = numeric(1, "TEMP-BRG", (None, Some(70.0)), (None, Some(85.0)));
    temp.asset_id = Some(asset);
    temp.description = Some("Bearing temperature, drive end".into());
    temp.requires_comment_on_exception = Some(true);
    let mut visual = draft(2, "VIS-LEAK", "pass_fail");
    visual.requires_photo = Some(true);
    let (template, _, _) = queries::create_inspection_template(
        db,
        CreateInspectionTemplateInput {
            code: "RND-PUMPS".into(),
            name: "Pump house round".into(),
            org_scope_id: None,
            route_scope: None,
            estimated_duration_minutes: Some(20),
            is_active: Some(true),
            checkpoints: vec![temp, visual, draft(3, "GUARD", "boolean")],
        },
    )
    .await
    .expect("create template");
    queries::schedule_inspection_round(
        db,
        ScheduleInspectionRoundInput {
            template_id: template.id,
            scheduled_at: Some("2026-10-12T06:00:00Z".into()),
            assigned_to_id: Some(technician(db).await),
            explicit_template_version_id: None,
            assigned_team_id: None,
        },
    )
    .await
    .expect("schedule")
}

async fn checkpoint_id(db: &DatabaseConnection, code: &str) -> i64 {
    scalar_i64(
        db,
        &format!("SELECT id FROM inspection_checkpoints WHERE checkpoint_code = '{code}'"),
    )
    .await
}

fn record(round_id: i64, checkpoint_id: i64) -> RecordInspectionResultInput {
    RecordInspectionResultInput {
        round_id,
        checkpoint_id,
        result_status: None,
        numeric_value: None,
        text_value: None,
        boolean_value: None,
        comment: None,
        expected_row_version: None,
    }
}

fn assert_validation<T: std::fmt::Debug>(res: Result<T, AppError>, needle: &str) {
    match res {
        Err(AppError::ValidationFailed(msgs)) => assert!(
            msgs.iter().any(|m| m.contains(needle)),
            "expected {needle:?} in {msgs:?}"
        ),
        other => panic!("expected ValidationFailed({needle:?}), got {other:?}"),
    }
}

async fn round(db: &DatabaseConnection, id: i64) -> InspectionRound {
    queries::get_inspection_round_by_id(db, id)
        .await
        .expect("load")
        .expect("round")
}

fn action(r: &InspectionRound) -> InspectionRoundActionInput {
    InspectionRoundActionInput {
        round_id: r.id,
        expected_row_version: r.row_version,
        note: None,
    }
}

async fn open_anomaly(db: &DatabaseConnection, where_clause: &str) -> (i64, i64) {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT id, row_version FROM inspection_anomalies WHERE {where_clause} AND resolution_status = 'open'"
            ),
        ))
        .await
        .expect("query")
        .expect("open anomaly");
    (
        row.try_get("", "id").expect("id"),
        row.try_get("", "row_version").expect("rv"),
    )
}

// ── Thresholds ─────────────────────────────────────────────────────────────────────────

#[test]
fn numeric_bands_two_sided_and_one_sided() {
    let two = cp_from(&numeric(1, "P", (Some(2.0), Some(4.0)), (Some(1.0), Some(5.0))));
    assert_eq!(eval_numeric_status(3.0, &two), "pass");
    assert_eq!(eval_numeric_status(4.5, &two), "warning");
    assert_eq!(eval_numeric_status(0.5, &two), "fail");
    assert_eq!(eval_numeric_status(5.0, &two), "warning", "band edges are inclusive");

    // "≤ 70 °C, warning up to 85 °C": one-sided limits were ignored before (always pass).
    let upper = cp_from(&numeric(1, "T", (None, Some(70.0)), (None, Some(85.0))));
    assert_eq!(eval_numeric_status(65.0, &upper), "pass");
    assert_eq!(eval_numeric_status(80.0, &upper), "warning");
    assert_eq!(eval_numeric_status(90.0, &upper), "fail");

    let normal_only = cp_from(&numeric(1, "N", (Some(10.0), None), (None, None)));
    assert_eq!(eval_numeric_status(9.0, &normal_only), "warning");

    let plain = cp_from(&numeric(1, "M", (None, None), (None, None)));
    assert_eq!(eval_numeric_status(1e6, &plain), "pass", "a plain measurement");
}

#[tokio::test]
async fn template_limits_are_validated() {
    let db = setup().await;
    let make = |cp: InspectionCheckpointDraft| CreateInspectionTemplateInput {
        code: format!("T-{}", cp.checkpoint_code),
        name: "Bad".into(),
        org_scope_id: None,
        route_scope: None,
        estimated_duration_minutes: None,
        is_active: None,
        checkpoints: vec![cp],
    };
    let inverted = numeric(1, "INV", (Some(5.0), Some(1.0)), (None, None));
    assert_validation(
        queries::create_inspection_template(&db, make(inverted)).await,
        "greater than",
    );
    let narrow_warning = numeric(1, "NW", (Some(0.0), Some(10.0)), (Some(1.0), Some(12.0)));
    assert_validation(
        queries::create_inspection_template(&db, make(narrow_warning)).await,
        "must contain the normal band",
    );
    let mut limits_on_bool = draft(1, "BOOL", "boolean");
    limits_on_bool.normal_max = Some(1.0);
    assert_validation(
        queries::create_inspection_template(&db, make(limits_on_bool)).await,
        "only allowed on numeric",
    );
}

#[tokio::test]
async fn invalid_escalation_rules_are_rejected_at_publish() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let template = queries::list_inspection_templates(&db)
        .await
        .expect("list")
        .into_iter()
        .find(|t| t.id == r.template_id)
        .expect("template");
    let res = queries::publish_inspection_template_version(
        &db,
        PublishInspectionTemplateVersionInput {
            template_id: template.id,
            expected_row_version: template.row_version,
            effective_from: None,
            requires_review: None,
            tolerance_rules_json: None,
            escalation_rules_json: Some(r#"{"anomaly_on":["broken"]}"#.into()),
            checkpoints: vec![draft(1, "X", "pass_fail")],
        },
    )
    .await;
    assert_validation(res, "unknown result status");
}

// ── Results and automatic anomalies ──────────────────────────────────────────────────────

#[tokio::test]
async fn first_result_starts_the_round_and_breach_creates_one_anomaly() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let temp = checkpoint_id(&db, "TEMP-BRG").await;

    let res = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(90.0),
            comment: Some("Hot to the touch".into()),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("record");
    assert_eq!(res.result_status, "fail");
    let started = round(&db, r.id).await;
    assert_eq!(started.status, "in_progress");
    assert!(started.started_at.is_some());

    // Re-recording a failing value refreshes the anomaly, never duplicates it.
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(91.0),
            comment: Some("Still hot".into()),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("re-record");
    let open = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM inspection_anomalies WHERE result_id = {} AND anomaly_type = 'threshold_breach' \
             AND resolution_status = 'open'",
            res.id
        ),
    )
    .await;
    assert_eq!(open, 1);
}

#[tokio::test]
async fn corrected_reading_cancels_the_open_anomaly_but_keeps_decided_ones() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let temp = checkpoint_id(&db, "TEMP-BRG").await;
    let fail = RecordInspectionResultInput {
        numeric_value: Some(95.0),
        comment: Some("Reading".into()),
        ..record(r.id, temp)
    };
    let res = record_inspection_result(&db, fail.clone(), tech).await.expect("fail");

    // Typo corrected: the open, unrouted anomaly is cancelled (kept for history, not deleted).
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(60.0),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("pass");
    let cancelled = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM inspection_anomalies WHERE result_id = {} AND resolution_status = 'cancelled'",
            res.id
        ),
    )
    .await;
    assert_eq!(cancelled, 1);

    // Failing again creates a new anomaly; once deferred (a decision was taken), a later
    // reading neither deletes nor duplicates it.
    record_inspection_result(&db, fail.clone(), tech)
        .await
        .expect("fail again");
    let (anomaly_id, rv) = open_anomaly(&db, &format!("result_id = {}", res.id)).await;
    routing::defer_inspection_anomaly(&db, anomaly_id, rv)
        .await
        .expect("defer");
    record_inspection_result(&db, fail, tech).await.expect("fail once more");
    let total = scalar_i64(
        &db,
        &format!("SELECT COUNT(*) FROM inspection_anomalies WHERE result_id = {}", res.id),
    )
    .await;
    assert_eq!(total, 2, "cancelled + deferred, no duplicate");
    let deferred = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM inspection_anomalies WHERE id = {anomaly_id} AND resolution_status = 'deferred'"
        ),
    )
    .await;
    assert_eq!(deferred, 1);
}

#[tokio::test]
async fn not_performed_checks_need_a_reason_and_exceptions_need_a_comment() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let visual = checkpoint_id(&db, "VIS-LEAK").await;
    let temp = checkpoint_id(&db, "TEMP-BRG").await;
    assert_validation(
        record_inspection_result(
            &db,
            RecordInspectionResultInput {
                result_status: Some("not_accessible".into()),
                ..record(r.id, visual)
            },
            tech,
        )
        .await,
        "reason",
    );
    assert_validation(
        record_inspection_result(
            &db,
            RecordInspectionResultInput {
                numeric_value: Some(80.0),
                ..record(r.id, temp)
            },
            tech,
        )
        .await,
        "comment or evidence required",
    );
}

// ── Round lifecycle ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn complete_requires_every_answer_and_photo_then_review_requires_decisions() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let started = runner::start_inspection_round(&db, action(&r)).await.expect("start");
    assert_eq!(started.round.status, "in_progress");

    let temp = checkpoint_id(&db, "TEMP-BRG").await;
    let visual = checkpoint_id(&db, "VIS-LEAK").await;
    let guard = checkpoint_id(&db, "GUARD").await;

    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(78.0),
            comment: Some("Slightly warm".into()),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("temp");
    let current = round(&db, r.id).await;
    assert_validation(
        runner::complete_inspection_round(&db, action(&current)).await,
        "unanswered checkpoints",
    );

    let visual_res = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            result_status: Some("pass".into()),
            ..record(r.id, visual)
        },
        tech,
    )
    .await
    .expect("visual");
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            boolean_value: Some(true),
            ..record(r.id, guard)
        },
        tech,
    )
    .await
    .expect("guard");

    // The mandatory photo of VIS-LEAK is still missing.
    let view = runner::get_inspection_round_runner(&db, r.id).await.expect("runner");
    let vis = view
        .checkpoints
        .iter()
        .find(|c| c.checkpoint.checkpoint_code == "VIS-LEAK")
        .expect("vis");
    assert_eq!(vis.missing, vec![runner::MISSING_PHOTO.to_string()]);
    assert_eq!(vis.open_anomalies.len(), 1, "missing-photo anomaly");
    let current = round(&db, r.id).await;
    assert_validation(
        runner::complete_inspection_round(&db, action(&current)).await,
        "VIS-LEAK: photo",
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let ev = runner::add_inspection_photo_evidence(
        &db,
        dir.path(),
        AddInspectionPhotoInput {
            result_id: visual_res.id,
            file_name: "leak.jpg".into(),
            mime_type: "image/jpeg".into(),
            file_bytes: vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3],
        },
    )
    .await
    .expect("photo");
    assert!(dir.path().join(&ev.file_path_or_value).exists(), "file written");
    let media = scalar_i64(
        &db,
        &format!(
            "SELECT COUNT(*) FROM media_attachments WHERE parent_entity_type = 'inspection_evidence' \
             AND parent_entity_sync_id = '{}'",
            ev.entity_sync_id
        ),
    )
    .await;
    assert_eq!(media, 1, "media row so the file syncs");

    let current = round(&db, r.id).await;
    let done = runner::complete_inspection_round(&db, action(&current))
        .await
        .expect("complete");
    assert_eq!(done.round.status, "completed_with_findings", "TEMP-BRG is a warning");
    assert_eq!(done.round.completion_percentage, Some(100.0));
    assert!(done.round.completed_at.is_some());

    // Results are frozen once the round is completed.
    assert_validation(
        record_inspection_result(
            &db,
            RecordInspectionResultInput {
                boolean_value: Some(false),
                ..record(r.id, guard)
            },
            tech,
        )
        .await,
        "can no longer be recorded",
    );

    // Review is refused while the threshold anomaly has no decision.
    let current = round(&db, r.id).await;
    let admin = admin_user(&db).await;
    assert_validation(
        runner::review_inspection_round(&db, action(&current), admin).await,
        "still need a decision",
    );
    let (id, rv) = open_anomaly(&db, &format!("round_id = {}", r.id)).await;
    routing::defer_inspection_anomaly(&db, id, rv).await.expect("defer");
    let current = round(&db, r.id).await;
    let reviewed = runner::review_inspection_round(
        &db,
        InspectionRoundActionInput {
            note: Some("Bearing to be checked at next stop".into()),
            ..action(&current)
        },
        admin,
    )
    .await
    .expect("review");
    assert_eq!(reviewed.round.status, "reviewed");
    assert_eq!(reviewed.round.reviewed_by_id, Some(admin));
    assert!(reviewed.reviewed_by_name.is_some());
}

#[tokio::test]
async fn not_accessible_is_a_finding_and_lowers_completion() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let temp = checkpoint_id(&db, "TEMP-BRG").await;
    let visual = checkpoint_id(&db, "VIS-LEAK").await;
    let guard = checkpoint_id(&db, "GUARD").await;
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(50.0),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("temp");
    let vis = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            result_status: Some("not_accessible".into()),
            comment: Some("Area fenced off".into()),
            ..record(r.id, visual)
        },
        tech,
    )
    .await
    .expect("visual");
    let dir = tempfile::tempdir().expect("tempdir");
    runner::add_inspection_photo_evidence(
        &db,
        dir.path(),
        AddInspectionPhotoInput {
            result_id: vis.id,
            file_name: "fence.png".into(),
            mime_type: "image/png".into(),
            file_bytes: vec![0x89, b'P', b'N', b'G'],
        },
    )
    .await
    .expect("photo");
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            boolean_value: Some(true),
            ..record(r.id, guard)
        },
        tech,
    )
    .await
    .expect("guard");
    let current = round(&db, r.id).await;
    let done = runner::complete_inspection_round(&db, action(&current))
        .await
        .expect("complete");
    assert_eq!(done.round.status, "completed_with_findings");
    let pct = done.round.completion_percentage.expect("pct");
    assert!((pct - 200.0 / 3.0).abs() < 1e-9, "{pct}");
}

#[tokio::test]
async fn photo_evidence_rejects_wrong_types_and_bad_names() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let guard = checkpoint_id(&db, "GUARD").await;
    let res = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            boolean_value: Some(true),
            ..record(r.id, guard)
        },
        tech,
    )
    .await
    .expect("guard");
    let dir = tempfile::tempdir().expect("tempdir");
    let photo = |mime: &str, name: &str| AddInspectionPhotoInput {
        result_id: res.id,
        file_name: name.into(),
        mime_type: mime.into(),
        file_bytes: vec![1, 2, 3],
    };
    assert_validation(
        runner::add_inspection_photo_evidence(&db, dir.path(), photo("application/pdf", "x.pdf")).await,
        "Unsupported photo type",
    );
    // A path is reduced to its file name: nothing is written outside the evidence folder.
    let ev = runner::add_inspection_photo_evidence(&db, dir.path(), photo("image/jpeg", "../../evil.jpg"))
        .await
        .expect("sanitised name");
    assert!(ev.file_path_or_value.starts_with("inspection_evidence/"));
    assert!(!ev.file_path_or_value.contains(".."));
}

// ── Runner view and offline queue ────────────────────────────────────────────────────────

#[tokio::test]
async fn runner_view_resolves_labels_and_summary_counts() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let temp = checkpoint_id(&db, "TEMP-BRG").await;
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(99.0),
            comment: Some("Smoke".into()),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("temp");

    let view = runner::get_inspection_round_runner(&db, r.id).await.expect("runner");
    assert_eq!(view.summary.template_name, "Pump house round");
    assert_eq!(view.summary.assigned_to_name.as_deref(), Some("Amina Tech"));
    assert_eq!(view.summary.checkpoint_count, 3);
    assert_eq!(view.summary.recorded_count, 1);
    assert_eq!(view.summary.exception_count, 1);
    assert_eq!(view.summary.open_anomaly_count, 1);
    let t = &view.checkpoints[0];
    assert_eq!(t.asset_code.as_deref(), Some("PMP-101"));
    assert_eq!(t.asset_name.as_deref(), Some("Feed pump 101"));
    assert_eq!(
        t.checkpoint.description.as_deref(),
        Some("Bearing temperature, drive end")
    );
    assert!(t.missing.is_empty());
    assert_eq!(view.checkpoints[1].sync_state, "not_recorded");
    assert_eq!(view.checkpoints[1].missing, vec![runner::MISSING_RESULT.to_string()]);

    let listed = runner::list_inspection_round_summaries(
        &db,
        InspectionRoundSummaryFilter {
            statuses: Some(vec!["in_progress".into()]),
            ..Default::default()
        },
    )
    .await
    .expect("list");
    assert_eq!(listed.len(), 1);
    assert_validation(
        runner::list_inspection_round_summaries(
            &db,
            InspectionRoundSummaryFilter {
                statuses: Some(vec!["bogus".into()]),
                ..Default::default()
            },
        )
        .await,
        "Unknown round status",
    );
}

#[tokio::test]
async fn row_sync_state_follows_journal_and_outbox() {
    let db = setup().await;
    for sql in [
        "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, operation) \
         VALUES ('inspection_results', 'a', 'upsert')",
        "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, operation, last_error) \
         VALUES ('inspection_results', 'b', 'upsert', 'stage failed')",
        "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, payload_json, \
         payload_hash, status) VALUES ('k1', 'inspection_results', 'c', 'upsert', 1, '{}', 'h1', 'acked')",
        "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, payload_json, \
         payload_hash, status) VALUES ('k2', 'inspection_results', 'c', 'upsert', 2, '{}', 'h2', 'pending')",
        "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, payload_json, \
         payload_hash, status) VALUES ('k3', 'inspection_results', 'd', 'upsert', 1, '{}', 'h3', 'rejected')",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("fixture");
    }
    let ids: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|s| (*s).to_string()).collect();
    let states = row_sync_states(&db, "inspection_results", &ids).await.expect("states");
    assert_eq!(states["a"], ROW_SYNC_WAITING);
    assert_eq!(states["b"], ROW_SYNC_ERROR);
    assert_eq!(states["c"], ROW_SYNC_WAITING, "latest outbox entry decides");
    assert_eq!(states["d"], ROW_SYNC_ERROR);
    assert_eq!(states["e"], ROW_SYNC_SYNCED);
}

// ── Phase 6.3: template gates, readings out of tolerance, history ─────────────────────────

#[tokio::test]
async fn template_rows_are_refused_with_stable_codes() {
    let db = setup().await;
    let make = |checkpoints: Vec<InspectionCheckpointDraft>| CreateInspectionTemplateInput {
        code: "T-GATES".into(),
        name: "Gates".into(),
        org_scope_id: None,
        route_scope: None,
        estimated_duration_minutes: None,
        is_active: None,
        checkpoints,
    };
    assert_validation(
        queries::create_inspection_template(&db, make(vec![])).await,
        "GATE_INSP_TEMPLATE_NO_CHECK",
    );
    assert_validation(
        queries::create_inspection_template(&db, make(vec![draft(1, "A", "pass_fail"), draft(2, "a", "boolean")]))
            .await,
        "GATE_INSP_CHECK_CODE_DUPLICATE: checkpoint 2",
    );
    assert_validation(
        queries::create_inspection_template(&db, make(vec![draft(1, "A", "pass_fail"), draft(2, " ", "boolean")]))
            .await,
        "GATE_INSP_CHECK_CODE_REQUIRED: checkpoint 2",
    );
    assert_validation(
        queries::create_inspection_template(
            &db,
            make(vec![numeric(1, "INV", (Some(5.0), Some(1.0)), (None, None))]),
        )
        .await,
        "GATE_INSP_CHECK_LIMITS_ORDER",
    );
}

/// Template with one plain numeric reading (no "comment on exception" flag) on PMP-101.
async fn pressure_round(db: &DatabaseConnection, code: &str, scheduled_at: &str) -> InspectionRound {
    let asset = scalar_i64(db, "SELECT id FROM equipment WHERE asset_id_code = 'PMP-101'").await;
    let template_id = match queries::list_inspection_templates(db)
        .await
        .expect("list")
        .into_iter()
        .find(|t| t.code == code)
    {
        Some(t) => t.id,
        None => {
            let mut press = numeric(1, "PRESS", (None, Some(7.5)), (None, Some(9.0)));
            press.measurement_unit = Some("bar".into());
            press.asset_id = Some(asset);
            press.description = Some("Discharge pressure".into());
            queries::create_inspection_template(
                db,
                CreateInspectionTemplateInput {
                    code: code.into(),
                    name: "Pressure round".into(),
                    org_scope_id: None,
                    route_scope: None,
                    estimated_duration_minutes: None,
                    is_active: Some(true),
                    checkpoints: vec![press],
                },
            )
            .await
            .expect("template")
            .0
            .id
        }
    };
    queries::schedule_inspection_round(
        db,
        ScheduleInspectionRoundInput {
            template_id,
            scheduled_at: Some(scheduled_at.into()),
            assigned_to_id: Some(technician(db).await),
            explicit_template_version_id: None,
            assigned_team_id: None,
        },
    )
    .await
    .expect("schedule")
}

#[tokio::test]
async fn a_reading_out_of_tolerance_needs_a_comment_and_is_missing_until_given() {
    let db = setup().await;
    let r = pressure_round(&db, "RND-PRESS", "2026-10-12T06:00:00Z").await;
    let tech = technician(&db).await;
    let press = checkpoint_id(&db, "PRESS").await;
    assert_validation(
        record_inspection_result(
            &db,
            RecordInspectionResultInput {
                numeric_value: Some(8.4),
                ..record(r.id, press)
            },
            tech,
        )
        .await,
        "GATE_INSP_READING_COMMENT_REQUIRED",
    );
    // Inside the limits no comment is needed.
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(7.0),
            ..record(r.id, press)
        },
        tech,
    )
    .await
    .expect("in tolerance");
    let res = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(8.4),
            comment: Some("Gauge vibrating".into()),
            ..record(r.id, press)
        },
        tech,
    )
    .await
    .expect("with comment");
    assert_eq!(res.result_status, "warning");
}

#[tokio::test]
async fn the_runner_shows_earlier_results_of_the_same_checkpoint() {
    let db = setup().await;
    let tech = technician(&db).await;
    let first = pressure_round(&db, "RND-PRESS", "2026-10-10T06:00:00Z").await;
    let press = checkpoint_id(&db, "PRESS").await;
    for (value, comment) in [(6.9, None), (8.1, Some("High"))] {
        record_inspection_result(
            &db,
            RecordInspectionResultInput {
                numeric_value: Some(value),
                comment: comment.map(str::to_string),
                ..record(first.id, press)
            },
            tech,
        )
        .await
        .expect("first round reading");
    }
    let second = pressure_round(&db, "RND-PRESS", "2026-10-11T06:00:00Z").await;
    let view = runner::get_inspection_round_runner(&db, second.id).await.expect("runner");
    let history = &view.checkpoints[0].history;
    assert_eq!(history.len(), 1, "one result per round, the latest reading");
    assert_eq!(history[0].round_id, first.id);
    assert_eq!(history[0].numeric_value, Some(8.1));
    assert_eq!(history[0].result_status, "warning");
    let own = runner::get_inspection_round_runner(&db, first.id).await.expect("runner");
    assert!(own.checkpoints[0].history.is_empty(), "a round never lists itself");
}

// ── Phase 6.3: anomaly → request / work order ────────────────────────────────────────────

#[tokio::test]
async fn an_out_of_tolerance_anomaly_becomes_a_linked_request_with_a_written_title() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let admin = admin_user(&db).await;
    let temp = checkpoint_id(&db, "TEMP-BRG").await;
    let res = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(90.0),
            comment: Some("Hot to the touch".into()),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("record");
    let (anomaly_id, rv) = open_anomaly(&db, &format!("result_id = {}", res.id)).await;

    let row = insights::get_inspection_anomaly_row(&db, anomaly_id).await.expect("row");
    assert_eq!(row.asset_code.as_deref(), Some("PMP-101"));
    assert!(row.asset_has_location);
    assert_eq!(row.numeric_value, Some(90.0));
    assert_eq!(row.violation_direction.as_deref(), Some("above"));
    assert_eq!(row.violation_limit, Some(85.0), "beyond the warning band: the warning limit");
    assert_eq!(row.result_comment.as_deref(), Some("Hot to the touch"));
    assert_eq!(row.template_name, "Pump house round");

    assert_validation(
        routing::route_inspection_anomaly_to_di(&db, anomaly_id, rv, admin, Some("  ".into()), None).await,
        "GATE_INSP_FOLLOW_UP_TITLE_REQUIRED",
    );
    let title = "Bearing temperature out of tolerance on PMP-101: 90 °C > 85 °C";
    let (di, anomaly) = routing::route_inspection_anomaly_to_di(
        &db,
        anomaly_id,
        rv,
        admin,
        Some(title.into()),
        Some("Round RND-PUMPS planned 12/10/2026. Reading 90 °C.".into()),
    )
    .await
    .expect("route to DI");
    assert_eq!(di.title, title);
    assert_eq!(di.equipment_id, row.asset_id.expect("asset"));
    assert_eq!(di.source_inspection_anomaly_id, Some(anomaly_id), "the request points back");
    assert!(di.description.contains("Reading 90"));
    assert_eq!(anomaly.linked_di_id, Some(di.id));
    assert_eq!(anomaly.resolution_status, "triaged");

    // The follow-up list links the created request.
    let page = insights::list_inspection_anomaly_rows(
        &db,
        InspectionAnomalyRowFilter {
            round_id: Some(r.id),
            ..Default::default()
        },
    )
    .await
    .expect("rows");
    let linked = page
        .items
        .iter()
        .find(|x| x.anomaly.id == anomaly_id)
        .expect("routed anomaly");
    assert_eq!(linked.linked_di_code.as_deref(), Some(di.code.as_str()));
    assert_eq!(linked.linked_di_title.as_deref(), Some(title));

    assert_validation(
        routing::route_inspection_anomaly_to_di(&db, anomaly_id, anomaly.row_version, admin, Some(title.into()), None)
            .await,
        "GATE_INSP_ANOMALY_ALREADY_ROUTED",
    );
}

#[tokio::test]
async fn a_follow_up_needs_equipment_and_a_work_order_links_back() {
    let db = setup().await;
    let r = scheduled_round(&db).await;
    let tech = technician(&db).await;
    let admin = admin_user(&db).await;
    let wo_type: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT code FROM work_order_types ORDER BY id LIMIT 1".to_string(),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "code")
        .expect("code");

    // VIS-LEAK has no equipment: neither a request nor a work order can be written from it.
    let visual = checkpoint_id(&db, "VIS-LEAK").await;
    let leak = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            result_status: Some("fail".into()),
            ..record(r.id, visual)
        },
        tech,
    )
    .await
    .expect("record");
    let (leak_anomaly, leak_rv) = open_anomaly(
        &db,
        &format!("result_id = {} AND anomaly_type = 'threshold_breach'", leak.id),
    )
    .await;
    assert_eq!(
        insights::get_inspection_anomaly_row(&db, leak_anomaly).await.expect("row").asset_id,
        None
    );
    assert_validation(
        routing::route_inspection_anomaly_to_di(&db, leak_anomaly, leak_rv, admin, Some("Leak".into()), None).await,
        "GATE_INSP_ANOMALY_NO_EQUIPMENT",
    );
    assert_validation(
        routing::route_inspection_anomaly_to_wo(
            &db,
            leak_anomaly,
            leak_rv,
            admin,
            wo_type.clone(),
            Some("Leak".into()),
            None,
        )
        .await,
        "GATE_INSP_ANOMALY_NO_EQUIPMENT",
    );

    // TEMP-BRG is on PMP-101: the work order takes the written title and points back.
    let temp = checkpoint_id(&db, "TEMP-BRG").await;
    let hot = record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(88.0),
            comment: Some("Hot".into()),
            ..record(r.id, temp)
        },
        tech,
    )
    .await
    .expect("record");
    let (anomaly_id, rv) = open_anomaly(&db, &format!("result_id = {}", hot.id)).await;
    assert_validation(
        routing::route_inspection_anomaly_to_wo(&db, anomaly_id, rv, admin, wo_type.clone(), None, None).await,
        "GATE_INSP_FOLLOW_UP_TITLE_REQUIRED",
    );
    let (wo, anomaly) = routing::route_inspection_anomaly_to_wo(
        &db,
        anomaly_id,
        rv,
        admin,
        wo_type,
        Some("Replace drive-end bearing on PMP-101".into()),
        Some("Reading 88 °C, warning limit 85 °C.".into()),
    )
    .await
    .expect("route to WO");
    assert_eq!(wo.title, "Replace drive-end bearing on PMP-101");
    assert_eq!(anomaly.linked_work_order_id, Some(wo.id));
    let source: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT source_inspection_anomaly_id FROM work_orders WHERE id = ?",
            [wo.id.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "source_inspection_anomaly_id")
        .expect("source");
    assert_eq!(source, Some(anomaly_id), "the work order points back");
    let linked = insights::get_inspection_anomaly_row(&db, anomaly_id).await.expect("row");
    assert_eq!(linked.linked_wo_code.as_deref(), Some(wo.code.as_str()));
}

// ── Phase 6.3: recurring rounds ──────────────────────────────────────────────────────────

fn utc(raw: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(raw).expect("ts").with_timezone(&chrono::Utc)
}

#[test]
fn calendar_dates_skip_closed_windows_on_the_first_run_then_continue_after_the_last_round() {
    let anchor = utc("2026-10-01T06:00:00Z");
    let daily = scheduling::schedule_recurrence("day", 1).expect("daily");
    let as_of = utc("2026-10-05T10:00:00Z");
    let grace = chrono::Duration::hours(6);
    let horizon = as_of + chrono::Duration::days(2);
    // First run: the windows of 1–4 Oct closed before 5 Oct 10:00; the 5th is open until 12:00.
    let (dates, cut) = scheduling::calendar_due_dates(anchor, daily, None, as_of, grace, horizon, 100);
    assert!(!cut);
    assert_eq!(
        dates,
        vec![
            utc("2026-10-05T06:00:00Z"),
            utc("2026-10-06T06:00:00Z"),
            utc("2026-10-07T06:00:00Z")
        ]
    );
    // Next run three days later: continues after the last generated date, closed ones included
    // (they are recorded as missed: the application was not running).
    let later = utc("2026-10-10T10:00:00Z");
    let (dates, _) = scheduling::calendar_due_dates(
        anchor,
        daily,
        Some(utc("2026-10-07T06:00:00Z")),
        later,
        grace,
        later + chrono::Duration::days(1),
        100,
    );
    assert_eq!(dates.first(), Some(&utc("2026-10-08T06:00:00Z")));
    assert_eq!(dates.last(), Some(&utc("2026-10-11T06:00:00Z")));
    // The limit cuts and says so.
    let (dates, cut) = scheduling::calendar_due_dates(anchor, daily, None, anchor, grace, horizon, 2);
    assert_eq!(dates.len(), 2);
    assert!(cut);
    // Months follow the calendar (31 Jan → 28 Feb → 31 Mar), as for PM.
    let monthly = scheduling::schedule_recurrence("month", 1).expect("monthly");
    let (dates, _) = scheduling::calendar_due_dates(
        utc("2027-01-31T06:00:00Z"),
        monthly,
        None,
        utc("2027-01-01T00:00:00Z"),
        grace,
        utc("2027-03-31T23:00:00Z"),
        10,
    );
    assert_eq!(
        dates,
        vec![
            utc("2027-01-31T06:00:00Z"),
            utc("2027-02-28T06:00:00Z"),
            utc("2027-03-31T06:00:00Z")
        ]
    );
}

fn schedule_input(template_id: i64, assignee: Option<i64>) -> SaveInspectionScheduleInput {
    SaveInspectionScheduleInput {
        id: None,
        expected_row_version: None,
        code: "SCH-PUMPS".into(),
        name: "Pump house, daily".into(),
        template_id,
        trigger_kind: "calendar".into(),
        interval_unit: Some("day".into()),
        interval_value: Some(1),
        anchor_at: None,
        meter_id: None,
        meter_interval: None,
        meter_start_reading: None,
        assigned_to_id: assignee,
        assigned_team_id: None,
        grace_hours: 6,
        horizon_days: 3,
        is_active: true,
    }
}

#[test]
fn schedule_input_is_validated_with_stable_codes() {
    let ok = SaveInspectionScheduleInput {
        anchor_at: Some("2026-10-15T06:00:00Z".into()),
        ..schedule_input(1, Some(1))
    };
    assert!(scheduling::validate_schedule_input(&ok).is_ok());
    let cases: Vec<(SaveInspectionScheduleInput, &str)> = vec![
        (
            SaveInspectionScheduleInput {
                code: " ".into(),
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_CODE_REQUIRED",
        ),
        (
            SaveInspectionScheduleInput {
                assigned_to_id: None,
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_ASSIGNEE_REQUIRED",
        ),
        (
            SaveInspectionScheduleInput {
                grace_hours: 0,
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_GRACE_RANGE",
        ),
        (
            SaveInspectionScheduleInput {
                horizon_days: 400,
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_HORIZON_RANGE",
        ),
        (
            SaveInspectionScheduleInput {
                interval_unit: Some("fortnight".into()),
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_INTERVAL_REQUIRED",
        ),
        (
            SaveInspectionScheduleInput {
                anchor_at: None,
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_ANCHOR_REQUIRED",
        ),
        (
            SaveInspectionScheduleInput {
                trigger_kind: "meter".into(),
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_METER_REQUIRED",
        ),
        (
            SaveInspectionScheduleInput {
                trigger_kind: "meter".into(),
                meter_id: Some(1),
                meter_interval: Some(0.0),
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_METER_INTERVAL",
        ),
        (
            SaveInspectionScheduleInput {
                trigger_kind: "weekly".into(),
                ..ok.clone()
            },
            "GATE_INSP_SCHEDULE_TRIGGER_INVALID",
        ),
    ];
    for (input, code) in cases {
        assert_validation(scheduling::validate_schedule_input(&input), code);
    }
}

async fn template_id(db: &DatabaseConnection) -> i64 {
    scheduled_round(db).await.template_id
}

async fn schedule_rounds(db: &DatabaseConnection, schedule_id: i64) -> Vec<InspectionRound> {
    let mut rows = runner::list_inspection_round_summaries(
        db,
        InspectionRoundSummaryFilter {
            schedule_id: Some(schedule_id),
            ..Default::default()
        },
    )
    .await
    .expect("rounds")
    .into_iter()
    .map(|s| s.round)
    .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.scheduled_at.cmp(&b.scheduled_at));
    rows
}

#[tokio::test]
async fn a_calendar_schedule_generates_assigned_rounds_once_and_marks_missed_ones() {
    let db = setup().await;
    let template = template_id(&db).await;
    let tech = technician(&db).await;
    let now = chrono::Utc::now();
    // Daily at "now − 50 h": the round of now − 2 h is still in its 6 h window.
    let anchor = now - chrono::Duration::hours(50);
    let saved = scheduling::save_inspection_schedule(
        &db,
        SaveInspectionScheduleInput {
            anchor_at: Some(scheduling::ts(anchor)),
            ..schedule_input(template, Some(tech))
        },
    )
    .await
    .expect("save schedule");
    assert_eq!(saved.template_name, "Pump house round");
    assert_eq!(saved.assigned_to_name.as_deref(), Some("Amina Tech"));
    let rounds = schedule_rounds(&db, saved.schedule.id).await;
    // now − 2 h, + 22 h, + 46 h, + 70 h (3-day look-ahead); closed windows are not back-filled.
    assert_eq!(rounds.len(), 4, "{rounds:?}");
    assert!(rounds.iter().all(|r| r.status == "scheduled"));
    assert!(rounds.iter().all(|r| r.assigned_to_id == Some(tech)));
    assert!(rounds.iter().all(|r| r.due_basis.as_deref().is_some_and(|b| b.starts_with("calendar:"))));
    let first = &rounds[0];
    let due = utc(first.scheduled_at.as_deref().expect("due"));
    assert_eq!(
        utc(first.window_end_at.as_deref().expect("window")),
        due + chrono::Duration::hours(6)
    );
    assert_eq!(saved.open_round_count, 4);
    assert_eq!(saved.overdue_round_count, 1, "the round of now − 2 h");

    // Generating again never duplicates.
    let again = scheduling::generate_inspection_rounds(&db, GenerateInspectionRoundsInput::default())
        .await
        .expect("again");
    assert_eq!(again.generated, 0);
    assert_eq!(again.skipped, 0, "dates after the last generated round only");
    assert_eq!(schedule_rounds(&db, saved.schedule.id).await.len(), 4);

    // Five hours later the first window closed: the unstarted round is missed.
    let later = scheduling::generate_inspection_rounds(
        &db,
        GenerateInspectionRoundsInput {
            as_of: Some(scheduling::ts(now + chrono::Duration::hours(5))),
            schedule_id: None,
        },
    )
    .await
    .expect("later");
    assert_eq!(later.missed, 1);
    assert!(later.issues.is_empty());
    let missed = round(&db, first.id).await;
    assert_eq!(missed.status, "missed");
    assert_eq!(missed.missed_at, first.window_end_at);

    let overdue_page = runner::list_inspection_round_page(
        &db,
        InspectionRoundSummaryFilter {
            overdue_only: Some(true),
            schedule_id: Some(saved.schedule.id),
            ..Default::default()
        },
    )
    .await
    .expect("overdue");
    assert_eq!(overdue_page.total, 0, "a missed round is no longer to do");
}

#[tokio::test]
async fn editing_a_schedule_replaces_its_unstarted_future_rounds() {
    let db = setup().await;
    let template = template_id(&db).await;
    let tech = technician(&db).await;
    let anchor = chrono::Utc::now() + chrono::Duration::hours(1);
    let saved = scheduling::save_inspection_schedule(
        &db,
        SaveInspectionScheduleInput {
            anchor_at: Some(scheduling::ts(anchor)),
            ..schedule_input(template, Some(tech))
        },
    )
    .await
    .expect("save");
    let before = schedule_rounds(&db, saved.schedule.id).await;
    assert_eq!(before.len(), 3, "+1 h, +25 h, +49 h within 3 days");
    // One round was started: it is kept whatever the new rules.
    runner::start_inspection_round(&db, action(&before[0])).await.expect("start");

    assert_validation(
        scheduling::save_inspection_schedule(
            &db,
            SaveInspectionScheduleInput {
                id: Some(saved.schedule.id),
                expected_row_version: Some(saved.schedule.row_version + 7),
                anchor_at: Some(scheduling::ts(anchor)),
                ..schedule_input(template, Some(tech))
            },
        )
        .await,
        "row_version mismatch",
    );
    let edited = scheduling::save_inspection_schedule(
        &db,
        SaveInspectionScheduleInput {
            id: Some(saved.schedule.id),
            expected_row_version: Some(saved.schedule.row_version),
            interval_value: Some(2),
            anchor_at: Some(scheduling::ts(anchor)),
            ..schedule_input(template, Some(tech))
        },
    )
    .await
    .expect("edit");
    let after = schedule_rounds(&db, saved.schedule.id).await;
    assert_eq!(after[0].id, before[0].id, "the started round stays");
    assert_eq!(after[0].status, "in_progress");
    // Every 2 days after the started round: +49 h (the +25 h round was replaced).
    assert_eq!(after.len(), 2, "{after:?}");
    assert_eq!(after[1].scheduled_at, before[2].scheduled_at);
    assert_ne!(after[1].entity_sync_id, before[2].entity_sync_id, "new rules, new identity");
    assert_eq!(edited.schedule.row_version, saved.schedule.row_version + 1);

    // Another schedule cannot take the same code.
    assert_validation(
        scheduling::save_inspection_schedule(
            &db,
            SaveInspectionScheduleInput {
                anchor_at: Some(scheduling::ts(anchor)),
                code: "sch-pumps".into(),
                ..schedule_input(template, Some(tech))
            },
        )
        .await,
        "GATE_INSP_SCHEDULE_CODE_TAKEN",
    );
}

#[tokio::test]
async fn generated_rounds_have_the_same_identity_on_every_device() {
    let a = setup().await;
    let b = setup().await;
    let mut ids = Vec::new();
    for db in [&a, &b] {
        let template = template_id(db).await;
        let tech = technician(db).await;
        // Same schedule identity on both devices (as after sync).
        let saved = scheduling::save_inspection_schedule(
            db,
            SaveInspectionScheduleInput {
                anchor_at: Some("2099-01-01T06:00:00Z".into()),
                is_active: false,
                ..schedule_input(template, Some(tech))
            },
        )
        .await
        .expect("save");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_schedules SET entity_sync_id = 'shared-schedule', is_active = 1 WHERE id = ?",
            [saved.schedule.id.into()],
        ))
        .await
        .expect("align identity");
        scheduling::generate_inspection_rounds(
            db,
            GenerateInspectionRoundsInput {
                as_of: Some("2098-12-31T06:00:00Z".into()),
                schedule_id: Some(saved.schedule.id),
            },
        )
        .await
        .expect("generate");
        ids.push(
            schedule_rounds(db, saved.schedule.id)
                .await
                .into_iter()
                .map(|r| r.entity_sync_id)
                .collect::<Vec<_>>(),
        );
    }
    assert!(!ids[0].is_empty());
    assert_eq!(ids[0], ids[1]);
}

#[tokio::test]
async fn a_meter_schedule_generates_one_round_when_the_meter_crosses_its_interval() {
    let db = setup().await;
    let template = template_id(&db).await;
    let tech = technician(&db).await;
    let asset = scalar_i64(&db, "SELECT id FROM equipment WHERE asset_id_code = 'PMP-101'").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit, current_reading, \
         expected_rate_per_day, is_primary, is_active, created_at, updated_at) \
         VALUES ('insp-meter', ?, 'Run hours', 'HOURS', 'h', 50, 0, 1, 1, datetime('now'), datetime('now'))",
        [asset.into()],
    ))
    .await
    .expect("meter");
    let meter = scalar_i64(&db, "SELECT id FROM equipment_meters WHERE entity_sync_id = 'insp-meter'").await;
    let saved = scheduling::save_inspection_schedule(
        &db,
        SaveInspectionScheduleInput {
            trigger_kind: "meter".into(),
            interval_unit: None,
            interval_value: None,
            meter_id: Some(meter),
            meter_interval: Some(100.0),
            meter_start_reading: Some(0.0),
            ..schedule_input(template, Some(tech))
        },
    )
    .await
    .expect("meter schedule");
    assert_eq!(saved.meter_name.as_deref(), Some("Run hours"));
    assert_eq!(saved.meter_asset_code.as_deref(), Some("PMP-101"));
    assert!(schedule_rounds(&db, saved.schedule.id).await.is_empty(), "50 h < 100 h");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment_meters SET current_reading = 120 WHERE id = ?",
        [meter.into()],
    ))
    .await
    .expect("reading");
    let run = scheduling::generate_inspection_rounds(&db, GenerateInspectionRoundsInput::default())
        .await
        .expect("generate");
    assert_eq!(run.generated, 1);
    let rounds = schedule_rounds(&db, saved.schedule.id).await;
    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0].due_meter_value, Some(100.0));
    assert!(rounds[0].due_basis.as_deref().is_some_and(|b| b.starts_with("meter:")));

    // While that round is open, no other one; once done, the next threshold is 200 h.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment_meters SET current_reading = 230 WHERE id = ?",
        [meter.into()],
    ))
    .await
    .expect("reading");
    let run = scheduling::generate_inspection_rounds(&db, GenerateInspectionRoundsInput::default())
        .await
        .expect("generate");
    assert_eq!(run.generated, 0);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_rounds SET status = 'completed', completed_at = datetime('now') WHERE id = ?",
        [rounds[0].id.into()],
    ))
    .await
    .expect("complete");
    scheduling::generate_inspection_rounds(&db, GenerateInspectionRoundsInput::default())
        .await
        .expect("generate");
    let rounds = schedule_rounds(&db, saved.schedule.id).await;
    assert_eq!(rounds.len(), 2);
    assert!(rounds.iter().any(|r| r.due_meter_value == Some(200.0)));
}

#[tokio::test]
async fn a_schedule_without_a_published_checklist_is_reported_not_fatal() {
    let db = setup().await;
    let template = template_id(&db).await;
    let tech = technician(&db).await;
    let saved = scheduling::save_inspection_schedule(
        &db,
        SaveInspectionScheduleInput {
            anchor_at: Some(scheduling::ts(chrono::Utc::now() + chrono::Duration::hours(1))),
            ..schedule_input(template, Some(tech))
        },
    )
    .await
    .expect("save");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_templates SET is_active = 0 WHERE id = ?",
        [template.into()],
    ))
    .await
    .expect("deactivate");
    let run = scheduling::generate_inspection_rounds(&db, GenerateInspectionRoundsInput::default())
        .await
        .expect("generate");
    assert_eq!(run.issues.len(), 1);
    assert_eq!(run.issues[0].schedule_code, saved.schedule.code);
    assert!(run.issues[0].message.starts_with("GATE_INSP_SCHEDULE_TEMPLATE_NOT_PUBLISHED"));
}

// ── Phase 6.3: module figures and round lists ────────────────────────────────────────────

#[tokio::test]
async fn kpis_count_due_done_on_time_missed_overdue_and_open_anomalies() {
    let db = setup().await;
    let tech = technician(&db).await;
    let now = chrono::Utc::now();
    let ts = |h: i64| scheduling::ts(now - chrono::Duration::hours(h));
    // Planned 10 h ago and done on time, 30 h ago and done late, 20 h ago still to do,
    // 40 h ago missed, 60 days ago (outside a 30-day period).
    let on_time = pressure_round(&db, "RND-PRESS", &ts(10)).await;
    let late = pressure_round(&db, "RND-PRESS", &ts(30)).await;
    let todo = pressure_round(&db, "RND-PRESS", &ts(20)).await;
    let missed = pressure_round(&db, "RND-PRESS", &ts(40)).await;
    let old = pressure_round(&db, "RND-PRESS", &ts(24 * 60)).await;
    for (r, status, completed, window) in [
        (&on_time, "completed", Some(ts(9)), Some(ts(4))),
        (&late, "completed_with_findings", Some(ts(2)), Some(ts(24))),
        (&missed, "missed", None, Some(ts(34))),
        (&old, "reviewed", Some(ts(24 * 60 - 1)), None),
    ] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_rounds SET status = ?, completed_at = ?, window_end_at = ? WHERE id = ?",
            [status.into(), completed.into(), window.into(), r.id.into()],
        ))
        .await
        .expect("set state");
    }
    let press = checkpoint_id(&db, "PRESS").await;
    record_inspection_result(
        &db,
        RecordInspectionResultInput {
            numeric_value: Some(9.5),
            comment: Some("Way above".into()),
            ..record(todo.id, press)
        },
        tech,
    )
    .await
    .expect("anomaly");

    let k = insights::get_inspection_kpis(&db, InspectionKpisInput { period_days: 30 })
        .await
        .expect("kpis");
    assert_eq!(k.due_count, 4);
    assert_eq!(k.done_count, 2);
    assert_eq!(k.done_on_time_count, 1);
    assert_eq!(k.missed_count, 1);
    assert_eq!(k.overdue_count, 1, "the round still to do (now in progress)");
    assert_eq!(k.open_anomaly_count, 1);
    assert_eq!(k.compliance_pct, Some(25.0));
    assert_validation(
        insights::get_inspection_kpis(&db, InspectionKpisInput { period_days: 0 }).await,
        "GATE_INSP_KPI_PERIOD_RANGE",
    );

    // The lists behind the tiles show the same rounds.
    let not_cancelled: Vec<String> = runner::ROUND_STATUSES
        .iter()
        .filter(|s| **s != "cancelled")
        .map(|s| (*s).to_string())
        .collect();
    let due = runner::list_inspection_round_page(
        &db,
        InspectionRoundSummaryFilter {
            statuses: Some(not_cancelled),
            due_from: Some(k.period_start.clone()),
            due_to: Some(k.period_end.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("due list");
    assert_eq!(due.total, k.due_count);
    let overdue = runner::list_inspection_round_page(
        &db,
        InspectionRoundSummaryFilter {
            overdue_only: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("overdue list");
    assert_eq!(overdue.total, k.overdue_count);
    assert!(overdue.items[0].is_overdue);
    let anomalies = insights::list_inspection_anomaly_rows(
        &db,
        InspectionAnomalyRowFilter {
            statuses: Some(vec!["open".into()]),
            ..Default::default()
        },
    )
    .await
    .expect("anomalies");
    assert_eq!(anomalies.total, k.open_anomaly_count);
}

#[tokio::test]
async fn the_round_page_searches_and_counts_beyond_the_page() {
    let db = setup().await;
    for h in 1..=3 {
        pressure_round(&db, "RND-PRESS", &format!("2026-10-1{h}T06:00:00Z")).await;
    }
    scheduled_round(&db).await;
    let page = runner::list_inspection_round_page(
        &db,
        InspectionRoundSummaryFilter {
            search: Some("pressure".into()),
            limit: Some(2),
            ..Default::default()
        },
    )
    .await
    .expect("page");
    assert_eq!(page.total, 3);
    assert_eq!(page.items.len(), 2);
    let next = runner::list_inspection_round_page(
        &db,
        InspectionRoundSummaryFilter {
            search: Some("pressure".into()),
            limit: Some(2),
            offset: Some(2),
            ..Default::default()
        },
    )
    .await
    .expect("next page");
    assert_eq!(next.items.len(), 1);
    let by_person = runner::list_inspection_round_page(
        &db,
        InspectionRoundSummaryFilter {
            search: Some("Amina".into()),
            ..Default::default()
        },
    )
    .await
    .expect("by person");
    assert_eq!(by_person.total, 4);
    assert_validation(
        runner::list_inspection_round_page(
            &db,
            InspectionRoundSummaryFilter {
                due_from: Some("yesterday".into()),
                ..Default::default()
            },
        )
        .await,
        "due_from must be an RFC 3339 timestamp",
    );
}
