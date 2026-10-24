//! RCM / FMECA → PM loop persistence: decision logic record, PM revision and WO with links,
//! criticality number.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::pm::domain::{CreatePmPlanInput, CreatePmPlanVersionInput};
use crate::reliability::rcm_loop::{
    apply_rcm_decision_logic, compute_fmeca_criticality, create_pm_revision_from_source, create_wo_from_source,
    list_rcm_decision_logic, ApplyRcmLogicInput, FmecaCriticalityInput, PmRevisionFromSourceInput, RcmAnswers,
    WoFromSourceInput,
};

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("i64")
}

struct Fixture {
    db: DatabaseConnection,
    plan_id: i64,
    decision_id: i64,
    item_id: i64,
}

async fn setup() -> Fixture {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
         VALUES (1, 'rl-model', 1, 'active', datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, \
         created_at, updated_at) VALUES (1, 'rl-type', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, \
         updated_at, structure_model_id) VALUES (1, 'rl-node', 'SITE-RL', 'Site', 1, 'active', \
         datetime('now'), datetime('now'), 1)",
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
         created_at, updated_at) VALUES ('rl-eq', 'FAN-07', 'Cooling fan', 'active_in_service', 1, \
         datetime('now'), datetime('now'))",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("fixture");
    }
    let eq = scalar(&db, "SELECT id FROM equipment WHERE asset_id_code = 'FAN-07'").await;
    let plan = crate::pm::queries::create_pm_plan(
        &db,
        CreatePmPlanInput {
            code: "PM-FAN".into(),
            title: "Fan inspection".into(),
            description: None,
            asset_scope_type: "equipment".into(),
            asset_scope_id: Some(eq),
            strategy_type: "event".into(),
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
        &db,
        plan.id,
        CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
            trigger_definition_json: r#"{"event_code":"INSPECTION_COMPLETED"}"#.into(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".into()),
            required_tools_json: None,
            estimated_duration_hours: Some(1.5),
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("version 1");

    for sql in [
        format!(
            "INSERT INTO rcm_studies (entity_sync_id, equipment_id, title, created_at, updated_at) \
             VALUES ('rl-study', {eq}, 'Fan RCM', datetime('now'), datetime('now'))"
        ),
        "INSERT INTO rcm_decisions (entity_sync_id, study_id, function_description, functional_failure, \
         consequence_category, selected_tactic, justification, row_version, updated_at) \
         VALUES ('rl-dec', (SELECT id FROM rcm_studies), 'Cool the drive', 'No airflow', '', 'run_to_failure', '', \
         1, datetime('now'))"
            .to_string(),
        format!(
            "INSERT INTO fmeca_analyses (entity_sync_id, equipment_id, title, created_at, updated_at) \
             VALUES ('rl-fa', {eq}, 'Fan FMECA', datetime('now'), datetime('now'))"
        ),
        "INSERT INTO fmeca_items (entity_sync_id, analysis_id, functional_failure, failure_effect, severity, \
         occurrence, detectability, rpn, recommended_action, current_control, row_version, updated_at) \
         VALUES ('rl-item', (SELECT id FROM fmeca_analyses), 'Bearing seizure', 'Motor overheats', 8, 4, 5, 160, \
         'Vibration monitoring', '', 1, datetime('now'))"
            .to_string(),
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .expect("rams fixture");
    }
    Fixture {
        plan_id: plan.id,
        decision_id: scalar(&db, "SELECT id FROM rcm_decisions").await,
        item_id: scalar(&db, "SELECT id FROM fmeca_items").await,
        db,
    }
}

fn answers() -> RcmAnswers {
    RcmAnswers {
        failure_evident: true,
        safety_or_environmental: false,
        operational_impact: true,
        multiple_failure_affects_safety: false,
        on_condition_feasible: true,
        scheduled_restoration_feasible: false,
        scheduled_discard_feasible: false,
        failure_finding_feasible: false,
        combination_feasible: false,
        weibull_beta: None,
    }
}

fn assert_validation<T: std::fmt::Debug>(res: Result<T, AppError>, needle: &str) {
    match res {
        Err(AppError::ValidationFailed(m)) => assert!(m.iter().any(|x| x.contains(needle)), "{needle:?} in {m:?}"),
        other => panic!("expected ValidationFailed({needle:?}), got {other:?}"),
    }
}

#[tokio::test]
async fn decision_logic_is_recorded_and_overrides_need_justification() {
    let f = setup().await;
    let rv = |db: &DatabaseConnection| {
        let db = db.clone();
        async move { scalar(&db, "SELECT row_version FROM rcm_decisions").await }
    };
    // Retaining another tactic without justification is refused.
    assert_validation(
        apply_rcm_decision_logic(
            &f.db,
            ApplyRcmLogicInput {
                decision_id: f.decision_id,
                expected_row_version: rv(&f.db).await,
                answers: answers(),
                selected_tactic: Some("run_to_failure".into()),
                justification: None,
            },
        )
        .await,
        "justification is required",
    );
    let out = apply_rcm_decision_logic(
        &f.db,
        ApplyRcmLogicInput {
            decision_id: f.decision_id,
            expected_row_version: rv(&f.db).await,
            answers: answers(),
            selected_tactic: None,
            justification: None,
        },
    )
    .await
    .expect("apply");
    assert_eq!(out.selected_tactic, "condition_based");
    assert_eq!(out.recommended_tactic.as_deref(), Some("condition_based"));
    assert_eq!(out.consequence_category, "operational");
    assert_eq!(out.answers.as_ref(), Some(&answers()));
    assert!(out.path.contains(&"on_condition:yes".to_string()));

    // Compulsory redesign cannot be overridden, even with a justification.
    let mut safety = answers();
    safety.safety_or_environmental = true;
    safety.on_condition_feasible = false;
    assert_validation(
        apply_rcm_decision_logic(
            &f.db,
            ApplyRcmLogicInput {
                decision_id: f.decision_id,
                expected_row_version: out.row_version,
                answers: safety,
                selected_tactic: Some("run_to_failure".into()),
                justification: Some("cheaper".into()),
            },
        )
        .await,
        "Redesign is compulsory",
    );
}

#[tokio::test]
async fn pm_revision_and_work_order_keep_their_source() {
    let f = setup().await;
    let rv = scalar(&f.db, "SELECT row_version FROM rcm_decisions").await;
    apply_rcm_decision_logic(
        &f.db,
        ApplyRcmLogicInput {
            decision_id: f.decision_id,
            expected_row_version: rv,
            answers: answers(),
            selected_tactic: None,
            justification: None,
        },
    )
    .await
    .expect("apply");

    let action = create_pm_revision_from_source(
        &f.db,
        PmRevisionFromSourceInput {
            source_type: "rcm_decision".into(),
            source_id: f.decision_id,
            pm_plan_id: f.plan_id,
            trigger_definition_json: None,
            change_note: "Add vibration route".into(),
        },
    )
    .await
    .expect("revision");
    assert_eq!(action.pm_version_no, Some(2));
    let version_id = action.pm_plan_version_id.expect("version");
    let source = f
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT source_type, source_id, status, change_reason, estimated_duration_hours FROM pm_plan_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await
        .expect("q")
        .expect("row");
    assert_eq!(source.try_get::<String>("", "source_type").unwrap(), "rcm_decision");
    assert_eq!(source.try_get::<i64>("", "source_id").unwrap(), f.decision_id);
    assert_eq!(source.try_get::<String>("", "status").unwrap(), "draft");
    assert!(source
        .try_get::<String>("", "change_reason")
        .unwrap()
        .contains("Add vibration route"));
    assert_eq!(
        source.try_get::<Option<f64>>("", "estimated_duration_hours").unwrap(),
        Some(1.5)
    );

    let study = scalar(&f.db, "SELECT id FROM rcm_studies").await;
    let logic = list_rcm_decision_logic(&f.db, study).await.expect("list");
    assert_eq!(logic[0].pm_plan_code.as_deref(), Some("PM-FAN"));
    assert_eq!(logic[0].pm_version_no, Some(2));

    // FMECA corrective action → work order on the analysed equipment, linked back.
    let admin = scalar(&f.db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let wo = create_wo_from_source(
        &f.db,
        WoFromSourceInput {
            source_type: "fmeca_item".into(),
            source_id: f.item_id,
            type_code: "corrective".into(),
            title: None,
        },
        admin,
    )
    .await
    .expect("wo");
    let wo_id = wo.work_order_id.expect("wo id");
    assert_eq!(
        scalar(
            &f.db,
            &format!("SELECT linked_work_order_id FROM fmeca_items WHERE id = {}", f.item_id)
        )
        .await,
        wo_id
    );
    assert_eq!(
        scalar(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM work_orders wo JOIN equipment e ON e.id = wo.equipment_id \
                 WHERE wo.id = {wo_id} AND e.asset_id_code = 'FAN-07' AND wo.title = 'Bearing seizure'"
            )
        )
        .await,
        1
    );

    // A run-to-failure decision is not turned into PM.
    f.db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE rcm_decisions SET selected_tactic = 'run_to_failure'".to_string(),
    ))
    .await
    .expect("rtf");
    assert_validation(
        create_pm_revision_from_source(
            &f.db,
            PmRevisionFromSourceInput {
                source_type: "rcm_decision".into(),
                source_id: f.decision_id,
                pm_plan_id: f.plan_id,
                trigger_definition_json: None,
                change_note: "x".into(),
            },
        )
        .await,
        "not carried out by preventive maintenance",
    );
}

#[tokio::test]
async fn pm_revision_copies_a_flagless_legacy_part_list_as_required() {
    let f = setup().await;
    let rv = scalar(&f.db, "SELECT row_version FROM rcm_decisions").await;
    apply_rcm_decision_logic(
        &f.db,
        ApplyRcmLogicInput {
            decision_id: f.decision_id,
            expected_row_version: rv,
            answers: answers(),
            selected_tactic: None,
            justification: None,
        },
    )
    .await
    .expect("apply");
    // The latest version holds a part list written before migration 231 (or received from a PC
    // that has not run it): a bare code and an entry without the required / optional flag.
    f.db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_plan_versions SET required_parts_json = ? WHERE pm_plan_id = ?",
        [
            r#"["FLT-01", {"article_code":"BRG-6204","quantity":2}]"#.into(),
            f.plan_id.into(),
        ],
    ))
    .await
    .expect("legacy part list");

    let action = create_pm_revision_from_source(
        &f.db,
        PmRevisionFromSourceInput {
            source_type: "rcm_decision".into(),
            source_id: f.decision_id,
            pm_plan_id: f.plan_id,
            trigger_definition_json: None,
            change_note: "Add vibration route".into(),
        },
    )
    .await
    .expect("the revision is created from a legacy part list, not refused");
    let version_id = action.pm_plan_version_id.expect("version");
    let copied: String =
        f.db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT required_parts_json FROM pm_plan_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await
        .expect("q")
        .expect("row")
        .try_get("", "required_parts_json")
        .expect("parts");
    crate::pm::wo_content::validate_required_parts_json(Some(&copied)).expect("the copy carries the flags");
    let parts = crate::pm::wo_content::parse_required_parts(Some(&copied));
    assert_eq!(
        parts
            .iter()
            .map(|p| (p.code.as_str(), p.quantity, p.required))
            .collect::<Vec<_>>(),
        vec![("FLT-01", None, true), ("BRG-6204", Some(2.0), true)],
        "the same parts, each read as required"
    );
}

#[tokio::test]
async fn criticality_number_from_inputs_and_from_history() {
    let f = setup().await;
    // No failure history and no λp known: the user must enter them.
    assert_validation(
        compute_fmeca_criticality(
            &f.db,
            FmecaCriticalityInput {
                item_id: f.item_id,
                expected_row_version: 1,
                effect_probability: 1.0,
                mode_ratio: None,
                failure_rate_per_hour: None,
                operating_hours: 1000.0,
            },
        )
        .await,
        "failure mode ratio",
    );
    let c = compute_fmeca_criticality(
        &f.db,
        FmecaCriticalityInput {
            item_id: f.item_id,
            expected_row_version: 1,
            effect_probability: 0.5,
            mode_ratio: Some(0.3),
            failure_rate_per_hour: Some(2e-4),
            operating_hours: 1000.0,
        },
    )
    .await
    .expect("criticality");
    assert!((c.criticality_number - 0.03).abs() < 1e-12);
    assert!((c.analysis_criticality - 0.03).abs() < 1e-12);
    assert_eq!(c.row_version, 2);
}

/// Owner decision D2 (a), audit finding AU-04: a work order raised from a reliability action gets
/// the plant's permit rule like any other.
#[tokio::test]
async fn a_work_order_from_a_reliability_action_gets_the_plant_permit_rule() {
    let f = setup().await;
    crate::permit::requirement_rules::add_published_rule(
        &f.db,
        "CORRECTIVE_LOTO",
        serde_json::json!({ "work_type_code": "corrective", "permit_type_code": "loto" }),
    )
    .await;
    let admin = scalar(&f.db, "SELECT id FROM user_accounts WHERE username = 'admin'").await;
    let action = create_wo_from_source(
        &f.db,
        WoFromSourceInput {
            source_type: "fmeca_item".into(),
            source_id: f.item_id,
            type_code: "corrective".into(),
            title: None,
        },
        admin,
    )
    .await
    .expect("wo");
    let wo = crate::wo::queries::get_work_order(&f.db, action.work_order_id.expect("wo id"))
        .await
        .expect("get")
        .expect("wo");
    assert!(wo.requires_permit);
    assert_eq!(wo.permit_rule_code.as_deref(), Some("CORRECTIVE_LOTO"));
    assert_eq!(wo.permit_rule_permit_type.as_deref(), Some("loto"));
}
