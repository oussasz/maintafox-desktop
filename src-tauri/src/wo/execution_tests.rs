//! Supervisor verification tests for Phase 2 SP05 File 02 — Option B lifecycle.
//!
//! Sprint S1:
//! V1 — Pause creates open delay segment.
//! V2 — Resume closes delay and updates waiting hours.
//! V3 — Mandatory task gate.
//! V4 — Parts gate.
//! V5 — Open downtime block.
//!
//! Sprint S2:
//! V1 — Permission on complete (structural: require_permission! with ot.edit).
//! V2 — Full execute path: submit → plan → assign → mark_ready → start → add_labor → open_downtime →
//!       close_downtime → confirm_no_parts → complete_wo_mechanically.
//! V3 — Command count (structural: 41 WO entries in invoke_handler).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::auth::rbac::{self, PermissionScope};
    use crate::errors::AppError;
    use crate::wo::closeout::{self, SaveFailureDetailInput, UpdateWoRcaInput};
    use crate::wo::delay::{self, OpenDowntimeInput};
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{
        self, WoAssignInput, WoMechCompleteInput, WoPauseInput, WoPlanInput, WoResumeInput, WoStartInput,
    };
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts;
    use crate::wo::queries;
    use crate::wo::tasks::{self, AddTaskInput};
    use crate::wo::workflow::actions::mark_ready::{mark_wo_ready, WoMarkReadyInput};
    use crate::wo::workflow::actions::submit::{submit_wo, WoSubmitInput};

    // ═══════════════════════════════════════════════════════════════════════
    // Setup
    // ═══════════════════════════════════════════════════════════════════════

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should connect");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations should apply cleanly");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder should run cleanly");

        db
    }

    async fn resolve_delay_reason_id(db: &sea_orm::DatabaseConnection, code: &str) -> i64 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT rv.id AS id \
                 FROM reference_values rv \
                 INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                 INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS' \
                   AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) \
                   AND rv.is_active = 1 \
                   AND rs.status = 'published' \
                 LIMIT 1",
                [code.into()],
            ))
            .await
            .expect("delay reason query")
            .expect("WORK.DELAY_REASONS value must be seeded");
        row.try_get::<i64>("", "id").expect("id")
    }

    /// Seed failure coding + RCA required before `complete_wo_mechanically` for corrective/emergency WOs.
    async fn seed_rams_for_complete(db: &sea_orm::DatabaseConnection, wo_id: i64) {
        let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("lookup symptom")
            .expect("seeded DI.SYMPTOM vibration");

        closeout::save_failure_detail(
            db,
            SaveFailureDetailInput {
                wo_id,
                symptom_id: Some(symptom_id),
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("Test failure detail notes for RAMS complete gate".into()),
            },
        )
        .await
        .expect("seed_rams_for_complete: save_failure_detail");

        closeout::update_wo_rca(
            db,
            UpdateWoRcaInput {
                wo_id,
                root_cause_summary: Some("Test root cause".into()),
                corrective_action_summary: Some("Test corrective action".into()),
            },
        )
        .await
        .expect("seed_rams_for_complete: update_wo_rca");
    }

    async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1".to_string(),
            ))
            .await
            .expect("query should succeed")
            .expect("admin user should exist");
        row.try_get::<i64>("", "id").unwrap()
    }

    async fn assign_role(db: &sea_orm::DatabaseConnection, user_id: i32, role_name: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, created_at, updated_at) \
             VALUES ('wo-test-assign-' || ?, ?, \
               (SELECT id FROM roles WHERE name = ?), \
               'tenant', ?, ?)",
            [
                user_id.into(),
                user_id.into(),
                role_name.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert user_scope_assignment");
    }

    /// Seed a minimal equipment row (id=1) for the readiness gate.
    async fn seed_test_equipment(db: &sea_orm::DatabaseConnection) {
        // Ensure org scaffolding for installed_at_node_id FK
        let _ = db
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO org_structure_models \
                 (id, sync_id, version_number, status, created_at, updated_at) \
                 VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));"
                    .to_string(),
            ))
            .await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_node_types \
             (id, sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert org_node_types");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_nodes \
             (id, sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
                     datetime('now'), datetime('now'), 1);"
                .to_string(),
        ))
        .await
        .expect("insert org_nodes");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO equipment \
             (id, sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
              created_at, updated_at) \
             VALUES (1, 'test-eq-exec-001', 'EQ-EXEC-001', 'Exec Test Equipment', \
                     'active_in_service', 1, datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert test equipment");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE equipment SET installed_at_node_id = 1 \
             WHERE id = 1 AND installed_at_node_id IS NULL;"
                .to_string(),
        ))
        .await
        .expect("ensure installed_at_node_id");
    }

    /// Create a WO and advance it to in_progress via the Option B lifecycle.
    /// Returns (wo_id, row_version_after_start).
    async fn create_wo_in_progress(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        seed_test_equipment(db).await;
        let actor = admin_id(db).await;

        let wo = queries::create_work_order(
            db,
            WoCreateInput {
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
                urgency_id: Some(3),
                title: "Test WO for execution".into(),
                description: Some("Integration test".into()),
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: None,
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create WO");

        let wo_id = wo.id;

        // submit: draft → planning
        let wo = submit_wo(
            db,
            WoSubmitInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit_wo");

        // plan (non-status save, stays planning)
        let wo = execution::plan_wo(
            db,
            WoPlanInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                planner_id: actor,
                planned_start: "2026-04-10T08:00:00Z".into(),
                planned_end: "2026-04-10T16:00:00Z".into(),
                shift: None,
                expected_duration_hours: Some(8.0),
                planned_downtime_hours: None,
                urgency_id: None,
            },
        )
        .await
        .expect("plan_wo");

        // assign (non-status save, stays planning)
        let wo = execution::assign_wo(
            db,
            WoAssignInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                assigned_group_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
            },
        )
        .await
        .expect("assign_wo");

        // mark ready: planning → ready
        let wo = mark_wo_ready(
            db,
            WoMarkReadyInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("mark_wo_ready");

        // start: ready → in_progress
        let wo = execution::start_wo(
            db,
            WoStartInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("start_wo");

        (wo_id, wo.row_version)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Pause creates open delay segment
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_pause_creates_open_delay_segment() {
        let db = setup().await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;
        let actor = admin_id(&db).await;

        let reason_id = resolve_delay_reason_id(&db, "no_parts").await;
        let _wo = execution::pause_wo(
            &db,
            WoPauseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                delay_reason_id: reason_id,
                comment: Some("Waiting for parts".into()),
            },
        )
        .await
        .expect("pause_wo should succeed");

        let segments = delay::list_delay_segments(&db, wo_id)
            .await
            .expect("list_delay_segments");

        assert_eq!(segments.len(), 1, "exactly one delay segment after pause");
        let seg = &segments[0];
        assert!(
            seg.ended_at.is_none(),
            "delay segment ended_at must be NULL (still open)"
        );
        assert_eq!(
            seg.delay_reason_id,
            Some(reason_id),
            "delay_reason_id must match reference value"
        );
        assert_eq!(seg.work_order_id, wo_id);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Resume closes delay and updates waiting hours
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_resume_closes_delay_and_updates_waiting_hours() {
        let db = setup().await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;
        let actor = admin_id(&db).await;

        let reason_id = resolve_delay_reason_id(&db, "backordered").await;
        let wo = execution::pause_wo(
            &db,
            WoPauseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                delay_reason_id: reason_id,
                comment: None,
            },
        )
        .await
        .expect("pause_wo");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_delay_segments SET started_at = \
             strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour') WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("backdate delay segment");

        let wo = execution::resume_wo(
            &db,
            WoResumeInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("resume_wo");

        let segments = delay::list_delay_segments(&db, wo_id)
            .await
            .expect("list_delay_segments");
        assert_eq!(segments.len(), 1);
        assert!(
            segments[0].ended_at.is_some(),
            "delay segment ended_at must be set after resume"
        );

        assert!(
            wo.total_waiting_hours.unwrap_or(0.0) > 0.0,
            "total_waiting_hours should be > 0 after resume (backdate 1h): got {:?}",
            wo.total_waiting_hours
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Mandatory task gate
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_mandatory_task_gate_blocks_completion() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        seed_test_equipment(&db).await;

        let wo = queries::create_work_order(
            &db,
            WoCreateInput {
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
                urgency_id: Some(3),
                title: "Task gate test".into(),
                description: None,
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: None,
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create WO");

        tasks::add_task(
            &db,
            AddTaskInput {
                wo_id: wo.id,
                task_description: "Check voltage levels".into(),
                sequence_order: 1,
                is_mandatory: true,
                estimated_minutes: Some(15),
                origin: None,
            },
        )
        .await
        .expect("add_task");

        // SQL-set in_progress directly (bypasses submission flow — valid for gate-only tests
        // since in_progress is a recognized status code after migration).
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = \
             (SELECT id FROM work_order_statuses WHERE code = 'in_progress'), \
             actual_start = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
             row_version = row_version + 1 \
             WHERE id = ?",
            [wo.id.into()],
        ))
        .await
        .expect("advance to in_progress");
        let rv = wo.row_version + 1;

        parts::confirm_no_parts_used(&db, wo.id, actor)
            .await
            .expect("confirm_no_parts");

        seed_rams_for_complete(&db, wo.id).await;

        let result = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await;

        assert!(result.is_err(), "complete_wo_mechanically must fail");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                let joined = errors.join(" | ");
                assert!(
                    joined.contains("task") || joined.contains("tâche") || joined.contains("Task"),
                    "error must mention mandatory task: got '{joined}'"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 — Parts gate
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v4_parts_gate_blocks_completion() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        seed_test_equipment(&db).await;

        let wo = queries::create_work_order(
            &db,
            WoCreateInput {
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
                urgency_id: Some(3),
                title: "Parts gate test".into(),
                description: None,
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: None,
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create WO");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = \
             (SELECT id FROM work_order_statuses WHERE code = 'in_progress'), \
             actual_start = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
             row_version = row_version + 1 \
             WHERE id = ?",
            [wo.id.into()],
        ))
        .await
        .expect("advance to in_progress");
        let rv = wo.row_version + 1;

        seed_rams_for_complete(&db, wo.id).await;

        let result = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await;

        assert!(result.is_err(), "complete_wo_mechanically must fail");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                let joined = errors.join(" | ");
                assert!(
                    joined.contains("parts") || joined.contains("pièces") || joined.contains("Parts"),
                    "error must mention parts: got '{joined}'"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V5 — Open downtime block
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v5_open_downtime_blocks_completion() {
        let db = setup().await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;
        let actor = admin_id(&db).await;

        delay::open_downtime_segment(
            &db,
            OpenDowntimeInput {
                wo_id,
                downtime_type: "full".into(),
                comment: Some("Equipment down".into()),
                classification_code: None,
                actor_id: actor,
            },
        )
        .await
        .expect("open_downtime_segment");

        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        seed_rams_for_complete(&db, wo_id).await;

        let result = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await;

        assert!(result.is_err(), "complete_wo_mechanically must fail");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                let joined = errors.join(" | ");
                assert!(
                    joined.contains("downtime") || joined.contains("arrêt") || joined.contains("Downtime"),
                    "error must mention open downtime: got '{joined}'"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Sprint S2 — V2: Full execute path (Option B)
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn s2_v2_full_execute_path_to_mechanically_complete() {
        let db = setup().await;
        seed_test_equipment(&db).await;
        let actor = admin_id(&db).await;

        // 1. Create draft WO with equipment
        let wo = queries::create_work_order(
            &db,
            WoCreateInput {
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
                urgency_id: Some(3),
                title: "Full path test".into(),
                description: Some("S2 V2".into()),
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: None,
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create WO");
        let wo_id = wo.id;
        assert_eq!(wo.status_code.as_deref(), Some("draft"), "new WO must be draft");

        // 2. submit: draft → planning
        let wo = submit_wo(
            &db,
            WoSubmitInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit_wo");
        assert_eq!(wo.status_code.as_deref(), Some("planning"));

        // 3. plan_wo (non-status save, stays planning)
        let wo = execution::plan_wo(
            &db,
            WoPlanInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                planner_id: actor,
                planned_start: "2026-04-10T08:00:00Z".into(),
                planned_end: "2026-04-10T16:00:00Z".into(),
                shift: None,
                expected_duration_hours: Some(8.0),
                planned_downtime_hours: None,
                urgency_id: None,
            },
        )
        .await
        .expect("plan_wo");
        assert_eq!(wo.status_code.as_deref(), Some("planning"));

        // 4. assign_wo (non-status save, stays planning)
        let wo = execution::assign_wo(
            &db,
            WoAssignInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                assigned_group_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
            },
        )
        .await
        .expect("assign_wo");
        assert_eq!(wo.status_code.as_deref(), Some("planning"));

        // 5. mark_wo_ready: planning → ready
        let wo = mark_wo_ready(
            &db,
            WoMarkReadyInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("mark_wo_ready");
        assert_eq!(wo.status_code.as_deref(), Some("ready"));

        // 6. start_wo: ready → in_progress
        let wo = execution::start_wo(
            &db,
            WoStartInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("start_wo");
        let mut rv = wo.row_version;
        assert_eq!(wo.status_code.as_deref(), Some("in_progress"));

        // 7. add_labor (with both timestamps — auto-closes)
        let _labor = labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T10:30:00Z".into()),
                hours_worked: None,
                hourly_rate: Some(45.0),
                notes: None,
            },
        )
        .await
        .expect("add_labor");

        // 8. open_downtime
        let seg = delay::open_downtime_segment(
            &db,
            OpenDowntimeInput {
                wo_id,
                downtime_type: "partial".into(),
                comment: Some("Reduced throughput".into()),
                classification_code: None,
                actor_id: actor,
            },
        )
        .await
        .expect("open_downtime");

        // 9. close_downtime
        let _seg = delay::close_downtime_segment(&db, seg.id, None)
            .await
            .expect("close_downtime");

        // 10. confirm_no_parts
        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        // Fetch latest rv (downtime segments may bump it)
        let refreshed = queries::get_work_order(&db, wo_id)
            .await
            .expect("get_work_order")
            .expect("WO must exist");
        rv = refreshed.row_version;

        seed_rams_for_complete(&db, wo_id).await;

        // 11. complete_wo_mechanically: in_progress → completed
        let wo = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await
        .expect("complete_wo_mechanically must succeed when all gates pass");

        assert_eq!(
            wo.status_code.as_deref(),
            Some("completed"),
            "WO must be in 'completed' after full execute path"
        );
        assert!(
            wo.mechanically_completed_at.is_some(),
            "mechanically_completed_at must be set"
        );
        assert!(
            wo.active_labor_hours.unwrap_or(0.0) > 0.0,
            "active_labor_hours must be computed from labor entries"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Sprint S2 — V1: Permission on complete
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn s2_v1_readonly_has_view_but_not_edit_for_complete() {
        let db = setup().await;

        assign_role(&db, 90, "Readonly").await;

        let can_view = rbac::check_permission(&db, 90, crate::rbac::permissions::OT_VIEW, &PermissionScope::Global)
            .await
            .expect("check ot.view");
        let can_edit = rbac::check_permission(&db, 90, crate::rbac::permissions::OT_EDIT, &PermissionScope::Global)
            .await
            .expect("check ot.edit");

        assert!(can_view, "Readonly must have ot.view");
        assert!(
            !can_edit,
            "Readonly must NOT have ot.edit; complete_wo_mechanically must return PermissionDenied"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Sprint S2 — V3: Command count and uniqueness
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn s2_v3_invoke_handler_has_25_unique_wo_commands() {
        let lib_rs = include_str!("../lib.rs");

        let wo_lines: Vec<&str> = lib_rs.lines().filter(|l| l.contains("commands::wo::")).collect();

        assert_eq!(wo_lines.len(), 54, "invoke_handler must contain exactly 54 WO commands");

        let mut seen = std::collections::HashSet::new();
        for line in &wo_lines {
            assert!(
                seen.insert(line.trim().to_string()),
                "duplicate WO command in invoke_handler: {line}"
            );
        }

        let required_new = [
            "commands::wo::plan_wo",
            "commands::wo::submit_wo",
            "commands::wo::evaluate_wo_readiness",
            "commands::wo::mark_wo_ready",
            "commands::wo::return_to_planning",
            "commands::wo::approve_planning",
            "commands::wo::assign_wo",
            "commands::wo::start_wo",
            "commands::wo::pause_wo",
            "commands::wo::resume_wo",
            "commands::wo::hold_wo",
            "commands::wo::complete_wo_mechanically",
            "commands::wo::add_labor",
            "commands::wo::close_labor",
            "commands::wo::list_labor",
            "commands::wo::add_part",
            "commands::wo::record_part_usage",
            "commands::wo::confirm_no_parts",
            "commands::wo::add_task",
            "commands::wo::complete_task",
            "commands::wo::list_tasks",
            "commands::wo::open_downtime",
            "commands::wo::close_downtime",
            "commands::wo::list_delay_segments",
            "commands::wo::list_downtime_segments",
        ];

        for cmd in required_new {
            assert!(
                lib_rs.contains(cmd),
                "missing required WO command in invoke_handler: {cmd}"
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Sprint S4 — V1: Shift is persisted through plan_wo
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn s4_v1_shift_is_persisted_through_plan_wo() {
        let db = setup().await;
        seed_test_equipment(&db).await;
        let actor = admin_id(&db).await;

        let wo = queries::create_work_order(
            &db,
            WoCreateInput {
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
                urgency_id: None,
                title: "Shift persistence test".into(),
                description: None,
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: None,
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create WO");

        assert!(wo.shift.is_none(), "new WO must have no shift");

        // submit: draft → planning (required before plan_wo in Option B)
        let wo = submit_wo(
            &db,
            WoSubmitInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("submit_wo");

        // plan_wo with shift = "nuit"
        let planned = execution::plan_wo(
            &db,
            WoPlanInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                planner_id: actor,
                planned_start: "2026-04-10T08:00:00Z".into(),
                planned_end: "2026-04-10T16:00:00Z".into(),
                shift: Some("nuit".into()),
                expected_duration_hours: None,
                planned_downtime_hours: None,
                urgency_id: None,
            },
        )
        .await
        .expect("plan_wo with shift");

        assert_eq!(
            planned.shift.as_deref(),
            Some("nuit"),
            "plan_wo response must carry shift = 'nuit'"
        );

        let reloaded = queries::get_work_order(&db, wo.id)
            .await
            .expect("get_work_order")
            .expect("WO must exist after plan_wo");

        assert_eq!(
            reloaded.shift.as_deref(),
            Some("nuit"),
            "shift must survive a round-trip through the database"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Sprint S4 — V2: Open labor entry blocks mechanical complete
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn s4_v2_open_labor_blocks_mechanical_completion() {
        let db = setup().await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;
        let actor = admin_id(&db).await;

        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: None,
                hours_worked: None,
                hourly_rate: None,
                notes: None,
            },
        )
        .await
        .expect("add open labor entry");

        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        seed_rams_for_complete(&db, wo_id).await;

        let result = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await;

        assert!(result.is_err(), "complete_wo_mechanically must fail with open labor");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                let joined = errors.join(" | ");
                assert!(
                    joined.to_lowercase().contains("labor")
                        || joined.contains("intervenant")
                        || joined.contains("main-d"),
                    "error must mention open labor entries: got '{joined}'"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }
}
