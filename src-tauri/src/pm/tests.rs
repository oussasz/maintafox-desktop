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
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
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
            assigned_group_id: None,
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
            assigned_group_id: None,
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
            assigned_group_id: None,
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
            assigned_group_id: None,
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
            assigned_group_id: None,
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
    let (plan_id, version_id) = create_plan_with_published_version(
        &db,
        "PM-PUB-001",
        "event",
        r#"{"event_code":"INSPECTION_COMPLETED"}"#,
    )
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

    let generated = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: first_occ.id,
            expected_row_version: first_occ.row_version,
            next_status: "generated".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("forecasted -> generated");

    let ready = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: generated.id,
            expected_row_version: generated.row_version,
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
        "INSERT INTO equipment (sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at)
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
        "INSERT INTO equipment_meters (sync_id, equipment_id, name, meter_type, unit, current_reading, expected_rate_per_day, is_primary, is_active, created_at, updated_at)
         VALUES ('meter_sync_pm', ?, 'Runtime', 'hours', 'h', 120, 0, 1, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [equipment_id.into()],
    ))
    .await
    .expect("insert equipment meter");

    let meter_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_meters WHERE sync_id = 'meter_sync_pm'",
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
        "INSERT INTO roles (sync_id, name, description, is_system, role_type, status, created_at, updated_at)
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
         WHERE p.name = 'pm.view'",
        [role_id.into()],
    ))
    .await
    .expect("grant pm.view");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts (sync_id, username, display_name, identity_mode, is_active, is_admin, force_password_change, created_at, updated_at)
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
        "INSERT INTO user_scope_assignments (sync_id, user_id, role_id, scope_type, scope_reference, valid_from, valid_to, assigned_by_id, notes, created_at, updated_at)
         VALUES ('usa_sync_pm_view', ?, ?, 'tenant', NULL, NULL, NULL, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [user_id.into(), role_id.into()],
    ))
    .await
    .expect("insert user scope assignment");

    let can_view = rbac::check_permission(&db, user_id, "pm.view", &PermissionScope::Global)
        .await
        .expect("check pm.view");
    let can_create = rbac::check_permission(&db, user_id, "pm.create", &PermissionScope::Global)
        .await
        .expect("check pm.create");
    let can_edit = rbac::check_permission(&db, user_id, "pm.edit", &PermissionScope::Global)
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
        "INSERT OR IGNORE INTO equipment (id, sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at)
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

    let occ = queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: occ.id,
            expected_row_version: occ.row_version,
            next_status: "generated".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("to generated");

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
            assigned_group_id: None,
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
    queries::transition_pm_occurrence(
        &db,
        TransitionPmOccurrenceInput {
            occurrence_id: first_occurrence.id,
            expected_row_version: first_occurrence.row_version,
            next_status: "generated".to_string(),
            reason_code: None,
            note: None,
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: Some(1),
        },
    )
    .await
    .expect("promote forecast occurrence to generated");

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
    assert!(blocker_codes.iter().any(|code| code == "missing_parts"));    assert!(blocker_codes.iter().any(|code| code == "permit_not_ready"));
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
        *occ = queries::transition_pm_occurrence(
            &db,
            TransitionPmOccurrenceInput {
                occurrence_id: occ.id,
                expected_row_version: occ.row_version,
                next_status: "generated".to_string(),
                reason_code: None,
                note: None,
                generate_work_order: Some(false),
                work_order_type_id: None,
                actor_id: Some(1),
            },
        )
        .await
        .expect("to generated");
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