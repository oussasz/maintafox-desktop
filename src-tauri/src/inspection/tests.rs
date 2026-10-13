//! Inspection rounds: thresholds, evidence rules, automatic anomalies, round lifecycle,
//! runner view and the visible offline queue (PRD §6.25, Phase 6.2).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::errors::AppError;
use crate::inspection::domain::{
    AddInspectionPhotoInput, CreateInspectionTemplateInput, InspectionCheckpoint, InspectionCheckpointDraft,
    InspectionRound, InspectionRoundActionInput, InspectionRoundSummaryFilter, PublishInspectionTemplateVersionInput,
    RecordInspectionResultInput, ScheduleInspectionRoundInput,
};
use crate::inspection::results::{eval_numeric_status, record_inspection_result};
use crate::inspection::{queries, routing, runner};
use crate::sync::row_state::{row_sync_states, ROW_SYNC_ERROR, ROW_SYNC_SYNCED, ROW_SYNC_WAITING};

async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::connect_in_memory().await.expect("in-memory SQLite");
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA foreign_keys = ON;".to_string(),
    ))
    .await
    .expect("PRAGMA foreign_keys");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db).await.expect("seeder");
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
            limit: None,
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
                limit: None,
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
