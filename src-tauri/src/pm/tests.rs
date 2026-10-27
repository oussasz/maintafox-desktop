use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::auth::rbac::{self, PermissionScope};
use crate::errors::AppError;
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, ExecutePmOccurrenceInput, GeneratePmOccurrencesInput,
    PmGovernanceKpiInput, PmOccurrenceFilter, PmPlanningReadinessInput, PublishPmPlanVersionInput,
    TransitionPmOccurrenceInput, TransitionPmPlanLifecycleInput, UpdatePmPlanInput,
};
use crate::pm::queries;

async fn setup_db() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY)".to_string(),
    ))
    .await
    .expect("ensure users table");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO users (id) VALUES (1)",
        [],
    ))
    .await
    .expect("seed user");
    db
}

async fn create_plan_with_published_version(
    db: &DatabaseConnection,
    code: &str,
    strategy_type: &str,
    trigger_definition_json: &str,
) -> (i64, i64) {
    let plan = queries::create_pm_plan(
        db,
        CreatePmPlanInput {
            code: code.to_string(),
            title: format!("{code} title"),
            description: None,
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(1),
            strategy_type: strategy_type.to_string(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("create plan");

    let proposed = queries::transition_pm_plan_lifecycle(
        db,
        TransitionPmPlanLifecycleInput {
            plan_id: plan.id,
            expected_row_version: plan.row_version,
            next_status: "proposed".to_string(),
        },
    )
    .await
    .expect("propose plan");

    let approved = queries::transition_pm_plan_lifecycle(
        db,
        TransitionPmPlanLifecycleInput {
            plan_id: plan.id,
            expected_row_version: proposed.row_version,
            next_status: "approved".to_string(),
        },
    )
    .await
    .expect("approve plan");

    let version = queries::create_pm_plan_version(
        db,
        plan.id,
        CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            effective_to: None,
            trigger_definition_json: trigger_definition_json.to_string(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".to_string()),
            required_tools_json: None,
            estimated_duration_hours: None,
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("create version");

    let published = queries::publish_pm_plan_version(
        db,
        PublishPmPlanVersionInput {
            version_id: version.id,
            expected_row_version: version.row_version,
        },
    )
    .await
    .expect("publish version");

    let plan_after = queries::get_pm_plan(db, approved.id).await.expect("get plan");
    (plan_after.id, published.id)
}

#[tokio::test]
async fn lifecycle_requires_valid_transition() {
    let db = setup_db().await;
    let plan = queries::create_pm_plan(
        &db,
        CreatePmPlanInput {
            code: "PM-LIFE-001".to_string(),
            title: "Test lifecycle".to_string(),
            description: None,
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(1),
            strategy_type: "fixed".to_string(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("create plan");

    let err = queries::transition_pm_plan_lifecycle(
        &db,
        TransitionPmPlanLifecycleInput {
            plan_id: plan.id,
            expected_row_version: plan.row_version,
            next_status: "active".to_string(),
        },
    )
    .await
    .expect_err("draft -> active must fail");
    assert!(matches!(err, AppError::ValidationFailed(_)));
}

#[tokio::test]
async fn stale_row_version_is_rejected() {
    let db = setup_db().await;
    let plan = queries::create_pm_plan(
        &db,
        CreatePmPlanInput {
            code: "PM-RV-001".to_string(),
            title: "Row version test".to_string(),
            description: None,
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(1),
            strategy_type: "event".to_string(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("create plan");

    let updated = queries::update_pm_plan(
        &db,
        plan.id,
        plan.row_version,
        UpdatePmPlanInput {
            title: Some("updated".to_string()),
            description: None,
            asset_scope_type: None,
            asset_scope_id: None,
            strategy_type: None,
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: None,
            requires_permit: None,
            is_active: None,
        },
    )
    .await
    .expect("first update");

    let stale = queries::update_pm_plan(
        &db,
        plan.id,
        plan.row_version,
        UpdatePmPlanInput {
            title: Some("stale".to_string()),
            description: None,
            asset_scope_type: None,
            asset_scope_id: None,
            strategy_type: None,
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: None,
            requires_permit: None,
            is_active: None,
        },
    )
    .await
    .expect_err("stale row version must fail");
    assert!(matches!(stale, AppError::ValidationFailed(_)));
    assert!(updated.row_version > plan.row_version);
}

#[tokio::test]
async fn publish_sets_plan_current_version() {
    let db = setup_db().await;
    let (plan_id, version_id) =
        create_plan_with_published_version(&db, "PM-PUB-001", "event", r#"{"event_code":"INSPECTION_COMPLETED"}"#)
            .await;

    let plan_after = queries::get_pm_plan(&db, plan_id).await.expect("plan after publish");
    assert_eq!(plan_after.current_version_id, Some(version_id));
}

#[tokio::test]
async fn fixed_generation_is_idempotent() {
    let db = setup_db().await;
    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-FIXED-001",
        "fixed",
        r#"{"interval_unit":"day","interval_value":7}"#,
    )
    .await;

    let first = queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(30),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("first generation");
    assert!(first.generated_count > 0);

    let second = queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(30),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("second generation");

    assert_eq!(second.generated_count, 0);
    assert!(second.skipped_count > 0);

    let occurrences = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list occurrences");

    assert_eq!(occurrences.len() as i64, first.generated_count);
}

#[tokio::test]
async fn floating_generation_reanchors_on_completion() {
    let db = setup_db().await;
    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-FLOAT-001",
        "floating",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;

    queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(2),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate floating occurrences");

    let mut occurrences = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list floating occurrences");
    let first_occ = occurrences.remove(0);
    assert_eq!(first_occ.status, "generated");

    let ready = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: first_occ.id,
            expected_row_version: first_occ.row_version,
            next_status: "ready_for_scheduling".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("generated -> ready");

    let scheduled = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: ready.id,
            expected_row_version: ready.row_version,
            next_status: "scheduled".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("ready -> scheduled");

    let in_progress = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: scheduled.id,
            expected_row_version: scheduled.row_version,
            next_status: "in_progress".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("scheduled -> in_progress");

    queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: in_progress.id,
            expected_row_version: in_progress.row_version,
            next_status: "completed".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("in_progress -> completed");

    let regen = queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-12T00:00:00Z".to_string()),
            horizon_days: Some(3),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("regenerate floating occurrences");

    assert!(regen.generated_count >= 1);
}

#[tokio::test]
async fn meter_generation_respects_threshold_and_idempotency() {
    let db = setup_db().await;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at)
         VALUES ('eq_sync_pm_meter', 'EQ-PM-METER', 'PM Meter Asset', 'active_in_service', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("insert equipment");

    let equipment_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE asset_id_code = 'EQ-PM-METER'",
            [],
        ))
        .await
        .expect("query equipment")
        .expect("equipment row");
    let equipment_id: i64 = equipment_row.try_get("", "id").expect("equipment id");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit_value_id, current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at)
         VALUES ('meter_sync_pm', ?, 'Runtime', 'hours', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'H'), 120, 0, 1, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [equipment_id.into()],
    ))
    .await
    .expect("insert equipment meter");

    let meter_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_meters WHERE entity_sync_id = 'meter_sync_pm'",
            [],
        ))
        .await
        .expect("query meter")
        .expect("meter row");
    let meter_id: i64 = meter_row.try_get("", "id").expect("meter id");

    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-METER-001",
        "meter",
        &format!(r#"{{"asset_meter_id":{},"threshold_value":100}}"#, meter_id),
    )
    .await;

    let first = queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(7),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("first meter generation");

    let second = queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(7),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("second meter generation");

    assert_eq!(first.generated_count, 1);
    assert_eq!(second.generated_count, 0);
    assert!(second.skipped_count >= 1);
}

#[tokio::test]
async fn pm_permission_boundaries_view_without_create_edit() {
    let db = setup_db().await;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO roles (entity_sync_id, name, description, is_system, role_type, status, created_at, updated_at)
         VALUES ('role_sync_pm_view_only', 'pm_view_only_role', 'PM view only', 0, 'custom', 'active', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("insert role");

    let role_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE name = 'pm_view_only_role'",
            [],
        ))
        .await
        .expect("query role")
        .expect("role row");
    let role_id: i64 = role_row.try_get("", "id").expect("role id");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO role_permissions (role_id, permission_id, granted_at)
         SELECT ?, p.id, strftime('%Y-%m-%dT%H:%M:%SZ','now')
         FROM permissions p
         WHERE p.name = ?",
        [role_id.into(), crate::rbac::permissions::PM_VIEW.into()],
    ))
    .await
    .expect("grant pm.view");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, force_password_change, created_at, updated_at)
         VALUES ('user_sync_pm_view', 'pm_view_user', 'PM View User', 'local', 1, 0, 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("insert user");

    let user_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = 'pm_view_user'",
            [],
        ))
        .await
        .expect("query user")
        .expect("user row");
    let user_id: i32 = user_row.try_get::<i64>("", "id").expect("user id") as i32;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference, valid_from, valid_to, assigned_by_id, notes, created_at, updated_at)
         VALUES ('usa_sync_pm_view', ?, ?, 'tenant', NULL, NULL, NULL, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [user_id.into(), role_id.into()],
    ))
    .await
    .expect("insert user scope assignment");

    let can_view = rbac::check_permission(
        &db,
        user_id,
        crate::rbac::permissions::PM_VIEW,
        &PermissionScope::Global,
    )
    .await
    .expect("check pm.view");
    let can_create = rbac::check_permission(
        &db,
        user_id,
        crate::rbac::permissions::PM_CREATE,
        &PermissionScope::Global,
    )
    .await
    .expect("check pm.create");
    let can_edit = rbac::check_permission(
        &db,
        user_id,
        crate::rbac::permissions::PM_EDIT,
        &PermissionScope::Global,
    )
    .await
    .expect("check pm.edit");

    assert!(can_view);
    assert!(!can_create);
    assert!(!can_edit);
}

#[tokio::test]
async fn execution_with_finding_creates_followups_and_events() {
    let db = setup_db().await;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO org_structure_models
            (id, entity_sync_id, version_number, status, description, activated_at, created_at, updated_at)
         VALUES (1, 'pm_org_model', 1, 'active', 'PM test model',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("insert org model");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO org_node_types
            (id, entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, is_root_type, is_active, created_at, updated_at)
         VALUES (1, 'pm_org_type', 1, 'SITE', 'Site', 1, 1, 1, 1,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("insert org node type");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO org_nodes
            (id, entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
             created_at, updated_at, row_version, structure_model_id)
         VALUES (1, 'pm_org_node', 'SITE-1', 'Test Site', 1, NULL, '/', 0, 'active',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 1, 1)",
        [],
    ))
    .await
    .expect("insert org node");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at)
         VALUES (1, 'eq_sync_pm_exec', 'EQ-PM-EXEC', 'PM Execution Asset', 'active_in_service', 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("insert execution equipment");
    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-EXEC-001",
        "fixed",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;

    let generated = queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(2),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate occurrences");

    assert!(generated.generated_count >= 1);

    let mut occurrences = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list occurrences");
    let occ = occurrences.remove(0);
    assert_eq!(occ.status, "generated");

    let occ = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "ready_for_scheduling".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("to ready");

    let occ = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "scheduled".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("to scheduled");

    let occ = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "in_progress".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("to in progress");

    let execution = queries::execute_pm_occurrence(
        &db,
        crate::pm::domain::ExecutePmOccurrenceInput {
            occurrence_id: occ.id,
            expected_occurrence_row_version: occ.row_version,
            execution_result: "completed_with_findings".to_string(),
            note: Some("Bearing wear observed".to_string()),
            actor_id: Some(1),
            work_order_id: None,
            defer_reason_code: None,
            miss_reason_code: None,
            findings: Some(vec![crate::pm::domain::PmExecutionFindingInput {
                finding_type: "BEARING_WEAR".to_string(),
                severity: Some("high".to_string()),
                description: "High vibration trend and bearing wear marks".to_string(),
                create_follow_up_di: Some(true),
                create_follow_up_work_order: Some(true),
                follow_up_work_order_type_id: None,
            }]),
        },
    )
    .await
    .expect("execute occurrence");

    assert_eq!(execution.occurrence.status, "completed");
    assert_eq!(execution.findings.len(), 1);
    let finding = &execution.findings[0];
    assert!(finding.follow_up_di_id.is_some());
    assert!(finding.follow_up_work_order_id.is_some());

    let notif_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM notification_events WHERE category_code = 'pm_follow_up_created' AND source_record_id = ?",
            [execution.occurrence.id.to_string().into()],
        ))
        .await
        .expect("query notif")
        .expect("notif row");
    let notif_count: i64 = notif_row.try_get("", "c").expect("notif count");
    assert!(notif_count >= 1);

    let activity_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM activity_events WHERE event_code = 'pm.finding.follow_up_created' AND source_record_id = ?",
            [execution.occurrence.id.to_string().into()],
        ))
        .await
        .expect("query activity")
        .expect("activity row");
    let activity_count: i64 = activity_row.try_get("", "c").expect("activity count");
    assert!(activity_count >= 1);
}

#[tokio::test]
async fn missed_execution_requires_reason_code() {
    let db = setup_db().await;
    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-EXEC-002",
        "fixed",
        r#"{"interval_unit":"day","interval_value":7}"#,
    )
    .await;

    queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(10),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate");

    let occ = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list")
    .into_iter()
    .next()
    .expect("occurrence");

    let err = queries::execute_pm_occurrence(
        &db,
        crate::pm::domain::ExecutePmOccurrenceInput {
            occurrence_id: occ.id,
            expected_occurrence_row_version: occ.row_version,
            execution_result: "missed".to_string(),
            note: None,
            actor_id: Some(1),
            work_order_id: None,
            defer_reason_code: None,
            miss_reason_code: None,
            findings: None,
        },
    )
    .await
    .expect_err("missed without reason must fail");

    assert!(matches!(err, AppError::ValidationFailed(_)));
}

#[tokio::test]
async fn planning_readiness_projection_returns_blockers_without_scheduler_side_effects() {
    let db = setup_db().await;
    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-READ-001",
        "fixed",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;
    let readiness_version = queries::create_pm_plan_version(
        &db,
        plan_id,
        CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            effective_to: None,
            trigger_definition_json: r#"{"interval_unit":"day","interval_value":1}"#.to_string(),
            task_package_json: None,
            required_parts_json: Some(r#"[{"code":"BRG-001","required":true}]"#.to_string()),
            required_skills_json: None,
            required_tools_json: None,
            estimated_duration_hours: None,
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: Some("readiness blockers".to_string()),
        },
    )
    .await
    .expect("create readiness version");

    queries::publish_pm_plan_version(
        &db,
        PublishPmPlanVersionInput {
            version_id: readiness_version.id,
            expected_row_version: readiness_version.row_version,
        },
    )
    .await
    .expect("publish readiness version");

    let plan = queries::get_pm_plan(&db, plan_id).await.expect("reload plan");
    queries::update_pm_plan(
        &db,
        plan_id,
        plan.row_version,
        UpdatePmPlanInput {
            title: None,
            description: None,
            asset_scope_type: None,
            asset_scope_id: None,
            strategy_type: None,
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: Some(true),
            requires_permit: Some(true),
            is_active: None,
        },
    )
    .await
    .expect("update plan permit/shutdown flags");

    queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(3),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate occurrences");

    let mut occurrences = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list generated occurrences");
    let first_occurrence = occurrences.remove(0);
    assert_eq!(first_occurrence.status, "generated");

    let readiness = queries::list_pm_planning_readiness(
        &db,
        PmPlanningReadinessInput {
            pm_plan_id: Some(plan_id),
            due_from: None,
            due_to: None,
            include_linked_work_orders: Some(false),
            limit: Some(100),
        },
    )
    .await
    .expect("list readiness projection");

    assert!(readiness.candidate_count >= 1);
    assert_eq!(readiness.ready_count, 0);
    assert!(readiness.blocked_count >= 1);

    let first = readiness.candidates.first().expect("at least one candidate");
    let blocker_codes: Vec<String> = first.blockers.iter().map(|b| b.code.clone()).collect();
    assert!(blocker_codes.iter().any(|code| code == "missing_parts"));
    assert!(blocker_codes.iter().any(|code| code == "permit_not_ready"));
    assert!(blocker_codes.iter().any(|code| code == "locked_window"));
}

#[tokio::test]
async fn governance_kpi_report_returns_transparent_derivations() {
    let db = setup_db().await;
    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-KPI-001",
        "fixed",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;

    let kpi_version = queries::create_pm_plan_version(
        &db,
        plan_id,
        CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            effective_to: None,
            trigger_definition_json: r#"{"interval_unit":"day","interval_value":1}"#.to_string(),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".to_string()),
            required_tools_json: None,
            estimated_duration_hours: Some(4.0),
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: Some("kpi effort baseline".to_string()),
        },
    )
    .await
    .expect("create kpi version");

    queries::publish_pm_plan_version(
        &db,
        PublishPmPlanVersionInput {
            version_id: kpi_version.id,
            expected_row_version: kpi_version.row_version,
        },
    )
    .await
    .expect("publish kpi version");

    queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(3),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate occurrences");

    let mut occurrences = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list occurrences");
    assert!(occurrences.len() >= 2);

    for occ in &mut occurrences {
        assert_eq!(occ.status, "generated");
        *occ = queries::transition_pm_occurrence(
            &db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: "ready_for_scheduling".to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect("to ready");
        *occ = queries::transition_pm_occurrence(
            &db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: "scheduled".to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect("to scheduled");
        *occ = queries::transition_pm_occurrence(
            &db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: "in_progress".to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect("to in_progress");
    }

    let exec_a = queries::execute_pm_occurrence(
        &db,
        ExecutePmOccurrenceInput {
            occurrence_id: occurrences[0].id,
            expected_occurrence_row_version: occurrences[0].row_version,
            execution_result: "completed_no_findings".to_string(),
            note: Some("first pass".to_string()),
            actor_id: Some(1),
            work_order_id: None,
            defer_reason_code: None,
            miss_reason_code: None,
            findings: None,
        },
    )
    .await
    .expect("execute first-pass occurrence");

    let exec_b = queries::execute_pm_occurrence(
        &db,
        ExecutePmOccurrenceInput {
            occurrence_id: occurrences[1].id,
            expected_occurrence_row_version: occurrences[1].row_version,
            execution_result: "completed_with_findings".to_string(),
            note: Some("completed with finding".to_string()),
            actor_id: Some(1),
            work_order_id: None,
            defer_reason_code: None,
            miss_reason_code: None,
            findings: Some(vec![crate::pm::domain::PmExecutionFindingInput {
                finding_type: "LUBE".to_string(),
                severity: Some("low".to_string()),
                description: "Lubrication drift observed".to_string(),
                create_follow_up_di: Some(false),
                create_follow_up_work_order: Some(false),
                follow_up_work_order_type_id: None,
            }]),
        },
    )
    .await
    .expect("execute finding occurrence");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_executions SET actual_labor_hours = ? WHERE id = ?",
        [3.0.into(), exec_a.execution.id.into()],
    ))
    .await
    .expect("set actual labor hours for execution A");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_executions SET actual_labor_hours = ? WHERE id = ?",
        [5.0.into(), exec_b.execution.id.into()],
    ))
    .await
    .expect("set actual labor hours for execution B");

    let report = queries::get_pm_governance_kpi_report(
        &db,
        PmGovernanceKpiInput {
            from: Some("2026-01-01T00:00:00Z".to_string()),
            to: Some("2026-12-31T23:59:59Z".to_string()),
            pm_plan_id: Some(plan_id),
            criticality_code: None,
        },
    )
    .await
    .expect("load governance KPI report");

    // The governance tile reads the one compliance definition (`pm::compliance`), not a second rate.
    let canonical = crate::pm::compliance::compliance_between(
        &db,
        chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
        chrono::DateTime::parse_from_rfc3339("2026-12-31T23:59:59Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
        chrono::Utc::now(),
        crate::pm::compliance::ComplianceScope {
            pm_plan_id: Some(plan_id),
            org_node_id: None,
        },
    )
    .await
    .expect("canonical compliance");
    assert_eq!(report.compliance.numerator, canonical.on_time_count);
    assert_eq!(report.compliance.denominator, canonical.due_count);
    assert_eq!(
        report.compliance.value_pct.is_some(),
        canonical.compliance_pct.is_some()
    );
    assert_eq!(report.first_pass_completion.numerator, 1);
    assert_eq!(report.first_pass_completion.denominator, 2);
    assert_eq!(report.follow_up_ratio.numerator, 0);
    assert_eq!(report.follow_up_ratio.denominator, 1);
    assert_eq!(report.effort_variance.sample_size, 2);
    assert!((report.effort_variance.estimated_hours - 8.0).abs() < 0.0001);
    assert!((report.effort_variance.actual_hours - 8.0).abs() < 0.0001);
    assert!((report.effort_variance.variance_hours).abs() < 0.0001);
    assert!(report.derivation_rules.len() >= 3);
}

#[tokio::test]
async fn scheduler_skips_when_disabled_and_runs_when_due() {
    let db = setup_db().await;
    let _ = create_plan_with_published_version(
        &db,
        "PM-SCHED-001",
        "fixed",
        r#"{"interval_unit":"day","interval_value":7,"anchor_at":"2026-01-01T00:00:00Z"}"#,
    )
    .await;

    crate::pm::generation_control::update_pm_generation_settings(
        &db,
        crate::pm::domain::UpdatePmGenerationSettingsInput {
            enabled: Some(false),
            cron_expr: None,
            horizon_days: Some(14),
        },
    )
    .await
    .expect("disable autopilot");

    crate::pm::scheduler::run_due_pm_generation_for_test(&db)
        .await
        .expect("disabled tick");

    let runs_disabled = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM pm_generation_runs",
            [],
        ))
        .await
        .expect("count runs")
        .expect("row");
    let disabled_count: i64 = runs_disabled.try_get("", "c").expect("c");
    assert_eq!(disabled_count, 0);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_generation_control
         SET enabled = 1, next_run_at = '2000-01-01T00:00:00Z', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = 1",
        [],
    ))
    .await
    .expect("force due");

    crate::pm::scheduler::run_due_pm_generation_for_test(&db)
        .await
        .expect("due tick");

    let control = crate::pm::generation_control::get_pm_generation_control(&db)
        .await
        .expect("control");
    assert_eq!(control.last_status.as_deref(), Some("success"));
    assert!(control.next_run_at.is_some());
    assert_ne!(control.next_run_at.as_deref(), Some("2000-01-01T00:00:00Z"));

    let runs_due = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM pm_generation_runs WHERE trigger_source = 'schedule'",
            [],
        ))
        .await
        .expect("count schedule runs")
        .expect("row");
    let due_count: i64 = runs_due.try_get("", "c").expect("c");
    assert_eq!(due_count, 1);
}

#[tokio::test]
async fn meter_reading_hook_generates_for_equipment_scope() {
    let db = setup_db().await;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment (entity_sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at)
         VALUES ('eq_sync_pm_hook', 'EQ-PM-HOOK', 'PM Hook Asset', 'active_in_service', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("insert equipment");

    let equipment_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE asset_id_code = 'EQ-PM-HOOK'",
            [],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "id")
        .expect("id");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit_value_id, current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at)
         VALUES ('meter_sync_pm_hook', ?, 'Runtime', 'hours', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'H'), 50, 0, 1, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [equipment_id.into()],
    ))
    .await
    .expect("insert meter");

    let meter_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_meters WHERE entity_sync_id = 'meter_sync_pm_hook'",
            [],
        ))
        .await
        .expect("query meter")
        .expect("row")
        .try_get("", "id")
        .expect("id");

    let plan = queries::create_pm_plan(
        &db,
        CreatePmPlanInput {
            code: "PM-METER-HOOK".to_string(),
            title: "Meter hook plan".to_string(),
            description: None,
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(equipment_id),
            strategy_type: "meter".to_string(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: false,
            requires_permit: false,
            is_active: Some(true),
        },
    )
    .await
    .expect("create plan");

    let proposed = queries::transition_pm_plan_lifecycle(
        &db,
        TransitionPmPlanLifecycleInput {
            plan_id: plan.id,
            expected_row_version: plan.row_version,
            next_status: "proposed".to_string(),
        },
    )
    .await
    .expect("propose");
    let approved = queries::transition_pm_plan_lifecycle(
        &db,
        TransitionPmPlanLifecycleInput {
            plan_id: plan.id,
            expected_row_version: proposed.row_version,
            next_status: "approved".to_string(),
        },
    )
    .await
    .expect("approve");
    let version = queries::create_pm_plan_version(
        &db,
        plan.id,
        CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            effective_to: None,
            trigger_definition_json: format!(r#"{{"asset_meter_id":{},"threshold_value":100}}"#, meter_id),
            task_package_json: None,
            required_parts_json: None,
            required_skills_json: Some("[]".to_string()),
            required_tools_json: None,
            estimated_duration_hours: None,
            planned_crew_size: None,
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("version");
    let _ = queries::publish_pm_plan_version(
        &db,
        PublishPmPlanVersionInput {
            version_id: version.id,
            expected_row_version: version.row_version,
        },
    )
    .await
    .expect("publish");
    let _ = approved;

    crate::assets::meters::record_meter_reading(
        &db,
        crate::assets::meters::RecordMeterReadingPayload {
            meter_id,
            reading_value: 150.0,
            reading_at: Some("2026-01-10T12:00:00Z".to_string()),
            source_type: "MANUAL".to_string(),
            source_reference: None,
            quality_flag: None,
        },
        1,
    )
    .await
    .expect("record reading");

    let occ_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM pm_occurrences WHERE pm_plan_id = ?",
            [plan.id.into()],
        ))
        .await
        .expect("count")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert!(occ_count >= 1);

    let meter_runs: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM pm_generation_runs WHERE trigger_source = 'meter'",
            [],
        ))
        .await
        .expect("runs")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert_eq!(meter_runs, 1);
}

#[tokio::test]
async fn wo_close_hook_completes_linked_occurrence_and_regens() {
    let db = setup_db().await;
    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-WOCLOSE-001",
        "floating",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;

    queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(2),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate");

    let mut occ = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list")
    .into_iter()
    .next()
    .expect("occurrence");

    for next in ["ready_for_scheduling", "scheduled", "in_progress"] {
        occ = queries::transition_pm_occurrence(
            &db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: next.to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect("advance");
    }

    // Minimal WO row for execute validation + link.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_orders (entity_sync_id, code, type_id, status_id, title, row_version, created_at, updated_at)
         SELECT ?, 'WO-PM-CLOSE-1', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'WORK.ORDER_TYPES' AND v.code = 'preventive'),
                (SELECT id FROM work_order_statuses WHERE code = 'closed' LIMIT 1),
                'PM close hook', 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        [crate::sync::identity::new_entity_sync_id().into()],
    ))
    .await
    .expect("insert wo");

    let wo_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_orders WHERE code = 'WO-PM-CLOSE-1'",
            [],
        ))
        .await
        .expect("wo")
        .expect("row")
        .try_get("", "id")
        .expect("id");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_occurrences SET linked_work_order_id = ? WHERE id = ?",
        [wo_id.into(), occ.id.into()],
    ))
    .await
    .expect("link wo");

    // The WO analytics feed points back to the PM occurrence and its plan (Phase 6.4).
    let snapshot = crate::wo::analytics::get_wo_analytics_snapshot(&db, wo_id)
        .await
        .expect("analytics snapshot");
    assert_eq!(snapshot.pm_occurrence_id, Some(occ.id));
    assert!(snapshot.pm_plan_code.is_some());

    // Refresh row_version after link update.
    let occ = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list")
    .into_iter()
    .find(|o| o.id == occ.id)
    .expect("occ");
    let _ = occ;

    crate::pm::generation_control::on_work_order_closed_for_pm(&db, wo_id, Some(1)).await;

    let completed: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM pm_occurrences WHERE id = ? AND status = 'completed'",
            [occ.id.into()],
        ))
        .await
        .expect("completed q")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert_eq!(completed, 1);

    let open_after: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM pm_occurrences
             WHERE pm_plan_id = ? AND status NOT IN ('completed','cancelled','missed')",
            [plan_id.into()],
        ))
        .await
        .expect("open q")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert!(open_after >= 1);

    let wo_runs: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM pm_generation_runs WHERE trigger_source = 'wo_close'",
            [],
        ))
        .await
        .expect("runs")
        .expect("row")
        .try_get("", "c")
        .expect("c");
    assert!(wo_runs >= 1);
}

// ── Generated OT content and OT-close outcome (UX unify) ─────────────────────────────────────

async fn seed_pm_equipment(db: &DatabaseConnection) {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models
            (id, entity_sync_id, version_number, status, description, activated_at, created_at, updated_at)
         VALUES (1, 'pm_wo_model', 1, 'active', 'PM WO model',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        "INSERT OR IGNORE INTO org_node_types
            (id, entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, is_root_type, is_active, created_at, updated_at)
         VALUES (1, 'pm_wo_type', 1, 'SITE', 'Site', 1, 1, 1, 1,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        "INSERT OR IGNORE INTO org_nodes
            (id, entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
             created_at, updated_at, row_version, structure_model_id)
         VALUES (1, 'pm_wo_node', 'SITE-1', 'Site', 1, NULL, '/', 0, 'active',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 1)",
        "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at)
         VALUES (1, 'eq_sync_pm_wo', 'P-101', 'Pompe de reprise', 'active_in_service', 1,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        "INSERT OR IGNORE INTO articles (article_code, article_name, unit_value_id, is_active)
         SELECT 'BRG-6204', 'Roulement 6204', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'U'), 1",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("seed PM equipment");
    }
    crate::locale::write_locale_config(db, "locale.default_language", "fr")
        .await
        .expect("tenant locale");
}

/// Active plan on equipment P-101 whose published version prescribes tasks, parts and tools;
/// returns its first generated occurrence moved to `ready_for_scheduling` with its OT.
/// A generated occurrence of a published plan whose version prescribes tasks, parts and tools.
async fn generated_occurrence(db: &DatabaseConnection, code: &str) -> crate::pm::domain::PmOccurrence {
    seed_pm_equipment(db).await;
    let plan = queries::create_pm_plan(
        db,
        CreatePmPlanInput {
            code: code.to_string(),
            title: "Graissage pompe".to_string(),
            description: Some("Routine mensuelle".to_string()),
            asset_scope_type: "equipment".to_string(),
            asset_scope_id: Some(1),
            strategy_type: "fixed".to_string(),
            criticality_value_id: None,
            assigned_team_id: None,
            requires_shutdown: true,
            requires_permit: true,
            is_active: Some(true),
        },
    )
    .await
    .expect("create plan");
    assert_eq!(plan.asset_code.as_deref(), Some("P-101"));
    let mut plan_rv = plan.row_version;
    for next in ["proposed", "approved"] {
        plan_rv = queries::transition_pm_plan_lifecycle(
            db,
            TransitionPmPlanLifecycleInput {
                plan_id: plan.id,
                expected_row_version: plan_rv,
                next_status: next.to_string(),
            },
        )
        .await
        .expect("lifecycle")
        .row_version;
    }
    let version = queries::create_pm_plan_version(
        db,
        plan.id,
        CreatePmPlanVersionInput {
            effective_from: "2026-01-01T00:00:00Z".to_string(),
            effective_to: None,
            trigger_definition_json: r#"{"interval_unit":"day","interval_value":1}"#.to_string(),
            task_package_json: Some(r#"{"steps":["Graisser le palier","Contrôler l'étanchéité"]}"#.to_string()),
            required_parts_json: Some(
                r#"[{"article_code":"BRG-6204","quantity":2,"required":true},{"code":"Graisse EP2","required":true}]"#
                    .to_string(),
            ),
            required_skills_json: Some("[]".to_string()),
            required_tools_json: Some(r#"["Pompe à graisse"]"#.to_string()),
            estimated_duration_hours: Some(1.5),
            planned_crew_size: Some(2),
            estimated_labor_cost: None,
            estimated_parts_cost: None,
            estimated_service_cost: None,
            change_reason: None,
        },
    )
    .await
    .expect("create version");
    queries::publish_pm_plan_version(
        db,
        PublishPmPlanVersionInput {
            version_id: version.id,
            expected_row_version: version.row_version,
        },
    )
    .await
    .expect("publish");
    queries::generate_pm_occurrences(
        db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(1),
            pm_plan_id: Some(plan.id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate");
    let occ = queries::list_pm_occurrences(
        db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan.id),
            status: Some("generated".to_string()),
            due_from: None,
            due_to: None,
            include_completed: Some(false),
        },
    )
    .await
    .expect("list")
    .into_iter()
    .next()
    .expect("occurrence");
    assert_eq!(occ.asset_code.as_deref(), Some("P-101"));
    assert_eq!(occ.asset_name.as_deref(), Some("Pompe de reprise"));
    occ
}

async fn occurrence_with_generated_wo(db: &DatabaseConnection, code: &str) -> crate::pm::domain::PmOccurrence {
    let occ = generated_occurrence(db, code).await;
    queries::transition_pm_occurrence(
        db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "ready_for_scheduling".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(true),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("ready with OT")
}

/// AU-20 (a): the occurrence is claimed and its OT created in one transaction. When someone else
/// changes the occurrence while the OT is being created (simulated by a trigger firing on the OT
/// insert), the transition is refused and no orphan OT is left.
#[tokio::test]
async fn a_stale_occurrence_claim_leaves_no_orphan_work_order() {
    let db = setup_db().await;
    let occ = generated_occurrence(&db, "PM-WO-RACE").await;
    let wos_before = count_for(&db, "SELECT COUNT(*) AS c FROM work_orders WHERE id > ?", 0).await;
    db.execute_unprepared(&format!(
        "CREATE TEMP TRIGGER au20_concurrent_edit AFTER INSERT ON work_orders BEGIN            UPDATE pm_occurrences SET row_version = row_version + 1 WHERE id = {};          END",
        occ.id
    ))
    .await
    .expect("trigger");

    let err = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "ready_for_scheduling".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(true),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect_err("the occurrence changed meanwhile");
    assert!(
        matches!(&err, AppError::ValidationFailed(l) if l.iter().any(|m| m.starts_with("GATE_RECORD_CHANGED_ELSEWHERE"))),
        "{err:?}"
    );
    db.execute_unprepared("DROP TRIGGER au20_concurrent_edit")
        .await
        .expect("drop trigger");

    assert_eq!(
        count_for(&db, "SELECT COUNT(*) AS c FROM work_orders WHERE id > ?", 0).await,
        wos_before,
        "no orphan OT"
    );
    let after = queries::get_pm_occurrence_by_id(&db, occ.id).await.expect("occurrence");
    assert_eq!(after.status, "generated");
    assert_eq!(
        after.row_version, occ.row_version,
        "the trigger's edit rolled back with the claim"
    );
    assert!(after.linked_work_order_id.is_none());

    // Reloaded, the same transition goes through and links its OT.
    let ready = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: after.row_version,
            next_status: "ready_for_scheduling".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(true),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("ready with OT");
    assert!(ready.linked_work_order_id.is_some());
    assert_eq!(
        count_for(&db, "SELECT COUNT(*) AS c FROM work_orders WHERE id > ?", 0).await,
        wos_before + 1
    );
}

/// AU-20 (a), review S3: the PM content (tasks, tools, parts with their reservation) is added in
/// the transaction that creates the OT and claims the occurrence. A part whose reservation is
/// refused leaves no OT, no reservation and the occurrence unchanged; the transition can be made
/// again once the stock can be reserved.
#[tokio::test]
async fn a_refused_part_reservation_leaves_no_work_order_and_the_occurrence_unchanged() {
    let db = setup_db().await;
    let occ = generated_occurrence(&db, "PM-WO-PART-REFUSED").await;
    // Stock of the PM part, so the OT content reserves it.
    db.execute_unprepared(
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty, updated_at) \
         SELECT a.id, l.warehouse_id, l.id, 10, 0, 10, strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         FROM articles a, stock_locations l WHERE a.article_code = 'BRG-6204' AND l.code = 'MAIN-BIN'",
    )
    .await
    .expect("stock of the PM part");
    let wos_before = count_for(&db, "SELECT COUNT(*) AS c FROM work_orders WHERE id > ?", 0).await;
    let reservations = |db: &DatabaseConnection| {
        let db = db.clone();
        async move { count_for(&db, "SELECT COUNT(*) AS c FROM stock_reservations WHERE id > ?", 0).await }
    };
    let reservations_before = reservations(&db).await;
    db.execute_unprepared(
        "CREATE TEMP TRIGGER s3_reservation_refused BEFORE INSERT ON stock_reservations \
         BEGIN SELECT RAISE(ABORT, 'forced reservation refusal'); END",
    )
    .await
    .expect("trigger");
    let transition = |expected_row_version: i64| TransitionPmOccurrenceInput {
        occurrence_id: occ.id,
        expected_row_version,
        next_status: "ready_for_scheduling".to_string(),
        reason_code: None,
        note: None,
        generate_work_order: Some(true),
        work_order_type_id: None,
        actor_id: Some(1),
    };

    queries::transition_pm_occurrence(&db, transition(occ.row_version))
        .await
        .expect_err("the part cannot be reserved");
    db.execute_unprepared("DROP TRIGGER s3_reservation_refused")
        .await
        .expect("drop trigger");

    assert_eq!(
        count_for(&db, "SELECT COUNT(*) AS c FROM work_orders WHERE id > ?", 0).await,
        wos_before,
        "no half-filled OT"
    );
    assert_eq!(reservations(&db).await, reservations_before, "no reservation");
    let after = queries::get_pm_occurrence_by_id(&db, occ.id).await.expect("occurrence");
    assert_eq!(after.status, "generated");
    assert_eq!(after.row_version, occ.row_version);
    assert!(after.linked_work_order_id.is_none());

    // The same transition goes through once the reservation can be made, with all its content.
    let ready = queries::transition_pm_occurrence(&db, transition(after.row_version))
        .await
        .expect("ready with OT");
    let wo_id = ready.linked_work_order_id.expect("linked OT");
    let parts = crate::wo::parts::list_wo_parts(&db, wo_id).await.expect("parts");
    assert_eq!(parts.len(), 1);
    assert!(parts[0].reservation_id.is_some(), "the part is reserved");
    assert_eq!(crate::wo::tasks::list_tasks(&db, wo_id).await.expect("tasks").len(), 2);
}

/// AU-73, review S4: a PM execution whose finding asks for a follow-up on equipment retired since
/// is refused before anything is written (no completed occurrence without its follow-up).
#[tokio::test]
async fn an_execution_with_a_follow_up_on_retired_equipment_writes_nothing() {
    let db = setup_db().await;
    let mut occ = generated_occurrence(&db, "PM-EXEC-RETIRED").await;
    for next in ["ready_for_scheduling", "scheduled", "in_progress"] {
        occ = queries::transition_pm_occurrence(
            &db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: next.to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect(next);
    }
    crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
        .await
        .expect("status tags");
    db.execute_unprepared("UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED' WHERE id = 1")
        .await
        .expect("retire equipment");

    for (di, wo, code) in [
        (true, false, "GATE_DI_EQUIPMENT_RETIRED"),
        (false, true, "GATE_WO_EQUIPMENT_RETIRED"),
    ] {
        let err = queries::execute_pm_occurrence(
            &db,
            crate::pm::domain::ExecutePmOccurrenceInput {
                occurrence_id: occ.id,
                expected_occurrence_row_version: occ.row_version,
                execution_result: "completed_with_findings".to_string(),
                note: None,
                actor_id: Some(1),
                work_order_id: None,
                defer_reason_code: None,
                miss_reason_code: None,
                findings: Some(vec![crate::pm::domain::PmExecutionFindingInput {
                    finding_type: "LEAK".to_string(),
                    severity: None,
                    description: "Fuite au presse-étoupe".to_string(),
                    create_follow_up_di: Some(di),
                    create_follow_up_work_order: Some(wo),
                    follow_up_work_order_type_id: None,
                }]),
            },
        )
        .await
        .expect_err("retired equipment");
        assert!(
            matches!(&err, AppError::ValidationFailed(l) if l.iter().any(|m| m.starts_with(code))),
            "{code}: {err:?}"
        );
        let after = queries::get_pm_occurrence_by_id(&db, occ.id).await.expect("occurrence");
        assert_eq!(after.status, "in_progress", "{code}: occurrence unchanged");
        assert_eq!(after.row_version, occ.row_version);
        assert_eq!(
            count_for(
                &db,
                "SELECT COUNT(*) AS c FROM pm_executions WHERE pm_occurrence_id = ?",
                occ.id
            )
            .await,
            0,
            "{code}: no execution recorded"
        );
    }
}

/// #185 N-R2-1: every refusal an execution can meet is met before anything is written: a blank
/// finding, a follow-up request on equipment with no location or an inactive one, a follow-up
/// without an acting user, a follow-up work order of a type that is not corrective, and an unknown
/// work order. The occurrence is unchanged and no execution is recorded.
#[tokio::test]
async fn an_execution_refused_for_any_reason_writes_nothing() {
    type Case = (
        &'static str,
        &'static str,
        fn(&mut crate::pm::domain::ExecutePmOccurrenceInput),
        &'static str,
    );
    fn finding_of(
        input: &mut crate::pm::domain::ExecutePmOccurrenceInput,
    ) -> &mut crate::pm::domain::PmExecutionFindingInput {
        &mut input.findings.as_mut().expect("findings")[0]
    }
    let cases: [Case; 8] = [
        (
            "blank description",
            "",
            |i| finding_of(i).description = "  ".into(),
            "GATE_PM_FINDING_DESCRIPTION_REQUIRED",
        ),
        (
            "blank type",
            "",
            |i| finding_of(i).finding_type = " ".into(),
            "GATE_PM_FINDING_TYPE_REQUIRED",
        ),
        (
            "no location",
            "UPDATE equipment SET installed_at_node_id = NULL WHERE id = 1",
            |i| finding_of(i).create_follow_up_di = Some(true),
            "GATE_PM_FOLLOWUP_NO_LOCATION",
        ),
        (
            "inactive location",
            "UPDATE org_nodes SET status = 'inactive' WHERE id = 1",
            |i| finding_of(i).create_follow_up_di = Some(true),
            "ORG_NODE_NOT_ACTIVE",
        ),
        (
            "no acting user (request)",
            "",
            |i| {
                finding_of(i).create_follow_up_di = Some(true);
                i.actor_id = None;
            },
            "GATE_PM_ACTING_USER_REQUIRED",
        ),
        (
            "no acting user (work order)",
            "",
            |i| {
                finding_of(i).create_follow_up_work_order = Some(true);
                i.actor_id = None;
            },
            "GATE_PM_ACTING_USER_REQUIRED",
        ),
        (
            "unknown work order type",
            "",
            |i| {
                finding_of(i).create_follow_up_work_order = Some(true);
                finding_of(i).follow_up_work_order_type_id = Some(987_654);
            },
            "GATE_PM_WO_TYPE_UNKNOWN",
        ),
        (
            "unknown work order",
            "",
            |i| i.work_order_id = Some(987_654),
            "GATE_PM_WO_UNKNOWN",
        ),
    ];
    for (label, setup_sql, change, code) in cases {
        let db = setup_db().await;
        let mut occ = generated_occurrence(&db, "PM-EXEC-REFUSED").await;
        for next in ["ready_for_scheduling", "scheduled", "in_progress"] {
            occ = queries::transition_pm_occurrence(
                &db,
                TransitionPmOccurrenceInput {
                    occurrence_id: occ.id,
                    expected_row_version: occ.row_version,
                    next_status: next.to_string(),
                    reason_code: None,
                    note: None,
                    generate_work_order: Some(false),
                    work_order_type_id: None,
                    actor_id: Some(1),
                },
            )
            .await
            .expect(next);
        }
        if !setup_sql.is_empty() {
            db.execute_unprepared(setup_sql).await.expect(label);
        }
        let mut input = crate::pm::domain::ExecutePmOccurrenceInput {
            occurrence_id: occ.id,
            expected_occurrence_row_version: occ.row_version,
            execution_result: "completed_with_findings".to_string(),
            note: None,
            actor_id: Some(1),
            work_order_id: None,
            defer_reason_code: None,
            miss_reason_code: None,
            findings: Some(vec![crate::pm::domain::PmExecutionFindingInput {
                finding_type: "LEAK".to_string(),
                severity: None,
                description: "Fuite au presse-étoupe".to_string(),
                create_follow_up_di: Some(false),
                create_follow_up_work_order: Some(false),
                follow_up_work_order_type_id: None,
            }]),
        };
        change(&mut input);
        let err = queries::execute_pm_occurrence(&db, input).await.expect_err(label);
        assert!(format!("{err:?}").contains(code), "{label}: {err:?}");
        let after = queries::get_pm_occurrence_by_id(&db, occ.id).await.expect("occurrence");
        assert_eq!(after.status, "in_progress", "{label}: occurrence unchanged");
        assert_eq!(after.row_version, occ.row_version, "{label}");
        assert_eq!(
            count_for(
                &db,
                "SELECT COUNT(*) AS c FROM pm_executions WHERE pm_occurrence_id = ?",
                occ.id
            )
            .await,
            0,
            "{label}: no execution recorded"
        );
    }
}

async fn count_for(db: &DatabaseConnection, sql: &str, id: i64) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
        .await
        .expect("count query")
        .expect("count row")
        .try_get("", "c")
        .expect("c")
}

#[tokio::test]
async fn generated_work_order_carries_the_pm_version_content() {
    let db = setup_db().await;
    let occ = occurrence_with_generated_wo(&db, "PM-WO-CONTENT").await;
    let wo_id = occ.linked_work_order_id.expect("linked OT");
    assert!(occ.linked_work_order_code.is_some());

    let wo = crate::wo::queries::get_work_order(&db, wo_id)
        .await
        .expect("get OT")
        .expect("OT exists");
    assert_eq!(wo.title, "Graissage pompe — P-101");
    let description = wo.description.clone().expect("description");
    assert!(description.starts_with("Routine mensuelle"));
    assert!(description.contains("Tâches :\n1. Graisser le palier\n2. Contrôler l'étanchéité"));
    assert!(description.contains("- BRG-6204 — Roulement 6204 × 2"));
    assert!(
        description.contains("- Graisse EP2"),
        "free-text part kept in the description"
    );
    assert!(description.contains("Outillage requis :\n- Pompe à graisse"));
    assert!(description.contains("Arrêt de l'équipement requis."));
    assert_eq!(wo.expected_duration_hours, Some(1.5));
    assert_eq!(wo.planned_crew_size, Some(2), "the PM crew size is copied onto the OT");
    assert!(wo.notes.is_none(), "no generated English note on the OT");

    let permit = count_for(&db, "SELECT requires_permit AS c FROM work_orders WHERE id = ?", wo_id).await;
    assert_eq!(permit, 1);

    let tasks = crate::wo::tasks::list_tasks(&db, wo_id).await.expect("tasks");
    assert_eq!(
        tasks.iter().map(|t| t.task_description.as_str()).collect::<Vec<_>>(),
        vec!["Graisser le palier", "Contrôler l'étanchéité"]
    );
    assert!(tasks.iter().all(|t| t.is_mandatory));

    let parts = crate::wo::parts::list_wo_parts(&db, wo_id).await.expect("parts");
    assert_eq!(
        parts.len(),
        1,
        "only the article with a quantity becomes a planned part"
    );
    assert!((parts[0].quantity_planned - 2.0).abs() < f64::EPSILON);
    assert_eq!(parts[0].article_ref.as_deref(), Some("BRG-6204"));

    let tools = crate::wo::tools::list_tools(&db, wo_id).await.expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_label, "Pompe à graisse");
}

#[tokio::test]
async fn generated_work_order_inherits_the_plan_shutdown_flag() {
    let db = setup_db().await;
    // The plan of this helper has `requires_shutdown = true`.
    let occ = occurrence_with_generated_wo(&db, "PM-WO-SHUTDOWN").await;
    let wo_id = occ.linked_work_order_id.expect("linked OT");
    let wo = crate::wo::queries::get_work_order(&db, wo_id)
        .await
        .expect("get OT")
        .expect("OT exists");
    assert!(
        wo.requires_shutdown,
        "the OT generated from a plan that needs a shutdown is flagged \"needs shutdown\", so planning checks its windows"
    );
    let stored = count_for(
        &db,
        "SELECT requires_shutdown AS c FROM work_orders WHERE id = ?",
        wo_id,
    )
    .await;
    assert_eq!(stored, 1);
}

#[tokio::test]
async fn wo_close_records_the_ot_outcome_not_a_blanket_no_findings() {
    let db = setup_db().await;
    let occ = occurrence_with_generated_wo(&db, "PM-WO-OUTCOME").await;
    let wo_id = occ.linked_work_order_id.expect("linked OT");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_tasks SET is_completed = 1, result_code = 'nok', notes = 'fuite au joint'
         WHERE work_order_id = ? AND sequence_order = 2",
        [wo_id.into()],
    ))
    .await
    .expect("task nok");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET corrective_action_summary = 'Joint remplacé' WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("close-out summary");

    crate::pm::generation_control::on_work_order_closed_for_pm(&db, wo_id, Some(1)).await;

    let executions = queries::list_pm_executions(
        &db,
        crate::pm::domain::PmExecutionFilter {
            occurrence_id: Some(occ.id),
            pm_plan_id: None,
        },
    )
    .await
    .expect("executions");
    assert_eq!(executions.len(), 1);
    let execution = &executions[0];
    assert_eq!(execution.execution_result, "completed_with_findings");
    assert_eq!(execution.notes.as_deref(), Some("Joint remplacé"));
    assert_eq!(execution.work_order_id, Some(wo_id));

    let findings = queries::list_pm_findings(&db, execution.id).await.expect("findings");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].finding_type,
        crate::pm::generation_control::PM_FINDING_WO_TASK_NOK
    );
    assert_eq!(findings[0].description, "Contrôler l'étanchéité — fuite au joint");
    assert!(findings[0].severity.is_none());

    // No fixed-language text written by the hook; the transitions carry a code instead.
    let english_notes = count_for(
        &db,
        "SELECT COUNT(*) AS c FROM pm_occurrence_transitions
         WHERE pm_occurrence_id = ? AND (note LIKE '%auto-pilot%' OR note LIKE '%Advanced%')",
        occ.id,
    )
    .await;
    assert_eq!(english_notes, 0);
    let coded = count_for(
        &db,
        "SELECT COUNT(*) AS c FROM pm_occurrence_transitions
         WHERE pm_occurrence_id = ? AND reason_code = 'WO_CLOSED'",
        occ.id,
    )
    .await;
    assert!(coded >= 2, "ready -> scheduled -> in_progress are coded WO_CLOSED");
}

#[tokio::test]
async fn wo_close_without_findings_on_the_ot_completes_without_findings() {
    let db = setup_db().await;
    let occ = occurrence_with_generated_wo(&db, "PM-WO-CLEAN").await;
    let wo_id = occ.linked_work_order_id.expect("linked OT");

    crate::pm::generation_control::on_work_order_closed_for_pm(&db, wo_id, Some(1)).await;

    let executions = queries::list_pm_executions(
        &db,
        crate::pm::domain::PmExecutionFilter {
            occurrence_id: Some(occ.id),
            pm_plan_id: None,
        },
    )
    .await
    .expect("executions");
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].execution_result, "completed_no_findings");
    assert!(executions[0].notes.is_none(), "no generated English note");
    let completed = count_for(
        &db,
        "SELECT COUNT(*) AS c FROM pm_occurrences WHERE id = ? AND status = 'completed'",
        occ.id,
    )
    .await;
    assert_eq!(completed, 1);
}

#[test]
fn meter_threshold_counts_from_start_reading_without_replaying_missed_intervals() {
    // Not reached yet: next threshold ahead.
    assert!((queries::next_meter_threshold(1000.0, 250.0, 1100.0, None) - 1250.0).abs() < 1e-9);
    // Crossed several intervals: the latest crossed threshold only.
    assert!((queries::next_meter_threshold(1000.0, 250.0, 1800.0, None) - 1750.0).abs() < 1e-9);
    // Already generated for the latest crossed threshold: the next one.
    assert!((queries::next_meter_threshold(1000.0, 250.0, 1800.0, Some(1750.0)) - 2000.0).abs() < 1e-9);
}

async fn generate_meter_plan(db: &DatabaseConnection, plan_id: i64) -> i64 {
    queries::generate_pm_occurrences(
        db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(7),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("meter generation")
    .generated_count
}

#[tokio::test]
async fn meter_plan_on_a_chosen_asset_meter_recurs_every_interval() {
    let db = setup_db().await;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at)
         VALUES (1, 'eq_sync_pm_interval', 'CMP-01', 'Compresseur', 'active_in_service',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            .to_string(),
    ))
    .await
    .expect("insert equipment");
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO equipment_meters (id, entity_sync_id, equipment_id, name, meter_type, unit_value_id, current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at)
         VALUES (7, 'meter_sync_interval', 1, 'Heures marche', 'hours', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id JOIN reference_domains d ON d.id = s.domain_id WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'H'), 1100, 0, 1, 1,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            .to_string(),
    ))
    .await
    .expect("insert meter");

    let (plan_id, _version_id) = create_plan_with_published_version(
        &db,
        "PM-METER-INT",
        "meter",
        r#"{"asset_meter_id":7,"interval_value":250,"start_reading":1000}"#,
    )
    .await;
    // 1100 h < 1250 h: nothing due yet.
    assert_eq!(generate_meter_plan(&db, plan_id).await, 0);

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE equipment_meters SET current_reading = 1260 WHERE id = 7".to_string(),
    ))
    .await
    .expect("reading");
    assert_eq!(generate_meter_plan(&db, plan_id).await, 1);
    assert_eq!(
        generate_meter_plan(&db, plan_id).await,
        0,
        "same threshold is not generated twice"
    );

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE equipment_meters SET current_reading = 1510 WHERE id = 7".to_string(),
    ))
    .await
    .expect("reading");
    assert_eq!(
        generate_meter_plan(&db, plan_id).await,
        1,
        "next interval (1500 h) is due"
    );

    let thresholds: Vec<f64> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT due_meter_value FROM pm_occurrences WHERE pm_plan_id = ? ORDER BY due_meter_value",
            [plan_id.into()],
        ))
        .await
        .expect("thresholds")
        .iter()
        .map(|r| r.try_get::<f64>("", "due_meter_value").expect("value"))
        .collect();
    assert_eq!(thresholds, vec![1250.0, 1500.0]);
}

// ── Owner decision 2026-10-15 (c): a PM follow-up OT is deferred corrective work ─────────────

/// Site, node type and asset the PM execution tests run on.
async fn seed_execution_asset(db: &DatabaseConnection) {
    for sql in [
        "INSERT OR IGNORE INTO org_structure_models
            (id, entity_sync_id, version_number, status, description, activated_at, created_at, updated_at)
         VALUES (1, 'pm_org_model', 1, 'active', 'PM test model',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        "INSERT OR IGNORE INTO org_node_types
            (id, entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, is_root_type, is_active,
             created_at, updated_at)
         VALUES (1, 'pm_org_type', 1, 'SITE', 'Site', 1, 1, 1, 1,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        "INSERT OR IGNORE INTO org_nodes
            (id, entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
             created_at, updated_at, row_version, structure_model_id)
         VALUES (1, 'pm_org_node', 'SITE-1', 'Test Site', 1, NULL, '/', 0, 'active',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 1)",
        "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id,
             created_at, updated_at)
         VALUES (1, 'eq_sync_pm_exec', 'EQ-PM-EXEC', 'PM Execution Asset', 'active_in_service', 1,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("seed execution asset");
    }
}

/// Brings the first occurrence of `plan_id` to `in_progress`.
async fn occurrence_in_progress(db: &DatabaseConnection, plan_id: i64) -> crate::pm::domain::PmOccurrence {
    queries::generate_pm_occurrences(
        db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(2),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate occurrences");
    let mut occ = queries::list_pm_occurrences(
        db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list occurrences")
    .remove(0);
    for next in ["ready_for_scheduling", "scheduled", "in_progress"] {
        occ = queries::transition_pm_occurrence(
            db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: next.to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("to {next}: {e:?}"));
    }
    occ
}

fn finding(kind: &str, di: bool, wo: bool, type_id: Option<i64>) -> crate::pm::domain::PmExecutionFindingInput {
    crate::pm::domain::PmExecutionFindingInput {
        finding_type: kind.to_string(),
        severity: Some("high".to_string()),
        description: format!("{kind} found during the PM round"),
        create_follow_up_di: Some(di),
        create_follow_up_work_order: Some(wo),
        follow_up_work_order_type_id: type_id,
    }
}

fn execute_input(
    occ: &crate::pm::domain::PmOccurrence,
    findings: Vec<crate::pm::domain::PmExecutionFindingInput>,
) -> ExecutePmOccurrenceInput {
    ExecutePmOccurrenceInput {
        occurrence_id: occ.id,
        expected_occurrence_row_version: occ.row_version,
        execution_result: "completed_with_findings".to_string(),
        note: None,
        actor_id: Some(1),
        work_order_id: None,
        defer_reason_code: None,
        miss_reason_code: None,
        findings: Some(findings),
    }
}

async fn type_id_of(db: &DatabaseConnection, code: &str) -> i64 {
    crate::wo::types::list_work_order_types(db)
        .await
        .expect("types")
        .into_iter()
        .find(|t| t.code == code)
        .unwrap_or_else(|| panic!("shipped type {code}"))
        .id
}

#[tokio::test]
async fn pm_follow_up_work_orders_are_corrective_failures_found_by_pm() {
    let db = setup_db().await;
    seed_execution_asset(&db).await;
    let (plan_id, _) = create_plan_with_published_version(
        &db,
        "PM-FIND-001",
        "fixed",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;
    let occ = occurrence_in_progress(&db, plan_id).await;
    let emergency = type_id_of(&db, "emergency").await;

    let done = queries::execute_pm_occurrence(
        &db,
        execute_input(
            &occ,
            vec![
                finding("BEARING_WEAR", false, true, None),
                finding("SEAL_LEAK", false, true, Some(emergency)),
                finding("PAINT", true, false, None),
            ],
        ),
    )
    .await
    .expect("execute occurrence");

    // Default follow-up OT: deferred corrective work, with its PM origin on the OT.
    let default_wo = done.findings[0].follow_up_work_order_id.expect("follow-up OT");
    let wo = crate::wo::queries::get_work_order(&db, default_wo)
        .await
        .expect("read OT")
        .expect("OT");
    assert_eq!(wo.nature_code.as_deref(), Some("corrective_deferred"));
    assert_eq!(wo.type_code.as_deref(), Some("corrective"));
    assert_eq!(wo.source_pm_plan_id, Some(plan_id));
    assert_eq!(wo.source_pm_plan_code.as_deref(), Some("PM-FIND-001"));
    assert_eq!(wo.source_pm_plan_title.as_deref(), Some("PM-FIND-001 title"));
    assert_eq!(wo.source_pm_occurrence_id, Some(occ.id));

    // An explicit corrective type is kept.
    let urgent_wo = done.findings[1].follow_up_work_order_id.expect("second OT");
    let urgent = crate::wo::queries::get_work_order(&db, urgent_wo)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(urgent.nature_code.as_deref(), Some("corrective_immediate"));

    // The count PM and reliability views read: corrective OTs raised from findings.
    let report = queries::get_pm_governance_kpi_report(
        &db,
        PmGovernanceKpiInput {
            from: Some("2000-01-01T00:00:00Z".to_string()),
            to: Some("2100-01-01T00:00:00Z".to_string()),
            pm_plan_id: Some(plan_id),
            criticality_code: None,
        },
    )
    .await
    .expect("kpi report");
    assert_eq!(report.failures_found_by_pm, 2, "the DI-only finding is not an OT");
    assert_eq!(report.follow_up_ratio.numerator, 3);
    assert_eq!(report.follow_up_ratio.denominator, 3);
}

#[tokio::test]
async fn a_pm_follow_up_work_order_of_a_preventive_type_is_refused() {
    let db = setup_db().await;
    seed_execution_asset(&db).await;
    let (plan_id, _) = create_plan_with_published_version(
        &db,
        "PM-FIND-002",
        "fixed",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;
    let occ = occurrence_in_progress(&db, plan_id).await;
    let preventive = type_id_of(&db, "preventive").await;

    let err = queries::execute_pm_occurrence(
        &db,
        execute_input(&occ, vec![finding("BEARING_WEAR", false, true, Some(preventive))]),
    )
    .await
    .expect_err("a follow-up OT is corrective work");
    assert!(
        matches!(err, AppError::ValidationFailed(ref m) if m.iter().any(|s| s.contains(queries::GATE_PM_FOLLOW_UP_NOT_CORRECTIVE))),
        "{err:?}"
    );
}

/// The governance "overdue" rate uses the one overdue rule (`PM_OVERDUE_SQL`): an occurrence past
/// its due date but still inside its tolerance window is not overdue.
#[tokio::test]
async fn governance_overdue_respects_the_tolerance_window() {
    let db = setup_db().await;
    let (plan_id, _) = create_plan_with_published_version(
        &db,
        "PM-GOV-TOL",
        "fixed",
        r#"{"interval_unit":"day","interval_value":7}"#,
    )
    .await;
    queries::generate_pm_occurrences(
        &db,
        GeneratePmOccurrencesInput {
            as_of: Some("2026-01-10T00:00:00Z".to_string()),
            horizon_days: Some(20),
            pm_plan_id: Some(plan_id),
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
    )
    .await
    .expect("generate");
    let occurrences = queries::list_pm_occurrences(
        &db,
        PmOccurrenceFilter {
            pm_plan_id: Some(plan_id),
            status: None,
            due_from: None,
            due_to: None,
            include_completed: Some(true),
        },
    )
    .await
    .expect("list");
    assert!(occurrences.len() >= 2, "two occurrences to compare");
    let now = chrono::Utc::now();
    let yesterday = (now - chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let tomorrow = (now + chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let last_week = (now - chrono::Duration::days(7))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    // One past due but inside its window, one past the end of its window.
    for (occ, window_end) in [(&occurrences[0], &tomorrow), (&occurrences[1], &last_week)] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE pm_occurrences SET due_at = ?, window_end_at = ?, status = 'scheduled' WHERE id = ?",
            [yesterday.clone().into(), window_end.clone().into(), occ.id.into()],
        ))
        .await
        .expect("set window");
    }
    for occ in occurrences.iter().skip(2) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE pm_occurrences SET status = 'cancelled' WHERE id = ?",
            [occ.id.into()],
        ))
        .await
        .expect("cancel the rest");
    }
    let report = queries::get_pm_governance_kpi_report(
        &db,
        PmGovernanceKpiInput {
            from: Some("2000-01-01T00:00:00Z".to_string()),
            to: Some("2100-01-01T00:00:00Z".to_string()),
            pm_plan_id: Some(plan_id),
            criticality_code: None,
        },
    )
    .await
    .expect("kpi report");
    assert_eq!(report.overdue_risk.denominator, 2);
    assert_eq!(report.overdue_risk.numerator, 1, "only the one past its window");
}

/// Reasons and finding severities of the PM dialogs are governed lists: shipped values listed,
/// unknown codes refused before anything is written, known ones stored as the list code.
#[tokio::test]
async fn pm_reasons_and_severities_come_from_the_reference_lists() {
    let db = setup_db().await;
    let options = crate::pm::reason_catalog::list_pm_execution_options(&db)
        .await
        .expect("options");
    let reasons: Vec<&str> = options.reasons.iter().map(|o| o.code.as_str()).collect();
    assert!(reasons.contains(&"capacity_constraint") && reasons.contains(&"other"));
    assert_eq!(
        options.severities.iter().map(|o| o.code.as_str()).collect::<Vec<_>>(),
        vec!["low", "medium", "high", "critical"]
    );
    assert!(options.reasons.iter().all(|o| o.label_en.is_some()));

    seed_execution_asset(&db).await;
    let (plan_id, _) = create_plan_with_published_version(
        &db,
        "PM-REASONS-001",
        "fixed",
        r#"{"interval_unit":"day","interval_value":1}"#,
    )
    .await;
    let occ = occurrence_in_progress(&db, plan_id).await;

    // Unknown miss reason / severity: coded refusal, nothing written.
    let mut missed = execute_input(&occ, Vec::new());
    missed.execution_result = "missed".to_string();
    missed.findings = None;
    missed.miss_reason_code = Some("forgot".to_string());
    let err = queries::execute_pm_occurrence(&db, missed.clone())
        .await
        .expect_err("unknown reason");
    assert!(format!("{err:?}").contains("PM_CODE_NOT_IN_LIST"), "{err:?}");
    let mut bad_finding = finding("leak", false, false, None);
    bad_finding.severity = Some("catastrophic".to_string());
    let err = queries::execute_pm_occurrence(&db, execute_input(&occ, vec![bad_finding]))
        .await
        .expect_err("unknown severity");
    assert!(format!("{err:?}").contains("PM_CODE_NOT_IN_LIST"), "{err:?}");

    // A known reason (any case) is stored as the list code (deferral: allowed from in progress).
    let mut deferred = missed;
    deferred.execution_result = "deferred".to_string();
    deferred.miss_reason_code = None;
    deferred.defer_reason_code = Some("MATERIAL_MISSING".to_string());
    let done = queries::execute_pm_occurrence(&db, deferred)
        .await
        .expect("known reason");
    assert_eq!(done.occurrence.deferral_reason.as_deref(), Some("material_missing"));
}

/// Owner decision D2 (a), audit finding AU-04: the work order a PM occurrence generates gets the
/// plant's permit rule (here on the preventive work type) on top of the PM version's permit flag.
#[tokio::test]
async fn generated_work_order_gets_the_plant_permit_rule() {
    let db = setup_db().await;
    let preventive =
        crate::wo::types::default_type_for_nature(&db, crate::wo::types::MaintenanceNature::PreventivePredetermined)
            .await
            .expect("preventive type");
    crate::permit::requirement_rules::add_published_rule(
        &db,
        "PREVENTIVE_LOTO",
        serde_json::json!({ "work_type_code": preventive.code, "permit_type_code": "loto" }),
    )
    .await;
    let occ = occurrence_with_generated_wo(&db, "PM-WO-RULE").await;
    let wo = crate::wo::queries::get_work_order(&db, occ.linked_work_order_id.expect("linked OT"))
        .await
        .expect("get OT")
        .expect("OT exists");
    assert!(wo.requires_permit);
    assert_eq!(wo.permit_rule_code.as_deref(), Some("PREVENTIVE_LOTO"));
    assert_eq!(wo.permit_rule_permit_type.as_deref(), Some("loto"));
}
