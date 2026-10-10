use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::auth::rbac::{self, PermissionScope};
use crate::errors::AppError;
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, ExecutePmOccurrenceInput, GeneratePmOccurrencesInput,
    PmGovernanceKpiInput, PmOccurrenceFilter, PmPlanningReadinessInput, PublishPmPlanVersionInput,
    TransitionPmOccurrenceInput, TransitionPmPlanLifecycleInput, UpdatePmPlanInput,
};
use crate::pm::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&db)
        .await
        .expect("seed system data");
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
    let (plan_id, version_id) = create_plan_with_published_version(
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
    let (plan_id, version_id) = create_plan_with_published_version(
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
        "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit, current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at)
         VALUES ('meter_sync_pm', ?, 'Runtime', 'hours', 'h', 120, 0, 1, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
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

    let (plan_id, version_id) = create_plan_with_published_version(
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
    let (plan_id, version_id) = create_plan_with_published_version(
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
    let (plan_id, version_id) = create_plan_with_published_version(
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
    let (plan_id, version_id) = create_plan_with_published_version(
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
            required_parts_json: Some(r#"[{"part":"BRG-001"}]"#.to_string()),
            required_skills_json: None,
            required_tools_json: None,
            estimated_duration_hours: None,
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
    let (plan_id, version_id) = create_plan_with_published_version(
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

    assert!(report.compliance.denominator >= 2);
    assert!(report.compliance.numerator >= 2);
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
        "INSERT INTO equipment_meters (entity_sync_id, equipment_id, name, meter_type, unit, current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at)
         VALUES ('meter_sync_pm_hook', ?, 'Runtime', 'hours', 'h', 50, 0, 1, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
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
         SELECT ?, 'WO-PM-CLOSE-1', (SELECT id FROM work_order_types LIMIT 1),
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
