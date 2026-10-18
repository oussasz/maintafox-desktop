//! Command-level tests of the reliability study workspace on a migrated in-memory database.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::pm::domain::{CreatePmPlanInput, CreatePmPlanVersionInput};
use crate::reliability::study::analysis::life_analysis;
use crate::reliability::study::decisions::{
    apply_decision, approve_decision, create_decision, inbound_self_approval_check, list_decisions, reject_decision,
    InboundDecisionVerdict, SELF_APPROVAL_SETTING,
};
use crate::reliability::study::domain::{
    CreateDecisionInput, CreateStudyInput, DecideDecisionInput, FleetViewFilter, LifeAnalysisRequest,
    SaveStudyCostsInput, SetStudyRecordInput, UpdateStudyInput,
};
use crate::reliability::study::fleet_view::fleet_view;
use crate::reliability::study::life::{analyse_life, LifeInputs};
use crate::reliability::study::records::build_dataset;
use crate::reliability::study::store::{
    append_record_event, create_study, save_costs, update_study, upsert_cost_profile,
};
use crate::reliability::weibull_fit::WeibullObservation;

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

fn close(actual: f64, expected: f64, tol: f64) {
    assert!((actual - expected).abs() <= tol, "expected {expected}, got {actual}");
}

struct Fixture {
    db: DatabaseConnection,
    p101: i64,
    p102: i64,
    class_id: i64,
    mode_id: i64,
    admin: i64,
    /// A second person: approves what the admin proposes (separation of duties).
    reviewer: i64,
}

const ELIGIBLE: &str = r#"{"eligible_unplanned_mtbf":true,"eligible_for_strict_mtbf":true}"#;
const NOT_ELIGIBLE: &str = r#"{"eligible_unplanned_mtbf":false,"eligible_for_strict_mtbf":false}"#;

async fn failure(
    db: &DatabaseConnection,
    tag: &str,
    eq: i64,
    failed_at: &str,
    restored_at: Option<&str>,
    mode: Option<i64>,
    source_wo: Option<i64>,
    downtime: f64,
) {
    let restored = restored_at.map_or("NULL".to_string(), |r| format!("'{r}'"));
    let mode_sql = mode.map_or("NULL".to_string(), |m| m.to_string());
    let flags = if mode.is_some() { ELIGIBLE } else { NOT_ELIGIBLE };
    let (src_type, src_id) = source_wo.map_or_else(
        || {
            (
                "manual",
                "(SELECT COALESCE(MAX(id), 0) + 100000 FROM failure_events)".to_string(),
            )
        },
        |w| ("work_order", w.to_string()),
    );
    exec(
        db,
        &format!(
            "INSERT INTO failure_events (entity_sync_id, source_type, source_id, equipment_id, detected_at, failed_at, \
             restored_at, downtime_duration_hours, active_repair_hours, waiting_hours, is_planned, failure_mode_id, \
             failure_cause_id, cause_not_determined, verification_status, eligible_flags_json, row_version, created_at, \
             updated_at) VALUES ('st-fe-{tag}', '{src_type}', {src_id}, {eq}, '{failed_at}', '{failed_at}', {restored}, \
             {downtime}, {}, 0, 0, {mode_sql}, NULL, 0, 'recorded', '{flags}', 1, '{failed_at}', '{failed_at}')",
            downtime / 2.0
        ),
    )
    .await;
}

async fn setup() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    // FMECA rating grids (Données de référence), as startup integrity seeds them.
    crate::reliability::study::scales::ensure_fmeca_rating_scales(&db)
        .await
        .expect("FMECA rating grids");
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'st-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'st-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, ancestor_path, depth, \
         created_at, updated_at, structure_model_id) VALUES (1, 'st-node', 'SITE-ST', 'Usine', 1, 'active', '/1/', 0, \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment_classes (entity_sync_id, code, name, parent_id, level, is_active, created_at, updated_at) \
         VALUES ('st-class-pump', 'ST_PUMP', 'Pompe centrifuge', NULL, 'class', 1, datetime('now'), datetime('now'))",
    ] {
        exec(&db, sql).await;
    }
    // Failure modes are governed reference values (domain WORK.FAILURE_MODES, migrations 114 / 178).
    for (code, label) in [("ST_BRD", "Rupture roulement"), ("ST_ELP", "Fuite externe")] {
        crate::wo::closeout_reference_catalog::ensure_value(&db, "WORK.FAILURE_MODES", code, label, None, None, 90)
            .await
            .expect("failure mode");
    }
    let class_id = scalar(&db, "SELECT id FROM equipment_classes WHERE code = 'ST_PUMP'").await;
    for (code, name) in [("P-101", "Pompe de transfert 1"), ("P-102", "Pompe de transfert 2")] {
        exec(
            &db,
            &format!(
                "INSERT INTO equipment (entity_sync_id, asset_id_code, name, class_id, lifecycle_status, \
                 installed_at_node_id, created_at, updated_at) VALUES ('st-{code}', '{code}', '{name}', {class_id}, \
                 'active_in_service', 1, datetime('now'), datetime('now'))"
            ),
        )
        .await;
    }
    exec(
        &db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, \
         is_admin, force_password_change, created_at, updated_at) \
         VALUES ('rs-user-reviewer', 'reviewer', 'Reviewer', 'local', 1, 0, 0, \
         strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
    )
    .await;
    Fixture {
        reviewer: scalar(&db, "SELECT id FROM user_accounts WHERE username = 'reviewer'").await,
        p101: scalar(&db, "SELECT id FROM equipment WHERE asset_id_code = 'P-101'").await,
        p102: scalar(&db, "SELECT id FROM equipment WHERE asset_id_code = 'P-102'").await,
        mode_id: scalar(&db, "SELECT id FROM reference_values WHERE code = 'ST_BRD'").await,
        admin: scalar(&db, "SELECT id FROM user_accounts WHERE username = 'admin'").await,
        class_id,
        db,
    }
}

/// A corrective WO raised from a DI submitted `di_at`; returns the WO id.
async fn wo_from_di(f: &Fixture, code: &str, eq: i64, di_at: &str) -> i64 {
    exec(
        &f.db,
        &format!(
            "INSERT INTO intervention_requests (code, equipment_id, org_node_id, status, title, description, origin_type, \
             impact_level, reported_urgency, submitted_at, submitter_id, created_at, updated_at) VALUES ('DI-{code}', \
             {eq}, 1, 'converted_to_work_order', 'Bruit roulement', 'desc', 'operator', 'unknown', 'medium', '{di_at}', \
             {admin}, '{di_at}', '{di_at}')",
            admin = f.admin
        ),
    )
    .await;
    exec(
        &f.db,
        &format!(
            "INSERT INTO work_orders (code, type_id, status_id, equipment_id, entity_id, title, source_di_id, row_version, \
             created_at, updated_at) VALUES ('{code}', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'corrective'), \
             (SELECT id FROM work_order_statuses WHERE code = 'closed'), {eq}, 1, 'Roulement', \
             (SELECT id FROM intervention_requests WHERE code = 'DI-{code}'), 1, '{di_at}', '{di_at}')"
        ),
    )
    .await;
    scalar(&f.db, &format!("SELECT id FROM work_orders WHERE code = '{code}'")).await
}

async fn pm_wo(f: &Fixture, code: &str, eq: i64, done_at: &str) -> i64 {
    exec(
        &f.db,
        &format!(
            "INSERT INTO work_orders (code, type_id, status_id, equipment_id, entity_id, title, actual_end, closed_at, \
             row_version, created_at, updated_at) VALUES ('{code}', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'preventive'), (SELECT id FROM work_order_statuses WHERE code = 'closed'), {eq}, 1, 'Remplacement roulement', \
             '{done_at}', '{done_at}', 1, '{done_at}', '{done_at}')"
        ),
    )
    .await;
    scalar(&f.db, &format!("SELECT id FROM work_orders WHERE code = '{code}'")).await
}

/// Calendar-time scenario (no run-hour data):
/// P-101: previous failure 2025-01-01 (restored 10:00); F1 2025-03-01 10:00 (DI 8 h earlier,
/// restored 20:00); PM 2025-05-01 20:00; F2 2025-07-01 20:00.
/// P-102: F3 2025-04-01 (no known start); F4 2025-08-01; F5 2025-09-01 without failure mode.
async fn scenario(f: &Fixture) -> i64 {
    let m = Some(f.mode_id);
    failure(
        &f.db,
        "prev",
        f.p101,
        "2025-01-01T00:00:00Z",
        Some("2025-01-01T10:00:00Z"),
        m,
        None,
        10.0,
    )
    .await;
    let wo1 = wo_from_di(f, "OT-2025-0412", f.p101, "2025-03-01T02:00:00Z").await;
    failure(
        &f.db,
        "f1",
        f.p101,
        "2025-03-01T10:00:00Z",
        Some("2025-03-01T20:00:00Z"),
        m,
        Some(wo1),
        10.0,
    )
    .await;
    pm_wo(f, "MP-2025-0310", f.p101, "2025-05-01T20:00:00Z").await;
    failure(&f.db, "f2", f.p101, "2025-07-01T20:00:00Z", None, m, None, 6.0).await;
    failure(&f.db, "f3", f.p102, "2025-04-01T00:00:00Z", None, m, None, 4.0).await;
    failure(&f.db, "f4", f.p102, "2025-08-01T00:00:00Z", None, m, None, 4.0).await;
    failure(&f.db, "f5", f.p102, "2025-09-01T00:00:00Z", None, None, None, 2.0).await;
    let study = create_study(
        &f.db,
        CreateStudyInput {
            title: None,
            equipment_ids: vec![f.p101, f.p102],
            period_start: "2025-02-01T00:00:00Z".into(),
            period_end: "2025-12-31T00:00:00Z".into(),
            failure_mode_id: Some(f.mode_id),
        },
        f.admin,
    )
    .await
    .expect("study");
    assert_eq!(study.title, "P-101, P-102 · Rupture roulement");
    assert_eq!(study.assets.len(), 2);
    study.id
}

fn rec<'a>(
    d: &'a crate::reliability::study::domain::StudyDataset,
    key_prefix: &str,
    tag_at: &str,
) -> &'a crate::reliability::study::domain::StudyRecord {
    d.records
        .iter()
        .find(|r| r.key.starts_with(key_prefix) && r.event_at.starts_with(tag_at))
        .unwrap_or_else(|| panic!("record {key_prefix} at {tag_at}"))
}

#[tokio::test]
async fn dataset_builds_ages_suspensions_and_checks() {
    let f = setup().await;
    let study_id = scenario(&f).await;
    let d = build_dataset(&f.db, study_id).await.expect("dataset").dataset;
    assert_eq!(d.effective_time_basis, "calendar");

    let f1 = rec(&d, "failure:", "2025-03-01");
    close(f1.age_hours.expect("age"), 59.0 * 24.0, 1e-9);
    assert!(f1.included);
    assert_eq!(f1.work_order_code.as_deref(), Some("OT-2025-0412"));
    assert_eq!(f1.di_code.as_deref(), Some("DI-OT-2025-0412"));
    assert!(f1.flags.contains(&"di_earlier".to_string()));
    close(f1.di_lead_hours.expect("lead"), 8.0, 1e-9);

    let pm = rec(&d, "pm:", "2025-05-01");
    assert!(!pm.included, "a preventive WO is a suspension only once confirmed");
    assert_eq!(pm.state_reason.as_deref(), Some("pm_not_confirmed"));
    close(pm.age_hours.expect("age"), 61.0 * 24.0, 1e-9);

    let f2 = rec(&d, "failure:", "2025-07-01");
    close(f2.age_hours.expect("age"), 122.0 * 24.0, 1e-9);

    let f3 = rec(&d, "failure:", "2025-04-01");
    assert!(f3.age_hours.is_none() && !f3.included);
    assert_eq!(f3.state_reason.as_deref(), Some("no_known_start"));
    let f4 = rec(&d, "failure:", "2025-08-01");
    close(
        f4.age_hours.expect("age after an unknown-age repair"),
        122.0 * 24.0,
        1e-9,
    );
    let f5 = rec(&d, "failure:", "2025-09-01");
    assert!(!f5.included && f5.flags.contains(&"mode_missing".to_string()));
    assert_eq!(f5.state_reason.as_deref(), Some("not_eligible"));

    // Still running at the end of the period.
    let end101 = d
        .records
        .iter()
        .find(|r| r.key == format!("end:{}", f.p101))
        .expect("end P-101");
    close(
        end101.age_hours.expect("age"),
        (31 + 31 + 30 + 31 + 30 + 30) as f64 * 24.0 - 20.0,
        1e-9,
    );
    assert!(end101.included);

    assert_eq!((d.failures_used, d.suspensions_used), (3, 2));
    assert_eq!(d.excluded, 3);
    for kind in [
        "di_earlier",
        "mode_missing",
        "no_operating_hours",
        "pm_to_confirm",
        "no_known_start",
    ] {
        assert!(d.checks.iter().any(|c| c.kind == kind), "check {kind}");
    }

    // Analyst choices: use the DI time, confirm the PM removal, exclude F2 with a reason.
    for (key, action, reason) in [
        (f1.key.clone(), "use_di_time", None),
        (pm.key.clone(), "include", Some("Roulement remplacé")),
        (f2.key.clone(), "exclude", Some("Défaut de montage")),
    ] {
        append_record_event(
            &f.db,
            &SetStudyRecordInput {
                study_id,
                record_key: key,
                action: action.into(),
                reason: reason.map(str::to_string),
            },
            f.admin,
        )
        .await
        .expect("choice");
    }
    let refused = append_record_event(
        &f.db,
        &SetStudyRecordInput {
            study_id,
            record_key: f4.key.clone(),
            action: "exclude".into(),
            reason: Some("  ".into()),
        },
        f.admin,
    )
    .await;
    assert!(matches!(refused, Err(AppError::ValidationFailed(m)) if m[0] == "STUDY_RECORD_REASON_REQUIRED"));

    let d = build_dataset(&f.db, study_id).await.expect("dataset").dataset;
    let f1 = rec(&d, "failure:", "2025-03-01T02");
    assert_eq!(f1.event_time_source, "di");
    close(f1.age_hours.expect("age"), 59.0 * 24.0 - 8.0, 1e-9);
    let pm = rec(&d, "pm:", "2025-05-01");
    assert!(pm.included && pm.state_reason.is_none());
    let f2 = rec(&d, "failure:", "2025-07-01");
    assert!(!f2.included);
    assert_eq!(f2.state_reason.as_deref(), Some("analyst"));
    assert_eq!(f2.analyst_note.as_deref(), Some("Défaut de montage"));
    // F2 is ignored: the end-of-period suspension counts from the PM removal.
    let end101 = d
        .records
        .iter()
        .find(|r| r.key == format!("end:{}", f.p101))
        .expect("end P-101");
    close(end101.age_hours.expect("age"), 244.0 * 24.0 - 20.0, 1e-9);
    assert_eq!(d.history.len(), 3);
    assert_eq!(d.history[0].reason, "Défaut de montage", "latest first");
}

#[tokio::test]
async fn run_hours_meter_gives_operating_ages() {
    let f = setup().await;
    exec(
        &f.db,
        &format!(
            "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit_value_id, current_reading, \
             is_primary, is_active, created_at, updated_at) VALUES ('st-meter', {}, 'Compteur', 'HOURS', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'H'), 0, 1, 1, \
             datetime('now'), datetime('now'))",
            f.p101
        ),
    )
    .await;
    let meter = scalar(
        &f.db,
        "SELECT id FROM equipment_meters WHERE entity_sync_id = 'st-meter'",
    )
    .await;
    // 12 h of running per day.
    for (day, reading) in [
        ("2025-01-01", 0.0),
        ("2025-03-01", 708.0),
        ("2025-07-01", 2172.0),
        ("2025-12-31", 4368.0),
    ] {
        exec(
            &f.db,
            &format!(
                "INSERT INTO asset_meter_readings (meter_id, reading_value, reading_at, source_type, quality_flag, \
                 created_at) VALUES ({meter}, {reading}, '{day}T00:00:00Z', 'manual', 'accepted', datetime('now'))"
            ),
        )
        .await;
    }
    let m = Some(f.mode_id);
    failure(&f.db, "a", f.p101, "2025-01-01T00:00:00Z", None, m, None, 1.0).await;
    failure(&f.db, "b", f.p101, "2025-03-01T00:00:00Z", None, m, None, 1.0).await;
    failure(&f.db, "c", f.p101, "2025-07-01T00:00:00Z", None, m, None, 1.0).await;
    let study = create_study(
        &f.db,
        CreateStudyInput {
            title: Some("Roulements P-101".into()),
            equipment_ids: vec![f.p101],
            period_start: "2025-02-01T00:00:00Z".into(),
            period_end: "2025-12-31T00:00:00Z".into(),
            failure_mode_id: m,
        },
        f.admin,
    )
    .await
    .expect("study");
    let d = build_dataset(&f.db, study.id).await.expect("dataset").dataset;
    assert_eq!(d.effective_time_basis, "operating");
    assert_eq!(d.clocks[0].source, "run_hours_meter");
    close(rec(&d, "failure:", "2025-03-01").age_hours.expect("age"), 708.0, 1e-9);
    close(
        rec(&d, "failure:", "2025-07-01").age_hours.expect("age"),
        2172.0 - 708.0,
        1e-9,
    );

    // Forcing calendar time switches every age to wall-clock hours.
    update_study(
        &f.db,
        UpdateStudyInput {
            id: study.id,
            expected_row_version: study.row_version,
            title: None,
            period_start: None,
            period_end: None,
            failure_mode_id: None,
            clear_failure_mode: false,
            time_basis: Some("calendar".into()),
            competing_modes: None,
            pm_plan_id: None,
            clear_pm_plan: false,
            equipment_ids: None,
            status: None,
        },
    )
    .await
    .expect("update");
    let d = build_dataset(&f.db, study.id).await.expect("dataset").dataset;
    assert_eq!(d.effective_time_basis, "calendar");
    close(
        rec(&d, "failure:", "2025-03-01").age_hours.expect("age"),
        59.0 * 24.0,
        1e-9,
    );
}

/// Six failures at known calendar ages on one pump; the command reproduces the direct fit and
/// prices the replacement interval with the class default, then the study's own costs.
async fn life_fixture(f: &Fixture) -> i64 {
    let m = Some(f.mode_id);
    // Renewal 2025-01-01, then failures after 40, 55, 70, 85, 100, 115 days of calendar time.
    failure(&f.db, "l0", f.p101, "2025-01-01T00:00:00Z", None, m, None, 8.0).await;
    let mut day = chrono::NaiveDate::from_ymd_opt(2025, 1, 1).expect("date");
    for (i, gap) in [40_i64, 55, 70, 85, 100, 115].iter().enumerate() {
        day += chrono::Duration::days(*gap);
        failure(
            &f.db,
            &format!("l{}", i + 1),
            f.p101,
            &format!("{day}T00:00:00Z"),
            None,
            m,
            None,
            8.0,
        )
        .await;
    }
    create_study(
        &f.db,
        CreateStudyInput {
            title: None,
            equipment_ids: vec![f.p101],
            period_start: "2025-01-02T00:00:00Z".into(),
            period_end: day.and_hms_opt(0, 0, 0).expect("t").and_utc().to_rfc3339(),
            failure_mode_id: m,
        },
        f.admin,
    )
    .await
    .expect("study")
    .id
}

#[tokio::test]
async fn life_analysis_matches_the_direct_fit_and_resolves_costs() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    let ages: Vec<WeibullObservation> = [40.0, 55.0, 70.0, 85.0, 100.0, 115.0]
        .iter()
        .map(|d| WeibullObservation::failure(d * 24.0))
        .collect();
    let direct = analyse_life(&LifeInputs {
        observations: ages,
        ..LifeInputs::default()
    });
    let req = |planned: Option<f64>, failure: Option<f64>| LifeAnalysisRequest {
        study_id,
        planned_cost: planned,
        failure_cost: failure,
        pm_plan_id: None,
    };
    let (a, _) = life_analysis(&f.db, req(None, None)).await.expect("analysis");
    let (fit, want) = (a.fit.as_ref().expect("fit"), direct.fit.as_ref().expect("direct fit"));
    assert_eq!(
        (a.n_failures, a.n_suspensions),
        (6, 0),
        "period ends on the last failure"
    );
    close(fit.beta, want.beta, 1e-12);
    close(fit.eta, want.eta, 1e-9);
    assert_eq!(a.cost_source, "missing");
    assert!(a.cost.is_none());
    assert_eq!(a.effective_time_basis, "calendar");
    close(a.operating_hours_per_year.expect("hours per year"), 8766.0, 1e-9);

    upsert_cost_profile(&f.db, f.class_id, 1_000.0, 9_000.0, "DZD", f.admin)
        .await
        .expect("profile");
    let (a, _) = life_analysis(&f.db, req(None, None)).await.expect("analysis");
    assert_eq!(a.cost_source, "class_default");
    assert_eq!(a.cost_class_name.as_deref(), Some("Pompe centrifuge"));
    let c = a.cost.as_ref().expect("cost");
    close(c.planned_cost, 1_000.0, 0.0);
    assert!(
        c.proposed.is_some(),
        "wear-out data with a 9:1 cost ratio has an optimum"
    );

    let study = crate::reliability::study::store::get_study(&f.db, study_id)
        .await
        .expect("study");
    save_costs(
        &f.db,
        SaveStudyCostsInput {
            study_id,
            expected_row_version: study.row_version,
            planned_cost: 2_000.0,
            failure_cost: 12_000.0,
            currency_code: "dzd".into(),
            save_as_class_default_for: None,
        },
        f.admin,
    )
    .await
    .expect("costs");
    let (a, _) = life_analysis(&f.db, req(None, None)).await.expect("analysis");
    assert_eq!(a.cost_source, "study");
    assert_eq!(a.cost_currency_code.as_deref(), Some("DZD"));
    let (a, _) = life_analysis(&f.db, req(Some(500.0), Some(800.0)))
        .await
        .expect("analysis");
    assert_eq!(a.cost_source, "input");
    close(a.cost.as_ref().expect("cost").failure_cost, 800.0, 0.0);
}

#[tokio::test]
async fn decision_creates_a_pm_revision_with_the_new_interval_then_a_work_order() {
    let f = setup().await;
    let study_id = life_fixture(&f).await;
    let plan = crate::pm::queries::create_pm_plan(
        &f.db,
        CreatePmPlanInput {
            code: "PM-ST-014".into(),
            title: "Roulement pompe".into(),
            description: None,
            asset_scope_type: "equipment".into(),
            asset_scope_id: Some(f.p101),
            strategy_type: "fixed".into(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("plan");
    crate::pm::queries::create_pm_plan_version(
        &f.db,
        plan.id,
        CreatePmPlanVersionInput {
            effective_from: "2025-01-01T00:00:00Z".into(),
            effective_to: None,
            trigger_definition_json: r#"{"interval_unit":"week","interval_value":36}"#.into(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".into()),
            required_tools_json: None,
            estimated_duration_hours: Some(3.0),
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("version 1");

    let (a, _) = life_analysis(
        &f.db,
        LifeAnalysisRequest {
            study_id,
            ..LifeAnalysisRequest::default()
        },
    )
    .await
    .expect("analysis");
    assert_eq!(a.selected_pm_plan_id, Some(plan.id));
    let candidate = a.pm_plans.iter().find(|p| p.pm_plan_id == plan.id).expect("candidate");
    close(candidate.interval_hours.expect("interval"), 36.0 * 168.0, 1e-9);

    // The pumps' node is a planning site: the decision stores it when proposed.
    make_site(&f, 1).await;
    let decision = create_decision(
        &f.db,
        CreateDecisionInput {
            study_id,
            title: "Remplacer le roulement toutes les 16 semaines".into(),
            action_kind: "pm_interval".into(),
            equipment_id: None,
            pm_plan_id: Some(plan.id),
            proposed_interval_hours: Some(16.0 * 168.0),
            wo_type_code: None,
            approver_id: None,
            fmeca_item_id: None,
            rcm_decision_id: None,
            justification: Some("Usure nette".into()),
            planned_cost: Some(1_000.0),
            failure_cost: Some(9_000.0),
        },
        f.admin,
    )
    .await
    .expect("decision");
    assert_eq!(decision.status, "proposed");
    assert_eq!(decision.proposed_trigger_interval_value, Some(16.0));
    assert_eq!(decision.proposed_trigger_interval_unit.as_deref(), Some("week"));
    assert_eq!(decision.evidence.n_failures, 6);
    assert!(decision.evidence.beta.is_some() && decision.evidence.current_interval.is_some());
    assert_eq!(decision.effect.state, "not_applied");

    let early = apply_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: decision.id,
            expected_row_version: decision.row_version,
            note: None,
        },
        f.admin,
    )
    .await;
    assert!(matches!(early, Err(AppError::ValidationFailed(m)) if m[0] == "DECISION_NOT_APPROVED"));
    // Four-eyes: the proposer never approves their own decision.
    let own_input = || DecideDecisionInput {
        decision_id: decision.id,
        expected_row_version: decision.row_version,
        note: Some("OK".into()),
    };
    let refusal = |r: Result<_, AppError>| match r {
        Err(AppError::ValidationFailed(m)) => m[0].clone(),
        other => panic!("expected a refusal, got {other:?}"),
    };
    // The equipment loses its planning site after the proposal: four-eyes applies and the
    // refusal says why (the stricter of the stored and the current site).
    exec(&f.db, "UPDATE org_node_types SET can_own_work = 0").await;
    assert_eq!(
        refusal(approve_decision(&f.db, own_input(), f.admin).await),
        "DECISION_SELF_APPROVAL_SITE_UNKNOWN"
    );
    // The pumps' node becomes a planning site (no rule set there: four-eyes). With nobody else
    // holding the approve right, the refusal says how to fix it.
    make_site(&f, 1).await;
    assert_eq!(
        refusal(approve_decision(&f.db, own_input(), f.admin).await),
        "DECISION_NO_SECOND_APPROVER"
    );
    grant_ram_manage(&f, f.reviewer).await;
    assert_eq!(
        refusal(approve_decision(&f.db, own_input(), f.admin).await),
        "DECISION_SELF_APPROVAL"
    );
    assert_eq!(decision.proposed_by_id, Some(f.admin));
    assert!(!decision.site_allows_self_approval);
    let approved = approve_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: decision.id,
            expected_row_version: decision.row_version,
            note: Some("OK".into()),
        },
        f.reviewer,
    )
    .await
    .expect("approve");
    assert_eq!(approved.status, "approved");
    assert!(!approved.self_approved, "approved by a second person");
    assert_eq!(approved.decided_by_id, Some(f.reviewer));
    let applied = apply_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: decision.id,
            expected_row_version: approved.row_version,
            note: None,
        },
        f.admin,
    )
    .await
    .expect("apply");
    assert_eq!(applied.status, "applied");
    assert_eq!(applied.linked_pm_version_no, Some(2));
    assert_eq!(applied.effect.state, "not_yet_measured");
    let trig: String =
        f.db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT trigger_definition_json || '|' || source_type FROM pm_plan_versions WHERE id = ?",
            [applied.linked_pm_plan_version_id.expect("version").into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get_by_index(0)
        .expect("s");
    let (json, source) = trig.split_once('|').expect("pair");
    let v: serde_json::Value = serde_json::from_str(json).expect("json");
    assert_eq!(
        (v["interval_unit"].as_str(), v["interval_value"].as_f64()),
        (Some("week"), Some(16.0))
    );
    assert_eq!(source, "reliability_study_decision");

    // A one-off work order on the pump, rejected first, then recorded again and applied.
    let wo_input = || CreateDecisionInput {
        study_id,
        title: "Contrôle d'alignement P-101".into(),
        action_kind: "one_off_wo".into(),
        equipment_id: Some(f.p101),
        pm_plan_id: None,
        proposed_interval_hours: None,
        wo_type_code: Some("corrective".into()),
        approver_id: None,
        fmeca_item_id: None,
        rcm_decision_id: None,
        justification: None,
        planned_cost: None,
        failure_cost: None,
    };
    let d = create_decision(&f.db, wo_input(), f.admin).await.expect("wo decision");
    let missing_reason = reject_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: d.id,
            expected_row_version: d.row_version,
            note: None,
        },
        f.admin,
    )
    .await;
    assert!(matches!(missing_reason, Err(AppError::ValidationFailed(m)) if m[0] == "DECISION_REJECT_REASON_REQUIRED"));
    let self_designated = create_decision(
        &f.db,
        CreateDecisionInput {
            approver_id: Some(f.admin),
            ..wo_input()
        },
        f.admin,
    )
    .await;
    assert!(matches!(self_designated, Err(AppError::ValidationFailed(m)) if m[0] == "DECISION_SELF_APPROVER"));
    let d = create_decision(&f.db, wo_input(), f.admin).await.expect("wo decision");
    let d = approve_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: d.id,
            expected_row_version: d.row_version,
            note: None,
        },
        f.reviewer,
    )
    .await
    .expect("approve");
    let d = apply_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: d.id,
            expected_row_version: d.row_version,
            note: None,
        },
        f.admin,
    )
    .await
    .expect("apply wo");
    assert!(d.linked_work_order_code.is_some());
    let all = list_decisions(&f.db, study_id).await.expect("list");
    assert_eq!(all.len(), 3);

    // The pumps' site opts out: the proposer may name themselves approver and approve; the record
    // says so.
    grant_ram_manage(&f, f.admin).await;
    set_opt_out(&f, 1, true, None).await;
    let mine = create_decision(
        &f.db,
        CreateDecisionInput {
            approver_id: Some(f.admin),
            ..wo_input()
        },
        f.admin,
    )
    .await
    .expect("self-designated under the site opt-out");
    assert!(mine.site_allows_self_approval);
    let mine = approve_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: mine.id,
            expected_row_version: mine.row_version,
            note: None,
        },
        f.admin,
    )
    .await
    .expect("self-approval under the site opt-out");
    assert!(mine.self_approved);

    // A self-approval arriving by synchronisation follows the same site rule.
    let study_sync: String = text(
        &f,
        &format!("SELECT entity_sync_id FROM reliability_studies WHERE id = {study_id}"),
    )
    .await;
    let pump_sync: String = text(
        &f,
        &format!("SELECT entity_sync_id FROM equipment WHERE id = {}", f.p101),
    )
    .await;
    let payload = |decider: &str, study: &str| {
        serde_json::json!({
            "entity_sync_id": "rsd-remote-1",
            "title": "Contrôle d'alignement",
            "status": "approved",
            "study_sync_id": study,
            "equipment_sync_id": pump_sync,
            "proposed_by_sync_id": "user-a",
            "decided_by_sync_id": decider,
        })
        .as_object()
        .cloned()
        .expect("object")
    };
    let db = &f.db;
    let inbound = |p: serde_json::Map<String, serde_json::Value>| async move {
        inbound_self_approval_check(
            db,
            p.get("entity_sync_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            &p,
        )
        .await
        .expect("check")
    };
    let is_conflict = |v: &Option<InboundDecisionVerdict>, needle: &str| matches!(v, Some(InboundDecisionVerdict::Conflict(m)) if m.contains(needle));
    assert!(
        inbound(payload("user-a", &study_sync)).await.is_none(),
        "site opted out"
    );

    // Back to four-eyes (audited): the stored self-approval keeps its label, derived at read.
    set_opt_out(&f, 1, false, Some(1)).await;
    let audit = text(
        &f,
        &format!(
            "SELECT COALESCE(old_value_json, '-') || '>' || new_value_json || '|' || changed_by_id || '|' || \
                    required_step_up || '|' || reason || '|' || (changed_at IS NOT NULL) \
             FROM site_setting_events WHERE site_node_id = 1 AND setting_key = '{SELF_APPROVAL_SETTING}' \
             ORDER BY id DESC LIMIT 1"
        ),
    )
    .await;
    assert_eq!(
        audit,
        format!("true>false|{}|1|Single reliability engineer on this site|1", f.admin)
    );
    let listed = list_decisions(&f.db, study_id).await.expect("list");
    assert!(listed
        .iter()
        .any(|d| d.id == mine.id && d.self_approved && !d.site_allows_self_approval));
    assert_eq!(listed.iter().filter(|d| d.self_approved).count(), 1);
    // A stale row version is refused (someone changed the rule meanwhile).
    let stale = crate::settings::site::set_site_setting(
        &f.db,
        &crate::settings::site::SetSiteSettingInput {
            site_node_id: 1,
            key: SELF_APPROVAL_SETTING.into(),
            value: true,
            reason: "Single reliability engineer on this site".into(),
            expected_row_version: Some(1),
        },
        i32::try_from(f.admin).expect("id"),
        true,
    )
    .await;
    assert!(matches!(stale, Err(AppError::ValidationFailed(m)) if m[0] == "SITE_SETTING_STALE"));

    assert!(is_conflict(&inbound(payload("user-a", &study_sync)).await, "four-eyes"));
    assert!(
        inbound(payload("user-b", &study_sync)).await.is_none(),
        "approved by a second person"
    );
    // A study not applied here yet: the row waits (missing parent), it is not judged.
    assert!(matches!(
        inbound(payload("user-a", "unknown-study")).await,
        Some(InboundDecisionVerdict::Defer(_))
    ));
    // A local id instead of a sync id is not trusted.
    let mut numeric = payload("user-a", &study_sync);
    numeric.insert("study_sync_id".into(), serde_json::json!(study_id));
    assert!(matches!(inbound(numeric).await, Some(InboundDecisionVerdict::Defer(_))));
    // Still proposed but naming its proposer as decider: refused.
    let mut proposed = payload("user-a", &study_sync);
    proposed.insert("status".into(), "proposed".into());
    assert!(is_conflict(&inbound(proposed).await, "still proposed"));
    // Rejected by its proposer (a withdrawal): accepted, as on this device.
    let mut withdrawn = payload("user-a", &study_sync);
    withdrawn.insert("status".into(), "rejected".into());
    assert!(inbound(withdrawn).await.is_none());

    // "Already held": the self-approval stored here before (under the opt-out) comes back.
    let mine_sync = text(
        &f,
        &format!(
            "SELECT entity_sync_id FROM reliability_study_decisions WHERE id = {}",
            mine.id
        ),
    )
    .await;
    let admin_sync = text(
        &f,
        &format!("SELECT entity_sync_id FROM user_accounts WHERE id = {}", f.admin),
    )
    .await;
    let held = |status: &str, equipment: &str| {
        serde_json::json!({
            "entity_sync_id": mine_sync,
            "title": "Contrôle d'alignement",
            "status": status,
            "study_sync_id": study_sync,
            "equipment_sync_id": equipment,
            "proposed_by_sync_id": admin_sync,
            "decided_by_sync_id": admin_sync,
        })
        .as_object()
        .cloned()
        .expect("object")
    };
    assert!(
        inbound(held("approved", &pump_sync)).await.is_none(),
        "same approval, same parties"
    );
    assert!(inbound(held("applied", &pump_sync)).await.is_none(), "further along");
    let other_pump = text(
        &f,
        &format!("SELECT entity_sync_id FROM equipment WHERE id = {}", f.p102),
    )
    .await;
    assert!(
        is_conflict(&inbound(held("approved", &other_pump)).await, "four-eyes"),
        "a held approval does not cover another equipment"
    );
    // Parties left out of the payload are taken from the local row.
    let mut partial = held("approved", &other_pump);
    partial.remove("proposed_by_sync_id");
    partial.remove("decided_by_sync_id");
    assert!(is_conflict(&inbound(partial).await, "four-eyes"));

    // The real wire format, through the materializer: the stored self-approval as this device
    // would send it comes back unchanged; the same row naming another equipment is a conflict.
    let layout = crate::sync::row_layout::layout(&f.db, "reliability_study_decisions")
        .await
        .expect("layout");
    let wire = crate::sync::row_codec::load_row_payload(
        &f.db,
        &layout,
        &crate::sync::row_codec::RowLocator::Identity(mine_sync.clone()),
    )
    .await
    .expect("payload")
    .expect("row");
    assert!(wire.payload.contains_key("proposed_by_sync_id") && wire.payload.contains_key("study_sync_id"));
    let replayed = crate::sync::row_codec::apply_row(
        &f.db,
        &layout,
        &wire.entity_sync_id,
        "upsert",
        &wire.payload_json().expect("json"),
    )
    .await
    .expect("apply");
    assert!(
        !matches!(replayed, crate::sync::materialize::ApplyOutcome::Conflict { .. }),
        "a held self-approval replayed is not refused: {replayed:?}"
    );
    let mut moved = wire.payload.clone();
    moved.insert("equipment_sync_id".into(), other_pump.clone().into());
    let refused = crate::sync::row_codec::apply_row(
        &f.db,
        &layout,
        &wire.entity_sync_id,
        "upsert",
        &serde_json::to_string(&moved).expect("json"),
    )
    .await
    .expect("apply");
    assert!(
        matches!(refused, crate::sync::materialize::ApplyOutcome::Conflict { ref code, .. } if code == "DECISION_SELF_APPROVAL"),
        "{refused:?}"
    );
}

#[tokio::test]
async fn the_decision_site_comes_from_the_resolved_equipment_and_the_strictest_site_applies() {
    let f = setup().await;
    let study_id = scenario(&f).await;
    make_site(&f, 1).await;
    grant_ram_manage(&f, f.admin).await;
    grant_ram_manage(&f, f.reviewer).await;
    set_opt_out(&f, 1, true, None).await;
    // P-102 leaves every site: a work order on P-101 still resolves to site 1 (no false
    // "site unknown" for a study spanning equipment without a site).
    exec(
        &f.db,
        &format!("UPDATE equipment SET installed_at_node_id = NULL WHERE id = {}", f.p102),
    )
    .await;
    let input = |equipment: i64| CreateDecisionInput {
        study_id,
        title: "Contrôle".into(),
        action_kind: "one_off_wo".into(),
        equipment_id: Some(equipment),
        pm_plan_id: None,
        proposed_interval_hours: None,
        wo_type_code: Some("corrective".into()),
        approver_id: Some(f.admin),
        fmeca_item_id: None,
        rcm_decision_id: None,
        justification: None,
        planned_cost: None,
        failure_cost: None,
    };
    let d = create_decision(&f.db, input(f.p101), f.admin)
        .await
        .expect("self-designated at site 1");
    assert!(d.site_allows_self_approval);
    let on_p102 = create_decision(&f.db, input(f.p102), f.admin).await;
    assert!(
        matches!(on_p102, Err(AppError::ValidationFailed(ref m)) if m[0] == "DECISION_SELF_APPROVAL_SITE_UNKNOWN"),
        "{on_p102:?}"
    );
    // Moving P-101 out of its site after the proposal does not lift four-eyes: the strictest of
    // the stored site (1, opted out) and the current one (none) applies.
    exec(
        &f.db,
        &format!("UPDATE equipment SET installed_at_node_id = NULL WHERE id = {}", f.p101),
    )
    .await;
    let refused = approve_decision(
        &f.db,
        DecideDecisionInput {
            decision_id: d.id,
            expected_row_version: d.row_version,
            note: None,
        },
        f.admin,
    )
    .await;
    assert!(
        matches!(refused, Err(AppError::ValidationFailed(ref m)) if m[0] == "DECISION_SELF_APPROVAL_SITE_UNKNOWN"),
        "{refused:?}"
    );
    let stored: i64 = scalar(
        &f.db,
        &format!(
            "SELECT site_recorded FROM reliability_study_decisions WHERE id = {}",
            d.id
        ),
    )
    .await;
    assert_eq!(stored, 1);
}

#[tokio::test]
async fn site_rules_are_validated_audited_and_fail_safe() {
    let f = setup().await;
    make_site(&f, 1).await;
    let set = |site: i64, key: &str, reason: &str| crate::settings::site::SetSiteSettingInput {
        site_node_id: site,
        key: key.into(),
        value: true,
        reason: reason.into(),
        expected_row_version: None,
    };
    let code = |r: AppResult<()>| match r {
        Err(AppError::ValidationFailed(m)) => m[0].clone(),
        other => panic!("expected a refusal, got {other:?}"),
    };
    let admin = i32::try_from(f.admin).expect("id");
    let reason = "Single reliability engineer on this site";
    assert_eq!(
        code(
            crate::settings::site::set_site_setting(&f.db, &set(1, SELF_APPROVAL_SETTING, "short"), admin, true).await
        ),
        "SETTING_REASON_REQUIRED"
    );
    assert_eq!(
        code(crate::settings::site::set_site_setting(&f.db, &set(1, "no.such.rule", reason), admin, true).await),
        "SITE_SETTING_UNKNOWN"
    );
    assert_eq!(
        code(
            crate::settings::site::set_site_setting(&f.db, &set(999_999, SELF_APPROVAL_SETTING, reason), admin, true)
                .await
        ),
        "SITE_SETTING_NOT_A_SITE"
    );

    // An unreadable stored value reads as four-eyes and is flagged, it never breaks the screens.
    exec(
        &f.db,
        &format!(
            "INSERT INTO site_settings (entity_sync_id, site_node_id, setting_key, value_json) \
             VALUES ('ss-bad', 1, '{SELF_APPROVAL_SETTING}', 'yes please')"
        ),
    )
    .await;
    assert_eq!(
        crate::settings::site::site_setting(&f.db, 1, SELF_APPROVAL_SETTING)
            .await
            .expect("read"),
        None
    );
    let views = crate::settings::site::list_site_settings(&f.db, SELF_APPROVAL_SETTING)
        .await
        .expect("list");
    assert!(views
        .iter()
        .any(|v| v.site_node_id == 1 && v.value_unreadable && v.value.is_none()));
    exec(&f.db, "DELETE FROM site_settings WHERE entity_sync_id = 'ss-bad'").await;
}

#[tokio::test]
async fn a_synced_site_opt_out_needs_its_audited_change() {
    use crate::settings::site::{inbound_site_setting_check, inbound_site_setting_event_check, InboundVerdict};
    let f = setup().await;
    make_site(&f, 1).await;
    let site_sync = text(&f, "SELECT entity_sync_id FROM org_nodes WHERE id = 1").await;
    let admin_sync = text(
        &f,
        &format!("SELECT entity_sync_id FROM user_accounts WHERE id = {}", f.admin),
    )
    .await;
    let rule = |value: &str, at: &str, by: &str| {
        serde_json::json!({
            "entity_sync_id": "ss-remote",
            "site_node_sync_id": site_sync,
            "setting_key": SELF_APPROVAL_SETTING,
            "value_json": value,
            "updated_by_sync_id": by,
            "updated_at": at,
        })
        .as_object()
        .cloned()
        .expect("object")
    };
    let verdict = |p: serde_json::Map<String, serde_json::Value>| {
        let db = &f.db;
        async move { inbound_site_setting_check(db, &p).await.expect("check") }
    };
    let conflict_code = |v: &Option<InboundVerdict>| match v {
        Some(InboundVerdict::Conflict { code, .. }) => *code,
        other => panic!("expected a conflict, got {other:?}"),
    };
    // Turned on with no audit trail anywhere: conflict, never applied.
    assert_eq!(
        conflict_code(&verdict(rule("true", "2026-10-17T10:00:00Z", &admin_sync)).await),
        "SITE_SETTING_UNAUDITED"
    );
    // Turning it off (the strict side) is applied.
    assert!(verdict(rule("false", "2026-10-17T10:00:00Z", &admin_sync))
        .await
        .is_none());
    // Unknown rule or unreadable value: conflict.
    let mut unknown = rule("true", "2026-10-17T10:00:00Z", &admin_sync);
    unknown.insert("setting_key".into(), "no.such.rule".into());
    assert_eq!(conflict_code(&verdict(unknown).await), "SITE_SETTING_UNKNOWN");
    assert_eq!(
        conflict_code(&verdict(rule("\"yes\"", "2026-10-17T10:00:00Z", &admin_sync)).await),
        "SITE_SETTING_INVALID"
    );
    // The audit event still in the inbox: the rule waits for it.
    let event_payload = serde_json::json!({
        "entity_sync_id": "sse-remote",
        "site_node_sync_id": site_sync,
        "setting_key": SELF_APPROVAL_SETTING,
        "old_value_json": null,
        "new_value_json": "true",
        "reason": "Single reliability engineer on this site",
        "changed_by_sync_id": admin_sync,
        "required_step_up": 1,
        "changed_at": "2026-10-17T10:00:00Z",
    });
    exec(
        &f.db,
        &format!(
            "INSERT INTO sync_inbox (server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation, \
             row_version, payload_json, payload_hash) VALUES ('b1', 'c1', 'site_setting_events', 'sse-remote', \
             'upsert', 1, '{}', 'h1')",
            event_payload.to_string().replace('\'', "''")
        ),
    )
    .await;
    assert!(matches!(
        verdict(rule("true", "2026-10-17T10:00:00Z", &admin_sync)).await,
        Some(InboundVerdict::Defer(_))
    ));
    // The audit event checks: a relaxing change without step-up or without a reason is refused.
    let mut no_step_up = event_payload.as_object().cloned().expect("object");
    no_step_up.insert("required_step_up".into(), serde_json::json!(0));
    assert!(matches!(
        inbound_site_setting_event_check(&f.db, &no_step_up)
            .await
            .expect("check"),
        Some(InboundVerdict::Conflict { .. })
    ));
    let mut no_reason = event_payload.as_object().cloned().expect("object");
    no_reason.insert("reason".into(), "ok".into());
    assert!(matches!(
        inbound_site_setting_event_check(&f.db, &no_reason)
            .await
            .expect("check"),
        Some(InboundVerdict::Conflict { .. })
    ));
    assert!(
        inbound_site_setting_event_check(&f.db, event_payload.as_object().expect("object"))
            .await
            .expect("check")
            .is_none()
    );
    // With its audited change here (made under step-up by a settings administrator): applied.
    crate::settings::site::set_site_setting(
        &f.db,
        &crate::settings::site::SetSiteSettingInput {
            site_node_id: 1,
            key: SELF_APPROVAL_SETTING.into(),
            value: true,
            reason: "Single reliability engineer on this site".into(),
            expected_row_version: None,
        },
        i32::try_from(f.admin).expect("id"),
        true,
    )
    .await
    .expect("local audited change");
    let at = text(
        &f,
        &format!("SELECT changed_at FROM site_setting_events WHERE setting_key = '{SELF_APPROVAL_SETTING}' ORDER BY id DESC LIMIT 1"),
    )
    .await;
    assert!(
        verdict(rule("true", &at, &admin_sync)).await.is_none(),
        "audited by an administrator"
    );
    // The same change claimed by someone else than its audited author: conflict.
    let reviewer_sync = text(
        &f,
        &format!("SELECT entity_sync_id FROM user_accounts WHERE id = {}", f.reviewer),
    )
    .await;
    assert_eq!(
        conflict_code(&verdict(rule("true", &at, &reviewer_sync)).await),
        "SITE_SETTING_UNAUDITED"
    );
}

/// Makes `node`'s type able to own work: the node is a planning site.
async fn make_site(f: &Fixture, node: i64) {
    exec(
        &f.db,
        &format!("UPDATE org_node_types SET can_own_work = 1 WHERE id = (SELECT node_type_id FROM org_nodes WHERE id = {node})"),
    )
    .await;
}

async fn text(f: &Fixture, sql: &str) -> String {
    f.db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("q")
        .expect("row")
        .try_get_by_index::<String>(0)
        .expect("text")
}

/// Gives `user` a tenant-wide role holding the approve right of reliability decisions.
async fn grant_ram_manage(f: &Fixture, user: i64) {
    let role = scalar(
        &f.db,
        &format!(
            "SELECT rp.role_id FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id              WHERE p.name = '{}' ORDER BY rp.role_id LIMIT 1",
            crate::rbac::permissions::RAM_MANAGE
        ),
    )
    .await;
    exec(
        &f.db,
        &format!(
            "INSERT OR IGNORE INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, \
             created_at, updated_at, row_version) VALUES ('rs-usa-{user}', {user}, {role}, 'tenant', \
             strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)"
        ),
    )
    .await;
}

/// Sets the site's four-eyes opt-out as Settings does (audited).
async fn set_opt_out(f: &Fixture, site: i64, allowed: bool, expected_row_version: Option<i64>) {
    crate::settings::site::set_site_setting(
        &f.db,
        &crate::settings::site::SetSiteSettingInput {
            site_node_id: site,
            key: SELF_APPROVAL_SETTING.into(),
            value: allowed,
            reason: "Single reliability engineer on this site".into(),
            expected_row_version,
        },
        i32::try_from(f.admin).expect("id"),
        true,
    )
    .await
    .expect("site rule");
}

#[tokio::test]
async fn fleet_view_ranks_equipment_by_downtime_with_pooled_kpis() {
    let f = setup().await;
    scenario(&f).await;
    let v = fleet_view(
        &f.db,
        FleetViewFilter {
            period_start: "2025-02-01T00:00:00Z".into(),
            period_end: "2025-12-31T00:00:00Z".into(),
            org_node_id: Some(1),
            equipment_class_id: Some(f.class_id),
            limit: None,
            offset: None,
        },
    )
    .await
    .expect("fleet");
    assert_eq!(v.kpis.equipment_count, 2);
    // P-101: F1 10 h + F2 6 h; P-102: 4 + 4 + 2 h.
    assert_eq!(v.kpis.failure_count, 5);
    close(v.kpis.downtime_hours, 26.0, 1e-9);
    assert_eq!(v.rows[0].asset_code, "P-101");
    close(v.rows[0].downtime_share, 16.0 / 26.0, 1e-12);
    let hours = 333.0 * 24.0;
    close(v.rows[0].mtbf_hours.expect("mtbf"), hours / 2.0, 1e-6);
    close(v.rows[0].mttr_hours.expect("mttr"), 8.0 / 2.0, 1e-12);
    close(v.kpis.mtbf_hours.expect("fleet mtbf"), 2.0 * hours / 5.0, 1e-6);
    close(
        v.kpis.availability_operational.expect("ao"),
        2.0 * hours / (2.0 * hours + 26.0),
        1e-12,
    );
    close(v.kpis.data_quality.expect("dq"), 0.0, 1e-12);
    assert_eq!(v.kpis.records_missing_mode, 1);
    assert_eq!(v.rows[0].data_quality, "no_operating_hours");
    assert!(v.rows[0].last_study_id.is_some());
    assert!(v.equipment_classes.iter().any(|c| c.id == f.class_id));
    assert!(v.org_nodes.iter().any(|n| n.id == 1));

    // Pages of the ranking: the second page holds the second asset; KPIs still cover both.
    let page2 = fleet_view(
        &f.db,
        FleetViewFilter {
            period_start: "2025-02-01T00:00:00Z".into(),
            period_end: "2025-12-31T00:00:00Z".into(),
            org_node_id: Some(1),
            equipment_class_id: Some(f.class_id),
            limit: Some(1),
            offset: Some(1),
        },
    )
    .await
    .expect("page 2");
    assert_eq!((page2.rows.len(), page2.total_rows), (1, 2));
    assert_eq!(page2.rows[0].equipment_id, v.rows[1].equipment_id);
    assert_eq!(page2.kpis.equipment_count, 2);

    let other = fleet_view(
        &f.db,
        FleetViewFilter {
            period_start: "2024-01-01T00:00:00Z".into(),
            period_end: "2024-06-01T00:00:00Z".into(),
            ..FleetViewFilter::default()
        },
    )
    .await
    .expect("empty period");
    assert_eq!(other.kpis.failure_count, 0);
    assert!(other.rows.is_empty() && other.kpis.data_quality.is_none());
}

// ── Part B: FMECA, RCM, system model, competing modes ───────────────────────────────────────

fn update_basis(id: i64, row_version: i64, competing: Option<&str>) -> UpdateStudyInput {
    UpdateStudyInput {
        id,
        expected_row_version: row_version,
        title: None,
        period_start: None,
        period_end: None,
        failure_mode_id: None,
        clear_failure_mode: false,
        time_basis: None,
        competing_modes: competing.map(str::to_string),
        pm_plan_id: None,
        clear_pm_plan: false,
        equipment_ids: None,
        status: None,
    }
}

async fn elp_id(f: &Fixture) -> i64 {
    scalar(&f.db, "SELECT id FROM reference_values WHERE code = 'ST_ELP'").await
}

#[tokio::test]
async fn competing_modes_become_suspensions_of_the_studied_mode() {
    let f = setup().await;
    let study_id = scenario(&f).await;
    let elp = elp_id(&f).await;
    failure(&f.db, "elp", f.p101, "2025-04-01T20:00:00Z", None, Some(elp), None, 3.0).await;
    let d = build_dataset(&f.db, study_id).await.expect("dataset").dataset;
    assert!(
        d.records.iter().all(|r| r.kind != "competing_mode"),
        "ignored by default"
    );
    let study = crate::reliability::study::store::get_study(&f.db, study_id)
        .await
        .expect("study");
    update_study(&f.db, update_basis(study_id, study.row_version, Some("suspend")))
        .await
        .expect("suspend");
    let d = build_dataset(&f.db, study_id).await.expect("dataset").dataset;
    let c = d
        .records
        .iter()
        .find(|r| r.kind == "competing_mode")
        .expect("competing record");
    assert!(c.included);
    close(c.age_hours.expect("age"), 31.0 * 24.0, 1e-9);
    // F2 now counts from the repair of the other mode (2025-04-01 20:00).
    let f2 = rec(&d, "failure:", "2025-07-01");
    close(f2.age_hours.expect("age"), 91.0 * 24.0, 1e-9);
    assert_eq!(d.suspensions_used, 3);
    // The analyst can still exclude it, with a reason.
    append_record_event(
        &f.db,
        &SetStudyRecordInput {
            study_id,
            record_key: c.key.clone(),
            action: "exclude".into(),
            reason: Some("Pas une remise à neuf du roulement".into()),
        },
        f.admin,
    )
    .await
    .expect("exclude");
    let d = build_dataset(&f.db, study_id).await.expect("dataset").dataset;
    close(
        rec(&d, "failure:", "2025-07-01").age_hours.expect("age"),
        122.0 * 24.0,
        1e-9,
    );
}

#[tokio::test]
async fn fmeca_worksheet_proposes_modes_saves_rows_and_feeds_rcm_and_decisions() {
    let f = setup().await;
    let study_id = scenario(&f).await;
    let elp = elp_id(&f).await;
    failure(&f.db, "elp", f.p101, "2025-04-01T20:00:00Z", None, Some(elp), None, 3.0).await;
    let admin = f.admin as i32;
    let ws = crate::reliability::study::fmeca::get_worksheet(&f.db, study_id)
        .await
        .expect("worksheet");
    assert!(
        ws.analysis_id.is_none() && ws.rows.is_empty(),
        "nothing is written on opening"
    );
    let brd = ws
        .proposals
        .iter()
        .find(|p| p.failure_mode_id == f.mode_id)
        .expect("BRD proposed")
        .clone();
    assert_eq!(brd.failures_in_period, 4);
    // 4 failures over 2 equipment × 333 days: history basis (3 failures used: no Weibull fit).
    let eq_years = 2.0 * 333.0 * 24.0 / 8766.0;
    close(brd.occurrence.equipment_years, eq_years, 1e-9);
    close(brd.occurrence.events_per_year, 4.0 / eq_years, 1e-9);
    assert_eq!(brd.occurrence.source, "history");
    assert_eq!(brd.occurrence.score, ws.scales.occurrence_score(4.0 / eq_years));
    assert!(ws
        .proposals
        .iter()
        .any(|p| p.failure_mode_id == elp && p.failures_in_period == 1));

    let ws = crate::reliability::study::fmeca::save_item(
        &f.db,
        crate::reliability::study::fmeca::SaveFmecaItemInput {
            study_id,
            item_id: None,
            expected_row_version: None,
            failure_mode_id: f.mode_id,
            component_label: Some("Roulement côté accouplement".into()),
            functional_failure: None,
            failure_effect: Some("Grippage pompe, arrêt de ligne".into()),
            failure_mechanism_id: None,
            failure_cause_id: None,
            severity: 8,
            occurrence: brd.occurrence.score,
            detectability: 5,
            current_control: None,
            recommended_action: None,
            occurrence_source: "history".into(),
            occurrence_events_per_year: Some(brd.occurrence.events_per_year),
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
        },
        admin,
    )
    .await
    .expect("save item");
    assert!(ws.analysis_id.is_some());
    let row = ws.rows[0].clone();
    assert_eq!(row.rpn, 8 * brd.occurrence.score * 5);
    assert_eq!(row.mode_label.as_deref(), Some("Rupture roulement"));
    assert_eq!(
        row.functional_failure, "Rupture roulement",
        "defaults to the mode label"
    );
    assert!(ws.proposals.iter().all(|p| p.failure_mode_id != f.mode_id));
    let cell = ws
        .matrix
        .iter()
        .find(|c| c.severity_class == row.severity_class && c.occurrence_class == row.occurrence_class)
        .expect("cell");
    assert_eq!(cell.count, 1);
    assert_eq!(cell.zone, row.zone);

    // RCM: pre-filled from the row (G 8 operational, D 5 evident, no fit) + a P-F interval.
    let rcm = crate::reliability::study::rcm::get_worksheet(&f.db, study_id)
        .await
        .expect("rcm");
    let r = &rcm.rows[0];
    assert!(r.saved.is_none() && r.prefilled.failure_evident && r.prefilled.operational_impact);
    assert!(
        !r.prefilled.scheduled_restoration_feasible,
        "no fit: not pre-answered yes"
    );
    let mut answers = r.prefilled.clone();
    answers.on_condition_feasible = true;
    let rcm = crate::reliability::study::rcm::save_decision(
        &f.db,
        crate::reliability::study::rcm::SaveRcmInput {
            study_id,
            fmeca_item_id: row.item_id,
            answers,
            selected_tactic: None,
            justification: None,
            pf_interval_hours: Some(400.0),
        },
        admin,
    )
    .await
    .expect("rcm decision");
    let r = &rcm.rows[0];
    let saved = r.saved.as_ref().expect("saved");
    assert_eq!(saved.selected_tactic, "condition_based");
    assert_eq!(r.inspection_interval_hours, Some(200.0));
    assert!(rcm.rcm_study_id.is_some());

    // Step 6 decision linked to the FMECA row and the RCM decision.
    let d = create_decision(
        &f.db,
        CreateDecisionInput {
            study_id,
            title: "Ronde vibratoire toutes les 200 h".into(),
            action_kind: "one_off_wo".into(),
            equipment_id: Some(f.p101),
            pm_plan_id: None,
            proposed_interval_hours: None,
            wo_type_code: Some("corrective".into()),
            approver_id: None,
            fmeca_item_id: Some(row.item_id),
            rcm_decision_id: Some(saved.decision_id),
            justification: None,
            planned_cost: None,
            failure_cost: None,
        },
        f.admin,
    )
    .await
    .expect("decision");
    assert_eq!(d.evidence.rpn, Some(row.rpn));
    assert_eq!(d.evidence.rcm_tactic.as_deref(), Some("condition_based"));
    assert_eq!(d.fmeca_item_id, Some(row.item_id));
    let ws = crate::reliability::study::fmeca::get_worksheet(&f.db, study_id)
        .await
        .expect("worksheet");
    assert_eq!(ws.rows[0].rcm_tactic.as_deref(), Some("condition_based"));
    assert_eq!(ws.rows[0].decision_status.as_deref(), Some("proposed"));
    let refused = crate::reliability::study::fmeca::delete_item(&f.db, study_id, row.item_id).await;
    assert!(matches!(refused, Err(AppError::ValidationFailed(m)) if m[0] == "FMECA_ITEM_HAS_DECISIONS"));

    // A row whose ratings nobody chose (former root-cause placeholder, migration 221): listed and
    // flagged, but out of the matrix, the RPN ranking, RCM and decisions until a person saves it.
    let analysis = ws.analysis_id.expect("analysis");
    exec(
        &f.db,
        &format!(
            "INSERT INTO fmeca_items (entity_sync_id, analysis_id, functional_failure, failure_mode_id, failure_effect,              severity, occurrence, detectability, rpn, recommended_action, current_control, row_version, updated_at,              review_required) VALUES ('fm-placeholder', {analysis}, 'Desserrage', {mode}, 'Desserrage', 5, 5, 5, 125,              'Address RCA cause: Desserrage', '', 1, datetime('now'), 1)",
            mode = f.mode_id
        ),
    )
    .await;
    let placeholder = scalar(
        &f.db,
        "SELECT id FROM fmeca_items WHERE entity_sync_id = 'fm-placeholder'",
    )
    .await;
    let ws = crate::reliability::study::fmeca::get_worksheet(&f.db, study_id)
        .await
        .expect("worksheet");
    assert_eq!(ws.rows_to_review, 1);
    assert_eq!(ws.rows.len(), 2, "still listed");
    let last = ws.rows.last().expect("row");
    assert!(
        last.review_required && last.item_id == placeholder,
        "ranked after the rated rows"
    );
    assert_eq!(
        ws.matrix.iter().map(|c| c.count).sum::<i64>(),
        1,
        "not counted in the matrix"
    );
    // The equipment FMECA list shows it flagged, after the ranked rows, never as a critical row.
    let equipment = scalar(
        &f.db,
        &format!("SELECT equipment_id FROM fmeca_analyses WHERE id = {analysis}"),
    )
    .await;
    let listed = crate::reliability::advanced_rams::queries::list_fmeca_items_for_equipment(
        &f.db,
        crate::reliability::advanced_rams::domain::FmecaItemsEquipmentFilter {
            equipment_id: equipment,
            severity: None,
            occurrence: None,
            limit: None,
        },
    )
    .await
    .expect("equipment fmeca");
    let tail = listed.last().expect("listed");
    assert!(tail.item.review_required && tail.item.id == placeholder);
    assert_eq!(tail.inventory_status, "not_applicable");
    // The RCM / criticality loop refuses to work on it.
    let refused = crate::reliability::rcm_loop::fmeca_criticality_evidence(&f.db, placeholder).await;
    assert!(
        matches!(&refused, Err(AppError::ValidationFailed(m)) if m[0] == "FMECA_ITEM_REVIEW_REQUIRED"),
        "{refused:?}"
    );
    let rcm = crate::reliability::study::rcm::get_worksheet(&f.db, study_id)
        .await
        .expect("rcm");
    assert_eq!((rcm.rows.len(), rcm.rows_to_review), (1, 1));
    let refused = create_decision(
        &f.db,
        CreateDecisionInput {
            study_id,
            title: "Sur une ligne non cotée".into(),
            action_kind: "one_off_wo".into(),
            equipment_id: Some(f.p101),
            pm_plan_id: None,
            proposed_interval_hours: None,
            wo_type_code: Some("corrective".into()),
            approver_id: None,
            fmeca_item_id: Some(placeholder),
            rcm_decision_id: None,
            justification: None,
            planned_cost: None,
            failure_cost: None,
        },
        f.admin,
    )
    .await;
    assert!(
        matches!(&refused, Err(AppError::ValidationFailed(m)) if m[0] == "FMECA_ITEM_REVIEW_REQUIRED"),
        "{refused:?}"
    );
    // The analyst rates it and saves: it counts again.
    let ws = crate::reliability::study::fmeca::save_item(
        &f.db,
        crate::reliability::study::fmeca::SaveFmecaItemInput {
            study_id,
            item_id: Some(placeholder),
            expected_row_version: Some(1),
            failure_mode_id: f.mode_id,
            component_label: None,
            functional_failure: None,
            failure_effect: Some("Desserrage du palier".into()),
            failure_mechanism_id: None,
            failure_cause_id: None,
            severity: 6,
            occurrence: 3,
            detectability: 4,
            current_control: None,
            recommended_action: None,
            occurrence_source: "analyst".into(),
            occurrence_events_per_year: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
        },
        admin,
    )
    .await
    .expect("review saved");
    assert_eq!(ws.rows_to_review, 0);
    assert!(ws.rows.iter().all(|r| !r.review_required));
    assert_eq!(ws.matrix.iter().map(|c| c.count).sum::<i64>(), 2);

    // A placeholder row synced from a device that had not run migration 221 arrives unflagged:
    // the startup pass flags it (exact signature only, once).
    exec(
        &f.db,
        &format!(
            "INSERT INTO ram_ishikawa_diagrams (entity_sync_id, equipment_id, title, flow_json, created_at, \
             updated_at) VALUES ('ish-synced', {equipment}, 'Ishikawa', '{{}}', datetime('now'), datetime('now'))"
        ),
    )
    .await;
    for (key, action) in [
        ("fm-synced", "Address RCA cause: Jeu"),
        ("fm-rated", "Contrôler le jeu"),
    ] {
        exec(
            &f.db,
            &format!(
                "INSERT INTO fmeca_items (entity_sync_id, analysis_id, functional_failure, failure_mode_id, \
                 failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control, \
                 source_ram_ishikawa_diagram_id, row_version, updated_at) \
                 VALUES ('{key}', {analysis}, 'Jeu', {mode}, 'Jeu', 5, 5, 5, 125, '{action}', '', \
                 (SELECT id FROM ram_ishikawa_diagrams WHERE entity_sync_id = 'ish-synced'), 1, datetime('now'))",
                mode = f.mode_id
            ),
        )
        .await;
    }
    let flagged = crate::reliability::study::fmeca::flag_placeholder_rows(&f.db)
        .await
        .expect("flag");
    assert_eq!(flagged, 1, "the rated row with its own action is left alone");
    assert_eq!(
        crate::reliability::study::fmeca::flag_placeholder_rows(&f.db)
            .await
            .expect("again"),
        0
    );
    assert_eq!(
        scalar(
            &f.db,
            "SELECT review_required FROM fmeca_items WHERE entity_sync_id = 'fm-synced'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn system_model_uses_equipment_history_and_saves_rbd_and_fault_tree() {
    use crate::reliability::study::system_model::{
        get_system_model, save_system_model, FtNode, FtSpec, LifeSource, RbdNode, RbdSpec, SaveSystemModelInput,
    };
    let f = setup().await;
    let study_id = scenario(&f).await;
    let model = get_system_model(&f.db, study_id).await.expect("model");
    assert!(model.rbd.is_none() && model.fta.is_none());
    let p101 = model
        .equipment
        .iter()
        .find(|e| e.equipment_id == f.p101)
        .expect("P-101")
        .clone();
    // P-101: F1 and F2 in the period, 333 days of calendar time.
    assert_eq!(p101.failures, 2);
    close(p101.mtbf_hours.expect("mtbf"), 333.0 * 24.0 / 2.0, 1e-6);
    let block = |id: &str, eq: i64| RbdNode {
        id: id.into(),
        label: id.into(),
        node_type: "block".into(),
        children: vec![],
        k: None,
        source: Some(LifeSource::EquipmentHistory { equipment_id: eq }),
        group: Some("Pompes".into()),
        mttr_hours: None,
    };
    let rbd = RbdSpec {
        root_id: "root".into(),
        nodes: vec![
            RbdNode {
                id: "root".into(),
                label: "Station".into(),
                node_type: "parallel".into(),
                children: vec!["a".into(), "b".into()],
                k: Some(1),
                source: None,
                group: None,
                mttr_hours: None,
            },
            block("a", f.p101),
            block("b", f.p102),
        ],
    };
    let fta = FtSpec {
        top_id: "top".into(),
        nodes: vec![
            FtNode {
                id: "top".into(),
                label: "Perte de transfert".into(),
                node_type: "and".into(),
                inputs: vec!["x".into(), "y".into()],
                source: None,
            },
            FtNode {
                id: "x".into(),
                label: "P-101 en panne".into(),
                node_type: "basic".into(),
                inputs: vec![],
                source: Some(LifeSource::EquipmentHistory { equipment_id: f.p101 }),
            },
            FtNode {
                id: "y".into(),
                label: "P-102 en panne".into(),
                node_type: "basic".into(),
                inputs: vec![],
                source: Some(LifeSource::Rate { per_hour: 0.0005 }),
            },
        ],
    };
    let saved = save_system_model(
        &f.db,
        SaveSystemModelInput {
            study_id,
            mission_hours: 1000.0,
            rbd: Some(rbd.clone()),
            fta: Some(fta.clone()),
        },
        f.admin,
    )
    .await
    .expect("save");
    assert_eq!(saved.mission_hours, Some(1000.0));
    assert_eq!(saved.rbd.as_ref().map(|m| &m.spec), Some(&rbd));
    assert_eq!(saved.fta.as_ref().map(|m| &m.spec), Some(&fta));
    // The stored graph is evaluated by the existing RBD evaluator with the resolved values.
    let result: String =
        f.db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT result_json FROM rbd_models WHERE id = ?",
            [saved.rbd.as_ref().expect("rbd").model_id.into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get_by_index(0)
        .expect("s");
    let v: serde_json::Value = serde_json::from_str(&result).expect("json");
    let ra = (-1000.0 / p101.mtbf_hours.unwrap()).exp();
    let p102 = saved
        .equipment
        .iter()
        .find(|e| e.equipment_id == f.p102)
        .expect("P-102");
    let rb = (-1000.0 / p102.mtbf_hours.unwrap()).exp();
    let expected = 1.0 - (1.0 - ra) * (1.0 - rb);
    let stored = v["evaluation"]["system_reliability"]
        .as_f64()
        .expect("system_reliability");
    close(stored, expected, 1e-9);
    let incomplete = save_system_model(
        &f.db,
        SaveSystemModelInput {
            study_id,
            mission_hours: 1000.0,
            rbd: Some(RbdSpec {
                root_id: "missing".into(),
                nodes: vec![],
            }),
            fta: None,
        },
        f.admin,
    )
    .await;
    assert!(matches!(incomplete, Err(AppError::ValidationFailed(m)) if m[0] == "SYSTEM_MODEL_INCOMPLETE"));
}
