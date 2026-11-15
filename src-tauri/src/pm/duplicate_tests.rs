//! "Dupliquer ce plan" (L6-11): what a copy carries, what it leaves, the meter rule, the right at
//! the target site, the audit entry; the source plan is never changed.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Value};

use crate::auth::scope::{check_anchor, AccessScope, Anchor};
use crate::errors::AppError;
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, DuplicatePmPlanInput, GeneratePmOccurrencesInput,
    PublishPmPlanVersionInput, TransitionPmPlanLifecycleInput, UpdatePmPlanVersionInput,
};
use crate::pm::{duplicate, queries};

const SITE_A: i64 = 1;
const SITE_B: i64 = 902;
const PUMP_A: i64 = 901;
const PUMP_B: i64 = 902;
const METER_A_HOURS: i64 = 911;
const METER_B_HOURS: i64 = 912;
const METER_B_KM: i64 = 913;
const METER_B_HOURS_OFF: i64 = 914;

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn unit_sql(code: &str) -> String {
    format!(
        "(SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
          JOIN reference_domains d ON d.id = s.domain_id \
          WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = '{code}')"
    )
}

/// Two sites, one pump on each, hour meters on both, a km meter and an inactive hour meter on B,
/// and an active crew.
async fn setup() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models
            (id, entity_sync_id, version_number, status, description, activated_at, created_at, updated_at)
         VALUES (1, 'dup_model', 1, 'active', 'model', datetime('now'), datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_node_types
            (id, entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, is_root_type, is_active, created_at, updated_at)
         VALUES (1, 'dup_type', 1, 'SITE', 'Site', 1, 1, 1, 1, datetime('now'), datetime('now'))",
        "INSERT OR IGNORE INTO org_nodes
            (id, entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
             created_at, updated_at, row_version, structure_model_id)
         VALUES (1, 'dup_site_a', 'SITE-A', 'Site A', 1, NULL, '/', 0, 'active', datetime('now'), datetime('now'), 1, 1)",
        "INSERT INTO org_nodes
            (id, entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
             created_at, updated_at, row_version, structure_model_id)
         VALUES (902, 'dup_site_b', 'SITE-B', 'Site B', 1, NULL, '/', 0, 'active', datetime('now'), datetime('now'), 1, 1)",
        "INSERT INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at)
         VALUES (901, 'dup_pump_a', 'P-901', 'Pompe A', 'active_in_service', 1, datetime('now'), datetime('now'))",
        "INSERT INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at)
         VALUES (902, 'dup_pump_b', 'P-902', 'Pompe B', 'active_in_service', 902, datetime('now'), datetime('now'))",
        "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at)
         VALUES ('dup-crew', 'DUP-CREW', 'Mécaniciens', 'maintenance', 1, 'active', datetime('now'), datetime('now'))",
    ] {
        exec(&db, sql).await;
    }
    for (id, equipment, unit, active) in [
        (METER_A_HOURS, PUMP_A, "H", 1),
        (METER_B_HOURS, PUMP_B, "H", 1),
        (METER_B_KM, PUMP_B, "KM", 1),
        (METER_B_HOURS_OFF, PUMP_B, "H", 0),
    ] {
        exec(
            &db,
            &format!(
                "INSERT INTO equipment_meters (id, entity_sync_id, equipment_id, name, meter_type, unit_value_id,
                    current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at)
                 VALUES ({id}, 'dup_meter_{id}', {equipment}, 'Compteur {id}', 'hours', {unit}, 1000, 0, 0, {active},
                    datetime('now'), datetime('now'))",
                unit = unit_sql(unit)
            ),
        )
        .await;
    }
    db
}

async fn crew_id(db: &DatabaseConnection) -> i64 {
    db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT id FROM teams WHERE code = 'DUP-CREW'".to_string(),
    ))
    .await
    .expect("crew")
    .expect("crew row")
    .try_get("", "id")
    .expect("crew id")
}

fn version_input(trigger: &str, interval_note: &str) -> CreatePmPlanVersionInput {
    CreatePmPlanVersionInput {
        effective_from: "2026-01-01T00:00:00Z".to_string(),
        effective_to: None,
        trigger_definition_json: trigger.to_string(),
        task_package_json: Some(
            json!([
                {"description": "Mesurer les vibrations", "is_mandatory": true, "estimated_minutes": 15,
                 "result_type": "measure", "min": 0.5, "max": 4.5, "unit": "mm/s"},
                {"description": "Contrôler l'alignement", "is_mandatory": false}
            ])
            .to_string(),
        ),
        required_parts_json: Some(r#"[{"code":"BRG-001","required":true}]"#.to_string()),
        // A skill published by the base data (migration 040).
        required_skills_json: Some(r#"["MECH_GENERAL"]"#.to_string()),
        required_tools_json: Some(r#"["Analyseur de vibrations"]"#.to_string()),
        estimated_duration_hours: Some(2.5),
        planned_crew_size: Some(2),
        estimated_labor_cost: Some(120.0),
        estimated_parts_cost: Some(45.0),
        estimated_service_cost: Some(10.0),
        change_reason: Some(interval_note.to_string()),
    }
}

/// A plan on pump A; `publish`: version 1 published (plan active).
async fn source_plan(db: &DatabaseConnection, code: &str, strategy: &str, trigger: &str, publish: bool) -> i64 {
    let plan = queries::create_pm_plan(
        db,
        CreatePmPlanInput {
            code: code.to_string(),
            title: "Graissage pompe".to_string(),
            description: Some("Graisser les paliers".to_string()),
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(PUMP_A),
            strategy_type: strategy.to_string(),
            criticality_value_id: None,
            assigned_team_id: Some(crew_id(db).await),
            requires_shutdown: true,
            requires_permit: true,
            is_active: Some(true),
        },
    )
    .await
    .expect("source plan");
    let version = queries::create_pm_plan_version(db, plan.id, version_input(trigger, "initial"))
        .await
        .expect("source version");
    if publish {
        approve(db, plan.id).await;
        queries::publish_pm_plan_version(
            db,
            PublishPmPlanVersionInput {
                version_id: version.id,
                expected_row_version: version.row_version,
            },
        )
        .await
        .expect("publish source");
    }
    plan.id
}

async fn approve(db: &DatabaseConnection, plan_id: i64) {
    for next in ["proposed", "approved"] {
        let plan = queries::get_pm_plan(db, plan_id).await.expect("plan");
        queries::transition_pm_plan_lifecycle(
            db,
            TransitionPmPlanLifecycleInput {
                plan_id,
                expected_row_version: plan.row_version,
                next_status: next.to_string(),
            },
        )
        .await
        .expect("lifecycle");
    }
}

fn copy_input(source_plan_id: i64, code: &str) -> DuplicatePmPlanInput {
    DuplicatePmPlanInput {
        source_plan_id,
        code: code.to_string(),
        title: "Copie de Graissage pompe".to_string(),
        target_scope_type: Some("equipment".to_string()),
        target_scope_id: Some(PUMP_B),
        target_meter_id: None,
    }
}

fn gate(err: &AppError) -> String {
    match err {
        AppError::ValidationFailed(messages) => messages.join(" | "),
        other => format!("{other:?}"),
    }
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("count")
        .expect("count row")
        .try_get("", "n")
        .expect("n")
}

/// Snapshot of everything stored for a plan (plan row and versions), to prove it is untouched.
async fn plan_snapshot(db: &DatabaseConnection, plan_id: i64) -> String {
    let plan = queries::get_pm_plan(db, plan_id).await.expect("plan");
    let versions = queries::list_pm_plan_versions(db, plan_id).await.expect("versions");
    serde_json::to_string(&(plan, versions)).expect("snapshot")
}

const CALENDAR: &str = r#"{"interval_value":3,"interval_unit":"month","tolerance":{"unit":"days","early":2,"late":5}}"#;

#[tokio::test]
async fn a_copy_is_a_new_draft_v1_with_the_published_content_and_none_of_the_history() {
    let db = setup().await;
    let source_id = source_plan(&db, "PM-SRC", "fixed", CALENDAR, true).await;
    // History of the source: generated occurrences, and a newer draft that must not be copied.
    queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-01T00:00:00Z".to_string()),
            horizon_days: Some(200),
            pm_plan_id: Some(source_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate source occurrences");
    assert!(
        count(
            &db,
            &format!("SELECT COUNT(*) AS n FROM pm_occurrences WHERE pm_plan_id = {source_id}")
        )
        .await
            > 0
    );
    queries::create_pm_plan_version(
        &db,
        source_id,
        version_input(r#"{"interval_value":9,"interval_unit":"week"}"#, "draft v2"),
    )
    .await
    .expect("draft v2");
    let before = plan_snapshot(&db, source_id).await;
    let source = queries::get_pm_plan(&db, source_id).await.expect("source");
    let published = queries::list_pm_plan_versions(&db, source_id)
        .await
        .expect("versions")
        .into_iter()
        .find(|v| v.status == "published")
        .expect("published");

    let copy = duplicate::duplicate_pm_plan(&db, copy_input(source_id, " pm-copy "), Some(1))
        .await
        .expect("duplicate");

    // A new plan, normal code rule, on the target pump, draft without a current version.
    let plan = &copy.plan;
    assert_ne!(plan.id, source_id);
    assert_eq!(plan.code, "PM-COPY");
    assert_eq!(plan.title, "Copie de Graissage pompe");
    assert_eq!(
        (plan.asset_scope_type.as_str(), plan.asset_scope_id),
        ("equipment", Some(PUMP_B))
    );
    assert_eq!(plan.lifecycle_status, "draft");
    assert_eq!(plan.current_version_id, None, "not published");
    // Copied plan attributes.
    assert_eq!(plan.strategy_type, "fixed");
    assert_eq!(plan.description, source.description);
    assert_eq!(plan.assigned_team_id, source.assigned_team_id);
    assert_eq!((plan.requires_shutdown, plan.requires_permit), (1, 1));
    assert_eq!(plan.criticality_value_id, None, "criticality follows the target asset");

    // Version 1, draft, the content of the PUBLISHED version (not the newer draft).
    let version = &copy.version;
    assert_eq!((version.version_no, version.status.as_str()), (1, "draft"));
    assert!(!copy.meter_to_choose);
    let trigger: Value = serde_json::from_str(&version.trigger_definition_json).expect("trigger");
    let expected: Value = serde_json::from_str(&published.trigger_definition_json).expect("trigger");
    assert_eq!(trigger, expected, "interval, unit and tolerance copied");
    assert_eq!(
        version.task_package_json, published.task_package_json,
        "tasks as written"
    );
    assert_eq!(
        version.required_parts_json,
        crate::pm::wo_content::normalized_required_parts_json(published.required_parts_json.as_deref())
    );
    assert_eq!(version.required_tools_json, published.required_tools_json);
    assert_eq!(
        version.required_skills_json.as_deref(),
        Some(r#"["MECH_GENERAL"]"#),
        "trade copied"
    );
    assert_eq!(version.required_skills_json, published.required_skills_json);
    assert_eq!(version.estimated_duration_hours, Some(2.5));
    assert_eq!(version.planned_crew_size, Some(2));
    // Not copied: costs, dates of the source.
    assert_eq!(
        (
            version.estimated_labor_cost,
            version.estimated_parts_cost,
            version.estimated_service_cost
        ),
        (None, None, None)
    );
    assert_ne!(version.effective_from, published.effective_from);
    assert_eq!(version.effective_to, None);
    assert_eq!(version.change_reason.as_deref(), Some("copié de PM-SRC v1"));
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM pm_plan_versions WHERE pm_plan_id = {}",
                plan.id
            )
        )
        .await,
        1,
        "no version history"
    );
    // No occurrence, work order or execution comes with the copy.
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM pm_occurrences WHERE pm_plan_id = {}",
                plan.id
            )
        )
        .await,
        0
    );
    // Synchronised like a plan made by hand.
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM pm_plans p JOIN pm_plan_versions v ON v.pm_plan_id = p.id
                 WHERE p.id = {} AND p.entity_sync_id IS NOT NULL AND v.entity_sync_id IS NOT NULL",
                plan.id
            )
        )
        .await,
        1
    );

    // Audit entry "copié de <code> v<n>" on the new plan, by the acting user.
    let audit = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT target_type, target_id, actor_id, details_json FROM audit_events
             WHERE action_code = 'pm.plan.duplicated'"
                .to_string(),
        ))
        .await
        .expect("audit")
        .expect("audit row");
    assert_eq!(audit.try_get::<String>("", "target_type").unwrap(), "pm_plan");
    let plan_sync_id = crate::sync::identity::optional_entity_sync_id(&db, "pm_plans", plan.id)
        .await
        .expect("sync id query")
        .expect("plan sync id");
    assert_eq!(audit.try_get::<String>("", "target_id").unwrap(), plan_sync_id);
    assert_eq!(audit.try_get::<Option<i64>>("", "actor_id").unwrap(), Some(1));
    let details: Value = serde_json::from_str(&audit.try_get::<String>("", "details_json").unwrap()).unwrap();
    assert_eq!(details["summary"], "copié de PM-SRC v1");

    // The source plan is exactly as it was.
    assert_eq!(plan_snapshot(&db, source_id).await, before);
}

#[tokio::test]
async fn without_a_published_version_the_latest_draft_is_copied_and_without_any_version_it_is_refused() {
    let db = setup().await;
    let source_id = source_plan(&db, "PM-DRAFT", "fixed", CALENDAR, false).await;
    queries::create_pm_plan_version(
        &db,
        source_id,
        version_input(r#"{"interval_value":6,"interval_unit":"week"}"#, "draft v2"),
    )
    .await
    .expect("draft v2");
    let copy = duplicate::duplicate_pm_plan(&db, copy_input(source_id, "PM-DRAFT-COPY"), Some(1))
        .await
        .expect("duplicate");
    let trigger: Value = serde_json::from_str(&copy.version.trigger_definition_json).unwrap();
    assert_eq!(trigger["interval_value"], 6, "latest draft");
    assert_eq!(copy.version.version_no, 1);
    assert_eq!(copy.version.change_reason.as_deref(), Some("copié de PM-DRAFT v2"));

    let empty = queries::create_pm_plan(
        &db,
        CreatePmPlanInput {
            code: "PM-EMPTY".to_string(),
            title: "Sans version".to_string(),
            description: None,
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(PUMP_A),
            strategy_type: "fixed".to_string(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: None,
        },
    )
    .await
    .expect("empty plan");
    let err = duplicate::duplicate_pm_plan(&db, copy_input(empty.id, "PM-EMPTY-COPY"), Some(1))
        .await
        .expect_err("nothing to copy");
    assert!(gate(&err).starts_with("GATE_PM_NO_VERSION_TO_COPY"), "{err:?}");
}

#[tokio::test]
async fn a_code_already_used_is_refused_and_nothing_is_written() {
    let db = setup().await;
    let source_id = source_plan(&db, "PM-TAKEN", "fixed", CALENDAR, true).await;
    let plans = count(&db, "SELECT COUNT(*) AS n FROM pm_plans").await;
    let versions = count(&db, "SELECT COUNT(*) AS n FROM pm_plan_versions").await;
    // Same code typed in lower case with spaces: the normal code rule makes it the same code.
    let err = duplicate::duplicate_pm_plan(&db, copy_input(source_id, " pm-taken"), Some(1))
        .await
        .expect_err("code taken");
    assert!(gate(&err).starts_with("GATE_PM_PLAN_CODE_TAKEN"), "{err:?}");
    let err = duplicate::duplicate_pm_plan(&db, copy_input(source_id, "  "), Some(1))
        .await
        .expect_err("code required");
    assert!(gate(&err).starts_with("GATE_PM_PLAN_CODE_TITLE_REQUIRED"), "{err:?}");
    assert_eq!(count(&db, "SELECT COUNT(*) AS n FROM pm_plans").await, plans);
    assert_eq!(count(&db, "SELECT COUNT(*) AS n FROM pm_plan_versions").await, versions);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = 'pm.plan.duplicated'"
        )
        .await,
        0
    );
}

const METER_TRIGGER: &str = r#"{"asset_meter_id":911,"interval_value":250,"start_reading":1000,"tolerance":{"unit":"percent","early":5,"late":10}}"#;

#[tokio::test]
async fn a_meter_trigger_keeps_a_meter_only_of_the_target_asset_with_the_same_unit() {
    let db = setup().await;
    let source_id = source_plan(&db, "PM-METER", "meter", METER_TRIGGER, true).await;

    // Same unit on the target pump: kept, on the chosen meter; the source reading is not copied.
    let mut input = copy_input(source_id, "PM-METER-B");
    input.target_meter_id = Some(METER_B_HOURS);
    let copy = duplicate::duplicate_pm_plan(&db, input, Some(1))
        .await
        .expect("same unit");
    let trigger: Value = serde_json::from_str(&copy.version.trigger_definition_json).unwrap();
    assert_eq!(trigger["asset_meter_id"], METER_B_HOURS);
    assert_eq!(trigger["interval_value"], 250);
    assert_eq!(trigger["tolerance"], json!({"unit":"percent","early":5,"late":10}));
    assert!(trigger.get("start_reading").is_none(), "readings are not copied");
    assert!(!copy.meter_to_choose);

    // Another unit, an inactive meter, a meter of another asset: refused.
    for meter in [METER_B_KM, METER_B_HOURS_OFF, METER_A_HOURS] {
        let mut input = copy_input(source_id, &format!("PM-METER-X{meter}"));
        input.target_meter_id = Some(meter);
        let err = duplicate::duplicate_pm_plan(&db, input, Some(1))
            .await
            .expect_err("meter refused");
        assert!(
            gate(&err).starts_with("GATE_PM_COPY_METER_MISMATCH"),
            "meter {meter}: {err:?}"
        );
    }

    // A calendar plan has no meter to choose.
    let calendar_id = source_plan(&db, "PM-CAL", "fixed", CALENDAR, true).await;
    let mut input = copy_input(calendar_id, "PM-CAL-B");
    input.target_meter_id = Some(METER_B_HOURS);
    let err = duplicate::duplicate_pm_plan(&db, input, Some(1))
        .await
        .expect_err("no meter expected");
    assert!(gate(&err).starts_with("GATE_PM_COPY_METER_NOT_EXPECTED"), "{err:?}");
}

#[tokio::test]
async fn a_meter_left_to_choose_blocks_publishing_until_it_is_chosen() {
    let db = setup().await;
    let source_id = source_plan(&db, "PM-METER2", "meter", METER_TRIGGER, true).await;
    let copy = duplicate::duplicate_pm_plan(&db, copy_input(source_id, "PM-METER2-B"), Some(1))
        .await
        .expect("meter left to choose");
    assert!(copy.meter_to_choose);
    let trigger: Value = serde_json::from_str(&copy.version.trigger_definition_json).unwrap();
    assert!(trigger.get("asset_meter_id").is_none(), "meter reference left blank");
    assert_eq!(trigger["interval_value"], 250);
    assert!(queries::meter_choice_missing(
        "meter",
        &copy.version.trigger_definition_json
    ));

    approve(&db, copy.plan.id).await;
    let err = queries::publish_pm_plan_version(
        &db,
        PublishPmPlanVersionInput {
            version_id: copy.version.id,
            expected_row_version: copy.version.row_version,
        },
    )
    .await
    .expect_err("publish refused");
    assert!(gate(&err).starts_with("GATE_PM_METER_REQUIRED"), "{err:?}");

    // The draft stays editable while the meter is to choose; once chosen, it publishes.
    let edited = queries::update_pm_plan_version(
        &db,
        copy.version.id,
        copy.version.row_version,
        UpdatePmPlanVersionInput {
            effective_from: None,
            effective_to: None,
            trigger_definition_json: None,
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: None,
            required_tools_json: None,
            estimated_duration_hours: Some(3.0),
            planned_crew_size: None,
            clear_planned_crew_size: false,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("edit before choosing the meter");
    let chosen = queries::update_pm_plan_version(
        &db,
        edited.id,
        edited.row_version,
        UpdatePmPlanVersionInput {
            effective_from: None,
            effective_to: None,
            trigger_definition_json: Some(format!(r#"{{"asset_meter_id":{METER_B_HOURS},"interval_value":250}}"#)),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: None,
            required_tools_json: None,
            estimated_duration_hours: None,
            planned_crew_size: None,
            clear_planned_crew_size: false,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("meter chosen");
    let published = queries::publish_pm_plan_version(
        &db,
        PublishPmPlanVersionInput {
            version_id: chosen.id,
            expected_row_version: chosen.row_version,
        },
    )
    .await
    .expect("publish once the meter is chosen");
    assert_eq!(published.status, "published");
}

#[test]
fn a_meter_is_missing_only_when_neither_a_meter_nor_a_meter_type_is_set() {
    assert!(queries::meter_choice_missing("meter", r#"{"interval_value":10}"#));
    assert!(!queries::meter_choice_missing(
        "meter",
        r#"{"asset_meter_id":3,"interval_value":10}"#
    ));
    assert!(!queries::meter_choice_missing(
        "meter",
        r#"{"meter_type":"HOURS","interval_value":10}"#
    ));
    assert!(!queries::meter_choice_missing(
        "meter",
        r#"{"meter_source":"odometer","interval_value":10}"#
    ));
    assert!(queries::meter_choice_missing(
        "fixed",
        r#"{"interval_value":1,"interval_unit":"month","meter":{"interval_value":500}}"#
    ));
    assert!(!queries::meter_choice_missing(
        "fixed",
        r#"{"interval_value":1,"interval_unit":"month"}"#
    ));
    assert!(!queries::meter_choice_missing("event", r#"{"event_code":"X"}"#));
}

#[tokio::test]
async fn the_right_is_checked_at_the_site_of_the_target_asset() {
    let db = setup().await;
    let perm = crate::rbac::permissions::PM_CREATE;
    assert_eq!(
        duplicate::target_anchor(&db, "equipment", PUMP_B)
            .await
            .expect("anchor"),
        Anchor::Node(SITE_B)
    );
    assert_eq!(
        duplicate::target_anchor(&db, "location", SITE_A).await.expect("anchor"),
        Anchor::Node(SITE_A)
    );
    let family = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM reference_values LIMIT 1".to_string(),
        ))
        .await
        .unwrap()
        .expect("a reference value")
        .try_get::<i64>("", "id")
        .unwrap();
    assert_eq!(
        duplicate::target_anchor(&db, "family", family).await.expect("anchor"),
        Anchor::Plant
    );
    let err = duplicate::target_anchor(&db, "equipment", 999_999)
        .await
        .expect_err("unknown");
    assert!(gate(&err).starts_with("GATE_PM_COPY_TARGET_UNKNOWN"), "{err:?}");

    // A planner of site A may copy onto pump A, not onto pump B (site B) nor a plant-wide scope.
    let site_a_only = AccessScope::Subtrees([SITE_A].into());
    let on_a = duplicate::target_anchor(&db, "equipment", PUMP_A).await.unwrap();
    let on_b = duplicate::target_anchor(&db, "equipment", PUMP_B).await.unwrap();
    check_anchor(&db, &site_a_only, perm, on_a).await.expect("own site");
    let refused = check_anchor(&db, &site_a_only, perm, on_b)
        .await
        .expect_err("other site");
    assert!(format!("{refused:?}").contains("SCOPE_OUTSIDE_SITE"), "{refused:?}");
    assert!(check_anchor(&db, &site_a_only, perm, Anchor::Plant).await.is_err());
    check_anchor(&db, &AccessScope::Everywhere, perm, on_b)
        .await
        .expect("plant-wide");

    // The target defaults to the source plan's asset when none is given.
    let source_id = source_plan(&db, "PM-DEFAULT", "fixed", CALENDAR, true).await;
    let source = queries::get_pm_plan(&db, source_id).await.unwrap();
    let mut input = copy_input(source_id, "PM-DEFAULT-COPY");
    input.target_scope_type = None;
    input.target_scope_id = None;
    assert_eq!(
        duplicate::resolve_target(&source, &input).expect("target"),
        ("equipment".to_string(), PUMP_A)
    );
}

#[tokio::test]
async fn a_failure_after_the_plan_row_is_written_leaves_nothing_behind() {
    let db = setup().await;
    let source_id = source_plan(&db, "PM-ROLLBACK", "fixed", CALENDAR, false).await;
    // The draft names a skill that is no longer published: the version of the copy is refused
    // after the plan row of the copy is already inserted in the transaction.
    exec(
        &db,
        "UPDATE reference_values SET is_active = 0
          WHERE code = 'MECH_GENERAL'
            AND set_id IN (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id
                            WHERE d.code = 'PERSONNEL.SKILLS')",
    )
    .await;
    let plans = count(&db, "SELECT COUNT(*) AS n FROM pm_plans").await;
    let versions = count(&db, "SELECT COUNT(*) AS n FROM pm_plan_versions").await;
    let audits = count(&db, "SELECT COUNT(*) AS n FROM audit_events").await;

    let err = duplicate::duplicate_pm_plan(&db, copy_input(source_id, "PM-ROLLBACK-B"), Some(1))
        .await
        .expect_err("version refused");
    assert!(gate(&err).starts_with("GATE_PM_SKILL_UNKNOWN"), "{err:?}");

    assert_eq!(
        count(&db, "SELECT COUNT(*) AS n FROM pm_plans WHERE code = 'PM-ROLLBACK-B'").await,
        0,
        "no plan row left"
    );
    assert_eq!(count(&db, "SELECT COUNT(*) AS n FROM pm_plans").await, plans);
    assert_eq!(count(&db, "SELECT COUNT(*) AS n FROM pm_plan_versions").await, versions);
    assert_eq!(count(&db, "SELECT COUNT(*) AS n FROM audit_events").await, audits);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = 'pm.plan.duplicated'"
        )
        .await,
        0
    );
}

const CALENDAR_WITH_METER: &str = r#"{"interval_value":3,"interval_unit":"month","tolerance":{"unit":"days","early":2,"late":5},"meter":{"asset_meter_id":911,"interval_value":500,"start_reading":100}}"#;

#[tokio::test]
async fn a_calendar_plan_with_a_meter_block_follows_the_same_meter_rule() {
    let db = setup().await;
    for (strategy, code) in [("fixed", "PM-WCF-FIX"), ("floating", "PM-WCF-FLO")] {
        let source_id = source_plan(&db, code, strategy, CALENDAR_WITH_METER, true).await;

        // A meter of the target pump with the same unit: the block keeps a meter, that one.
        let mut input = copy_input(source_id, &format!("{code}-B"));
        input.target_meter_id = Some(METER_B_HOURS);
        let copy = duplicate::duplicate_pm_plan(&db, input, Some(1))
            .await
            .expect("same unit");
        assert!(!copy.meter_to_choose, "{strategy}");
        let trigger: Value = serde_json::from_str(&copy.version.trigger_definition_json).unwrap();
        assert_eq!(trigger["interval_value"], 3, "{strategy}: calendar part copied");
        assert_eq!(trigger["interval_unit"], "month");
        assert_eq!(trigger["tolerance"], json!({"unit":"days","early":2,"late":5}));
        assert_eq!(trigger["meter"]["asset_meter_id"], METER_B_HOURS, "{strategy}");
        assert_eq!(trigger["meter"]["interval_value"], 500);
        assert!(
            trigger["meter"].get("start_reading").is_none(),
            "{strategy}: reading not copied"
        );

        // Another unit: refused, nothing written.
        let mut input = copy_input(source_id, &format!("{code}-KM"));
        input.target_meter_id = Some(METER_B_KM);
        let err = duplicate::duplicate_pm_plan(&db, input, Some(1))
            .await
            .expect_err("other unit");
        assert!(
            gate(&err).starts_with("GATE_PM_COPY_METER_MISMATCH"),
            "{strategy}: {err:?}"
        );

        // No meter chosen: the block's meter is left blank and publishing waits for it.
        let copy = duplicate::duplicate_pm_plan(&db, copy_input(source_id, &format!("{code}-LATER")), Some(1))
            .await
            .expect("meter left to choose");
        assert!(copy.meter_to_choose, "{strategy}");
        let trigger: Value = serde_json::from_str(&copy.version.trigger_definition_json).unwrap();
        assert!(trigger["meter"].get("asset_meter_id").is_none(), "{strategy}");
        assert_eq!(trigger["meter"]["interval_value"], 500);
        assert_eq!(trigger["interval_value"], 3);
        approve(&db, copy.plan.id).await;
        let err = queries::publish_pm_plan_version(
            &db,
            PublishPmPlanVersionInput {
                version_id: copy.version.id,
                expected_row_version: copy.version.row_version,
            },
        )
        .await
        .expect_err("publish refused");
        assert!(gate(&err).starts_with("GATE_PM_METER_REQUIRED"), "{strategy}: {err:?}");
    }
}
