use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::planning::domain::{
    CreateCapacityRuleInput, CreateScheduleBreakInInput, CreateScheduleCommitmentInput, FreezeSchedulePeriodInput,
    NotifyTeamsInput, PlanningGanttFilter, RefreshScheduleCandidatesInput, RescheduleCommitmentInput,
    ScheduleBreakInFilter, ScheduleCandidateFilter, ScheduleCommitmentFilter,
};
use crate::planning::queries;
use crate::planning::scheduling;

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

async fn insert_candidate(
    db: &DatabaseConnection,
    source_type: &str,
    source_id: i64,
    readiness_status: &str,
    readiness_score: f64,
) -> i64 {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO schedule_candidates (
            source_type,
            source_id,
            readiness_status,
            readiness_score,
            permit_status,
            prerequisite_status
         ) VALUES (?, ?, ?, ?, 'not_required', 'ready')",
        [
            source_type.into(),
            source_id.into(),
            readiness_status.into(),
            readiness_score.into(),
        ],
    ))
    .await
    .expect("insert schedule candidate");

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM schedule_candidates WHERE source_type = ? AND source_id = ?",
            [source_type.into(), source_id.into()],
        ))
        .await
        .expect("query candidate")
        .expect("candidate row");
    row.try_get("", "id").expect("candidate id")
}

async fn insert_conflict(db: &DatabaseConnection, candidate_id: i64, conflict_type: &str, resolved: bool) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO scheduling_conflicts (
            candidate_id,
            conflict_type,
            reason_code,
            severity,
            resolved_at
         ) VALUES (?, ?, 'TEST_CONFLICT', 'high', ?)",
        [
            candidate_id.into(),
            conflict_type.into(),
            (if resolved {
                Some("2026-01-01T00:00:00Z".to_string())
            } else {
                None
            })
            .into(),
        ],
    ))
    .await
    .expect("insert conflict");
}

async fn insert_approved_di(db: &DatabaseConnection, code: &str) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO intervention_requests (
            code,
            equipment_id,
            org_node_id,
            status,
            title,
            description,
            origin_type,
            impact_level,
            reported_urgency,
            submitted_at,
            submitter_id,
            created_at,
            updated_at
         ) VALUES (?, 1, 1, 'approved_for_planning', ?, ?, 'manual', 'medium', 'medium', ?, 1, ?, ?)",
        [
            code.into(),
            format!("DI {code}").into(),
            "planning test".to_string().into(),
            "2026-04-01T10:00:00Z".to_string().into(),
            "2026-04-01T10:00:00Z".to_string().into(),
            "2026-04-01T10:00:00Z".to_string().into(),
        ],
    ))
    .await
    .expect("insert approved di");
}

async fn get_any_team_id(db: &DatabaseConnection) -> i64 {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM teams WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
            [],
        ))
        .await
        .expect("query crew");
    if let Some(existing) = existing {
        return existing.try_get("", "id").expect("crew id");
    }

    let node_id = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes ORDER BY id LIMIT 1",
            [],
        ))
        .await
        .expect("query org node")
        .and_then(|r| r.try_get::<i64>("", "id").ok());

    let primary_node_id = if let Some(id) = node_id {
        id
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO org_structure_models (entity_sync_id, version_number, status, description, created_at, updated_at)
             VALUES ('planning-test-structure', 1, 'active', 'planning test structure', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        ))
        .await
        .expect("insert org structure");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO org_node_types (
                entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work, can_carry_cost_center,
                can_aggregate_kpis, can_receive_permits, is_root_type, is_active, created_at, updated_at
             ) VALUES (
                'planning-test-site-type',
                (SELECT id FROM org_structure_models WHERE entity_sync_id = 'planning-test-structure'),
                'SITE',
                'Site',
                0, 1, 0, 1, 0, 1, 1,
                '2026-01-01T00:00:00Z',
                '2026-01-01T00:00:00Z'
             )",
            [],
        ))
        .await
        .expect("insert org node type");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO org_nodes (
                entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
                created_at, updated_at, row_version, structure_model_id
             ) VALUES (
                'planning-test-site',
                'SITE-PLN',
                'Planning Site',
                (SELECT id FROM org_node_types WHERE entity_sync_id = 'planning-test-site-type'),
                NULL,
                '/',
                0,
                'active',
                '2026-01-01T00:00:00Z',
                '2026-01-01T00:00:00Z',
                1,
                (SELECT id FROM org_structure_models WHERE entity_sync_id = 'planning-test-structure')
             )",
            [],
        ))
        .await
        .expect("insert org node");

        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE entity_sync_id = 'planning-test-site'",
            [],
        ))
        .await
        .expect("query inserted org node")
        .and_then(|r| r.try_get("", "id").ok())
        .expect("inserted org node id")
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO teams (
            entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at
         ) VALUES (
            'planning-test-crew',
            'CREW-PLN',
            'Planning Crew',
            'maintenance',
            ?,
            'active',
            '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z'
         )",
        [primary_node_id.into()],
    ))
    .await
    .expect("insert crew");

    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM teams WHERE entity_sync_id = 'planning-test-crew'",
        [],
    ))
    .await
    .expect("query inserted crew")
    .and_then(|r| r.try_get("", "id").ok())
    .expect("inserted crew id")
}

async fn ensure_personnel_with_rate(db: &DatabaseConnection, employee_code: &str) -> i64 {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM personnel WHERE employee_code = ?",
            [employee_code.to_string().into()],
        ))
        .await
        .expect("query personnel");
    let personnel_id = if let Some(existing) = existing {
        existing.try_get("", "id").expect("personnel id")
    } else {
        let inserted = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO personnel (employee_code, full_name, availability_status) VALUES (?, ?, 'available')",
                [employee_code.to_string().into(), format!("Tech {employee_code}").into()],
            ))
            .await
            .expect("insert personnel");
        i64::try_from(inserted.last_insert_id()).expect("personnel id fits i64")
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_rate_cards (personnel_id, effective_from, labor_rate, overtime_rate, source_type)
         VALUES (?, '2026-01-01T00:00:00Z', 45.0, 60.0, 'manual')",
        [personnel_id.into()],
    ))
    .await
    .expect("insert rate card");
    personnel_id
}

async fn ensure_approver_user(db: &DatabaseConnection, user_id: i64, personnel_id: i64) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO users (id) VALUES (?)",
        [user_id.into()],
    ))
    .await
    .expect("insert approver legacy user");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO user_accounts
            (id, entity_sync_id, username, display_name, is_active, is_admin, force_password_change, personnel_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, 1, 0, 0, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [
            user_id.into(),
            format!("approver-sync-{user_id}").into(),
            format!("approver-{user_id}").into(),
            format!("Approver {user_id}").into(),
            personnel_id.into(),
        ],
    ))
    .await
    .expect("insert approver user");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO roles (id, entity_sync_id, name, created_at, updated_at, row_version)
         VALUES (777, 'planning-approver-role', 'planning-approver', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        [],
    ))
    .await
    .expect("insert approver role");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO role_permissions (role_id, permission_id, granted_at, granted_by_id)
         SELECT 777, id, '2026-01-01T00:00:00Z', 1 FROM permissions WHERE name = ?",
        [crate::rbac::permissions::PLAN_CONFIRM.into()],
    ))
    .await
    .expect("grant approver permission");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO user_scope_assignments
            (entity_sync_id, user_id, role_id, scope_type, scope_reference, valid_from, created_at, updated_at)
         VALUES (?, ?, 777, 'tenant', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [format!("planning-approver-usa-{user_id}").into(), user_id.into()],
    ))
    .await
    .expect("assign approver role");
}

async fn ensure_planning_notification_rule(db: &DatabaseConnection) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO notification_categories
            (code, label, default_severity, default_requires_ack, is_user_configurable)
         VALUES ('planning.schedule', 'Planning schedule', 'info', 0, 1)",
        [],
    ))
    .await
    .expect("insert planning category");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO notification_rules
            (category_code, routing_mode, requires_ack, dedupe_window_minutes, is_active)
         VALUES ('planning.schedule', 'team', 0, 120, 1)",
        [],
    ))
    .await
    .expect("insert planning rule");
}

#[tokio::test]
async fn list_candidates_hides_unresolved_conflicts_by_default() {
    let db = setup_db().await;
    let candidate_id = insert_candidate(&db, "manual_test", 1, "blocked", 40.0).await;
    insert_conflict(&db, candidate_id, "missing_critical_part", false).await;

    let hidden = queries::list_schedule_candidates(
        &db,
        ScheduleCandidateFilter {
            source_type: Some("manual_test".to_string()),
            readiness_status: None,
            assigned_personnel_id: None,
            include_resolved_conflicts: None,
            limit: Some(20),
        },
    )
    .await
    .expect("list hidden");
    assert!(hidden.is_empty());

    let visible = queries::list_schedule_candidates(
        &db,
        ScheduleCandidateFilter {
            source_type: Some("manual_test".to_string()),
            readiness_status: None,
            assigned_personnel_id: None,
            include_resolved_conflicts: Some(true),
            limit: Some(20),
        },
    )
    .await
    .expect("list visible");
    assert_eq!(visible.len(), 1);
}

#[tokio::test]
async fn snapshot_decomposes_readiness_dimensions() {
    let db = setup_db().await;
    let ready_id = insert_candidate(&db, "manual_snapshot", 10, "ready", 100.0).await;
    let blocked_id = insert_candidate(&db, "manual_snapshot", 11, "blocked", 60.0).await;
    insert_conflict(&db, blocked_id, "missing_critical_part", false).await;
    insert_conflict(&db, blocked_id, "double_booking", false).await;

    let snapshot = queries::get_schedule_backlog_snapshot(
        &db,
        ScheduleCandidateFilter {
            source_type: Some("manual_snapshot".to_string()),
            readiness_status: None,
            assigned_personnel_id: None,
            include_resolved_conflicts: Some(true),
            limit: Some(20),
        },
    )
    .await
    .expect("snapshot");

    assert_eq!(snapshot.candidate_count, 2);
    assert_eq!(snapshot.ready_count, 1);
    assert_eq!(snapshot.blocked_count, 1);
    assert!(snapshot
        .conflict_summary
        .iter()
        .any(|item| item.candidate_id == ready_id && item.blocker_dimensions.is_empty()));
    let blocked_summary = snapshot
        .conflict_summary
        .iter()
        .find(|item| item.candidate_id == blocked_id)
        .expect("blocked summary");
    assert!(blocked_summary.blocker_dimensions.iter().any(|dim| dim == "parts"));
    assert!(blocked_summary.blocker_dimensions.iter().any(|dim| dim == "windows"));
}

#[tokio::test]
async fn refresh_from_approved_di_creates_schedule_candidates() {
    let db = setup_db().await;
    insert_approved_di(&db, "DI-PLAN-001").await;

    let result = queries::refresh_schedule_candidates(
        &db,
        RefreshScheduleCandidatesInput {
            include_work_orders: Some(false),
            include_pm_occurrences: Some(false),
            include_approved_di: Some(true),
            limit_per_source: Some(20),
        },
    )
    .await
    .expect("refresh candidates");

    assert!(result.inserted_count >= 1);
    assert!(result.evaluated_count >= 1);

    let candidates = queries::list_schedule_candidates(
        &db,
        ScheduleCandidateFilter {
            source_type: Some("inspection_follow_up".to_string()),
            readiness_status: None,
            assigned_personnel_id: None,
            include_resolved_conflicts: Some(true),
            limit: Some(20),
        },
    )
    .await
    .expect("list candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].source_di_id, Some(candidates[0].source_id));
}

#[tokio::test]
async fn commitment_requires_all_three_gates() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate_id = insert_candidate(&db, "pm_occurrence", 3001, "ready", 100.0).await;

    let no_capacity = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-05-01T08:00:00Z".to_string(),
            committed_end: "2026-05-01T16:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await;
    assert!(no_capacity.is_ok(), "default capacity should allow first commitment");

    scheduling::create_capacity_rule(
        &db,
        CreateCapacityRuleInput {
            entity_id: None,
            team_id,
            effective_start: "2026-05-01".to_string(),
            effective_end: Some("2026-05-01".to_string()),
            available_hours_per_day: 1.0,
            max_overtime_hours_per_day: 0.0,
        },
    )
    .await
    .expect("capacity rule");
    let candidate2 = insert_candidate(&db, "pm_occurrence", 3002, "ready", 100.0).await;
    let overloaded = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate2,
            expected_candidate_row_version: None,
            committed_start: "2026-05-01T08:00:00Z".to_string(),
            committed_end: "2026-05-01T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect_err("capacity gate must reject");
    assert!(format!("{overloaded:?}").contains("Capacity gate failed"));
}

#[tokio::test]
async fn assignee_unavailable_is_hard_rejected() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let personnel_id = ensure_personnel_with_rate(&db, "TECH-UNAVAILABLE").await;
    let candidate_id = insert_candidate(&db, "pm_occurrence", 4001, "ready", 100.0).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_availability_blocks (personnel_id, block_type, start_at, end_at, reason_note, is_critical, created_by_id)
         VALUES (?, 'leave', '2026-06-01T08:00:00Z', '2026-06-01T12:00:00Z', 'PTO', 1, 1)",
        [personnel_id.into()],
    ))
    .await
    .expect("insert block");

    let err = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-06-01T09:00:00Z".to_string(),
            committed_end: "2026-06-01T11:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(personnel_id),
            allow_double_booking_override: Some(true),
            override_reason: Some("Force it".to_string()),
            budget_threshold: None,
        },
    )
    .await
    .expect_err("availability must hard reject");
    assert!(format!("{err:?}").contains("ASSIGNEE_UNAVAILABLE"));
}

#[tokio::test]
async fn double_booking_requires_override_audit() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let personnel_id = ensure_personnel_with_rate(&db, "TECH-BOOKED").await;
    let c1 = insert_candidate(&db, "pm_occurrence", 5001, "ready", 100.0).await;
    let c2 = insert_candidate(&db, "pm_occurrence", 5002, "ready", 100.0).await;

    scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: c1,
            expected_candidate_row_version: None,
            committed_start: "2026-07-02T08:00:00Z".to_string(),
            committed_end: "2026-07-02T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(personnel_id),
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: Some(50.0),
        },
    )
    .await
    .expect("seed commitment");

    let rejected = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: c2,
            expected_candidate_row_version: None,
            committed_start: "2026-07-02T09:00:00Z".to_string(),
            committed_end: "2026-07-02T11:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(personnel_id),
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: Some(50.0),
        },
    )
    .await
    .expect_err("double-booking should reject without override");
    assert!(format!("{rejected:?}").contains("Double-booking detected"));

    let overridden = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: c2,
            expected_candidate_row_version: None,
            committed_start: "2026-07-02T09:00:00Z".to_string(),
            committed_end: "2026-07-02T11:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(personnel_id),
            allow_double_booking_override: Some(true),
            override_reason: Some("Emergency overlap approved".to_string()),
            budget_threshold: Some(50.0),
        },
    )
    .await
    .expect("override should pass");
    assert_eq!(overridden.cost_variance_warning, 1);

    let log_rows = scheduling::list_schedule_change_log(&db, Some(overridden.id))
        .await
        .expect("change log");
    assert!(log_rows.iter().any(|r| r.action_type == "create_commitment"));
}

#[tokio::test]
async fn freeze_blocks_reschedule_and_preserves_integrity() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate_id = insert_candidate(&db, "pm_occurrence", 6001, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-08-10T08:00:00Z".to_string(),
            committed_end: "2026-08-10T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("commitment");

    let frozen = scheduling::freeze_schedule_period(
        &db,
        1,
        FreezeSchedulePeriodInput {
            period_start: "2026-08-10T00:00:00Z".to_string(),
            period_end: "2026-08-11T00:00:00Z".to_string(),
            reason: Some("Weekly freeze".to_string()),
        },
    )
    .await
    .expect("freeze");
    assert!(frozen >= 1);

    let stale = scheduling::reschedule_schedule_commitment(
        &db,
        1,
        RescheduleCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: commitment.row_version + 99,
            committed_start: "2026-08-10T12:00:00Z".to_string(),
            committed_end: "2026-08-10T14:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect_err("stale row version rejected");
    assert!(format!("{stale:?}").contains("stale row_version"));

    let blocked = scheduling::reschedule_schedule_commitment(
        &db,
        1,
        RescheduleCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: commitment.row_version + 1,
            committed_start: "2026-08-10T12:00:00Z".to_string(),
            committed_end: "2026-08-10T14:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect_err("frozen commitment must reject edits");
    assert!(format!("{blocked:?}").contains("frozen"));
}

#[tokio::test]
async fn break_in_requires_approval_or_dangerous_override() {
    let db = setup_db().await;
    ensure_planning_notification_rule(&db).await;
    let team_id = get_any_team_id(&db).await;
    let assignee_id = ensure_personnel_with_rate(&db, "TECH-BREAKIN").await;
    let approver_personnel = ensure_personnel_with_rate(&db, "APPROVER-01").await;
    ensure_approver_user(&db, 42, approver_personnel).await;
    let candidate_id = insert_candidate(&db, "pm_occurrence", 6101, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-08-15T08:00:00Z".to_string(),
            committed_end: "2026-08-15T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(assignee_id),
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("seed commitment");

    let rejected = scheduling::create_schedule_break_in(
        &db,
        1,
        CreateScheduleBreakInInput {
            schedule_commitment_id: commitment.id,
            expected_commitment_row_version: commitment.row_version,
            break_in_reason: "emergency".to_string(),
            approved_by_user_id: None,
            new_slot_start: "2026-08-15T11:00:00Z".to_string(),
            new_slot_end: "2026-08-15T12:00:00Z".to_string(),
            new_assigned_team_id: Some(team_id),
            new_assigned_personnel_id: Some(assignee_id),
            bypass_availability: Some(false),
            bypass_qualification: Some(false),
            override_reason: None,
            dangerous_override_reason: None,
        },
    )
    .await
    .expect_err("emergency break-in should require approval");
    assert!(format!("{rejected:?}").contains("require approver evidence"));

    let created = scheduling::create_schedule_break_in(
        &db,
        1,
        CreateScheduleBreakInInput {
            schedule_commitment_id: commitment.id,
            expected_commitment_row_version: commitment.row_version,
            break_in_reason: "emergency".to_string(),
            approved_by_user_id: Some(42),
            new_slot_start: "2026-08-15T11:00:00Z".to_string(),
            new_slot_end: "2026-08-15T12:00:00Z".to_string(),
            new_assigned_team_id: Some(team_id),
            new_assigned_personnel_id: Some(assignee_id),
            bypass_availability: Some(false),
            bypass_qualification: Some(false),
            override_reason: Some("line stop".to_string()),
            dangerous_override_reason: None,
        },
    )
    .await
    .expect("approved break-in");
    assert_eq!(created.approved_by_user_id, Some(42));

    let break_ins = scheduling::list_schedule_break_ins(
        &db,
        ScheduleBreakInFilter {
            period_start: Some("2026-08-15T00:00:00Z".to_string()),
            period_end: Some("2026-08-16T00:00:00Z".to_string()),
            break_in_reason: Some("emergency".to_string()),
            approved_by_user_id: Some(42),
        },
    )
    .await
    .expect("list break-ins");
    assert_eq!(break_ins.len(), 1);
}

#[tokio::test]
async fn freeze_breach_notifications_are_deduped_and_notify_teams_emits_payloads() {
    let db = setup_db().await;
    ensure_planning_notification_rule(&db).await;
    let team_id = get_any_team_id(&db).await;
    let assignee_id = ensure_personnel_with_rate(&db, "TECH-NOTIFY").await;
    ensure_approver_user(&db, 44, assignee_id).await;
    let candidate_id = insert_candidate(&db, "pm_occurrence", 6201, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-08-20T08:00:00Z".to_string(),
            committed_end: "2026-08-20T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(assignee_id),
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("commitment");
    scheduling::freeze_schedule_period(
        &db,
        1,
        FreezeSchedulePeriodInput {
            period_start: "2026-08-20T00:00:00Z".to_string(),
            period_end: "2026-08-21T00:00:00Z".to_string(),
            reason: Some("freeze test".to_string()),
        },
    )
    .await
    .expect("freeze");

    let frozen = scheduling::list_schedule_commitments(
        &db,
        ScheduleCommitmentFilter {
            period_start: Some("2026-08-20T00:00:00Z".to_string()),
            period_end: Some("2026-08-21T00:00:00Z".to_string()),
            team_id: Some(team_id),
            personnel_id: None,
        },
    )
    .await
    .expect("reload frozen commitment")
    .into_iter()
    .find(|c| c.id == commitment.id)
    .expect("commitment after freeze");

    for _ in 0..2 {
        let err = scheduling::reschedule_schedule_commitment(
            &db,
            1,
            RescheduleCommitmentInput {
                commitment_id: frozen.id,
                expected_row_version: frozen.row_version,
                committed_start: "2026-08-20T12:00:00Z".to_string(),
                committed_end: "2026-08-20T14:00:00Z".to_string(),
                assigned_team_id: team_id,
                assigned_personnel_id: Some(assignee_id),
                allow_double_booking_override: Some(false),
                override_reason: None,
                budget_threshold: None,
            },
        )
        .await
        .expect_err("frozen commitment rejects reschedule");
        assert!(format!("{err:?}").contains("frozen"));
    }

    let dedupe_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM notification_events WHERE dedupe_key LIKE 'planning.freeze-breach.%'",
            [],
        ))
        .await
        .expect("freeze dedupe query")
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or_default();
    assert_eq!(dedupe_count, 1, "freeze breach notifications should dedupe");

    let notify = scheduling::notify_schedule_teams(
        &db,
        1,
        NotifyTeamsInput {
            period_start: "2026-08-20T00:00:00Z".to_string(),
            period_end: "2026-08-21T00:00:00Z".to_string(),
            team_id: Some(team_id),
            include_break_ins: Some(true),
        },
    )
    .await
    .expect("notify teams");
    assert!(notify.emitted_count >= 1);
}

#[tokio::test]
async fn gantt_snapshot_and_pdf_export_use_real_commitments() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let roster_person = ensure_personnel_with_rate(&db, "TECH-GANTT-ROSTER").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO personnel_team_assignments (
            personnel_id, team_id, role_code, allocation_percent,
            valid_from, valid_to, is_lead, created_at, updated_at
         ) VALUES (?, ?, 'member', 100.0, '2026-01-01T00:00:00Z', NULL, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [roster_person.into(), team_id.into()],
    ))
    .await
    .expect("assign roster person to crew");

    let candidate_id = insert_candidate(&db, "pm_occurrence", 7001, "ready", 100.0).await;
    let _commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-09-01T08:00:00Z".to_string(),
            committed_end: "2026-09-01T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("create commitment");

    let snapshot = scheduling::get_planning_gantt_snapshot(
        &db,
        PlanningGanttFilter {
            period_start: "2026-09-01T00:00:00Z".to_string(),
            period_end: "2026-09-02T00:00:00Z".to_string(),
            team_id: Some(team_id),
        },
    )
    .await
    .expect("snapshot");
    assert_eq!(snapshot.commitments.len(), 1);
    assert!(!snapshot.capacity.is_empty());
    assert!(
        snapshot
            .assignee_lanes
            .iter()
            .any(|lane| lane.personnel_id == roster_person),
        "gantt snapshot must seed crew roster lanes even without per-person commitments"
    );
    assert!(
        snapshot.crew_lanes.is_empty(),
        "single-crew gantt must not populate plant-wide crew_lanes"
    );

    let plant_wide = scheduling::get_planning_gantt_snapshot(
        &db,
        PlanningGanttFilter {
            period_start: "2026-09-01T00:00:00Z".to_string(),
            period_end: "2026-09-02T00:00:00Z".to_string(),
            team_id: None,
        },
    )
    .await
    .expect("plant-wide snapshot");
    assert_eq!(plant_wide.commitments.len(), 1);
    assert!(
        plant_wide.assignee_lanes.is_empty(),
        "All Crews overview uses crew lanes, not technician lanes"
    );
    assert!(
        plant_wide.crew_lanes.iter().any(|lane| lane.team_id == team_id),
        "All Crews snapshot must include active crew lanes"
    );

    let pdf = scheduling::export_planning_gantt_pdf(
        &db,
        crate::planning::domain::ExportPlanningGanttPdfInput {
            period_start: "2026-09-01T00:00:00Z".to_string(),
            period_end: "2026-09-02T00:00:00Z".to_string(),
            team_id: Some(team_id),
            paper_size: Some("A4".to_string()),
        },
    )
    .await
    .expect("export");
    assert!(pdf.bytes.len() > 32);
    assert_eq!(pdf.mime_type, "application/pdf");

    let listed = scheduling::list_schedule_commitments(
        &db,
        ScheduleCommitmentFilter {
            period_start: Some("2026-09-01T00:00:00Z".to_string()),
            period_end: Some("2026-09-02T00:00:00Z".to_string()),
            team_id: Some(team_id),
            personnel_id: None,
        },
    )
    .await
    .expect("list commitments");
    assert_eq!(listed.len(), 1);
}

async fn seed_equipment_for_wo(db: &DatabaseConnection) {
    let _ = get_any_team_id(db).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO equipment
            (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, created_at, updated_at)
         VALUES (
            1,
            'planning-eq-001',
            'EQ-PLN-001',
            'Planning Test Equipment',
            'active_in_service',
            (SELECT id FROM org_nodes ORDER BY id LIMIT 1),
            '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z'
         )",
        [],
    ))
    .await
    .expect("seed equipment");
}

async fn create_planning_wo(db: &DatabaseConnection, title: &str) -> (i64, i64) {
    seed_equipment_for_wo(db).await;
    let actor = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE is_active = 1 ORDER BY id LIMIT 1",
            [],
        ))
        .await
        .expect("query actor")
        .and_then(|r| r.try_get::<i64>("", "id").ok())
        .unwrap_or(1);

    let wo = crate::wo::queries::create_work_order(
        db,
        crate::wo::domain::WoCreateInput {
            type_code: "corrective".into(),
            equipment_id: Some(1),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_code: Some("normal".into()),
            subtype_code: None,
            title: title.into(),
            description: Some("planning sync test".into()),
            notes: None,
            planned_start: Some("2026-10-01T08:00:00Z".into()),
            planned_end: Some("2026-10-01T10:00:00Z".into()),
            shift: None,
            expected_duration_hours: Some(2.0),
            creator_id: actor,
            requires_permit: None,
        },
    )
    .await
    .expect("create WO");

    let wo = crate::wo::workflow::actions::submit::submit_wo(
        db,
        crate::wo::workflow::actions::submit::WoSubmitInput {
            wo_id: wo.id,
            actor_id: actor,
            expected_row_version: wo.row_version,
        },
    )
    .await
    .expect("submit to planning");

    (wo.id, actor)
}

async fn load_wo_planning_fields(
    db: &DatabaseConnection,
    wo_id: i64,
) -> (Option<String>, Option<String>, Option<i64>, Option<i64>) {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT planned_start, planned_end, assigned_team_id, primary_responsible_id
             FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("query wo")
        .expect("wo row");
    (
        row.try_get("", "planned_start").ok().flatten(),
        row.try_get("", "planned_end").ok().flatten(),
        row.try_get("", "assigned_team_id").ok().flatten(),
        row.try_get("", "primary_responsible_id").ok().flatten(),
    )
}

#[tokio::test]
async fn commitment_write_back_updates_work_order_fields() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let personnel_id = ensure_personnel_with_rate(&db, "TECH-SYNC").await;
    let user_id = 55_i64;
    ensure_approver_user(&db, user_id, personnel_id).await;

    let (wo_id, actor_id) = create_planning_wo(&db, "Sync commit WO").await;
    let candidate_id = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;

    let commitment = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-10-05T08:00:00Z".to_string(),
            committed_end: "2026-10-05T12:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(personnel_id),
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("create commitment with write-back");

    let (start, end, group, responsible) = load_wo_planning_fields(&db, wo_id).await;
    assert_eq!(start.as_deref(), Some("2026-10-05T08:00:00Z"));
    assert_eq!(end.as_deref(), Some("2026-10-05T12:00:00Z"));
    assert_eq!(group, Some(team_id));
    assert_eq!(responsible, Some(user_id));

    let candidate = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT window_start, window_end, assigned_team_id, assigned_personnel_id
             FROM schedule_candidates WHERE id = ?",
            [candidate_id.into()],
        ))
        .await
        .expect("query candidate")
        .expect("candidate");
    assert_eq!(
        candidate
            .try_get::<Option<String>>("", "window_start")
            .unwrap()
            .as_deref(),
        Some("2026-10-05T08:00:00Z")
    );
    assert_eq!(
        candidate.try_get::<Option<i64>>("", "assigned_team_id").unwrap(),
        Some(team_id)
    );
    assert_eq!(
        candidate.try_get::<Option<i64>>("", "assigned_personnel_id").unwrap(),
        Some(personnel_id)
    );

    let action_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM wo_action_events
             WHERE wo_id = ? AND action_code IN ('schedule_changed', 'assigned')",
            [wo_id.into()],
        ))
        .await
        .expect("count actions")
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or(0);
    assert!(action_count >= 2);

    let change_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM wo_change_events
             WHERE wo_id = ? AND action IN ('planned', 'assigned')
               AND summary LIKE '%planning commitment%'",
            [wo_id.into()],
        ))
        .await
        .expect("count change events")
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or(0);
    assert!(change_count >= 2);

    let rescheduled = scheduling::reschedule_schedule_commitment(
        &db,
        actor_id,
        RescheduleCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: commitment.row_version,
            committed_start: "2026-10-06T09:00:00Z".to_string(),
            committed_end: "2026-10-06T11:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(personnel_id),
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("reschedule write-back");

    let (start, end, group, responsible) = load_wo_planning_fields(&db, wo_id).await;
    assert_eq!(start.as_deref(), Some("2026-10-06T09:00:00Z"));
    assert_eq!(end.as_deref(), Some("2026-10-06T11:00:00Z"));
    assert_eq!(group, Some(team_id));
    assert_eq!(responsible, Some(user_id));
    assert_eq!(rescheduled.committed_start, "2026-10-06T09:00:00Z");
}

#[tokio::test]
async fn break_in_write_back_updates_work_order_slot() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let personnel_id = ensure_personnel_with_rate(&db, "TECH-BREAKIN-SYNC").await;
    let approver_personnel = ensure_personnel_with_rate(&db, "APPROVER-SYNC").await;
    ensure_approver_user(&db, 66, approver_personnel).await;

    let (wo_id, actor_id) = create_planning_wo(&db, "Sync break-in WO").await;
    let candidate_id = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-11-01T08:00:00Z".to_string(),
            committed_end: "2026-11-01T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: Some(personnel_id),
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("commitment");

    scheduling::create_schedule_break_in(
        &db,
        actor_id,
        CreateScheduleBreakInInput {
            schedule_commitment_id: commitment.id,
            expected_commitment_row_version: commitment.row_version,
            break_in_reason: "emergency".to_string(),
            new_slot_start: "2026-11-01T14:00:00Z".to_string(),
            new_slot_end: "2026-11-01T16:00:00Z".to_string(),
            new_assigned_team_id: Some(team_id),
            new_assigned_personnel_id: Some(personnel_id),
            approved_by_user_id: Some(66),
            override_reason: Some("Emergency move".to_string()),
            dangerous_override_reason: None,
            bypass_availability: Some(true),
            bypass_qualification: Some(true),
        },
    )
    .await
    .expect("break-in");

    let (start, end, group, _) = load_wo_planning_fields(&db, wo_id).await;
    assert_eq!(start.as_deref(), Some("2026-11-01T14:00:00Z"));
    assert_eq!(end.as_deref(), Some("2026-11-01T16:00:00Z"));
    assert_eq!(group, Some(team_id));
}

#[tokio::test]
async fn frozen_reschedule_leaves_work_order_unchanged() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (wo_id, actor_id) = create_planning_wo(&db, "Frozen sync WO").await;
    let candidate_id = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-12-01T08:00:00Z".to_string(),
            committed_end: "2026-12-01T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect("commitment");

    scheduling::freeze_schedule_period(
        &db,
        actor_id,
        FreezeSchedulePeriodInput {
            period_start: "2026-12-01T00:00:00Z".to_string(),
            period_end: "2026-12-02T00:00:00Z".to_string(),
            reason: Some("Weekly freeze".to_string()),
        },
    )
    .await
    .expect("freeze");

    let err = scheduling::reschedule_schedule_commitment(
        &db,
        actor_id,
        RescheduleCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: commitment.row_version + 1, // freeze bumps row_version
            committed_start: "2026-12-01T12:00:00Z".to_string(),
            committed_end: "2026-12-01T14:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect_err("frozen reschedule blocked");
    assert!(format!("{err:?}").contains("frozen"));

    let (start, end, group, _) = load_wo_planning_fields(&db, wo_id).await;
    assert_eq!(start.as_deref(), Some("2026-12-01T08:00:00Z"));
    assert_eq!(end.as_deref(), Some("2026-12-01T10:00:00Z"));
    assert_eq!(group, Some(team_id));
}

#[tokio::test]
async fn commitment_rejects_when_work_order_not_planning_or_ready() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (wo_id, actor_id) = create_planning_wo(&db, "Draft-block sync WO").await;

    // Force WO back to draft by rewriting status (simulates non-schedulable state).
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders
         SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'draft'),
             row_version = row_version + 1
         WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("force draft");

    let candidate_id = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    let err = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        CreateScheduleCommitmentInput {
            schedule_candidate_id: candidate_id,
            expected_candidate_row_version: None,
            committed_start: "2026-12-10T08:00:00Z".to_string(),
            committed_end: "2026-12-10T10:00:00Z".to_string(),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            budget_threshold: None,
        },
    )
    .await
    .expect_err("draft WO must fail write-back");
    assert!(format!("{err:?}").contains("planning or ready"));

    let commitment_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM schedule_commitments WHERE source_type = 'work_order' AND source_id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("count")
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or(0);
    assert_eq!(commitment_count, 0);
}
