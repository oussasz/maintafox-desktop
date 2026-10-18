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
            prerequisite_status,
            estimated_duration_hours,
            planned_crew_size
         ) VALUES (?, ?, ?, ?, 'not_required', 'ready', 1.0, 1)",
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

async fn set_estimate(db: &DatabaseConnection, candidate_id: i64, hours: Option<f64>) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_candidates SET estimated_duration_hours = ? WHERE id = ?",
        [hours.into(), candidate_id.into()],
    ))
    .await
    .expect("set estimate");
}

async fn set_crew(db: &DatabaseConnection, candidate_id: i64, crew: Option<i64>) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_candidates SET planned_crew_size = ? WHERE id = ?",
        [crew.into(), candidate_id.into()],
    ))
    .await
    .expect("set crew");
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
         ) VALUES (?, 1, 1, 'approved', ?, ?, 'manual', 'medium', 'medium', ?, 1, ?, ?)",
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

/// Capacity comes from the roster (no default hours): the test crew gets one member on a
/// 7-day 08:00–16:00 shift, i.e. 8 h available every day.
async fn give_team_roster_capacity(db: &DatabaseConnection, team_id: i64) {
    for sql in [
        // A continuous Classe horaire (works through site rest days and holidays), 8 h every day,
        // all of it available for work orders.
        "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active) \
         SELECT 'planning-test-7d-shift', rs.id, 'PLN-7D', 'Planning test 7-day shift', 99, 1 \
         FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(rd.code) = 'ORG.SCHEDULE_CLASS' AND rs.status = 'published' \
         ORDER BY rs.version_no DESC LIMIT 1",
        "INSERT OR IGNORE INTO schedule_class_versions (entity_sync_id, reference_value_id, effective_from, \
         cycle_length_days, rotation_start_date, days_json, is_continuous, utilisation_percent, created_at, updated_at) \
         SELECT 'planning-test-7d-version', rv.id, '2000-01-01', 7, NULL, \
         (SELECT json_group_array(json_object('cycle_day', n, 'shift_start', '08:00', 'shift_end', '16:00', \
                 'break_minutes', 0, 'is_rest_day', json('false'))) \
            FROM (SELECT 1 AS n UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 \
                  UNION SELECT 7)), \
         1, 100.0, datetime('now'), datetime('now') \
         FROM reference_values rv WHERE rv.entity_sync_id = 'planning-test-7d-shift'",
        "INSERT OR IGNORE INTO schedule_details (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day) \
         SELECT rv.id, d.n, '08:00', '16:00', 0 FROM reference_values rv \
         CROSS JOIN (SELECT 1 AS n UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 \
                     UNION SELECT 7) d \
         WHERE rv.entity_sync_id = 'planning-test-7d-shift'",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("test shift schedule");
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO personnel (employee_code, full_name, availability_status, primary_team_id, \
         home_schedule_reference_value_id) \
         SELECT ?, 'Planning roster technician', 'available', ?, rv.id FROM reference_values rv \
         WHERE rv.entity_sync_id = 'planning-test-7d-shift'",
        [format!("PLN-ROSTER-{team_id}").into(), team_id.into()],
    ))
    .await
    .expect("roster member");
}

async fn get_any_team_id(db: &DatabaseConnection) -> i64 {
    let team_id = get_any_team_id_inner(db).await;
    give_team_roster_capacity(db, team_id).await;
    team_id
}

async fn get_any_team_id_inner(db: &DatabaseConnection) -> i64 {
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

/// A technician on the test crew's 08:00–16:00 shift (primary team, home Classe horaire).
async fn join_test_crew(db: &DatabaseConnection, personnel_id: i64) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE personnel SET \
            primary_team_id = (SELECT id FROM teams WHERE deleted_at IS NULL ORDER BY id LIMIT 1), \
            home_schedule_reference_value_id = \
                (SELECT id FROM reference_values WHERE entity_sync_id = 'planning-test-7d-shift') \
         WHERE id = ?",
        [personnel_id.into()],
    ))
    .await
    .expect("join test crew");
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
    join_test_crew(db, personnel_id).await;
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
async fn list_candidates_shows_blocked_work_when_asked_and_can_hide_it() {
    let db = setup_db().await;
    let candidate_id = insert_candidate(&db, "manual_test", 1, "blocked", 40.0).await;
    insert_conflict(&db, candidate_id, "missing_critical_part", false).await;

    let hidden = queries::list_schedule_candidates(
        &db,
        ScheduleCandidateFilter {
            source_type: Some("manual_test".to_string()),
            readiness_status: None,
            assigned_personnel_id: None,
            include_resolved_conflicts: Some(false),
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
    assert_eq!(visible.len(), 1, "blocked work is listed when asked for");
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
    // A request still under review is not planned.
    insert_approved_di(&db, "DI-PLAN-002").await;
    exec_sql(
        &db,
        "UPDATE intervention_requests SET status = 'in_review' WHERE code = 'DI-PLAN-002'",
    )
    .await;

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
    assert_eq!(candidates.len(), 1, "only the approved request: {candidates:?}");
    assert_eq!(candidates[0].source_di_id, Some(candidates[0].source_id));
    let approved = scalar(&db, "SELECT id FROM intervention_requests WHERE code = 'DI-PLAN-001'").await;
    assert_eq!(candidates[0].source_id, approved);
}

#[tokio::test]
async fn commitment_requires_all_three_gates() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate_id = insert_candidate(&db, "pm_occurrence", 3001, "ready", 100.0).await;
    set_estimate(&db, candidate_id, Some(8.0)).await;

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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            available_hours_per_day: None,
            max_overtime_hours_per_day: 0.0,
        },
    )
    .await
    .expect("capacity rule");
    let candidate2 = insert_candidate(&db, "pm_occurrence", 3002, "ready", 100.0).await;
    set_estimate(&db, candidate2, Some(2.0)).await;
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
            budget_threshold: None,
        },
    )
    .await
    .expect_err("capacity gate must reject");
    assert!(format!("{overloaded:?}").contains("PLANNING_CAPACITY_EXCEEDED"));
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
    // 2 h × 1 person at 45/h = 90 > the 50 budget threshold (the cost follows the labour hours).
    set_estimate(&db, c2, Some(2.0)).await;

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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
            budget_threshold: Some(50.0),
        },
    )
    .await
    .expect_err("double-booking should reject without override");
    assert!(format!("{rejected:?}").contains("PLANNING_DOUBLE_BOOKING"));

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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            acknowledged_assignee_issues: Vec::new(),
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
            acknowledged_assignee_issues: Vec::new(),
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            clear_assignee: None,
            bypass_availability: Some(false),
            override_reason: None,
            acknowledged_assignee_issues: Vec::new(),
            dangerous_override_reason: None,
        },
    )
    .await
    .expect_err("emergency break-in should require approval");
    assert!(format!("{rejected:?}").contains("PLANNING_BREAK_IN_APPROVAL_REQUIRED"));

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
            clear_assignee: None,
            bypass_availability: Some(false),
            override_reason: Some("line stop".to_string()),
            acknowledged_assignee_issues: Vec::new(),
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
                acknowledged_assignee_issues: Vec::new(),
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            acknowledged_assignee_issues: Vec::new(),
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            clear_assignee: None,
            approved_by_user_id: Some(66),
            override_reason: Some("Emergency move".to_string()),
            acknowledged_assignee_issues: Vec::new(),
            dangerous_override_reason: None,
            bypass_availability: Some(true),
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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
            acknowledged_assignee_issues: Vec::new(),
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
            acknowledged_assignee_issues: Vec::new(),
            frozen_week_break_in: None,
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

// ── Step A: load over working days, finished work, estimates, no-capacity days ───────────────

/// An RFC 3339 instant for a local date-time (`YYYY-MM-DDTHH:MM`), as the planning screen sends it.
fn local(at: &str) -> String {
    use chrono::TimeZone;
    let naive = chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M").expect("local date-time");
    chrono::Local
        .from_local_datetime(&naive)
        .earliest()
        .expect("local instant")
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn day(d: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").expect("day")
}

fn commit_input(candidate_id: i64, team_id: i64, start: &str, end: &str) -> CreateScheduleCommitmentInput {
    CreateScheduleCommitmentInput {
        schedule_candidate_id: candidate_id,
        expected_candidate_row_version: None,
        committed_start: local(start),
        committed_end: local(end),
        assigned_team_id: team_id,
        assigned_personnel_id: None,
        allow_double_booking_override: Some(false),
        override_reason: None,
        acknowledged_assignee_issues: Vec::new(),
        frozen_week_break_in: None,
        budget_threshold: None,
    }
}

async fn team_loads(
    db: &DatabaseConnection,
    team_id: i64,
    from: &str,
    to: &str,
) -> std::collections::BTreeMap<String, (f64, Option<f64>, String)> {
    scheduling::list_team_capacity_load(
        db,
        format!("{from}T00:00:00Z"),
        format!("{to}T23:00:00Z"),
        Some(team_id),
    )
    .await
    .expect("capacity load")
    .into_iter()
    .map(|r| (r.work_date.clone(), (r.committed_hours, r.utilization_ratio, r.basis)))
    .collect()
}

fn shift_hours(d: &str, start: &str, end: &str, hours: f64) -> crate::planning::load::DayHours {
    let (s, e) = crate::personnel::capacity::shift_window(day(d), start, end).expect("window");
    crate::planning::load::DayHours {
        windows: vec![crate::planning::load::WorkWindow {
            start: s,
            end: e,
            hours,
        }],
        unwindowed: 0.0,
    }
}

#[test]
fn a_slot_spreads_its_labour_over_working_hours_not_calendar_hours() {
    use crate::planning::load::{slot_days, spread, DayHours};
    // Mon 08:00 → Wed 17:00, 8 h shifts 08:00–16:00: 24 h of labour is 8 / 8 / 8 (the calendar
    // share gave 6.7 / 10.1 / 7.2 and refused a job that fits).
    let days = std::collections::BTreeMap::from([
        (day("2026-05-04"), shift_hours("2026-05-04", "08:00", "16:00", 8.0)),
        (day("2026-05-05"), shift_hours("2026-05-05", "08:00", "16:00", 8.0)),
        (day("2026-05-06"), shift_hours("2026-05-06", "08:00", "16:00", 8.0)),
    ]);
    let shares = spread(24.0, &local("2026-05-04T08:00"), &local("2026-05-06T17:00"), &days).expect("spread");
    for d in ["2026-05-04", "2026-05-05", "2026-05-06"] {
        assert!((shares[&day(d)] - 8.0).abs() < 1e-6, "{shares:?}");
    }
    // A rest day in the middle takes nothing.
    let mut with_rest = days.clone();
    with_rest.insert(day("2026-05-05"), DayHours::default());
    let shares = spread(12.0, &local("2026-05-04T08:00"), &local("2026-05-06T16:00"), &with_rest).expect("spread");
    assert!((shares[&day("2026-05-04")] - 6.0).abs() < 1e-6, "{shares:?}");
    assert!(!shares.contains_key(&day("2026-05-05")), "nothing on the rest day");
    assert!((shares[&day("2026-05-06")] - 6.0).abs() < 1e-6, "{shares:?}");
    // A member whose window is not known counts in full on the day the slot touches.
    let unknown = std::collections::BTreeMap::from([(
        day("2026-05-04"),
        DayHours {
            windows: Vec::new(),
            unwindowed: 8.0,
        },
    )]);
    let shares = spread(3.0, &local("2026-05-04T10:00"), &local("2026-05-04T11:00"), &unknown).expect("spread");
    assert!((shares[&day("2026-05-04")] - 3.0).abs() < 1e-6, "{shares:?}");
    // A slot ending at midnight stops the day before.
    let one_day = slot_days(&local("2026-05-04T08:00"), &local("2026-05-05T00:00")).expect("days");
    assert_eq!(one_day.len(), 1);
}

#[test]
fn a_night_shift_books_on_the_day_it_starts() {
    use crate::planning::load::spread;
    let days = std::collections::BTreeMap::from([
        (day("2026-05-03"), shift_hours("2026-05-03", "22:00", "06:00", 8.0)),
        (day("2026-05-04"), shift_hours("2026-05-04", "22:00", "06:00", 8.0)),
        (day("2026-05-05"), shift_hours("2026-05-05", "22:00", "06:00", 8.0)),
    ]);
    // 22:00 → 06:00: all of it on the start day (the calendar share put 75 % on the next day).
    let shares = spread(8.0, &local("2026-05-04T22:00"), &local("2026-05-05T06:00"), &days).expect("spread");
    assert_eq!(shares.len(), 1, "{shares:?}");
    assert!((shares[&day("2026-05-04")] - 8.0).abs() < 1e-6, "{shares:?}");
    // 02:00 → 04:00 on the 5th sits in the shift that started on the 4th.
    let shares = spread(2.0, &local("2026-05-05T02:00"), &local("2026-05-05T04:00"), &days).expect("spread");
    assert!((shares[&day("2026-05-04")] - 2.0).abs() < 1e-6, "{shares:?}");
}

#[tokio::test]
async fn a_multi_day_job_is_committed_and_loads_each_day_with_its_share() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await; // 8 h every day
    let candidate = insert_candidate(&db, "pm_occurrence", 7101, "ready", 100.0).await;
    set_estimate(&db, candidate, Some(12.0)).await;
    // Monday 08:00 → Tuesday 16:00: 12 labour hours over two days (it used to count 32 h on Monday).
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-05T16:00"),
    )
    .await
    .expect("a two-day job fits two days of 8 h");
    let loads = team_loads(&db, team_id, "2026-05-04", "2026-05-05").await;
    assert!((loads["2026-05-04"].0 - 6.0).abs() < 0.01, "{loads:?}");
    assert!((loads["2026-05-05"].0 - 6.0).abs() < 0.01, "{loads:?}");
    assert!((loads["2026-05-04"].1.expect("ratio") - 0.75).abs() < 0.01);
    let stored = scheduling::list_schedule_commitments(
        &db,
        ScheduleCommitmentFilter {
            period_start: None,
            period_end: None,
            team_id: Some(team_id),
            personnel_id: None,
        },
    )
    .await
    .expect("commitments");
    assert_eq!(stored[0].planned_labor_hours, Some(12.0), "the labour hours it booked");

    // A second 6 h job on Monday no longer fits (6 + 6 > 8).
    let second = insert_candidate(&db, "pm_occurrence", 7102, "ready", 100.0).await;
    set_estimate(&db, second, Some(6.0)).await;
    let refused = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(second, team_id, "2026-05-04T08:00", "2026-05-04T14:00"),
    )
    .await
    .expect_err("overloaded");
    assert!(format!("{refused:?}").contains("2026-05-04"), "{refused:?}");
}

#[tokio::test]
async fn work_without_an_estimate_cannot_be_committed() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate = insert_candidate(&db, "pm_occurrence", 7201, "ready", 100.0).await;
    set_estimate(&db, candidate, None).await;
    let refused = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T10:00"),
    )
    .await
    .expect_err("not estimated");
    assert!(format!("{refused:?}").contains("PLANNING_NOT_ESTIMATED"), "{refused:?}");
}

#[tokio::test]
async fn finished_work_no_longer_loads_the_team() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (wo_id, _) = create_planning_wo(&db, "Finished job").await;
    let candidate = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO schedule_commitments (schedule_candidate_id, source_type, source_id, schedule_period_start, \
         schedule_period_end, committed_start, committed_end, assigned_team_id, planned_labor_hours) \
         VALUES (?, 'work_order', ?, '2026-05-04', '2026-05-04', ?, ?, ?, 4.0)",
        [
            candidate.into(),
            wo_id.into(),
            local("2026-05-04T08:00").into(),
            local("2026-05-04T12:00").into(),
            team_id.into(),
        ],
    ))
    .await
    .expect("commitment");
    assert!((team_loads(&db, team_id, "2026-05-04", "2026-05-04").await["2026-05-04"].0 - 4.0).abs() < 0.01);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET cancelled_at = '2026-05-03T10:00:00Z' WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("cancel");
    assert_eq!(
        team_loads(&db, team_id, "2026-05-04", "2026-05-04").await["2026-05-04"].0,
        0.0,
        "cancelled work drops out"
    );
}

#[tokio::test]
async fn a_day_without_capacity_is_reported_as_such_not_as_full() {
    let db = setup_db().await;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO teams (entity_sync_id, code, name, team_type, status, created_at, updated_at) \
         VALUES ('pln-empty-team', 'PLN-EMPTY', 'Équipe vide', 'maintenance', 'active', datetime('now'), datetime('now'))"
            .to_string(),
    ))
    .await
    .expect("team");
    let team_id: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM teams WHERE code = 'PLN-EMPTY'".to_string(),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "id")
        .expect("id");
    let loads = team_loads(&db, team_id, "2026-05-04", "2026-05-04").await;
    let (committed, ratio, basis) = &loads["2026-05-04"];
    assert_eq!(*committed, 0.0);
    assert_eq!(*ratio, None, "no capacity is not 100 %");
    assert_eq!(basis, "no_members");
}

#[tokio::test]
async fn a_shutdown_requirement_is_checked_against_open_windows() {
    let db = setup_db().await;
    let candidate = insert_candidate(&db, "manual_test", 7301, "ready", 100.0).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_candidates SET shutdown_requirement = 'required', window_start = ?, window_end = ? WHERE id = ?",
        [
            local("2026-05-04T08:00").into(),
            local("2026-05-04T12:00").into(),
            candidate.into(),
        ],
    ))
    .await
    .expect("needs a shutdown");
    let refresh = || async {
        queries::refresh_schedule_candidates(
            &db,
            RefreshScheduleCandidatesInput {
                include_work_orders: Some(false),
                include_pm_occurrences: Some(false),
                include_approved_di: Some(false),
                limit_per_source: Some(50),
            },
        )
        .await
        .expect("the refresh no longer fails on a shutdown requirement")
    };
    refresh().await;
    let conflicts = |db: &DatabaseConnection| {
        let db = db.clone();
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM scheduling_conflicts WHERE candidate_id = ? AND reason_code = 'NO_FEASIBLE_WINDOW'",
                [candidate.into()],
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get::<i64>("", "c")
            .expect("count")
        }
    };
    assert_eq!(conflicts(&db).await, 1, "no open window yet");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO planning_windows (window_type, start_datetime, end_datetime, is_locked) VALUES ('shutdown', ?, ?, 0)",
        [local("2026-05-04T06:00").into(), local("2026-05-04T18:00").into()],
    ))
    .await
    .expect("window");
    refresh().await;
    assert_eq!(conflicts(&db).await, 0, "the shutdown window covers the work");
}

#[tokio::test]
async fn a_break_in_can_unassign_the_technician() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let assignee_id = ensure_personnel_with_rate(&db, "TECH-UNASSIGN").await;
    let candidate = insert_candidate(&db, "pm_occurrence", 7401, "ready", 100.0).await;
    let mut input = commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T09:00");
    input.assigned_personnel_id = Some(assignee_id);
    let commitment = scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("committed");
    let break_in = scheduling::create_schedule_break_in(
        &db,
        1,
        CreateScheduleBreakInInput {
            schedule_commitment_id: commitment.id,
            expected_commitment_row_version: commitment.row_version,
            break_in_reason: "other".to_string(),
            approved_by_user_id: None,
            new_slot_start: local("2026-05-04T10:00"),
            new_slot_end: local("2026-05-04T11:00"),
            new_assigned_team_id: None,
            new_assigned_personnel_id: None,
            clear_assignee: Some(true),
            bypass_availability: Some(false),
            override_reason: None,
            acknowledged_assignee_issues: Vec::new(),
            dangerous_override_reason: None,
        },
    )
    .await
    .expect("break-in");
    let moved = scheduling::list_schedule_commitments(
        &db,
        ScheduleCommitmentFilter {
            period_start: None,
            period_end: None,
            team_id: Some(team_id),
            personnel_id: None,
        },
    )
    .await
    .expect("commitments")
    .into_iter()
    .find(|c| c.id == break_in.schedule_commitment_id)
    .expect("moved commitment");
    assert_eq!(
        moved.assigned_personnel_id, None,
        "\"Unassigned\" removes the technician"
    );
}

#[tokio::test]
async fn labour_is_duration_times_crew_and_work_without_a_crew_cannot_be_committed() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await; // 8 h every day
    let candidate = insert_candidate(&db, "pm_occurrence", 7401, "ready", 100.0).await;
    set_estimate(&db, candidate, Some(3.0)).await;
    set_crew(&db, candidate, Some(2)).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T11:00"),
    )
    .await
    .expect("3 h × 2 people = 6 h fits 8 h");
    let loads = team_loads(&db, team_id, "2026-05-04", "2026-05-04").await;
    assert!((loads["2026-05-04"].0 - 6.0).abs() < 0.01, "{loads:?}");

    // 2 h × 2 people = 4 h no longer fits the 2 h left.
    let second = insert_candidate(&db, "pm_occurrence", 7402, "ready", 100.0).await;
    set_estimate(&db, second, Some(2.0)).await;
    set_crew(&db, second, Some(2)).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(second, team_id, "2026-05-04T13:00", "2026-05-04T15:00"),
    )
    .await
    .expect_err("the crew's hours exceed the day");

    // A duration without a crew size is not estimated: refused, nothing assumed.
    let third = insert_candidate(&db, "pm_occurrence", 7403, "ready", 100.0).await;
    set_estimate(&db, third, Some(1.0)).await;
    set_crew(&db, third, None).await;
    let refused = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(third, team_id, "2026-05-05T08:00", "2026-05-05T09:00"),
    )
    .await
    .expect_err("no crew size");
    assert!(format!("{refused:?}").contains("PLANNING_NOT_ESTIMATED"), "{refused:?}");
}

#[tokio::test]
async fn the_work_order_crew_size_reaches_the_backlog() {
    let db = setup_db().await;
    let (wo_id, _) = create_planning_wo(&db, "Crew job").await;
    let refresh = || async {
        queries::refresh_schedule_candidates(
            &db,
            RefreshScheduleCandidatesInput {
                include_work_orders: Some(true),
                include_pm_occurrences: Some(false),
                include_approved_di: Some(false),
                limit_per_source: Some(50),
            },
        )
        .await
        .expect("refresh");
    };
    let crew_and_conflict = |db: &DatabaseConnection| {
        let db = db.clone();
        async move {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT sc.planned_crew_size AS crew,
                            (SELECT COUNT(*) FROM scheduling_conflicts c
                              WHERE c.candidate_id = sc.id AND c.reason_code = 'NOT_ESTIMATED'
                                AND c.resolved_at IS NULL) AS not_estimated
                     FROM schedule_candidates sc
                     WHERE sc.source_type = 'work_order' AND sc.source_id = ?",
                    [wo_id.into()],
                ))
                .await
                .expect("query")
                .expect("candidate");
            (
                row.try_get::<Option<i64>>("", "crew").expect("crew"),
                row.try_get::<i64>("", "not_estimated").expect("count"),
            )
        }
    };
    refresh().await;
    assert_eq!(
        crew_and_conflict(&db).await,
        (None, 1),
        "2 h but no crew size: not estimated"
    );
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET planned_crew_size = 3 WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("crew size");
    refresh().await;
    assert_eq!(
        crew_and_conflict(&db).await,
        (Some(3), 0),
        "estimated once the crew size is set"
    );
}

/// The test crew on a 7-day night shift 22:00–06:00 (8 h), instead of the day shift.
async fn give_team_night_shift(db: &DatabaseConnection, team_id: i64) {
    for sql in [
        "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, code, label, sort_order, is_active) \
         SELECT 'planning-test-night-shift', rs.id, 'PLN-NIGHT', 'Planning test night shift', 98, 1 \
         FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
         WHERE UPPER(rd.code) = 'ORG.SCHEDULE_CLASS' AND rs.status = 'published' \
         ORDER BY rs.version_no DESC LIMIT 1",
        "INSERT OR IGNORE INTO schedule_class_versions (entity_sync_id, reference_value_id, effective_from, \
         cycle_length_days, rotation_start_date, days_json, is_continuous, utilisation_percent, created_at, updated_at) \
         SELECT 'planning-test-night-version', rv.id, '2000-01-01', 7, NULL, \
         (SELECT json_group_array(json_object('cycle_day', n, 'shift_start', '22:00', 'shift_end', '06:00', \
                 'break_minutes', 0, 'is_rest_day', json('false'))) \
            FROM (SELECT 1 AS n UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 \
                  UNION SELECT 7)), \
         1, 100.0, datetime('now'), datetime('now') \
         FROM reference_values rv WHERE rv.entity_sync_id = 'planning-test-night-shift'",
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("night shift schedule");
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO personnel (employee_code, full_name, availability_status, primary_team_id, \
         home_schedule_reference_value_id) \
         SELECT ?, 'Planning night technician', 'available', ?, rv.id FROM reference_values rv \
         WHERE rv.entity_sync_id = 'planning-test-night-shift'",
        [format!("PLN-NIGHT-{team_id}").into(), team_id.into()],
    ))
    .await
    .expect("night member");
}

#[tokio::test]
async fn a_three_day_job_fits_three_eight_hour_days() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await; // 08:00–16:00 every day
    let candidate = insert_candidate(&db, "pm_occurrence", 7501, "ready", 100.0).await;
    set_estimate(&db, candidate, Some(8.0)).await;
    set_crew(&db, candidate, Some(3)).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-06T17:00"),
    )
    .await
    .expect("8 h × 3 people over three 8 h days fits");
    let loads = team_loads(&db, team_id, "2026-05-04", "2026-05-06").await;
    for d in ["2026-05-04", "2026-05-05", "2026-05-06"] {
        assert!((loads[d].0 - 8.0).abs() < 0.01, "{loads:?}");
    }
}

#[tokio::test]
async fn a_night_job_loads_the_night_it_starts() {
    let db = setup_db().await;
    let team_id = get_any_team_id_inner(&db).await;
    give_team_night_shift(&db, team_id).await;
    let candidate = insert_candidate(&db, "pm_occurrence", 7601, "ready", 100.0).await;
    set_estimate(&db, candidate, Some(8.0)).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T22:00", "2026-05-05T06:00"),
    )
    .await
    .expect("an 8 h night job fits the 8 h night shift");
    let loads = team_loads(&db, team_id, "2026-05-04", "2026-05-05").await;
    assert!((loads["2026-05-04"].0 - 8.0).abs() < 0.01, "{loads:?}");
    assert!(loads["2026-05-05"].0.abs() < 0.01, "nothing on the next day: {loads:?}");
    // A second job the same night no longer fits, and the refusal names the night's start day.
    let second = insert_candidate(&db, "pm_occurrence", 7602, "ready", 100.0).await;
    set_estimate(&db, second, Some(2.0)).await;
    let refused = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(second, team_id, "2026-05-05T02:00", "2026-05-05T04:00"),
    )
    .await
    .expect_err("the night is full");
    assert!(format!("{refused:?}").contains("2026-05-04"), "{refused:?}");
}

#[tokio::test]
async fn a_slot_longer_than_a_month_is_refused() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate = insert_candidate(&db, "pm_occurrence", 7701, "ready", 100.0).await;
    let refused = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-01T08:00", "2026-06-05T16:00"),
    )
    .await
    .expect_err("35 days");
    assert!(format!("{refused:?}").contains("PLANNING_SLOT_TOO_LONG"), "{refused:?}");
}

#[tokio::test]
async fn the_crew_size_is_bounded_whole_and_clearable() {
    let db = setup_db().await;
    let (wo_id, actor) = create_planning_wo(&db, "Crew bounds").await;
    let row_version = |db: &DatabaseConnection| {
        let db = db.clone();
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT row_version, planned_crew_size FROM work_orders WHERE id = ?",
                [wo_id.into()],
            ))
            .await
            .expect("query")
            .map(|r| {
                (
                    r.try_get::<i64>("", "row_version").expect("rv"),
                    r.try_get::<Option<i64>>("", "planned_crew_size").expect("crew"),
                )
            })
            .expect("work order")
        }
    };
    let save = |rv: i64, crew: Option<i64>, clear: bool| crate::wo::workflow::actions::WoScheduleSaveInput {
        wo_id,
        actor_id: actor,
        expected_row_version: rv,
        planner_id: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        shift_class_version_id: None,
        clear_shift: false,
        expected_duration_hours: None,
        planned_crew_size: crew,
        clear_crew_size: clear,
        urgency_code: None,
        planned_downtime_hours: None,
    };
    for bad in [0, crate::planning::load::MAX_CREW_SIZE + 1] {
        let (rv, _) = row_version(&db).await;
        let err = crate::wo::workflow::actions::save_schedule(&db, save(rv, Some(bad), false))
            .await
            .expect_err("out of bounds");
        assert!(format!("{err:?}").contains("PLANNING_CREW_SIZE_INVALID"), "{err:?}");
    }
    let (rv, _) = row_version(&db).await;
    crate::wo::workflow::actions::save_schedule(&db, save(rv, Some(4), false))
        .await
        .expect("4 people");
    assert_eq!(row_version(&db).await.1, Some(4));
    let (rv, _) = row_version(&db).await;
    crate::wo::workflow::actions::save_schedule(&db, save(rv, None, false))
        .await
        .expect("omitted keeps it");
    assert_eq!(row_version(&db).await.1, Some(4));
    let (rv, _) = row_version(&db).await;
    crate::wo::workflow::actions::save_schedule(&db, save(rv, None, true))
        .await
        .expect("cleared");
    assert_eq!(row_version(&db).await.1, None, "the crew size can be removed");
    // The database refuses an impossible crew size whatever writes it (migration 207).
    for value in ["99", "2.5", "0"] {
        let err = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("UPDATE work_orders SET planned_crew_size = {value} WHERE id = ?"),
                [wo_id.into()],
            ))
            .await
            .expect_err("trigger");
        assert!(err.to_string().contains("planned_crew_size"), "{err}");
    }
}

#[tokio::test]
async fn finance_counts_a_team_month_like_the_board() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    // 3 h × 2 people in May 2026.
    let counted = insert_candidate(&db, "pm_occurrence", 7801, "ready", 100.0).await;
    set_estimate(&db, counted, Some(3.0)).await;
    set_crew(&db, counted, Some(2)).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(counted, team_id, "2026-05-04T08:00", "2026-05-04T11:00"),
    )
    .await
    .expect("counted job");
    // Cancelled work in May 2026: left out.
    let (wo_id, _) = create_planning_wo(&db, "Cancelled job").await;
    let cancelled = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(cancelled, team_id, "2026-05-05T08:00", "2026-05-05T09:00"),
    )
    .await
    .expect("cancelled job");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET cancelled_at = '2026-05-01T00:00:00Z' WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("cancel");
    // May of another year: not merged into May 2026.
    let other_year = insert_candidate(&db, "pm_occurrence", 7802, "ready", 100.0).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(other_year, team_id, "2025-05-05T08:00", "2025-05-05T09:00"),
    )
    .await
    .expect("2025 job");

    let months = crate::planning::load::team_month_loads(&db, 2026)
        .await
        .expect("month loads");
    assert!((months[&(team_id, 5)] - 6.0).abs() < 0.01, "{months:?}");
}

async fn roster_person(db: &DatabaseConnection, team_id: i64) -> i64 {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM personnel WHERE employee_code = ?",
        [format!("PLN-ROSTER-{team_id}").into()],
    ))
    .await
    .expect("query")
    .expect("roster person")
    .try_get("", "id")
    .expect("id")
}

/// A work order the technician carries outside planning: responsible, planned slot, duration.
async fn wo_outside_planning(db: &DatabaseConnection, person: i64, start: &str, end: &str, hours: f64) -> i64 {
    let (wo_id, actor) = create_planning_wo(db, "Assigned outside planning").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET personnel_id = ? WHERE id = ?",
        [person.into(), actor.into()],
    ))
    .await
    .expect("link login");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET primary_responsible_id = ?, planned_start = ?, planned_end = ?, \
             expected_duration_hours = ? WHERE id = ?",
        [
            actor.into(),
            local(start).into(),
            local(end).into(),
            hours.into(),
            wo_id.into(),
        ],
    ))
    .await
    .expect("assign outside planning");
    wo_id
}

async fn assignee_overrides(db: &DatabaseConnection) -> i64 {
    db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT COUNT(*) AS c FROM schedule_change_log WHERE action_type = 'override_assignee'".to_string(),
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get("", "c")
    .expect("count")
}

#[tokio::test]
async fn a_technician_carries_work_orders_assigned_outside_planning() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await; // the roster technician works 08:00–16:00
                                              // A second technician on the crew: the crew has room, only this technician's day is full.
    ensure_personnel_with_rate(&db, "PLN-SECOND").await;
    let person = roster_person(&db, team_id).await;
    let wo_id = wo_outside_planning(&db, person, "2026-05-04T08:00", "2026-05-04T16:00", 6.0).await;

    let loads = crate::planning::person_load::person_day_loads(
        &db,
        &[person],
        day("2026-05-04"),
        day("2026-05-04"),
        None,
        None,
    )
    .await
    .expect("person loads");
    assert!((loads[&(person, day("2026-05-04"))] - 6.0).abs() < 0.01, "{loads:?}");

    // Committing that same work order to them does not count it twice.
    let own = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    set_estimate(&db, own, Some(6.0)).await;
    let mut input = commit_input(own, team_id, "2026-05-04T08:00", "2026-05-04T14:00");
    input.assigned_personnel_id = Some(person);
    scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("its own work order is not counted against it");
    let loads = crate::planning::person_load::person_day_loads(
        &db,
        &[person],
        day("2026-05-04"),
        day("2026-05-04"),
        None,
        None,
    )
    .await
    .expect("person loads");
    assert!(
        (loads[&(person, day("2026-05-04"))] - 6.0).abs() < 0.01,
        "counted once: {loads:?}"
    );

    // Another 4 h job the same day: 6 + 4 > 8, refused for the technician.
    let other = insert_candidate(&db, "pm_occurrence", 7901, "ready", 100.0).await;
    set_estimate(&db, other, Some(4.0)).await;
    let mut input = commit_input(other, team_id, "2026-05-04T10:00", "2026-05-04T14:00");
    input.assigned_personnel_id = Some(person);
    let refused = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("technician overloaded");
    assert!(
        format!("{refused:?}").contains("PLANNING_ASSIGNEE_OVERLOADED"),
        "{refused:?}"
    );

    // The picker says the same before committing: 2 h left, the shift, the refusal.
    let picker = scheduling::list_assignee_capacity(
        &db,
        crate::planning::domain::AssigneeCapacityInput {
            team_id,
            start: local("2026-05-04T10:00"),
            end: local("2026-05-04T14:00"),
            candidate_id: Some(other),
            commitment_id: None,
        },
    )
    .await
    .expect("picker");
    let row = picker
        .iter()
        .find(|r| r.personnel_id == person)
        .expect("technician listed");
    assert!((row.days[0].remaining_hours - 2.0).abs() < 0.01, "{row:?}");
    assert_eq!(row.days[0].shift_start.as_deref(), Some("08:00"));
    assert!(
        row.issues.contains(&"PLANNING_ASSIGNEE_OVERLOADED".to_string()),
        "{row:?}"
    );

    // Overridden with a reason, the refusal acknowledged: committed and logged.
    input.allow_double_booking_override = Some(true);
    input.override_reason = Some("Short job, overtime agreed with the shift lead".to_string());
    input.acknowledged_assignee_issues = vec!["PLANNING_ASSIGNEE_OVERLOADED".to_string()];
    scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("overridden");
    assert_eq!(assignee_overrides(&db).await, 1, "the override is logged");
}

#[tokio::test]
async fn an_assignee_off_shift_or_outside_the_crew_is_refused_unless_overridden() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let person = roster_person(&db, team_id).await;

    // 18:00–20:00: after the 08:00–16:00 shift.
    let late = insert_candidate(&db, "pm_occurrence", 8001, "ready", 100.0).await;
    let mut input = commit_input(late, team_id, "2026-05-04T18:00", "2026-05-04T20:00");
    input.assigned_personnel_id = Some(person);
    let refused = scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect_err("off shift");
    assert!(
        format!("{refused:?}").contains("PLANNING_ASSIGNEE_NOT_WORKING"),
        "{refused:?}"
    );

    // Someone who is not in the crew.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT INTO personnel (employee_code, full_name, availability_status) \
         VALUES ('PLN-OUTSIDER', 'Outside technician', 'available')"
            .to_string(),
    ))
    .await
    .expect("outsider");
    let outsider: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM personnel WHERE employee_code = 'PLN-OUTSIDER'".to_string(),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "id")
        .expect("id");
    let job = insert_candidate(&db, "pm_occurrence", 8002, "ready", 100.0).await;
    let mut input = commit_input(job, team_id, "2026-05-05T09:00", "2026-05-05T10:00");
    input.assigned_personnel_id = Some(outsider);
    let refused = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("not in the crew");
    assert!(
        format!("{refused:?}").contains("PLANNING_ASSIGNEE_NOT_IN_CREW"),
        "{refused:?}"
    );

    // A reason alone does not cover refusals the planner did not see.
    input.allow_double_booking_override = Some(true);
    input.override_reason = Some("Borrowed from the electrical crew for the day".to_string());
    let refused = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("refusals not acknowledged");
    assert!(
        format!("{refused:?}").contains("PLANNING_ASSIGNEE_NOT_IN_CREW"),
        "{refused:?}"
    );
    // Acknowledged but without a reason: refused; with both, the commitment is made and logged.
    input.acknowledged_assignee_issues = vec![
        "PLANNING_ASSIGNEE_NOT_IN_CREW".to_string(),
        "PLANNING_ASSIGNEE_NOT_WORKING".to_string(),
    ];
    input.override_reason = None;
    scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("a reason is required");
    input.override_reason = Some("Borrowed from the electrical crew for the day".to_string());
    scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("overridden with a reason");
    assert_eq!(assignee_overrides(&db).await, 1);
}

async fn set_wo_dates(db: &DatabaseConnection, wo_id: i64, start: &str, end: &str) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET planned_start = ?, planned_end = ? WHERE id = ?",
        [start.into(), end.into(), wo_id.into()],
    ))
    .await
    .expect("set work order dates");
}

async fn wo_code(db: &DatabaseConnection, wo_id: i64) -> String {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT code FROM work_orders WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get("", "code")
    .expect("code")
}

async fn person_hours_on(db: &DatabaseConnection, person: i64, on: &str) -> f64 {
    crate::planning::person_load::person_day_loads(db, &[person], day(on), day(on), None, None)
        .await
        .expect("person loads")
        .get(&(person, day(on)))
        .copied()
        .unwrap_or(0.0)
}

#[tokio::test]
async fn typed_and_unreadable_work_order_dates_never_break_the_checks() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let person = roster_person(&db, team_id).await;
    // Dates typed in a form (local, no offset) are read like the shift code reads them.
    let typed = wo_outside_planning(&db, person, "2026-05-04T08:00", "2026-05-04T11:00", 3.0).await;
    set_wo_dates(&db, typed, "2026-05-04T08:00", "2026-05-04T11:00").await;
    // Dates that cannot be read: skipped and reported, never an error.
    let broken = wo_outside_planning(&db, person, "2026-05-04T12:00", "2026-05-04T13:00", 1.0).await;
    set_wo_dates(&db, broken, "next tuesday", "soon").await;
    assert!((person_hours_on(&db, person, "2026-05-04").await - 3.0).abs() < 0.01);

    let job = insert_candidate(&db, "pm_occurrence", 8101, "ready", 100.0).await;
    let picker = scheduling::list_assignee_capacity(
        &db,
        crate::planning::domain::AssigneeCapacityInput {
            team_id,
            start: local("2026-05-04T13:00"),
            end: local("2026-05-04T14:00"),
            candidate_id: Some(job),
            commitment_id: None,
        },
    )
    .await
    .expect("the picker still works");
    let row = picker
        .iter()
        .find(|r| r.personnel_id == person)
        .expect("technician listed");
    let code = wo_code(&db, broken).await;
    assert!(row.unreadable_work_orders.iter().any(|w| w.code == code), "{row:?}");

    let mut input = commit_input(job, team_id, "2026-05-04T13:00", "2026-05-04T14:00");
    input.assigned_personnel_id = Some(person);
    scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("the gate still works");
}

#[tokio::test]
async fn a_commitment_without_a_technician_does_not_hide_the_work_order_from_its_responsible() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let person = roster_person(&db, team_id).await;
    let wo_id = wo_outside_planning(&db, person, "2026-05-04T08:00", "2026-05-04T16:00", 6.0).await;
    let candidate = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    set_estimate(&db, candidate, Some(6.0)).await;
    // Planning commits the work order to the crew without naming a technician.
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T14:00"),
    )
    .await
    .expect("crew commitment");
    // The write-back keeps the OT's responsible (only a named technician or an explicit
    // "Unassigned" changes it) ...
    let responsible: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ua.personnel_id AS p FROM work_orders wo \
               LEFT JOIN user_accounts ua ON ua.id = wo.primary_responsible_id WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "p")
        .expect("responsible");
    assert_eq!(responsible, Some(person), "the OT keeps its responsible");
    // ... and that technician carries the job once (through the OT, the commitment names nobody).
    assert!(
        (person_hours_on(&db, person, "2026-05-04").await - 6.0).abs() < 0.01,
        "the responsible technician still carries it, once"
    );
}

#[tokio::test]
async fn the_crew_load_counts_its_work_orders_outside_planning_once() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (wo_id, _) = create_planning_wo(&db, "Crew job outside planning").await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET assigned_team_id = ?, planned_start = ?, planned_end = ?, \
             expected_duration_hours = 2.0, planned_crew_size = 3 WHERE id = ?",
        [
            team_id.into(),
            local("2026-05-04T08:00").into(),
            local("2026-05-04T12:00").into(),
            wo_id.into(),
        ],
    ))
    .await
    .expect("crew OT outside planning");
    let loads = team_loads(&db, team_id, "2026-05-04", "2026-05-04").await;
    assert!((loads["2026-05-04"].0 - 6.0).abs() < 0.01, "2 h × 3 people: {loads:?}");

    // Committed by planning: counted once, as the commitment.
    let candidate = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    set_estimate(&db, candidate, Some(2.0)).await;
    set_crew(&db, candidate, Some(3)).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T12:00"),
    )
    .await
    .expect("its own OT is not counted against it");
    let loads = team_loads(&db, team_id, "2026-05-04", "2026-05-04").await;
    assert!((loads["2026-05-04"].0 - 6.0).abs() < 0.01, "counted once: {loads:?}");
}

/// Planning load follows the shared "Open (work to do)" rule on the OT's status: a completed OT
/// awaiting close-out is not load, even when its mechanical-completion time was never written.
#[tokio::test]
async fn a_completed_work_order_without_completion_time_is_not_planning_load() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let person = roster_person(&db, team_id).await;
    let wo_id = wo_outside_planning(&db, person, "2026-05-04T08:00", "2026-05-04T12:00", 2.0).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET assigned_team_id = ? WHERE id = ?",
        [team_id.into(), wo_id.into()],
    ))
    .await
    .expect("crew OT outside planning");
    let team_hours = |loads: std::collections::BTreeMap<String, (f64, Option<f64>, String)>| {
        loads.get("2026-05-04").map(|l| l.0).unwrap_or(0.0)
    };
    assert!((person_hours_on(&db, person, "2026-05-04").await - 2.0).abs() < 0.01);
    assert!(team_hours(team_loads(&db, team_id, "2026-05-04", "2026-05-04").await) > 0.0);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'completed'), \
             mechanically_completed_at = NULL WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("completed without a completion time");
    assert!(
        person_hours_on(&db, person, "2026-05-04").await.abs() < 0.01,
        "a completed OT is no longer the technician's load"
    );
    let team = team_hours(team_loads(&db, team_id, "2026-05-04", "2026-05-04").await);
    assert!(team.abs() < 0.01, "a completed OT is no longer the crew's load: {team}");
}

#[test]
fn a_slot_books_net_working_hours_not_the_raw_overlap() {
    use crate::planning::load::{net_hours_in, working_hours_in};
    // An 8 h window worth 6 net hours (break and utilisation): half the window is 3 net hours.
    let days =
        std::collections::BTreeMap::from([(day("2026-05-04"), shift_hours("2026-05-04", "08:00", "16:00", 6.0))]);
    let net = net_hours_in(&local("2026-05-04T08:00"), &local("2026-05-04T12:00"), &days).expect("net");
    assert!((net - 3.0).abs() < 1e-6, "{net}");
    let raw = working_hours_in(&local("2026-05-04T08:00"), &local("2026-05-04T12:00"), &days).expect("raw");
    assert!((raw - 4.0).abs() < 1e-6, "{raw}");
}

#[tokio::test]
async fn a_work_order_without_a_duration_books_the_working_time_of_its_slot() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await; // 08:00–16:00, 8 h net
    let person = roster_person(&db, team_id).await;
    let wo_id = wo_outside_planning(&db, person, "2026-05-04T08:00", "2026-05-04T12:00", 1.0).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET expected_duration_hours = NULL WHERE id = ?",
        [wo_id.into()],
    ))
    .await
    .expect("no duration");
    assert!((person_hours_on(&db, person, "2026-05-04").await - 4.0).abs() < 0.01);
}

#[tokio::test]
async fn a_break_in_overrides_only_acknowledged_refusals_and_logs_them() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let person = roster_person(&db, team_id).await;
    let candidate = insert_candidate(&db, "pm_occurrence", 8201, "ready", 100.0).await;
    let mut input = commit_input(candidate, team_id, "2026-05-04T09:00", "2026-05-04T10:00");
    input.assigned_personnel_id = Some(person);
    let commitment = scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("commitment");

    // Moved to 18:00–20:00, after the technician's shift.
    let break_in = |reason: Option<&str>, acknowledged: Vec<String>| CreateScheduleBreakInInput {
        schedule_commitment_id: commitment.id,
        expected_commitment_row_version: commitment.row_version,
        break_in_reason: "emergency".to_string(),
        approved_by_user_id: None,
        new_slot_start: local("2026-05-04T18:00"),
        new_slot_end: local("2026-05-04T20:00"),
        new_assigned_team_id: Some(team_id),
        new_assigned_personnel_id: Some(person),
        clear_assignee: None,
        bypass_availability: Some(false),
        override_reason: reason.map(str::to_string),
        acknowledged_assignee_issues: acknowledged,
        dangerous_override_reason: Some("Pump failure, line stopped".to_string()),
    };
    let refused = scheduling::create_schedule_break_in(&db, 1, break_in(Some("Night callout"), Vec::new()))
        .await
        .expect_err("not acknowledged");
    assert!(
        format!("{refused:?}").contains("PLANNING_ASSIGNEE_NOT_WORKING"),
        "{refused:?}"
    );

    scheduling::create_schedule_break_in(
        &db,
        1,
        break_in(Some("Night callout"), vec!["PLANNING_ASSIGNEE_NOT_WORKING".to_string()]),
    )
    .await
    .expect("acknowledged, with a reason");
    let logged = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM schedule_change_log WHERE action_type = 'override_assignee' \
               AND commitment_id = ? AND actor_id = 1 AND reason = 'Night callout' \
               AND reason_code LIKE '%PLANNING_ASSIGNEE_NOT_WORKING%'",
            [commitment.id.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "c")
        .expect("count");
    assert_eq!(logged, 1, "logged with actor, reason and the overridden codes");
}

#[tokio::test]
async fn an_older_commitment_counts_labour_per_person_or_is_reported_unknown() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    ensure_personnel_with_rate(&db, "PLN-SECOND-OLD").await;
    let person = roster_person(&db, team_id).await;
    let candidate = insert_candidate(&db, "pm_occurrence", 8301, "ready", 100.0).await;
    set_estimate(&db, candidate, Some(4.0)).await;
    set_crew(&db, candidate, Some(2)).await;
    let mut input = commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T12:00");
    input.assigned_personnel_id = Some(person);
    let commitment = scheduling::create_schedule_commitment(&db, 1, input)
        .await
        .expect("commitment");
    // As before migration 210: no hours per person recorded; 8 h of labour ÷ 2 people.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_commitments SET planned_person_hours = NULL WHERE id = ?",
        [commitment.id.into()],
    ))
    .await
    .expect("older commitment");
    assert!((person_hours_on(&db, person, "2026-05-04").await - 4.0).abs() < 0.01);

    // Without a crew size the hours are unknown: not counted, and reported.
    set_crew(&db, candidate, None).await;
    assert!(
        person_hours_on(&db, person, "2026-05-04").await.abs() < 0.01,
        "never the slot's length"
    );
    let picker = scheduling::list_assignee_capacity(
        &db,
        crate::planning::domain::AssigneeCapacityInput {
            team_id,
            start: local("2026-05-04T13:00"),
            end: local("2026-05-04T14:00"),
            candidate_id: None,
            commitment_id: None,
        },
    )
    .await
    .expect("picker");
    let row = picker
        .iter()
        .find(|r| r.personnel_id == person)
        .expect("technician listed");
    assert_eq!(row.unknown_hours_jobs, 1, "{row:?}");
}

// ── Step C: planning windows per site / equipment, reasons, qualifications of every job ──────

async fn exec_sql(db: &DatabaseConnection, sql: &str) {
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
        .expect("value")
}

/// The site of the test equipment (its location) and a sub-node of it that holds nothing.
async fn site_and_sub_node(db: &DatabaseConnection) -> (i64, i64) {
    seed_equipment_for_wo(db).await;
    let site = scalar(db, "SELECT installed_at_node_id FROM equipment WHERE id = 1").await;
    exec_sql(
        db,
        &format!(
            "INSERT OR IGNORE INTO org_nodes (entity_sync_id, code, name, node_type_id, parent_id, ancestor_path,
                depth, status, created_at, updated_at, row_version, structure_model_id)
             SELECT 'planning-sub-node', 'SUB-PLN', 'Atelier voisin', node_type_id, id, ancestor_path || id || '/',
                    depth + 1, 'active', created_at, updated_at, 1, structure_model_id
               FROM org_nodes WHERE id = {site}"
        ),
    )
    .await;
    let sub = scalar(
        db,
        "SELECT id FROM org_nodes WHERE entity_sync_id = 'planning-sub-node'",
    )
    .await;
    (site, sub)
}

fn window_input(
    entity_id: Option<i64>,
    equipment_id: Option<i64>,
    window_type: &str,
    start: &str,
    end: &str,
    locked: bool,
) -> crate::planning::domain::CreatePlanningWindowInput {
    crate::planning::domain::CreatePlanningWindowInput {
        entity_id,
        equipment_id,
        window_type: window_type.to_string(),
        start_datetime: local(start),
        end_datetime: local(end),
        is_locked: Some(locked),
        lock_reason: locked.then(|| "Production campaign".to_string()),
    }
}

#[tokio::test]
async fn a_window_belongs_to_a_site_or_one_equipment_and_has_a_governed_type() {
    let db = setup_db().await;
    let (site, sub) = site_and_sub_node(&db).await;

    // The types are the SYSTEM list PLANNING.WINDOW_TYPES.
    let types = crate::planning::windows::window_types(&db).await.expect("types");
    for code in ["production", "maintenance", "shutdown", "freeze"] {
        assert!(types.contains_key(code), "{code} is a window type");
    }
    let unknown = scheduling::create_planning_window(
        &db,
        window_input(Some(site), None, "party", "2026-05-04T06:00", "2026-05-04T18:00", false),
    )
    .await
    .expect_err("not a window type");
    assert!(
        format!("{unknown:?}").contains("PLANNING_WINDOW_TYPE_UNKNOWN"),
        "{unknown:?}"
    );

    // An equipment alone: the window's site is the equipment's location.
    let on_equipment = scheduling::create_planning_window(
        &db,
        window_input(
            None,
            Some(1),
            "maintenance",
            "2026-05-04T06:00",
            "2026-05-04T18:00",
            false,
        ),
    )
    .await
    .expect("equipment window");
    assert_eq!(on_equipment.entity_id, Some(site));
    assert_eq!(on_equipment.equipment_id, Some(1));
    assert_eq!(on_equipment.equipment_code.as_deref(), Some("EQ-PLN-001"));
    assert!(on_equipment.entity_name.is_some());

    // The equipment is not installed on the sub-node.
    let elsewhere = scheduling::create_planning_window(
        &db,
        window_input(
            Some(sub),
            Some(1),
            "maintenance",
            "2026-05-04T06:00",
            "2026-05-04T18:00",
            false,
        ),
    )
    .await
    .expect_err("equipment not on that site");
    assert!(
        format!("{elsewhere:?}").contains("PLANNING_WINDOW_EQUIPMENT_NOT_ON_SITE"),
        "{elsewhere:?}"
    );

    // The scope is editable: the whole site, then locked.
    let widened = scheduling::update_planning_window(
        &db,
        on_equipment.id,
        on_equipment.row_version,
        crate::planning::domain::UpdatePlanningWindowInput {
            entity_id: None,
            equipment_id: Some(None),
            window_type: None,
            start_datetime: None,
            end_datetime: None,
            is_locked: Some(true),
            lock_reason: Some("Line stop".to_string()),
        },
    )
    .await
    .expect("whole site");
    assert_eq!(widened.entity_id, Some(site));
    assert_eq!(widened.equipment_id, None);
    assert_eq!(widened.is_locked, 1);
    let stale = scheduling::update_planning_window(
        &db,
        on_equipment.id,
        on_equipment.row_version,
        crate::planning::domain::UpdatePlanningWindowInput {
            entity_id: None,
            equipment_id: None,
            window_type: None,
            start_datetime: None,
            end_datetime: None,
            is_locked: Some(false),
            lock_reason: None,
        },
    )
    .await
    .expect_err("stale version");
    assert!(format!("{stale:?}").contains("stale"), "{stale:?}");
}

#[tokio::test]
async fn a_locked_window_refuses_only_the_jobs_of_its_site_or_equipment() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (site, sub) = site_and_sub_node(&db).await;
    let (wo_id, actor_id) = create_planning_wo(&db, "Locked window WO").await;
    let candidate_id = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;

    let window = scheduling::create_planning_window(
        &db,
        window_input(
            Some(site),
            None,
            "production",
            "2026-10-05T06:00",
            "2026-10-05T18:00",
            true,
        ),
    )
    .await
    .expect("locked window on the equipment's site");
    let refused = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate_id, team_id, "2026-10-05T08:00", "2026-10-05T10:00"),
    )
    .await
    .expect_err("the job's site is locked");
    assert!(format!("{refused:?}").contains("PLANNING_WINDOW_LOCKED"), "{refused:?}");

    // The same window on a neighbouring node no longer concerns this job.
    scheduling::update_planning_window(
        &db,
        window.id,
        window.row_version,
        crate::planning::domain::UpdatePlanningWindowInput {
            entity_id: Some(Some(sub)),
            equipment_id: None,
            window_type: None,
            start_datetime: None,
            end_datetime: None,
            is_locked: None,
            lock_reason: None,
        },
    )
    .await
    .expect("moved to the sub-node");
    scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate_id, team_id, "2026-10-05T08:00", "2026-10-05T10:00"),
    )
    .await
    .expect("another node's window does not apply");
}

#[tokio::test]
async fn work_needing_a_shutdown_is_committed_only_inside_an_open_shutdown_window() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate = insert_candidate(&db, "manual_test", 7501, "ready", 100.0).await;
    exec_sql(
        &db,
        &format!("UPDATE schedule_candidates SET shutdown_requirement = 'required' WHERE id = {candidate}"),
    )
    .await;

    let none = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T10:00"),
    )
    .await
    .expect_err("no shutdown window");
    // Seen by the backlog first: no open shutdown window at all over its due dates.
    assert!(format!("{none:?}").contains("NO_FEASIBLE_WINDOW"), "{none:?}");

    // A production window is not a shutdown.
    scheduling::create_planning_window(
        &db,
        window_input(None, None, "production", "2026-05-04T06:00", "2026-05-04T18:00", false),
    )
    .await
    .expect("production window");
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T10:00"),
    )
    .await
    .expect_err("still no shutdown window");

    scheduling::create_planning_window(
        &db,
        window_input(None, None, "maintenance", "2026-05-04T06:00", "2026-05-04T12:00", false),
    )
    .await
    .expect("maintenance window");
    // Partly outside the maintenance window: refused by the commit itself.
    let outside = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T11:00", "2026-05-04T13:00"),
    )
    .await
    .expect_err("the slot leaves the window");
    assert!(
        format!("{outside:?}").contains("PLANNING_NO_SHUTDOWN_WINDOW"),
        "{outside:?}"
    );
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T10:00"),
    )
    .await
    .expect("inside the maintenance window");
}

#[tokio::test]
async fn a_commitment_with_open_conflicts_is_critical_and_says_why() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate = insert_candidate(&db, "manual_test", 7601, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T10:00"),
    )
    .await
    .expect("committed");
    assert_eq!(commitment.has_blocking_conflict, 0);
    assert_eq!(commitment.conflict_reasons_json.as_deref(), Some("[]"));

    // Parts no longer available after the commitment: the bar turns critical, with the reason.
    insert_conflict(&db, candidate, "missing_critical_part", false).await;
    insert_conflict(&db, candidate, "permit_not_ready", true).await;
    let listed = scheduling::list_schedule_commitments(
        &db,
        ScheduleCommitmentFilter {
            period_start: None,
            period_end: None,
            team_id: None,
            personnel_id: None,
        },
    )
    .await
    .expect("list");
    let row = listed.iter().find(|c| c.id == commitment.id).expect("commitment");
    assert_eq!(row.has_blocking_conflict, 1);
    let reasons: Vec<serde_json::Value> =
        serde_json::from_str(row.conflict_reasons_json.as_deref().expect("reasons")).expect("json");
    assert_eq!(reasons.len(), 1, "resolved conflicts are not reasons: {reasons:?}");
    assert_eq!(reasons[0]["conflict_type"], "missing_critical_part");
    assert_eq!(reasons[0]["reason_code"], "TEST_CONFLICT");

    // A backlog card (work not committed yet) carries its reasons the same way.
    let candidate = insert_candidate(&db, "manual_test", 7602, "ready", 100.0).await;
    insert_conflict(&db, candidate, "missing_critical_part", false).await;
    let snapshot = queries::get_schedule_backlog_snapshot(
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
    .expect("snapshot");
    let summary = snapshot
        .conflict_summary
        .iter()
        .find(|s| s.candidate_id == candidate)
        .expect("summary");
    assert_eq!(summary.reasons.len(), 1);
    assert_eq!(summary.reasons[0].conflict_type, "missing_critical_part");
}

/// A PM plan whose version lists `skills`, with one occurrence on equipment 1.
async fn pm_occurrence_with_skills(db: &DatabaseConnection, skills: &str) -> i64 {
    seed_equipment_for_wo(db).await;
    exec_sql(
        db,
        "INSERT INTO pm_plans (entity_sync_id, code, title, asset_scope_type, asset_scope_id, strategy_type,
            lifecycle_status, requires_shutdown, requires_permit)
         VALUES ('pln-qual-plan', 'PM-QUAL', 'Graissage pompe', 'equipment', 1, 'time_based', 'active', 0, 0)",
    )
    .await;
    let plan = scalar(db, "SELECT id FROM pm_plans WHERE code = 'PM-QUAL'").await;
    exec_sql(
        db,
        &format!(
            "INSERT INTO pm_plan_versions (entity_sync_id, pm_plan_id, version_no, status, effective_from,
                trigger_definition_json, required_skills_json)
             VALUES ('pln-qual-version', {plan}, 1, 'published', '2026-01-01', '{{}}', '{skills}')"
        ),
    )
    .await;
    let version = scalar(
        db,
        "SELECT id FROM pm_plan_versions WHERE entity_sync_id = 'pln-qual-version'",
    )
    .await;
    exec_sql(
        db,
        &format!(
            "INSERT INTO pm_occurrences (entity_sync_id, pm_plan_id, plan_version_id, due_basis, due_at, status,
                equipment_id)
             VALUES ('pln-qual-occ', {plan}, {version}, 'test', '2026-05-04T00:00:00Z', 'planned', 1)"
        ),
    )
    .await;
    scalar(
        db,
        "SELECT id FROM pm_occurrences WHERE entity_sync_id = 'pln-qual-occ'",
    )
    .await
}

#[tokio::test]
async fn a_pm_or_a_request_is_refused_to_a_technician_without_its_qualifications() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let person = roster_person(&db, team_id).await;
    let skill_code: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT rv.code FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id
               JOIN reference_domains d ON d.id = rs.domain_id
              WHERE d.code = 'PERSONNEL.SKILLS' AND rs.status = 'published' AND rv.is_active = 1
              ORDER BY rv.id LIMIT 1"
                .to_string(),
        ))
        .await
        .expect("query")
        .expect("a seeded skill")
        .try_get("", "code")
        .expect("code");

    // PM: the skills its plan version lists.
    let occurrence = pm_occurrence_with_skills(&db, &format!("[\"{skill_code}\"]")).await;
    let pm_candidate = insert_candidate(&db, "pm_occurrence", occurrence, "ready", 100.0).await;
    let mut input = commit_input(pm_candidate, team_id, "2026-05-04T08:00", "2026-05-04T10:00");
    input.assigned_personnel_id = Some(person);
    let refused = scheduling::create_schedule_commitment(&db, 1, input.clone())
        .await
        .expect_err("lacks the PM skill");
    assert!(
        format!("{refused:?}").contains("QUALIFICATION_GATE_BLOCKED"),
        "{refused:?}"
    );

    // The backlog says so too, naming the skill.
    exec_sql(
        &db,
        &format!("UPDATE schedule_candidates SET assigned_personnel_id = {person} WHERE id = {pm_candidate}"),
    )
    .await;
    queries::refresh_schedule_candidates(
        &db,
        RefreshScheduleCandidatesInput {
            include_work_orders: Some(false),
            include_pm_occurrences: Some(false),
            include_approved_di: Some(false),
            limit_per_source: Some(50),
        },
    )
    .await
    .expect("refresh");
    let gap = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) FROM scheduling_conflicts WHERE candidate_id = {pm_candidate}
               AND reason_code = 'SKILL_GAP' AND resolved_at IS NULL"
        ),
    )
    .await;
    assert_eq!(gap, 1, "the backlog shows the qualification gap");

    // Request: what the work-order type it becomes asks.
    insert_approved_di(&db, "DI-QUAL-1").await;
    let di = scalar(&db, "SELECT id FROM intervention_requests WHERE code = 'DI-QUAL-1'").await;
    let request_type: String = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT request_type FROM intervention_requests WHERE id = {di}"),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "request_type")
        .expect("request type");
    let nature = crate::di::conversion::conversion_nature(
        &db,
        &request_type,
        crate::reference::urgency_scale::UrgencyCode::Normal,
    )
    .await
    .expect("nature");
    let wo_type = crate::wo::types::default_type_for_nature(&db, nature)
        .await
        .expect("type");
    exec_sql(
        &db,
        &format!(
            "INSERT INTO qualification_requirements (entity_sync_id, target_kind, work_order_type_id,
                skill_reference_value_id, coverage, created_at, updated_at)
             SELECT 'pln-qual-type-req', 'work_order_type', {}, rv.id, 'each', '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z'
               FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id
               JOIN reference_domains d ON d.id = rs.domain_id
              WHERE d.code = 'PERSONNEL.SKILLS' AND rs.status = 'published' AND rv.is_active = 1
                AND rv.code <> '{skill_code}'
              ORDER BY rv.id LIMIT 1",
            wo_type.id
        ),
    )
    .await;
    let check =
        crate::planning::qualifications::check_assignee(&db, "inspection_follow_up", di, person, day("2026-05-04"))
            .await
            .expect("check")
            .expect("a known request");
    assert!(!check.qualified, "the request's work-order type asks another skill");
    assert_eq!(check.gaps.len(), 1);
}

async fn re_evaluate(db: &DatabaseConnection) {
    queries::refresh_schedule_candidates(
        db,
        RefreshScheduleCandidatesInput {
            include_work_orders: Some(false),
            include_pm_occurrences: Some(false),
            include_approved_di: Some(false),
            limit_per_source: Some(200),
        },
    )
    .await
    .expect("refresh");
}

/// The open conflicts of a candidate: (reason code, severity), in order.
async fn open_conflicts(db: &DatabaseConnection, candidate: i64) -> Vec<(String, String)> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "SELECT reason_code, severity FROM scheduling_conflicts
              WHERE candidate_id = {candidate} AND resolved_at IS NULL ORDER BY id"
        ),
    ))
    .await
    .expect("conflicts")
    .iter()
    .map(|r| {
        (
            r.try_get::<String>("", "reason_code").expect("code"),
            r.try_get::<String>("", "severity").expect("severity"),
        )
    })
    .collect()
}

async fn readiness_of(db: &DatabaseConnection, candidate: i64) -> String {
    db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT readiness_status FROM schedule_candidates WHERE id = {candidate}"),
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get("", "readiness_status")
    .expect("status")
}

#[tokio::test]
async fn short_parts_block_while_unreserved_parts_and_permits_only_warn() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (wo_id, actor_id) = create_planning_wo(&db, "Readiness WO").await;
    let candidate = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;

    exec_sql(
        &db,
        "INSERT INTO articles (article_code, article_name, unit_value_id, is_active)
         SELECT 'PLN-BRG', 'Roulement 6205', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id
                JOIN reference_domains d ON d.id = s.domain_id
               WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'U'), 1",
    )
    .await;
    let article = scalar(&db, "SELECT id FROM articles WHERE article_code = 'PLN-BRG'").await;
    exec_sql(
        &db,
        "INSERT INTO warehouses (id, code, name) VALUES (950, 'MAG-PLN', 'Magasin planning')",
    )
    .await;
    exec_sql(
        &db,
        "INSERT INTO stock_locations (id, warehouse_id, code, name) VALUES (960, 950, 'P1', 'Allée P1')",
    )
    .await;
    exec_sql(
        &db,
        &format!(
            "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty)
             VALUES ({article}, 950, 960, 5, 0, 5)"
        ),
    )
    .await;
    // Two bearings planned, none reserved; a free-text seal; a line declared not used.
    exec_sql(
        &db,
        &format!(
            "INSERT INTO work_order_parts (work_order_id, article_id, article_ref, quantity_planned)
             VALUES ({wo_id}, {article}, NULL, 2), ({wo_id}, NULL, 'Joint spécial', 1)"
        ),
    )
    .await;
    exec_sql(
        &db,
        &format!(
            "INSERT INTO work_order_parts (work_order_id, article_id, quantity_planned, consumption_status)
             VALUES ({wo_id}, {article}, 40, 'not_used')"
        ),
    )
    .await;

    re_evaluate(&db).await;
    let conflicts = open_conflicts(&db, candidate).await;
    assert_eq!(
        conflicts,
        vec![
            ("PARTS_NOT_RESERVED".to_string(), "warning".to_string()),
            ("PART_NOT_IN_CATALOGUE".to_string(), "warning".to_string()),
        ],
        "the unused line is not counted"
    );
    assert_eq!(readiness_of(&db, candidate).await, "ready", "warnings never block");

    // Only one bearing left in stock: short, the commitment is refused with the reason.
    exec_sql(
        &db,
        &format!("UPDATE stock_balances SET available_qty = 1 WHERE article_id = {article}"),
    )
    .await;
    re_evaluate(&db).await;
    assert!(open_conflicts(&db, candidate)
        .await
        .contains(&("PARTS_SHORT".to_string(), "blocking".to_string())));
    assert_eq!(readiness_of(&db, candidate).await, "blocked");
    let refused = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate, team_id, "2026-10-05T08:00", "2026-10-05T10:00"),
    )
    .await
    .expect_err("short parts block");
    assert!(
        format!("{refused:?}").contains("PLANNING_CANDIDATE_BLOCKED"),
        "{refused:?}"
    );
    assert!(format!("{refused:?}").contains("PARTS_SHORT"), "{refused:?}");

    // Stock back, but a permit is now required and none exists: planning warns, the commitment
    // goes through, and the OT start gate refuses (the hard enforcement).
    exec_sql(
        &db,
        &format!("UPDATE stock_balances SET available_qty = 5 WHERE article_id = {article}"),
    )
    .await;
    exec_sql(
        &db,
        &format!("UPDATE work_orders SET requires_permit = 1 WHERE id = {wo_id}"),
    )
    .await;
    re_evaluate(&db).await;
    assert!(open_conflicts(&db, candidate)
        .await
        .contains(&("PERMIT_TO_REQUEST".to_string(), "warning".to_string())));
    scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate, team_id, "2026-10-05T08:00", "2026-10-05T10:00"),
    )
    .await
    .expect("a permit to request warns only");
    let start = crate::permit::wo_gate::assert_in_progress_permit_gate(&db, wo_id).await;
    assert!(format!("{start:?}").contains("PERMIT_REQUIRED_TO_START"), "{start:?}");
}

#[test]
fn only_warnings_let_a_commitment_through_and_unknown_severities_block() {
    assert!(!queries::is_blocking_severity("warning"));
    assert!(queries::is_blocking_severity("blocking"));
    // Rows written before the split, and anything unknown, fail closed.
    assert!(queries::is_blocking_severity("high"));
    assert!(queries::is_blocking_severity("medium"));
    assert!(queries::is_blocking_severity(""));
}

fn break_in_input(
    commitment: &crate::planning::domain::ScheduleCommitment,
    start: &str,
    end: &str,
    dangerous_override_reason: Option<&str>,
) -> CreateScheduleBreakInInput {
    CreateScheduleBreakInInput {
        schedule_commitment_id: commitment.id,
        expected_commitment_row_version: commitment.row_version,
        break_in_reason: "other".to_string(),
        approved_by_user_id: None,
        new_slot_start: local(start),
        new_slot_end: local(end),
        new_assigned_team_id: None,
        new_assigned_personnel_id: None,
        clear_assignee: None,
        bypass_availability: Some(false),
        override_reason: None,
        acknowledged_assignee_issues: Vec::new(),
        dangerous_override_reason: dangerous_override_reason.map(str::to_string),
    }
}

#[tokio::test]
async fn a_break_in_runs_the_blocking_checks_unless_forced_with_a_recorded_reason() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate = insert_candidate(&db, "manual_test", 7701, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T09:00"),
    )
    .await
    .expect("committed");

    // After the commitment the job turns out to need a shutdown, and no shutdown window exists.
    exec_sql(
        &db,
        &format!("UPDATE schedule_candidates SET shutdown_requirement = 'required' WHERE id = {candidate}"),
    )
    .await;
    let refused = scheduling::create_schedule_break_in(
        &db,
        1,
        break_in_input(&commitment, "2026-05-04T10:00", "2026-05-04T11:00", None),
    )
    .await
    .expect_err("blocking conflict on a break-in");
    assert!(format!("{refused:?}").contains("NO_FEASIBLE_WINDOW"), "{refused:?}");

    // Forced with the dangerous-override reason: moved, recorded, and the bar stays critical.
    scheduling::create_schedule_break_in(
        &db,
        1,
        break_in_input(
            &commitment,
            "2026-05-04T10:00",
            "2026-05-04T11:00",
            Some("Line stopped by the breakdown itself"),
        ),
    )
    .await
    .expect("forced break-in");
    let moved = scheduling::list_schedule_commitments(
        &db,
        ScheduleCommitmentFilter {
            period_start: None,
            period_end: None,
            team_id: Some(team_id),
            personnel_id: None,
        },
    )
    .await
    .expect("list")
    .into_iter()
    .find(|c| c.id == commitment.id)
    .expect("commitment");
    assert_eq!(moved.committed_start, local("2026-05-04T10:00"));
    assert_eq!(moved.has_blocking_conflict, 1);
    let recorded = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) FROM schedule_change_log WHERE commitment_id = {}
               AND action_type = 'break_in_override'
               AND json_extract(details_json, '$.forced_blocking') = 1
               AND json_extract(details_json, '$.dangerous_override_reason') = 'Line stopped by the breakdown itself'",
            commitment.id
        ),
    )
    .await;
    assert_eq!(recorded, 1, "the forced override is recorded with its reason");
}

#[tokio::test]
async fn a_job_without_a_site_warns_that_site_windows_are_not_checked() {
    let db = setup_db().await;
    // A PM plan scoped to an equipment family: its occurrence has no single site.
    exec_sql(
        &db,
        "INSERT INTO pm_plans (entity_sync_id, code, title, asset_scope_type, asset_scope_id, strategy_type,
            lifecycle_status, requires_shutdown, requires_permit)
         VALUES ('pln-family-plan', 'PM-FAMILY', 'Contrôle pompes', 'family', NULL, 'time_based', 'active', 0, 0)",
    )
    .await;
    let plan = scalar(&db, "SELECT id FROM pm_plans WHERE code = 'PM-FAMILY'").await;
    exec_sql(
        &db,
        &format!(
            "INSERT INTO pm_plan_versions (entity_sync_id, pm_plan_id, version_no, status, effective_from,
                trigger_definition_json)
             VALUES ('pln-family-version', {plan}, 1, 'published', '2026-01-01', '{{}}')"
        ),
    )
    .await;
    let version = scalar(
        &db,
        "SELECT id FROM pm_plan_versions WHERE entity_sync_id = 'pln-family-version'",
    )
    .await;
    exec_sql(
        &db,
        &format!(
            "INSERT INTO pm_occurrences (entity_sync_id, pm_plan_id, plan_version_id, due_basis, due_at, status)
             VALUES ('pln-family-occ', {plan}, {version}, 'test', '2026-05-04T00:00:00Z', 'planned')"
        ),
    )
    .await;
    let occurrence = scalar(
        &db,
        "SELECT id FROM pm_occurrences WHERE entity_sync_id = 'pln-family-occ'",
    )
    .await;
    let candidate = insert_candidate(&db, "pm_occurrence", occurrence, "ready", 100.0).await;

    re_evaluate(&db).await;
    assert!(
        open_conflicts(&db, candidate)
            .await
            .contains(&("LOCATION_UNKNOWN".to_string(), "warning".to_string())),
        "the planner is told the site windows were not checked"
    );
    assert_eq!(readiness_of(&db, candidate).await, "ready", "a warning, not a block");
}

#[tokio::test]
async fn an_expired_emergency_grant_gives_no_planning_approval_right() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let approver_person = ensure_personnel_with_rate(&db, "PLN-APPROVER-EMERG").await;
    let approver = 57_i64;
    ensure_approver_user(&db, approver, approver_person).await;
    let candidate = insert_candidate(&db, "manual_test", 7801, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-05-04T08:00", "2026-05-04T09:00"),
    )
    .await
    .expect("committed");
    let emergency_break_in = |commitment: &crate::planning::domain::ScheduleCommitment| {
        let mut input = break_in_input(commitment, "2026-05-04T10:00", "2026-05-04T11:00", None);
        input.break_in_reason = "emergency".to_string();
        input.approved_by_user_id = Some(approver);
        input
    };

    // The approver's plan.confirm comes from an emergency grant that has expired.
    exec_sql(
        &db,
        &format!(
            "UPDATE user_scope_assignments SET is_emergency = 1, emergency_reason = 'night callout',
                    emergency_expires_at = '2000-01-01T00:00:00Z' WHERE user_id = {approver}"
        ),
    )
    .await;
    let expired = scheduling::create_schedule_break_in(&db, 1, emergency_break_in(&commitment))
        .await
        .expect_err("expired emergency grant");
    assert!(
        format!("{expired:?}").contains(crate::rbac::permissions::PLAN_CONFIRM),
        "{expired:?}"
    );

    // A grant that has not started yet gives nothing either.
    exec_sql(
        &db,
        &format!(
            "UPDATE user_scope_assignments SET is_emergency = 0, emergency_expires_at = NULL,
                    valid_from = '2999-01-01T00:00:00Z' WHERE user_id = {approver}"
        ),
    )
    .await;
    scheduling::create_schedule_break_in(&db, 1, emergency_break_in(&commitment))
        .await
        .expect_err("grant not started yet");

    // An emergency grant still in force counts.
    exec_sql(
        &db,
        &format!(
            "UPDATE user_scope_assignments SET valid_from = '2026-01-01T00:00:00Z', is_emergency = 1,
                    emergency_expires_at = '2999-01-01T00:00:00Z' WHERE user_id = {approver}"
        ),
    )
    .await;
    scheduling::create_schedule_break_in(&db, 1, emergency_break_in(&commitment))
        .await
        .expect("emergency grant in force");
}

// ── Step D: weekly schedule per site, freeze / unfreeze, break-ins, commitment lifecycle ─────

/// A planning site (a node whose type can own work) and a crew attached to it, with roster
/// capacity. The site's week follows the seeded all-sites weekend (Friday-Saturday): Sunday to
/// Saturday.
async fn weekly_site(db: &DatabaseConnection, code: &str) -> (i64, i64) {
    let _ = get_any_team_id(db).await;
    exec_sql(
        db,
        &format!(
            "INSERT INTO org_node_types (entity_sync_id, structure_model_id, code, label, can_host_assets, can_own_work,
                can_carry_cost_center, can_aggregate_kpis, can_receive_permits, is_root_type, is_active, created_at, updated_at)
             SELECT 'wk-type-{code}', structure_model_id, 'WKSITE{code}', 'Site', 1, 1, 0, 1, 0, 0, 1,
                    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z' FROM org_nodes ORDER BY id LIMIT 1"
        ),
    )
    .await;
    exec_sql(
        db,
        &format!(
            "INSERT INTO org_nodes (entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
                created_at, updated_at, row_version, structure_model_id)
             SELECT 'wk-node-{code}', 'WK-{code}', 'Site {code}', t.id, NULL, '/', 0, 'active',
                    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, t.structure_model_id
               FROM org_node_types t WHERE t.entity_sync_id = 'wk-type-{code}'"
        ),
    )
    .await;
    let site = scalar(
        db,
        &format!("SELECT id FROM org_nodes WHERE entity_sync_id = 'wk-node-{code}'"),
    )
    .await;
    exec_sql(
        db,
        &format!(
            "INSERT INTO teams (entity_sync_id, code, name, team_type, primary_node_id, status, created_at, updated_at)
             VALUES ('wk-team-{code}', 'CREW-{code}', 'Equipe {code}', 'maintenance', {site}, 'active',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let team = scalar(
        db,
        &format!("SELECT id FROM teams WHERE entity_sync_id = 'wk-team-{code}'"),
    )
    .await;
    give_team_roster_capacity(db, team).await;
    (site, team)
}

fn week_ref(site: i64, day: &str, rv: Option<i64>) -> crate::planning::weekly_schedule::WeeklyScheduleRef {
    crate::planning::weekly_schedule::WeeklyScheduleRef {
        site_id: site,
        day: day.to_string(),
        expected_row_version: rv,
    }
}

fn utc_at(local_at: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(&local(local_at))
        .expect("instant")
        .with_timezone(&chrono::Utc)
}

async fn commitment_status(db: &DatabaseConnection, id: i64) -> String {
    db.query_one(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT status FROM schedule_commitments WHERE id = {id}"),
    ))
    .await
    .expect("query")
    .expect("row")
    .try_get("", "status")
    .expect("status")
}

#[tokio::test]
async fn a_site_week_runs_from_the_day_after_its_weekend_and_needs_a_weekend() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "A").await;
    let view = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-06")
        .await
        .expect("week");
    assert_eq!(
        view.week.week_start,
        day("2026-05-03"),
        "Sunday after a Friday-Saturday weekend"
    );
    assert_eq!(view.week.week_end, day("2026-05-09"));
    assert_eq!(view.week.last_working_day, Some(day("2026-05-07")), "Thursday");
    assert!(view.schedule.is_none(), "nothing done yet: a draft");
    assert!(
        view.crews.iter().any(|c| c.team_id == team),
        "the site's crews are its rows"
    );

    // A holiday on the Thursday moves the last working day, not the week.
    exec_sql(
        &db,
        "INSERT INTO work_calendar_holidays (entity_sync_id, holiday_date, label, entity_id, is_active, created_at, updated_at)
         VALUES ('wk-holiday', '2026-05-07', 'Fête', NULL, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .await;
    let view = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-06")
        .await
        .expect("week");
    assert_eq!(view.week.week_start, day("2026-05-03"));
    assert_eq!(view.week.last_working_day, Some(day("2026-05-06")));

    // A site whose calendar has no weekend has no planning week: a coded refusal.
    exec_sql(
        &db,
        &format!(
            "INSERT INTO site_work_weeks (entity_sync_id, entity_id, effective_from, weekend_days, created_at, updated_at)
             VALUES ('wk-noweekend', {site}, '2026-01-01', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let refused = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-06")
        .await
        .expect_err("no weekend");
    assert!(
        format!("{refused:?}").contains("PLANNING_WEEK_NO_WEEKEND"),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_frozen_week_changes_only_through_break_ins_and_a_four_eyes_unfreeze() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "B").await;
    let candidate = insert_candidate(&db, "manual_test", 7901, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team, "2026-05-04T08:00", "2026-05-04T09:00"),
    )
    .await
    .expect("committed");

    let approved =
        crate::planning::weekly_schedule::approve_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
            .await
            .expect("approved");
    let schedule = approved.schedule.expect("schedule");
    assert_eq!(schedule.status, "approved");
    let frozen = crate::planning::weekly_schedule::freeze_weekly_schedule(
        &db,
        1,
        week_ref(site, "2026-05-04", Some(schedule.row_version)),
    )
    .await
    .expect("frozen");
    let schedule = frozen.schedule.expect("schedule");
    assert_eq!(schedule.status, "frozen");
    assert_eq!(schedule.baseline_no, 1);
    assert_eq!(schedule.frozen_without_approval, 0);
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM weekly_schedule_baseline_items WHERE weekly_schedule_id = {}",
                schedule.id
            )
        )
        .await,
        1,
        "the baseline holds the week's commitment"
    );

    // Releasing a frozen commitment is refused; new work needs a break-in reason.
    let frozen_rv = scalar(
        &db,
        &format!(
            "SELECT row_version FROM schedule_commitments WHERE id = {}",
            commitment.id
        ),
    )
    .await;
    let release = crate::planning::lifecycle::end_commitment(
        &db,
        1,
        crate::planning::lifecycle::EndCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: frozen_rv,
            reason: "Parts late".to_string(),
        },
        crate::planning::lifecycle::STATUS_RELEASED,
    )
    .await
    .expect_err("frozen");
    assert!(
        format!("{release:?}").contains("PLANNING_COMMITMENT_FROZEN"),
        "{release:?}"
    );

    let urgent = insert_candidate(&db, "manual_test", 7902, "ready", 100.0).await;
    let refused = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(urgent, team, "2026-05-05T08:00", "2026-05-05T09:00"),
    )
    .await
    .expect_err("frozen week");
    assert!(format!("{refused:?}").contains("PLANNING_WEEK_FROZEN"), "{refused:?}");
    // Moving a job of another week into the frozen week is refused the same way.
    let later = insert_candidate(&db, "manual_test", 7903, "ready", 100.0).await;
    let later = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(later, team, "2026-05-12T08:00", "2026-05-12T09:00"),
    )
    .await
    .expect("next week is open");
    let moved = scheduling::reschedule_schedule_commitment(
        &db,
        1,
        RescheduleCommitmentInput {
            commitment_id: later.id,
            expected_row_version: later.row_version,
            committed_start: local("2026-05-06T08:00"),
            committed_end: local("2026-05-06T09:00"),
            assigned_team_id: team,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            acknowledged_assignee_issues: Vec::new(),
            budget_threshold: None,
        },
    )
    .await
    .expect_err("frozen target week");
    assert!(format!("{moved:?}").contains("PLANNING_WEEK_FROZEN"), "{moved:?}");
    let mut emergency = commit_input(urgent, team, "2026-05-05T08:00", "2026-05-05T09:00");
    emergency.frozen_week_break_in = Some(crate::planning::domain::CommitBreakIn {
        reason: "emergency".to_string(),
        approved_by_user_id: None,
        dangerous_override_reason: None,
    });
    let refused = scheduling::create_schedule_commitment(&db, 1, emergency.clone())
        .await
        .expect_err("urgent needs evidence");
    assert!(
        format!("{refused:?}").contains("PLANNING_BREAK_IN_APPROVAL_REQUIRED"),
        "{refused:?}"
    );
    emergency.frozen_week_break_in = Some(crate::planning::domain::CommitBreakIn {
        reason: "emergency".to_string(),
        approved_by_user_id: None,
        dangerous_override_reason: Some("Pump seized on line 2".to_string()),
    });
    let inserted = scheduling::create_schedule_commitment(&db, 1, emergency)
        .await
        .expect("emergency break-in");
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_break_ins WHERE schedule_commitment_id = {} AND break_in_reason = 'emergency'",
                inserted.id
            )
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_commitments WHERE id = {} AND frozen_at IS NOT NULL",
                inserted.id
            )
        )
        .await,
        1,
        "the inserted job is part of the frozen week"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_break_ins WHERE schedule_commitment_id = {} AND kind = 'insert' \
                   AND old_slot_start IS NULL AND old_slot_end IS NULL AND approved_by_user_id IS NULL \
                   AND override_reason = 'Pump seized on line 2'",
                inserted.id
            )
        )
        .await,
        1,
        "an inserted job has no old slot and no approver it did not have"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM notification_events WHERE event_code = 'planning.break_in.created' \
                   AND source_record_id = '{}'",
                inserted.id
            )
        )
        .await,
        1,
        "the break-in is announced to the crew"
    );

    // Unfreeze: requested with a reason, decided by someone else.
    let other_person = ensure_personnel_with_rate(&db, "PLN-WK-APPROVER").await;
    ensure_approver_user(&db, 58, other_person).await;
    let view = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-04")
        .await
        .expect("view");
    let rv = view.schedule.expect("schedule").row_version;
    let requested = crate::planning::weekly_schedule::request_unfreeze(
        &db,
        1,
        crate::planning::weekly_schedule::UnfreezeRequestInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: Some(rv),
            reason: "Line stop moved by production".to_string(),
        },
    )
    .await
    .expect("requested");
    let rv = requested.schedule.expect("schedule").row_version;
    let decision = |actor: i64, rv: i64| {
        crate::planning::weekly_schedule::decide_unfreeze(
            &db,
            actor,
            crate::planning::weekly_schedule::UnfreezeDecisionInput {
                site_id: site,
                day: "2026-05-04".to_string(),
                expected_row_version: Some(rv),
                approve: true,
                comment: None,
            },
        )
    };
    let same = decision(1, rv).await.expect_err("same person");
    assert!(
        format!("{same:?}").contains("PLANNING_UNFREEZE_SAME_PERSON"),
        "{same:?}"
    );
    let unfrozen = decision(58, rv).await.expect("unfrozen by another planner");
    let schedule = unfrozen.schedule.expect("schedule");
    assert_eq!(schedule.status, "approved");
    assert!(unfrozen
        .events
        .iter()
        .any(|e| e.action == "unfrozen" && e.approver_id == Some(58)));
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_commitments WHERE id = {} AND frozen_at IS NULL",
                commitment.id
            )
        )
        .await,
        1,
        "the baseline's commitments are no longer frozen"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM schedule_commitments WHERE id = {} AND frozen_at IS NULL",
                inserted.id
            )
        )
        .await,
        1,
        "a job that entered as a break-in is unfrozen with the week"
    );
    // The automatic freeze never undoes an approved unfreeze: the week stays open.
    let ticked = crate::planning::weekly_schedule::auto_freeze_due(&db, utc_at("2026-05-08T00:30"))
        .await
        .expect("tick");
    assert!(!ticked.contains(&(site, day("2026-05-03"))), "{ticked:?}");
    let still_open = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-04")
        .await
        .expect("view")
        .schedule
        .expect("schedule");
    assert_eq!(still_open.status, "approved");

    // Frozen again (new baseline), then published.
    let refrozen = crate::planning::weekly_schedule::freeze_weekly_schedule(
        &db,
        1,
        week_ref(site, "2026-05-04", Some(schedule.row_version)),
    )
    .await
    .expect("frozen again");
    let schedule = refrozen.schedule.expect("schedule");
    assert_eq!(schedule.baseline_no, 2);
    let published = crate::planning::weekly_schedule::publish_weekly_schedule(
        &db,
        1,
        week_ref(site, "2026-05-04", Some(schedule.row_version)),
    )
    .await
    .expect("published");
    assert_eq!(published.schedule.expect("schedule").status, "published");
}

#[tokio::test]
async fn the_next_week_freezes_itself_but_never_counts_as_approved() {
    let db = setup_db().await;
    ensure_planning_notification_rule(&db).await;
    let (site, team) = weekly_site(&db, "C").await;
    let candidate = insert_candidate(&db, "manual_test", 7911, "ready", 100.0).await;
    scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team, "2026-05-11T08:00", "2026-05-11T09:00"),
    )
    .await
    .expect("next week's work");

    // Thursday 2026-05-07 is the last working day of the week of 2026-05-03. Before its end, only
    // the current week is caught up (its own freeze fell due last week, no desktop ran then);
    // after, the week of 2026-05-10 freezes without approval.
    let frozen = crate::planning::weekly_schedule::auto_freeze_due(&db, utc_at("2026-05-07T20:00"))
        .await
        .expect("tick");
    assert!(frozen.contains(&(site, day("2026-05-03"))), "{frozen:?}");
    assert!(!frozen.contains(&(site, day("2026-05-10"))), "{frozen:?}");
    let frozen = crate::planning::weekly_schedule::auto_freeze_due(&db, utc_at("2026-05-08T00:30"))
        .await
        .expect("tick");
    assert!(frozen.contains(&(site, day("2026-05-10"))), "{frozen:?}");
    let view = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-11")
        .await
        .expect("view");
    let schedule = view.schedule.clone().expect("schedule");
    assert_eq!(schedule.status, "frozen");
    assert_eq!(schedule.frozen_without_approval, 1);
    assert!(schedule.frozen_by_id.is_none(), "frozen by the system");
    crate::planning::weekly_schedule::remind_unapproved_weeks(&db, utc_at("2026-05-08T00:31"))
        .await
        .expect("remind");
    let notified = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) FROM notification_events WHERE category_code = 'planning.week_unapproved' \
               AND source_record_id = '{}'",
            schedule.id
        ),
    )
    .await;
    assert_eq!(notified, 1, "planners are told the week froze without approval");
    assert!(view.awaiting_approval);
    assert!(view
        .events
        .iter()
        .any(|e| e.action == "auto_frozen" && e.actor_id.is_none()));
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM weekly_schedule_baseline_items WHERE weekly_schedule_id = {}",
                schedule.id
            )
        )
        .await,
        1,
        "the baseline is taken anyway"
    );

    // Approved after the fact: the mark clears, the baseline does not move.
    let approved = crate::planning::weekly_schedule::approve_weekly_schedule(
        &db,
        1,
        week_ref(site, "2026-05-11", Some(schedule.row_version)),
    )
    .await
    .expect("approved after the fact");
    let after_fact = approved.schedule.expect("schedule");
    assert_eq!(after_fact.status, "frozen");
    assert_eq!(after_fact.baseline_no, 1);
    assert_eq!(after_fact.frozen_without_approval, 1, "the history stays");
    assert!(!approved.awaiting_approval);
    assert!(approved.events.iter().any(|e| e.action == "approved_after_freeze"));

    // A week approved in time freezes as approved.
    crate::planning::weekly_schedule::approve_weekly_schedule(&db, 1, week_ref(site, "2026-05-18", None))
        .await
        .expect("approved ahead");
    crate::planning::weekly_schedule::auto_freeze_due(&db, utc_at("2026-05-15T01:00"))
        .await
        .expect("tick");
    let view = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-18")
        .await
        .expect("view");
    let schedule = view.schedule.expect("schedule");
    assert_eq!(schedule.status, "frozen");
    assert_eq!(schedule.frozen_without_approval, 0);
    assert!(!view.awaiting_approval);
}

#[tokio::test]
async fn a_released_slot_returns_the_job_to_the_backlog_and_finished_work_closes_its_commitment() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (wo_id, actor_id) = create_planning_wo(&db, "Lifecycle WO").await;
    let candidate = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate, team_id, "2026-10-05T08:00", "2026-10-05T10:00"),
    )
    .await
    .expect("committed");
    let in_backlog = |db: DatabaseConnection| async move {
        queries::list_schedule_candidates(
            &db,
            ScheduleCandidateFilter {
                source_type: Some("work_order".to_string()),
                readiness_status: None,
                assigned_personnel_id: None,
                include_resolved_conflicts: Some(true),
                limit: Some(50),
            },
        )
        .await
        .expect("backlog")
        .iter()
        .any(|c| c.id == candidate)
    };
    assert!(
        !in_backlog(db.clone()).await,
        "a job holding a slot is not in the backlog"
    );
    let again = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate, team_id, "2026-10-06T08:00", "2026-10-06T10:00"),
    )
    .await
    .expect_err("one slot at a time");
    assert!(format!("{again:?}").contains("PLANNING_ALREADY_COMMITTED"), "{again:?}");

    let rv = scalar(
        &db,
        &format!(
            "SELECT row_version FROM schedule_commitments WHERE id = {}",
            commitment.id
        ),
    )
    .await;
    let missing_reason = crate::planning::lifecycle::end_commitment(
        &db,
        actor_id,
        crate::planning::lifecycle::EndCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: rv,
            reason: "  ".to_string(),
        },
        crate::planning::lifecycle::STATUS_RELEASED,
    )
    .await
    .expect_err("reason");
    assert!(format!("{missing_reason:?}").contains("PLANNING_COMMITMENT_REASON_REQUIRED"));
    crate::planning::lifecycle::end_commitment(
        &db,
        actor_id,
        crate::planning::lifecycle::EndCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: rv,
            reason: "Spare part delayed".to_string(),
        },
        crate::planning::lifecycle::STATUS_RELEASED,
    )
    .await
    .expect("released");
    assert_eq!(commitment_status(&db, commitment.id).await, "released");
    let (start, end, _, _) = load_wo_planning_fields(&db, wo_id).await;
    assert_eq!((start, end), (None, None), "the work order's slot is given back");
    assert!(in_backlog(db.clone()).await, "the job is back in the backlog");

    // Committed again, then the work order is cancelled: its commitment closes with it.
    let second = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate, team_id, "2026-10-06T08:00", "2026-10-06T10:00"),
    )
    .await
    .expect("committed again");
    let wo_rv = scalar(&db, &format!("SELECT row_version FROM work_orders WHERE id = {wo_id}")).await;
    crate::wo::queries::cancel_work_order(
        &db,
        crate::wo::domain::WoCancelInput {
            id: wo_id,
            expected_row_version: wo_rv,
            actor_id,
            cancel_reason: "Equipment replaced".to_string(),
        },
    )
    .await
    .expect("cancelled");
    assert_eq!(commitment_status(&db, second.id).await, "closed");
    let logged = scalar(
        &db,
        &format!(
            "SELECT COUNT(*) FROM schedule_change_log WHERE commitment_id = {}                AND action_type = 'close_commitment' AND reason_code = 'work_finished'",
            second.id
        ),
    )
    .await;
    assert_eq!(logged, 1, "the job's history says why its slot ended");

    // The sweep closes commitments of work finished elsewhere (a work order completed outside
    // this path, here by its date).
    let (other_wo, _) = create_planning_wo(&db, "Lifecycle WO 2").await;
    let other_candidate = insert_candidate(&db, "work_order", other_wo, "ready", 100.0).await;
    let third = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(other_candidate, team_id, "2026-10-07T08:00", "2026-10-07T10:00"),
    )
    .await
    .expect("committed");
    exec_sql(
        &db,
        &format!("UPDATE work_orders SET mechanically_completed_at = '2026-10-07T10:00:00Z' WHERE id = {other_wo}"),
    )
    .await;
    let closed = crate::planning::lifecycle::close_finished_commitments(&db)
        .await
        .expect("sweep");
    assert!(closed >= 1);
    assert_eq!(commitment_status(&db, third.id).await, "closed");
}

#[tokio::test]
async fn break_in_reasons_are_the_plants_own_list() {
    let db = setup_db().await;
    let reason = crate::planning::break_in_reasons::resolve_break_in_reason(&db, " Emergency ")
        .await
        .expect("starter reason");
    assert_eq!(reason.code, "emergency");
    assert!(reason.urgent);
    // A reason the plant adds is accepted; a deactivated one is refused.
    exec_sql(
        &db,
        "INSERT INTO reference_values (entity_sync_id, set_id, code, label, sort_order, semantic_tag, is_active)
         SELECT 'plant-outage-reason', rs.id, 'customer_outage', 'Arrêt client', 9, 'planning_break_in', 1
           FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id
          WHERE d.code = 'PLANNING.BREAK_IN_REASONS' AND rs.status = 'published'",
    )
    .await;
    let added = crate::planning::break_in_reasons::resolve_break_in_reason(&db, "customer_outage")
        .await
        .expect("plant reason");
    assert!(!added.urgent);
    exec_sql(
        &db,
        "UPDATE reference_values SET is_active = 0 WHERE code = 'customer_outage'",
    )
    .await;
    let refused = crate::planning::break_in_reasons::resolve_break_in_reason(&db, "customer_outage")
        .await
        .expect_err("inactive");
    assert!(format!("{refused:?}").contains("PLANNING_BREAK_IN_REASON_UNKNOWN"));
}

// ── Planning step D review (#34) ──────────────────────────────────────────────────────────

#[tokio::test]
async fn a_week_frozen_without_approval_is_recalled_to_planners_until_approved() {
    let db = setup_db().await;
    let (site, _team) = weekly_site(&db, "N").await;
    let planner = ensure_personnel_with_rate(&db, "PLN-WK-NOTIFY").await;
    ensure_approver_user(&db, 58, planner).await;
    let frozen = crate::planning::weekly_schedule::auto_freeze_due(&db, utc_at("2026-05-08T00:30"))
        .await
        .expect("tick");
    assert!(frozen.contains(&(site, day("2026-05-10"))), "{frozen:?}");
    let schedule = crate::planning::weekly_schedule::get_weekly_schedule(&db, site, "2026-05-11")
        .await
        .expect("view")
        .schedule
        .expect("schedule");

    crate::planning::weekly_schedule::remind_unapproved_weeks(&db, utc_at("2026-05-08T00:31"))
        .await
        .expect("remind");
    let event = scalar(
        &db,
        &format!(
            "SELECT id FROM notification_events WHERE category_code = 'planning.week_unapproved' \
               AND source_record_id = '{}' AND resolved_at IS NULL",
            schedule.id
        ),
    )
    .await;
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM notification_events WHERE id = {event} \
                   AND json_extract(payload_json, '$.org_node_id') = {site}"
            )
        )
        .await,
        1,
        "routed by the site"
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM notifications WHERE notification_event_id = {event} AND recipient_user_id = 58"
            )
        )
        .await,
        1,
        "a holder of the planning confirm right is told"
    );

    // Read a day ago and still not approved: recalled (unread again).
    exec_sql(
        &db,
        &format!(
            "UPDATE notifications SET delivery_state = 'read', read_at = '2000-01-01T00:00:00Z' \
              WHERE notification_event_id = {event}"
        ),
    )
    .await;
    crate::planning::weekly_schedule::remind_unapproved_weeks(&db, utc_at("2026-05-08T00:40"))
        .await
        .expect("remind");
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM notifications WHERE notification_event_id = {event} AND delivery_state = 'read'"
            )
        )
        .await,
        0,
        "recalled"
    );
    // Read just now: not recalled within the rule's window.
    let read_at = utc_at("2026-05-08T00:41").format("%Y-%m-%dT%H:%M:%SZ").to_string();
    exec_sql(
        &db,
        &format!(
            "UPDATE notifications SET delivery_state = 'read', read_at = '{read_at}' WHERE notification_event_id = {event}"
        ),
    )
    .await;
    crate::planning::weekly_schedule::remind_unapproved_weeks(&db, utc_at("2026-05-08T00:42"))
        .await
        .expect("remind");
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) FROM notifications WHERE notification_event_id = {event} AND delivery_state <> 'read'"
            )
        )
        .await,
        0,
        "not recalled before the window"
    );

    // Approved after the fact: the notification is resolved, never recalled again.
    crate::planning::weekly_schedule::approve_weekly_schedule(
        &db,
        1,
        week_ref(site, "2026-05-11", Some(schedule.row_version)),
    )
    .await
    .expect("approved after the fact");
    crate::planning::weekly_schedule::remind_unapproved_weeks(&db, utc_at("2026-05-09T09:00"))
        .await
        .expect("remind");
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT COUNT(*) FROM notification_events WHERE id = {event} AND resolved_at IS NOT NULL")
        )
        .await,
        1,
        "resolved once approved"
    );
}

#[tokio::test]
async fn two_devices_freezing_the_same_week_offline_make_the_same_rows() {
    use crate::sync::test_support::{started_device_db, TestServer};
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    let (_site, team) = weekly_site(&a, "S").await;
    // A real job (a work order), so that it synchronises like any planned work.
    let (wo_id, actor_id) = create_planning_wo(&a, "Two-device WO").await;
    let candidate = insert_candidate(&a, "work_order", wo_id, "ready", 100.0).await;
    scheduling::create_schedule_commitment(
        &a,
        actor_id,
        commit_input(candidate, team, "2026-05-11T08:00", "2026-05-11T09:00"),
    )
    .await
    .expect("committed on A");
    server.exchange("A", &a).await;
    let received = server.exchange("B", &b).await;
    assert!(
        received.materialized.errors.is_empty()
            && received.materialized.deferred == 0
            && received.materialized.conflicts == 0,
        "{received:?} {:?}",
        b.query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT entity_type || ' ' || apply_status || ' ' || COALESCE(rejection_code, '') || ' ' || COALESCE(rejection_message, '') AS v \
               FROM sync_inbox WHERE apply_status <> 'applied'"
                .to_string()
        ))
        .await
        .expect("inbox")
        .iter()
        .map(|r| r.try_get::<String>("", "v").unwrap_or_default())
        .collect::<Vec<_>>()
    );

    // Both offline at the deadline: each freezes the week by itself.
    for db in [&a, &b] {
        let frozen = crate::planning::weekly_schedule::auto_freeze_due(db, utc_at("2026-05-08T00:30"))
            .await
            .expect("tick");
        assert!(frozen.iter().any(|(_, w)| *w == day("2026-05-10")), "{frozen:?}");
    }
    async fn ids(db: &DatabaseConnection) -> Vec<String> {
        let mut out = Vec::new();
        for sql in [
            "SELECT ws.entity_sync_id AS v FROM weekly_schedules ws JOIN org_nodes n ON n.id = ws.entity_id \
              WHERE n.entity_sync_id = 'wk-node-S' AND ws.week_start = '2026-05-10'",
            "SELECT bi.entity_sync_id AS v FROM weekly_schedule_baseline_items bi \
               JOIN weekly_schedules ws ON ws.id = bi.weekly_schedule_id \
               JOIN org_nodes n ON n.id = ws.entity_id WHERE n.entity_sync_id = 'wk-node-S' AND ws.week_start = '2026-05-10'",
            "SELECT e.entity_sync_id AS v FROM weekly_schedule_events e \
               JOIN weekly_schedules ws ON ws.id = e.weekly_schedule_id \
               JOIN org_nodes n ON n.id = ws.entity_id \
              WHERE n.entity_sync_id = 'wk-node-S' AND ws.week_start = '2026-05-10' AND e.action = 'auto_frozen'",
        ] {
            let rows = db
                .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
                .await
                .expect("ids");
            let mut found: Vec<String> = rows.iter().map(|r| r.try_get("", "v").expect("v")).collect();
            assert_eq!(found.len(), 1, "{sql}");
            out.append(&mut found);
        }
        out
    }
    assert_eq!(
        ids(&a).await,
        ids(&b).await,
        "same schedule, baseline item and event ids"
    );

    // Exchanged afterwards: the same rows meet, nothing clashes.
    for (device, db) in [("A", &a), ("B", &b), ("A", &a)] {
        let report = server.exchange(device, db).await;
        assert!(
            report.materialized.errors.is_empty(),
            "{device}: {:?}",
            report.materialized.errors
        );
    }
    for db in [&a, &b] {
        assert_eq!(
            scalar(
                db,
                "SELECT COUNT(*) FROM weekly_schedules ws JOIN org_nodes n ON n.id = ws.entity_id \
                  WHERE n.entity_sync_id = 'wk-node-S' AND ws.week_start = '2026-05-10'"
            )
            .await,
            1
        );
    }
}

/// The week as it travels: its real sync payload (`sync::row_codec`, the site reference under
/// `entity_sync_id`) with the status / baseline a peer would send, applied through the real
/// inbound path (`row_codec::apply_row` via `apply_inbound_payload`). A step back in the
/// lifecycle is a WEEK_LIFECYCLE_BACKWARDS conflict and nothing is written; a step forward applies.
#[tokio::test]
async fn a_received_week_never_goes_back_in_its_lifecycle() {
    use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
    let db = setup_db().await;
    let (site, _team) = weekly_site(&db, "L").await;
    crate::planning::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");
    let week_sync: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM weekly_schedules WHERE entity_id = ?",
            [site.into()],
        ))
        .await
        .expect("q")
        .expect("week row")
        .try_get("", "entity_sync_id")
        .expect("sync id");
    let real = crate::sync::test_support::current_payload(&db, "weekly_schedules", &week_sync, &Default::default())
        .await
        .expect("payload")
        .expect("row");
    assert_eq!(
        real.get("entity_sync_id").and_then(serde_json::Value::as_str),
        Some("wk-node-L"),
        "the site travels as entity_sync_id"
    );
    assert!(real.get("entity_id").is_none(), "never as entity_id");

    let send = |status: &'static str, baseline: i64| {
        let db = db.clone();
        let mut p = real.clone();
        let week_sync = week_sync.clone();
        async move {
            p.insert("status".into(), status.into());
            p.insert("baseline_no".into(), baseline.into());
            let rv = p.get("row_version").and_then(serde_json::Value::as_i64).unwrap_or(1) + 1;
            p.insert("row_version".into(), rv.into());
            apply_inbound_payload(
                &db,
                "weekly_schedules",
                &week_sync,
                "upsert",
                rv,
                &serde_json::Value::Object(p).to_string(),
            )
            .await
            .expect("apply")
        }
    };
    let backwards =
        |o: &ApplyOutcome| matches!(o, ApplyOutcome::Conflict { code, .. } if code == "WEEK_LIFECYCLE_BACKWARDS");
    let status_here = || {
        let db = db.clone();
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT status FROM weekly_schedules WHERE entity_id = ?",
                [site.into()],
            ))
            .await
            .expect("q")
            .expect("row")
            .try_get::<String>("", "status")
            .expect("status")
        }
    };

    assert!(
        backwards(&send("draft", 0).await),
        "frozen here, draft received: refused"
    );
    assert_eq!(status_here().await, "frozen", "nothing written");
    assert!(
        backwards(&send("approved", 0).await),
        "frozen here, approved received: refused"
    );
    let forward = send("published", 1).await;
    assert!(!backwards(&forward), "published after the freeze applies: {forward:?}");
    assert_eq!(status_here().await, "published");

    // Direct check on the same real payload keys (same site and week under another identity).
    let mut other = real.clone();
    other.insert("status".into(), "draft".into());
    other.insert("baseline_no".into(), 0.into());
    assert!(
        crate::planning::weekly_schedule::inbound_lifecycle_conflict(&db, "another-identity", &other)
            .await
            .expect("check")
            .is_some(),
        "found by site and week when the identity differs"
    );
}

#[tokio::test]
async fn unfreezing_a_week_keeps_a_job_running_into_another_frozen_week_frozen() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "M").await;
    let planner = ensure_personnel_with_rate(&db, "PLN-WK-M").await;
    ensure_approver_user(&db, 58, planner).await;
    let first = insert_candidate(&db, "manual_test", 7961, "ready", 100.0).await;
    let inside = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(first, team, "2026-05-05T08:00", "2026-05-05T09:00"),
    )
    .await
    .expect("inside week 1");
    let second = insert_candidate(&db, "manual_test", 7962, "ready", 100.0).await;
    let spanning = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(second, team, "2026-05-09T22:00", "2026-05-10T02:00"),
    )
    .await
    .expect("across both weeks");
    for day_of_week in ["2026-05-04", "2026-05-11"] {
        crate::planning::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, day_of_week, None))
            .await
            .expect("frozen");
    }
    let requested = crate::planning::weekly_schedule::request_unfreeze(
        &db,
        1,
        crate::planning::weekly_schedule::UnfreezeRequestInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: None,
            reason: "Shutdown moved".to_string(),
        },
    )
    .await
    .expect("requested");
    crate::planning::weekly_schedule::decide_unfreeze(
        &db,
        58,
        crate::planning::weekly_schedule::UnfreezeDecisionInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: requested.schedule.map(|s| s.row_version),
            approve: true,
            comment: None,
        },
    )
    .await
    .expect("unfrozen");
    let frozen = |id: i64| {
        let db = db.clone();
        async move {
            scalar(
                &db,
                &format!("SELECT COUNT(*) FROM schedule_commitments WHERE id = {id} AND frozen_at IS NOT NULL"),
            )
            .await
        }
    };
    assert_eq!(frozen(inside.id).await, 0, "week 1 is open again");
    assert_eq!(frozen(spanning.id).await, 1, "still in the frozen week 2");
}

#[test]
fn a_midnight_lost_to_daylight_saving_starts_the_day_at_the_end_of_the_gap() {
    use crate::planning::weekly_schedule::first_instant_of_day as first;
    use chrono::{LocalResult, TimeZone, Timelike};
    let as_utc = |local: chrono::NaiveDateTime| chrono::Utc.from_utc_datetime(&local);
    // Clocks jump from 00:00 to 01:00: the day starts at 01:00.
    let gap = first(day("2026-03-29"), |local: chrono::NaiveDateTime| {
        if local.hour() == 0 {
            LocalResult::None
        } else {
            LocalResult::Single(as_utc(local))
        }
    })
    .expect("instant");
    assert_eq!(gap, chrono::Utc.with_ymd_and_hms(2026, 3, 29, 1, 0, 0).unwrap());
    // Midnight happens twice: the first one.
    let twice = first(day("2026-10-25"), |local: chrono::NaiveDateTime| {
        LocalResult::Ambiguous(as_utc(local), as_utc(local) + chrono::Duration::hours(1))
    })
    .expect("instant");
    assert_eq!(twice, chrono::Utc.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap());
}

#[tokio::test]
async fn a_site_without_a_planning_week_does_not_stop_the_automatic_freeze_of_the_others() {
    let db = setup_db().await;
    let (good, _) = weekly_site(&db, "G").await;
    let (broken, _) = weekly_site(&db, "H").await;
    exec_sql(
        &db,
        &format!(
            "INSERT INTO site_work_weeks (entity_sync_id, entity_id, effective_from, weekend_days, created_at, updated_at)
             VALUES ('wk-broken', {broken}, '2026-01-01', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
    )
    .await;
    let frozen = crate::planning::weekly_schedule::auto_freeze_due(&db, utc_at("2026-05-08T00:30"))
        .await
        .expect("the tick goes on");
    assert!(frozen.contains(&(good, day("2026-05-10"))), "{frozen:?}");
    assert!(!frozen.iter().any(|(site, _)| *site == broken), "{frozen:?}");
}

#[tokio::test]
async fn finished_work_closes_its_slot_at_the_moment_it_ended() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let (wo_id, actor_id) = create_planning_wo(&db, "Finish time WO").await;
    let candidate = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        actor_id,
        commit_input(candidate, team_id, "2026-09-29T08:00", "2026-09-29T10:00"),
    )
    .await
    .expect("committed");
    exec_sql(
        &db,
        &format!("UPDATE work_orders SET mechanically_completed_at = '2026-09-29T09:45:00Z' WHERE id = {wo_id}"),
    )
    .await;
    let closed_at = |db: DatabaseConnection| async move {
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT status_changed_at FROM schedule_commitments WHERE id = {} AND status = 'closed'",
                commitment.id
            ),
        ))
        .await
        .expect("query")
        .map(|r| r.try_get::<String>("", "status_changed_at").expect("time"))
    };

    // The live sweep records when the work ended, not when the sweep ran.
    crate::planning::lifecycle::close_finished_commitments(&db)
        .await
        .expect("sweep");
    assert_eq!(closed_at(db.clone()).await.as_deref(), Some("2026-09-29T09:45:00Z"));

    // The upgrade closes the same way (migration 212), on every device alike.
    exec_sql(
        &db,
        &format!(
            "UPDATE schedule_commitments SET status = 'committed', status_changed_at = NULL WHERE id = {}",
            commitment.id
        ),
    )
    .await;
    exec_sql(
        &db,
        crate::migrations::m20261017_000212_weekly_schedules::CLOSE_FINISHED_SQL,
    )
    .await;
    assert_eq!(closed_at(db.clone()).await.as_deref(), Some("2026-09-29T09:45:00Z"));
}

#[tokio::test]
async fn work_entering_a_week_whose_freeze_is_due_needs_a_break_in_even_before_the_freeze_runs() {
    let db = setup_db().await;
    let (_site, team) = weekly_site(&db, "D").await;
    // The current week froze at the end of the previous week's last working day; the automatic
    // freeze has not run here (it waits for the first synchronisation).
    let today = chrono::Local::now().date_naive();
    let at = |hour: &str| format!("{}T{hour}", today.format("%Y-%m-%d"));
    let candidate = insert_candidate(&db, "manual_test", 7971, "ready", 100.0).await;
    let refused =
        scheduling::create_schedule_commitment(&db, 1, commit_input(candidate, team, &at("10:00"), &at("11:00")))
            .await
            .expect_err("due week");
    assert!(format!("{refused:?}").contains("PLANNING_WEEK_FROZEN"), "{refused:?}");
}

#[tokio::test]
async fn only_a_held_slot_is_moved_or_broken_into() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let candidate = insert_candidate(&db, "manual_test", 7981, "ready", 100.0).await;
    let commitment = scheduling::create_schedule_commitment(
        &db,
        1,
        commit_input(candidate, team_id, "2026-09-22T08:00", "2026-09-22T09:00"),
    )
    .await
    .expect("committed");
    let rv = |db: DatabaseConnection| async move {
        scalar(
            &db,
            &format!(
                "SELECT row_version FROM schedule_commitments WHERE id = {}",
                commitment.id
            ),
        )
        .await
    };
    crate::planning::lifecycle::end_commitment(
        &db,
        1,
        crate::planning::lifecycle::EndCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: rv(db.clone()).await,
            reason: "Not needed".to_string(),
        },
        crate::planning::lifecycle::STATUS_CANCELLED,
    )
    .await
    .expect("cancelled");
    let moved = scheduling::reschedule_schedule_commitment(
        &db,
        1,
        RescheduleCommitmentInput {
            commitment_id: commitment.id,
            expected_row_version: rv(db.clone()).await,
            committed_start: local("2026-09-23T08:00"),
            committed_end: local("2026-09-23T09:00"),
            assigned_team_id: team_id,
            assigned_personnel_id: None,
            allow_double_booking_override: Some(false),
            override_reason: None,
            acknowledged_assignee_issues: Vec::new(),
            budget_threshold: None,
        },
    )
    .await
    .expect_err("cancelled");
    assert!(
        format!("{moved:?}").contains("PLANNING_COMMITMENT_NOT_ACTIVE"),
        "{moved:?}"
    );
}

#[test]
fn the_planning_tick_waits_for_a_synchronisation_when_the_device_synchronises() {
    use crate::planning::scheduler::shared_actions_allowed;
    let started = chrono::Utc::now();
    let mut status = crate::sync::transport::worker::SyncWorkerHandle::new().status();
    assert!(!shared_actions_allowed(&status, started), "starting: wait");
    status.last_success_at = Some("2000-01-01T00:00:00Z".to_string());
    assert!(
        !shared_actions_allowed(&status, started),
        "a success before this start does not count"
    );
    status.last_success_at = Some(
        (started + chrono::Duration::seconds(5))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
    );
    assert!(shared_actions_allowed(&status, started), "synced since the start");
    let mut not_syncing = crate::sync::transport::worker::SyncWorkerHandle::new().status();
    not_syncing.blocker = Some(crate::sync::transport::worker::BLOCKER_NOT_CONFIGURED.to_string());
    assert!(
        shared_actions_allowed(&not_syncing, started),
        "a device that does not synchronise acts at once"
    );
}

// ── Planning KPIs against the frozen baseline (step E) ────────────────────────────────────

#[tokio::test]
async fn planning_kpis_measure_the_frozen_baseline() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "K").await;
    // Two jobs in the week of 2026-05-03 (Sunday to Saturday).
    let (wo_done, actor) = create_planning_wo(&db, "KPI done").await;
    let (wo_missed, _) = create_planning_wo(&db, "KPI missed").await;
    let done = scheduling::create_schedule_commitment(
        &db,
        actor,
        commit_input(
            insert_candidate(&db, "work_order", wo_done, "ready", 100.0).await,
            team,
            "2026-05-04T08:00",
            "2026-05-04T10:00",
        ),
    )
    .await
    .expect("done job");
    let missed = scheduling::create_schedule_commitment(
        &db,
        actor,
        commit_input(
            insert_candidate(&db, "work_order", wo_missed, "ready", 100.0).await,
            team,
            "2026-05-05T08:00",
            "2026-05-05T11:00",
        ),
    )
    .await
    .expect("missed job");
    crate::planning::weekly_schedule::freeze_weekly_schedule(&db, actor, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");

    // The first job's work is finished inside the week; the second never is.
    exec_sql(
        &db,
        &format!("UPDATE work_orders SET mechanically_completed_at = '2026-05-06T12:00:00Z' WHERE id = {wo_done}"),
    )
    .await;
    // An emergency job enters the frozen week (an `insert` break-in) …
    let (wo_urgent, _) = create_planning_wo(&db, "KPI urgent").await;
    let mut urgent = commit_input(
        insert_candidate(&db, "work_order", wo_urgent, "ready", 100.0).await,
        team,
        "2026-05-07T08:00",
        "2026-05-07T09:00",
    );
    urgent.frozen_week_break_in = Some(crate::planning::domain::CommitBreakIn {
        reason: "emergency".to_string(),
        approved_by_user_id: None,
        dangerous_override_reason: Some("Line stopped".to_string()),
    });
    let urgent = scheduling::create_schedule_commitment(&db, actor, urgent)
        .await
        .expect("break-in");
    // … and the missed job is moved inside the week after the freeze (displaced).
    let rv = scalar(
        &db,
        &format!("SELECT row_version FROM schedule_commitments WHERE id = {}", missed.id),
    )
    .await;
    scheduling::create_schedule_break_in(
        &db,
        actor,
        CreateScheduleBreakInInput {
            schedule_commitment_id: missed.id,
            expected_commitment_row_version: rv,
            break_in_reason: "other".to_string(),
            approved_by_user_id: None,
            new_slot_start: local("2026-05-06T08:00"),
            new_slot_end: local("2026-05-06T11:00"),
            new_assigned_team_id: Some(team),
            new_assigned_personnel_id: None,
            clear_assignee: None,
            bypass_availability: Some(false),
            override_reason: None,
            acknowledged_assignee_issues: Vec::new(),
            dangerous_override_reason: None,
        },
    )
    .await
    .expect("moved");

    let hours = |id: i64| {
        let db = db.clone();
        async move {
            db.query_one(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT COALESCE(planned_labor_hours, (julianday(committed_end) - julianday(committed_start)) * 24.0) AS h \
                       FROM schedule_commitments WHERE id = {id}"
                ),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get::<f64>("", "h")
            .expect("h")
        }
    };
    let (h_done, h_missed, h_urgent) = (hours(done.id).await, hours(missed.id).await, hours(urgent.id).await);
    let round = |v: f64| (v * 10.0).round() / 10.0;

    let filter = crate::planning::kpis::PlanningKpiFilter {
        site_id: Some(site),
        from_day: "2026-05-01".to_string(),
        to_day: "2026-05-09".to_string(),
        exclude_unapproved: false,
    };
    // During the week: loading and break-ins, compliance not judged yet.
    let during = crate::planning::kpis::planning_kpis(&db, &filter, utc_at("2026-05-06T15:00"))
        .await
        .expect("kpis");
    assert_eq!(during.weeks.len(), 1);
    assert_eq!(during.weeks[0].compliance_pct, None, "not judged before the week ends");

    let kpis = crate::planning::kpis::planning_kpis(&db, &filter, utc_at("2026-05-20T09:00"))
        .await
        .expect("kpis");
    let week = &kpis.weeks[0];
    assert_eq!(week.week_start, "2026-05-03");
    assert_eq!(
        week.baseline_jobs, 2,
        "the baseline holds the two jobs frozen with the week"
    );
    assert_eq!(week.baseline_hours, round(h_done + h_missed));
    assert_eq!(week.done_hours, Some(round(h_done)));
    assert_eq!(week.compliance_pct, Some(round(h_done / (h_done + h_missed) * 100.0)));
    assert_eq!(week.break_in_jobs, 1);
    assert_eq!(week.break_in_hours, round(h_urgent));
    assert_eq!(week.break_in_pct, Some(round(h_urgent / (h_done + h_missed) * 100.0)));
    assert_eq!(week.displaced_jobs, 1);
    assert!(week.available_hours > 0.0, "{week:?}");
    assert_eq!(
        week.loading_pct,
        Some(round((h_done + h_missed) / week.available_hours * 100.0)),
        "{week:?}"
    );
    assert!(!week.awaiting_approval, "frozen by a planner: approved");
    assert_eq!(kpis.totals.weeks_judged, 1);
    assert_eq!(kpis.totals.compliance_pct, week.compliance_pct);

    // Analytics, the home dashboard and the monthly report read the same number.
    let ratio = crate::analytics::smrp::schedule_compliance(
        &db,
        crate::analytics::smrp::SmrpWindow {
            from: utc_at("2026-05-01T00:00"),
            to: utc_at("2026-05-20T00:00"),
        },
        utc_at("2026-05-20T09:00"),
        None,
    )
    .await
    .expect("smrp");
    assert_eq!(round(ratio.numerator), round(h_done));
    assert_eq!(round(ratio.denominator), round(h_done + h_missed));
}

#[tokio::test]
async fn weeks_frozen_without_approval_are_flagged_and_can_be_left_out() {
    let db = setup_db().await;
    let (site, _team) = weekly_site(&db, "U").await;
    crate::planning::weekly_schedule::auto_freeze_due(&db, utc_at("2026-05-08T00:30"))
        .await
        .expect("tick");
    let filter = |exclude: bool| crate::planning::kpis::PlanningKpiFilter {
        site_id: Some(site),
        from_day: "2026-05-01".to_string(),
        to_day: "2026-05-16".to_string(),
        exclude_unapproved: exclude,
    };
    let all = crate::planning::kpis::planning_kpis(&db, &filter(false), utc_at("2026-05-08T01:00"))
        .await
        .expect("kpis");
    assert!(all.weeks.iter().all(|w| w.awaiting_approval), "{:?}", all.weeks);
    assert_eq!(all.totals.weeks_awaiting_approval, all.totals.weeks);
    assert!(all.totals.weeks >= 1);
    let approved_only = crate::planning::kpis::planning_kpis(&db, &filter(true), utc_at("2026-05-08T01:00"))
        .await
        .expect("kpis");
    assert!(approved_only.weeks.is_empty());
}

/// Hours a commitment weighs in the KPIs (its planned labour, else its slot length), rounded.
async fn kpi_hours(db: &DatabaseConnection, commitment_id: i64) -> f64 {
    let h: f64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(planned_labor_hours, (julianday(committed_end) - julianday(committed_start)) * 24.0) AS h \
                   FROM schedule_commitments WHERE id = {commitment_id}"
            ),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "h")
        .expect("h");
    (h * 10.0).round() / 10.0
}

#[tokio::test]
async fn released_and_cancelled_baseline_jobs_are_not_done_and_a_reopened_week_stays_measured() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "R").await;
    let (other_site, _) = weekly_site(&db, "R2").await;
    // The site sits under a plant node: compliance scoped to the plant includes it.
    exec_sql(
        &db,
        "INSERT INTO org_nodes (entity_sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status,
            created_at, updated_at, row_version, structure_model_id)
         SELECT 'wk-plant-R', 'WK-PLANT-R', 'Plant R', node_type_id, NULL, '/', 0, 'active',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, structure_model_id
           FROM org_nodes WHERE entity_sync_id = 'wk-node-R'",
    )
    .await;
    let plant = scalar(&db, "SELECT id FROM org_nodes WHERE entity_sync_id = 'wk-plant-R'").await;
    exec_sql(
        &db,
        &format!("UPDATE org_nodes SET parent_id = {plant} WHERE id = {site}"),
    )
    .await;

    let planner = ensure_personnel_with_rate(&db, "PLN-WK-R").await;
    ensure_approver_user(&db, 58, planner).await;
    let mut jobs = Vec::new();
    for (title, day) in [
        ("KPI R done", "2026-05-04"),
        ("KPI R released", "2026-05-05"),
        ("KPI R cancelled", "2026-05-06"),
    ] {
        let (wo, actor) = create_planning_wo(&db, title).await;
        let commitment = scheduling::create_schedule_commitment(
            &db,
            actor,
            commit_input(
                insert_candidate(&db, "work_order", wo, "ready", 100.0).await,
                team,
                &format!("{day}T08:00"),
                &format!("{day}T10:00"),
            ),
        )
        .await
        .expect("job");
        jobs.push((wo, commitment.id));
    }
    crate::planning::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");
    // Every work order is finished inside the week, but: the second job's slot was released
    // (scheduled, then given back) and the third work order was cancelled.
    for (wo, _) in &jobs {
        exec_sql(
            &db,
            &format!("UPDATE work_orders SET mechanically_completed_at = '2026-05-06T12:00:00Z' WHERE id = {wo}"),
        )
        .await;
    }
    exec_sql(
        &db,
        &format!(
            "UPDATE schedule_commitments SET status = 'released' WHERE id = {}",
            jobs[1].1
        ),
    )
    .await;
    exec_sql(
        &db,
        &format!(
            "UPDATE work_orders SET cancelled_at = '2026-05-06T13:00:00Z' WHERE id = {}",
            jobs[2].0
        ),
    )
    .await;
    let h_done = kpi_hours(&db, jobs[0].1).await;
    let h_all = h_done + kpi_hours(&db, jobs[1].1).await + kpi_hours(&db, jobs[2].1).await;

    let filter = crate::planning::kpis::PlanningKpiFilter {
        site_id: Some(site),
        from_day: "2026-05-03".to_string(),
        to_day: "2026-05-03".to_string(),
        exclude_unapproved: false,
    };
    let now = utc_at("2026-05-20T09:00");
    let kpis = crate::planning::kpis::planning_kpis(&db, &filter, now)
        .await
        .expect("kpis");
    let week = &kpis.weeks[0];
    assert_eq!(week.baseline_jobs, 3);
    assert_eq!(week.baseline_released_jobs, 1);
    assert_eq!(
        week.done_hours,
        Some(h_done),
        "a released job and a cancelled work order are not done, even with the work finished"
    );
    assert!(!week.reopened);

    // Compliance scoped by organisation: the plant above the site includes it, another site not.
    let window = |org: Option<i64>| {
        let db = db.clone();
        async move {
            crate::planning::kpis::baseline_schedule_compliance(
                &db,
                utc_at("2026-05-01T00:00"),
                utc_at("2026-05-20T00:00"),
                org,
                now,
            )
            .await
            .expect("compliance")
        }
    };
    let under_plant = window(Some(plant)).await;
    assert_eq!(under_plant.weeks, 1);
    assert_eq!((under_plant.done_hours * 10.0).round() / 10.0, h_done);
    assert_eq!(
        (under_plant.baseline_hours * 10.0).round() / 10.0,
        (h_all * 10.0).round() / 10.0
    );
    assert_eq!(under_plant.released_jobs, 1);
    assert_eq!(
        window(Some(other_site)).await.weeks,
        0,
        "another site's scope leaves it out"
    );

    // Unfrozen for editing: still measured on its last baseline, flagged reopened.
    let requested = crate::planning::weekly_schedule::request_unfreeze(
        &db,
        1,
        crate::planning::weekly_schedule::UnfreezeRequestInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: None,
            reason: "Shutdown moved".to_string(),
        },
    )
    .await
    .expect("requested");
    crate::planning::weekly_schedule::decide_unfreeze(
        &db,
        58,
        crate::planning::weekly_schedule::UnfreezeDecisionInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: requested.schedule.map(|s| s.row_version),
            approve: true,
            comment: None,
        },
    )
    .await
    .expect("unfrozen");
    let reopened = crate::planning::kpis::planning_kpis(&db, &filter, now)
        .await
        .expect("kpis");
    assert_eq!(reopened.weeks.len(), 1, "a reopened week does not drop out");
    assert!(reopened.weeks[0].reopened);
    assert_eq!(reopened.weeks[0].done_hours, Some(h_done));
    assert_eq!(reopened.totals.weeks_reopened, 1);
    assert_eq!(
        reopened.totals.weeks_not_frozen, 0,
        "it has a baseline: not counted as never frozen"
    );
    assert_eq!(window(Some(plant)).await.weeks, 1, "analytics keeps reading it");
}

#[tokio::test]
async fn a_week_never_frozen_counts_once_its_freeze_falls_due_and_current_weeks_read_the_frozen_one() {
    let db = setup_db().await;
    let (site, _team) = weekly_site(&db, "NF").await;
    let week = super::weekly::site_week(&db, site, day("2026-05-04"))
        .await
        .expect("week");
    let due = crate::planning::weekly_schedule::freeze_due_at(&db, &week)
        .await
        .expect("due");
    assert!(
        due < utc_at("2026-05-03T00:00"),
        "the freeze falls due before the week starts: {due}"
    );
    let filter = crate::planning::kpis::PlanningKpiFilter {
        site_id: Some(site),
        from_day: "2026-05-03".to_string(),
        to_day: "2026-05-03".to_string(),
        exclude_unapproved: false,
    };
    let not_frozen = |now: chrono::DateTime<chrono::Utc>| {
        let db = db.clone();
        let filter = filter.clone();
        async move {
            crate::planning::kpis::planning_kpis(&db, &filter, now)
                .await
                .expect("kpis")
                .totals
                .weeks_not_frozen
        }
    };
    assert_eq!(not_frozen(due - chrono::Duration::minutes(1)).await, 0, "not due yet");
    assert_eq!(
        not_frozen(due).await,
        1,
        "due and never frozen, before the week even starts"
    );

    // Frozen: measured, no longer counted apart, and it is the site's current week.
    crate::planning::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");
    assert_eq!(not_frozen(utc_at("2026-05-05T10:00")).await, 0);
    let (current, sites) = crate::planning::kpis::current_weeks(&db, utc_at("2026-05-05T10:00"))
        .await
        .expect("current");
    assert!(sites >= 1);
    assert!(
        current
            .iter()
            .any(|w| w.site_id == site && w.week_start == "2026-05-03"),
        "{current:?}"
    );
    let (later, _) = crate::planning::kpis::current_weeks(&db, utc_at("2026-05-12T10:00"))
        .await
        .expect("current");
    assert!(
        !later.iter().any(|w| w.site_id == site),
        "next week was never frozen: {later:?}"
    );
}

#[tokio::test]
async fn capacity_rule_edits_are_checked_and_a_stale_edit_is_refused_with_a_code() {
    let db = setup_db().await;
    let team_id = get_any_team_id(&db).await;
    let rule = scheduling::create_capacity_rule(
        &db,
        CreateCapacityRuleInput {
            entity_id: None,
            team_id,
            effective_start: "2026-07-01".to_string(),
            effective_end: Some("2026-07-31".to_string()),
            available_hours_per_day: Some(8.0),
            max_overtime_hours_per_day: 2.0,
        },
    )
    .await
    .expect("rule");
    let edit = |end: Option<&str>, clear: bool| crate::planning::domain::UpdateCapacityRuleInput {
        effective_start: None,
        effective_end: end.map(str::to_string),
        clear_effective_end: clear,
        available_hours_per_day: None,
        max_overtime_hours_per_day: None,
    };

    let refused = scheduling::update_capacity_rule(&db, rule.id, rule.row_version, edit(Some("2026-06-30"), false))
        .await
        .expect_err("ends before it starts");
    assert!(
        format!("{refused:?}").contains("PLANNING_CAPACITY_RULE_END_BEFORE_START"),
        "{refused:?}"
    );

    let open = scheduling::update_capacity_rule(&db, rule.id, rule.row_version, edit(None, true))
        .await
        .expect("end removed");
    assert_eq!(open.effective_end, None, "the rule applies with no end again");
    assert_eq!(open.row_version, rule.row_version + 1);

    let stale = scheduling::update_capacity_rule(&db, rule.id, rule.row_version, edit(Some("2026-08-31"), false))
        .await
        .expect_err("read before the last edit");
    assert!(
        format!("{stale:?}").contains("PLANNING_CAPACITY_RULE_STALE"),
        "{stale:?}"
    );
}

#[tokio::test]
async fn only_moves_recorded_while_the_baseline_held_count_as_displaced() {
    let db = setup_db().await;
    let (site, team) = weekly_site(&db, "DW").await;
    let planner = ensure_personnel_with_rate(&db, "PLN-WK-DW").await;
    ensure_approver_user(&db, 58, planner).await;
    let mut jobs = Vec::new();
    for (title, day) in [
        ("KPI DW before", "2026-05-04"),
        ("KPI DW during", "2026-05-05"),
        ("KPI DW after", "2026-05-06"),
        ("KPI DW at freeze", "2026-05-03"),
        ("KPI DW at unfreeze", "2026-05-07"),
    ] {
        let (wo, actor) = create_planning_wo(&db, title).await;
        let job = scheduling::create_schedule_commitment(
            &db,
            actor,
            commit_input(
                insert_candidate(&db, "work_order", wo, "ready", 100.0).await,
                team,
                &format!("{day}T08:00"),
                &format!("{day}T10:00"),
            ),
        )
        .await
        .expect("job");
        jobs.push((job.id, day));
    }
    crate::planning::weekly_schedule::freeze_weekly_schedule(&db, 1, week_ref(site, "2026-05-04", None))
        .await
        .expect("frozen");
    let requested = crate::planning::weekly_schedule::request_unfreeze(
        &db,
        1,
        crate::planning::weekly_schedule::UnfreezeRequestInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: None,
            reason: "Shutdown moved".to_string(),
        },
    )
    .await
    .expect("requested");
    crate::planning::weekly_schedule::decide_unfreeze(
        &db,
        58,
        crate::planning::weekly_schedule::UnfreezeDecisionInput {
            site_id: site,
            day: "2026-05-04".to_string(),
            expected_row_version: requested.schedule.map(|s| s.row_version),
            approve: true,
            comment: None,
        },
    )
    .await
    .expect("unfrozen");
    // The baseline held from 2026-05-01 00:00 to 2026-05-05 12:00 (UTC).
    let schedule = scalar(
        &db,
        &format!("SELECT id FROM weekly_schedules WHERE entity_id = {site} AND week_start = '2026-05-03'"),
    )
    .await;
    exec_sql(
        &db,
        &format!(
            "UPDATE weekly_schedule_events SET created_at = CASE action \
               WHEN 'unfrozen' THEN '2026-05-05T12:00:00Z' ELSE '2026-05-01T00:00:00Z' END \
             WHERE weekly_schedule_id = {schedule} AND action IN ('frozen', 'auto_frozen', 'unfrozen')"
        ),
    )
    .await;
    // Each job is moved once inside the week: before the freeze, while frozen, after the unfreeze,
    // exactly at the freeze (counted: the window is [freeze, unfreeze)) and exactly at the
    // unfreeze (not counted).
    let moved_at = [
        "2026-04-30T10:00:00Z",
        "2026-05-03T10:00:00Z",
        "2026-05-06T10:00:00Z",
        "2026-05-01T00:00:00Z",
        "2026-05-05T12:00:00Z",
    ];
    for (n, ((job, day), at)) in jobs.iter().zip(moved_at).enumerate() {
        exec_sql(
            &db,
            &format!(
                "INSERT INTO schedule_break_ins
                    (schedule_commitment_id, break_in_reason, kind, old_slot_start, old_slot_end,
                     new_slot_start, new_slot_end, notification_dedupe_key, created_by_id, created_at)
                 VALUES ({job}, 'other', 'move', '{old_start}', '{old_end}', '{new_start}', '{new_end}',
                         'dw-move-{n}', 1, '{at}')",
                old_start = local(&format!("{day}T08:00")),
                old_end = local(&format!("{day}T10:00")),
                new_start = local(&format!("{day}T13:00")),
                new_end = local(&format!("{day}T15:00")),
            ),
        )
        .await;
    }
    let kpis = crate::planning::kpis::planning_kpis(
        &db,
        &crate::planning::kpis::PlanningKpiFilter {
            site_id: Some(site),
            from_day: "2026-05-03".to_string(),
            to_day: "2026-05-03".to_string(),
            exclude_unapproved: false,
        },
        utc_at("2026-05-20T09:00"),
    )
    .await
    .expect("kpis");
    let week = &kpis.weeks[0];
    assert!(week.reopened);
    assert_eq!(
        week.displaced_jobs, 2,
        "only the moves recorded in [freeze, unfreeze) count: {week:?}"
    );
}

#[tokio::test]
async fn planning_requires_an_isolation_for_every_permit_type_flagged_for_energy_isolation() {
    let db = setup_db().await;
    let loto_reasons = |wo_id: i64| {
        let db = db.clone();
        async move {
            super::readiness::permit_findings(&db, "work_order", wo_id, None)
                .await
                .expect("findings")
                .into_iter()
                .filter(|f| f.reason_code == "LOTO_NOT_VERIFIED")
                .count()
        }
    };
    let permit_on = |wo_id: i64, type_code: &'static str| {
        let db = db.clone();
        async move {
            let type_id = scalar(&db, &format!("SELECT id FROM permit_types WHERE code = '{type_code}'")).await;
            crate::permit::queries::create_work_permit(
                &db,
                crate::permit::domain::WorkPermitCreateInput {
                    permit_type_id: type_id,
                    asset_id: None,
                    entity_id: None,
                    linked_work_order_id: Some(wo_id),
                    expires_at: Some((chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339()),
                },
            )
            .await
            .expect("permit")
        }
    };
    let job = |title: &'static str| {
        let db = db.clone();
        async move {
            let (wo, _) = create_planning_wo(&db, title).await;
            exec_sql(
                &db,
                &format!("UPDATE work_orders SET requires_permit = 1 WHERE id = {wo}"),
            )
            .await;
            wo
        }
    };

    // LOTO (flagged by migration 225) without any isolation point: not ready.
    let loto_wo = job("Readiness LOTO").await;
    let loto = permit_on(loto_wo, "loto").await;
    assert!(loto.permit_type_requires_energy_isolation);
    assert_eq!(loto_reasons(loto_wo).await, 1, "LOTO still needs an isolation");

    // Hot work, not flagged: no isolation needed.
    let hot_wo = job("Readiness hot work").await;
    let hot = permit_on(hot_wo, "hot_work").await;
    assert!(!hot.permit_type_requires_energy_isolation);
    assert_eq!(loto_reasons(hot_wo).await, 0);

    // The same non-LOTO type once flagged for energy isolation: an isolation is needed at planning,
    // whatever the type is called.
    exec_sql(
        &db,
        "UPDATE permit_types SET requires_energy_isolation = 1 WHERE code = 'hot_work'",
    )
    .await;
    assert_eq!(
        loto_reasons(hot_wo).await,
        1,
        "a flagged non-LOTO type without isolation is not ready"
    );

    // A permit whose type row is gone can never start (the OT gate refuses it): planning reports
    // it as blocking, never as "no isolation needed".
    let candidate = insert_candidate(&db, "work_order", hot_wo, "ready", 100.0).await;
    re_evaluate(&db).await;
    let before = open_conflicts(&db, candidate).await;
    assert!(
        before.contains(&("LOTO_NOT_VERIFIED".to_string(), "warning".to_string())),
        "{before:?}"
    );
    assert!(!before.iter().any(|(code, _)| code == "PERMIT_TYPE_MISSING"));
    exec_sql(&db, "PRAGMA foreign_keys = OFF").await;
    exec_sql(
        &db,
        &format!("UPDATE work_permits SET permit_type_id = 999999 WHERE id = {}", hot.id),
    )
    .await;
    exec_sql(&db, "PRAGMA foreign_keys = ON").await;
    re_evaluate(&db).await;
    let after = open_conflicts(&db, candidate).await;
    assert!(
        after.contains(&("PERMIT_TYPE_MISSING".to_string(), "blocking".to_string())),
        "{after:?}"
    );
    assert!(
        !after.iter().any(|(code, _)| code == "LOTO_NOT_VERIFIED"),
        "never read as a type without isolation: {after:?}"
    );
    assert_eq!(readiness_of(&db, candidate).await, "blocked");
}

/// A corrective work order flagged "needs shutdown" is checked against the shutdown windows like
/// a PM plan (same NO_FEASIBLE_WINDOW warning); unflagged, it is not. The flag is read live from
/// the work order, so clearing it lifts the warning at the next evaluation.
#[tokio::test]
async fn a_corrective_work_order_needing_a_shutdown_is_checked_against_windows() {
    let db = setup_db().await;
    let (wo_id, actor) = create_planning_wo(&db, "Shutdown job").await;
    let refresh = || async {
        queries::refresh_schedule_candidates(
            &db,
            RefreshScheduleCandidatesInput {
                include_work_orders: Some(true),
                include_pm_occurrences: Some(false),
                include_approved_di: Some(false),
                limit_per_source: Some(50),
            },
        )
        .await
        .expect("refresh");
    };
    let no_window = |db: &DatabaseConnection| {
        let db = db.clone();
        async move {
            db.query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM scheduling_conflicts c
                   JOIN schedule_candidates sc ON sc.id = c.candidate_id
                  WHERE sc.source_type = 'work_order' AND sc.source_id = ?
                    AND c.reason_code = 'NO_FEASIBLE_WINDOW' AND c.resolved_at IS NULL",
                [wo_id.into()],
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get::<i64>("", "c")
            .expect("count")
        }
    };
    let row_version = |db: &DatabaseConnection| {
        let db = db.clone();
        async move {
            crate::wo::queries::get_work_order(&db, wo_id)
                .await
                .expect("read")
                .expect("wo")
                .row_version
        }
    };

    refresh().await;
    assert_eq!(no_window(&db).await, 0, "not flagged: no shutdown check");

    let wo = crate::wo::workflow::actions::schedule::save_shutdown_requirement(
        &db,
        wo_id,
        true,
        row_version(&db).await,
        actor,
    )
    .await
    .expect("flag");
    assert!(wo.requires_shutdown);
    refresh().await;
    assert_eq!(no_window(&db).await, 1, "flagged and no open shutdown window: warned");

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO planning_windows (window_type, start_datetime, end_datetime, is_locked) VALUES ('shutdown', ?, ?, 0)",
        ["2026-10-01T06:00:00Z".into(), "2026-10-01T18:00:00Z".into()],
    ))
    .await
    .expect("window");
    refresh().await;
    assert_eq!(no_window(&db).await, 0, "the shutdown window covers the work");

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "DELETE FROM planning_windows WHERE window_type = 'shutdown'".to_string(),
    ))
    .await
    .expect("no window again");
    crate::wo::workflow::actions::schedule::save_shutdown_requirement(&db, wo_id, false, row_version(&db).await, actor)
        .await
        .expect("unflag");
    refresh().await;
    assert_eq!(no_window(&db).await, 0, "unflagged: no shutdown check");
}

/// Only a short REQUIRED part blocks the plan; a short OPTIONAL part warns (owner-approved Step 7).
#[tokio::test]
async fn a_short_optional_part_warns_and_only_a_short_required_part_blocks() {
    let db = setup_db().await;
    let (wo_id, _actor) = create_planning_wo(&db, "Optional parts WO").await;
    let candidate = insert_candidate(&db, "work_order", wo_id, "ready", 100.0).await;
    for code in ["PLN-OPT", "PLN-REQ"] {
        exec_sql(
            &db,
            &format!(
                "INSERT INTO articles (article_code, article_name, unit_value_id, is_active)
                 SELECT '{code}', 'Article {code}', (SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id
                        JOIN reference_domains d ON d.id = s.domain_id
                       WHERE d.code = 'MEASURE.UNITS' AND s.status = 'published' AND v.code = 'U'), 1"
            ),
        )
        .await;
    }
    let optional = scalar(&db, "SELECT id FROM articles WHERE article_code = 'PLN-OPT'").await;
    let required = scalar(&db, "SELECT id FROM articles WHERE article_code = 'PLN-REQ'").await;
    // Neither article has stock: an optional line first.
    exec_sql(
        &db,
        &format!(
            "INSERT INTO work_order_parts (work_order_id, article_id, quantity_planned, is_required)
             VALUES ({wo_id}, {optional}, 2, 0)"
        ),
    )
    .await;
    re_evaluate(&db).await;
    assert_eq!(
        open_conflicts(&db, candidate).await,
        vec![("PARTS_SHORT_OPTIONAL".to_string(), "warning".to_string())]
    );
    assert_eq!(
        readiness_of(&db, candidate).await,
        "ready",
        "an optional part never blocks"
    );

    exec_sql(
        &db,
        &format!(
            "INSERT INTO work_order_parts (work_order_id, article_id, quantity_planned, is_required)
             VALUES ({wo_id}, {required}, 1, 1)"
        ),
    )
    .await;
    re_evaluate(&db).await;
    let conflicts = open_conflicts(&db, candidate).await;
    assert!(
        conflicts.contains(&("PARTS_SHORT".to_string(), "blocking".to_string())),
        "{conflicts:?}"
    );
    assert!(conflicts.contains(&("PARTS_SHORT_OPTIONAL".to_string(), "warning".to_string())));
    assert_eq!(readiness_of(&db, candidate).await, "blocked");
}

/// No hidden default: a new part line must say whether it is required.
#[test]
fn a_new_part_line_must_say_whether_it_is_required() {
    let without: Result<crate::wo::parts::AddPartInput, _> = serde_json::from_value(serde_json::json!({
        "wo_id": 1, "article_id": 2, "article_ref": null, "quantity_planned": 1.0, "unit_cost": null,
        "stock_location_id": null, "auto_reserve": false, "notes": null, "origin": null
    }));
    assert!(without.is_err(), "is_required is mandatory");
    assert!(crate::pm::wo_content::validate_required_parts_json(Some(r#"[{"code":"X"}]"#)).is_err());
    assert!(crate::pm::wo_content::validate_required_parts_json(Some(r#"["X"]"#)).is_err());
    assert!(crate::pm::wo_content::validate_required_parts_json(Some(r#"[{"code":"X","required":false}]"#)).is_ok());
}
