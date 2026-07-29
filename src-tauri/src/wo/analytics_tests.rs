//! Sprint S2 verification tests for Phase 2 SP05 File 03. Option B lifecycle.
//!
//! V1 — Analytics snapshot completeness: close WO with 2 labor, 1 part, 1 task,
//!       1 failure detail, 1 verification → counts > 0, costs match.
//! V2 — Cost posting hook: wo_code, total_cost, type_code, entity_id populated.
//! V3 — Permission guard (code inspection): reopen_wo IPC command requires ot.admin.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::wo::analytics;
    use crate::wo::closeout::{
        self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput,
    };
    use crate::wo::costs;
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput};
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts::{self, AddPartInput};
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

    async fn create_second_user(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO user_accounts \
             (sync_id, username, display_name, identity_mode, password_hash, \
              is_active, is_admin, force_password_change, \
              failed_login_attempts, created_at, updated_at, row_version) \
             VALUES ('test-verifier-sync', 'verifier', 'Test Verifier', 'local', \
                     'no-login-needed', 1, 0, 0, 0, ?, ?, 1)",
            [now.clone().into(), now.into()],
        ))
        .await
        .expect("insert second user");

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'verifier' LIMIT 1".to_string(),
            ))
            .await
            .expect("query")
            .expect("verifier user should exist");
        row.try_get::<i64>("", "id").unwrap()
    }

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
             VALUES (1, 'test-eq-analytics-001', 'EQ-ANALYTICS-001', 'Analytics Test Equipment', \
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

    async fn create_wo_draft(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
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
                title: "S2 analytics test WO".into(),
                description: Some("Integration test for analytics snapshot".into()),
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: Some(8.0),
                creator_id: actor,
                requires_permit: None,
            },
        )
        .await
        .expect("create WO");

        (wo.id, wo.row_version)
    }

    /// Advance from draft to in_progress via Option B lifecycle.
    async fn advance_to_in_progress(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        rv: i64,
    ) -> i64 {
        let actor = admin_id(db).await;

        // submit: draft → planning
        let wo = submit_wo(
            db,
            WoSubmitInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
            },
        )
        .await
        .expect("submit_wo");

        // plan (non-status save)
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

        // assign (non-status save)
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

        wo.row_version
    }

    /// Full close pipeline from in_progress: add labor + parts + service cost,
    /// mech complete, failure detail, root_cause, verification, close.
    /// Returns the closed WO's row_version.
    async fn close_wo_with_data(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        rv: i64,
    ) -> i64 {
        let actor = admin_id(db).await;
        let verifier = create_second_user(db).await;

        // 2h×50=100, 3h×40=120 → labor=220
        labor::add_labor_entry(
            db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T10:00:00Z".into()),
                hours_worked: Some(2.0),
                hourly_rate: Some(50.0),
                notes: None,
            },
        )
        .await
        .expect("add labor entry 1");

        labor::add_labor_entry(
            db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T10:00:00Z".into()),
                ended_at: Some("2026-04-10T13:00:00Z".into()),
                hours_worked: Some(3.0),
                hourly_rate: Some(40.0),
                notes: None,
            },
        )
        .await
        .expect("add labor entry 2");

        // 5×20=100
        let part = parts::add_planned_part(
            db,
            AddPartInput {
                wo_id,
                article_id: None,
                article_ref: Some("BEARING-6205".into()),
                quantity_planned: 5.0,
                unit_cost: Some(20.0),
                stock_location_id: None,
                auto_reserve: Some(false),
                notes: None,
                origin: None,
            },
        )
        .await
        .expect("add planned part");

        parts::record_actual_usage(db, part.id, 5.0, Some(20.0))
            .await
            .expect("record actual usage");

        costs::update_service_cost(db, wo_id, 50.0, actor)
            .await
            .expect("update_service_cost");

        seed_rams_for_complete(db, wo_id).await;

        let wo = execution::complete_wo_mechanically(
            db,
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
        .expect("complete_wo_mechanically");
        let rv = wo.row_version;

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        // save_verification: stamps technically_verified_at, status stays completed
        let (_ver, wo) = closeout::save_verification(
            db,
            SaveVerificationInput {
                wo_id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("low".into()),
                notes: None,
                expected_row_version: rv,
            },
        )
        .await
        .expect("save_verification");
        let rv = wo.row_version;

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        let closed_wo = closeout::close_wo(
            db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await
        .expect("close_wo");

        assert_eq!(closed_wo.status_code.as_deref(), Some("closed"));

        closed_wo.row_version
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Analytics snapshot completeness
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_analytics_snapshot_completeness() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = create_wo_draft(&db).await;

        // Add 1 task while WO is still in draft
        let task = tasks::add_task(
            &db,
            AddTaskInput {
                wo_id,
                task_description: "Inspect bearing alignment".into(),
                sequence_order: 1,
                is_mandatory: true,
                estimated_minutes: Some(30),
                origin: None,
            },
        )
        .await
        .expect("add_task in draft");

        let rv = advance_to_in_progress(&db, wo_id, rv).await;

        tasks::complete_task(&db, task.id, actor, "ok".into(), Some("Done".into()))
            .await
            .expect("complete_task");

        let _rv = close_wo_with_data(&db, wo_id, rv).await;

        let snap = analytics::get_wo_analytics_snapshot(&db, wo_id)
            .await
            .expect("get_wo_analytics_snapshot should succeed on closed WO");

        assert_eq!(snap.wo_id, wo_id);
        assert!(!snap.wo_code.is_empty(), "wo_code must not be empty");
        assert_eq!(snap.type_code, "corrective");

        assert_eq!(snap.labor_entries_count, 2, "expected 2 labor entries");
        assert_eq!(snap.parts_entries_count, 1, "expected 1 parts entry");
        assert_eq!(snap.task_count, 1, "expected 1 task");
        assert_eq!(snap.mandatory_task_count, 1, "expected 1 mandatory task");
        assert_eq!(snap.completed_task_count, 1, "expected 1 completed task");

        assert_eq!(snap.failure_details.len(), 1, "expected 1 failure detail");
        assert_eq!(snap.verifications.len(), 1, "expected 1 verification");
        assert_eq!(snap.recurrence_risk_level.as_deref(), Some("low"));
        assert!(snap.root_cause_summary.is_some(), "root_cause_summary should be set");

        assert!(
            (snap.labor_cost - 220.0).abs() < 0.01,
            "labor_cost expected 220, got {}",
            snap.labor_cost
        );
        assert!(
            (snap.parts_cost - 100.0).abs() < 0.01,
            "parts_cost expected 100, got {}",
            snap.parts_cost
        );
        assert!(
            (snap.service_cost - 50.0).abs() < 0.01,
            "service_cost expected 50, got {}",
            snap.service_cost
        );
        assert!(
            (snap.total_cost - 370.0).abs() < 0.01,
            "total_cost expected 370, got {}",
            snap.total_cost
        );

        assert!(snap.submitted_at.is_some(), "submitted_at must be set");
        assert!(snap.actual_start.is_some(), "actual_start must be set");
        assert!(snap.closed_at.is_some(), "closed_at must be set");
        // technically_verified_at is still stamped by save_verification even though
        // the status stays completed in Option B
        assert!(
            snap.technically_verified_at.is_some(),
            "technically_verified_at must be stamped by save_verification"
        );

        assert!(snap.was_planned, "WO was planned so was_planned must be true");
        // record_actual_usage does not set parts_actuals_confirmed
        assert!(!snap.parts_actuals_confirmed, "parts actuals not explicitly confirmed");
        assert_eq!(snap.reopen_count, 0, "no reopens happened");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Cost posting hook
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_cost_posting_hook() {
        let db = setup().await;

        let (wo_id, rv) = create_wo_draft(&db).await;
        let rv = advance_to_in_progress(&db, wo_id, rv).await;
        let _rv = close_wo_with_data(&db, wo_id, rv).await;

        let hook = costs::get_cost_posting_hook(&db, wo_id)
            .await
            .expect("get_cost_posting_hook should succeed on closed WO");

        assert_eq!(hook.wo_id, wo_id);
        assert!(
            hook.wo_code.starts_with("OT-"),
            "wo_code should start with 'OT-', got '{}'",
            hook.wo_code
        );
        assert_eq!(hook.type_code, "corrective");
        assert!(
            (hook.total_cost - 370.0).abs() < 0.01,
            "total_cost expected 370, got {}",
            hook.total_cost
        );
        assert!(
            (hook.labor_cost - 220.0).abs() < 0.01,
            "labor_cost expected 220, got {}",
            hook.labor_cost
        );
        assert!(
            (hook.parts_cost - 100.0).abs() < 0.01,
            "parts_cost expected 100, got {}",
            hook.parts_cost
        );
        assert!(
            (hook.service_cost - 50.0).abs() < 0.01,
            "service_cost expected 50, got {}",
            hook.service_cost
        );
        assert!(hook.closed_at.is_some(), "closed_at should be set on closed WO");
        // create_work_order derives entity_id from equipment.installed_at_node_id;
        // asset_id is equipment_id on the cost posting hook.
        assert_eq!(hook.entity_id, Some(1));
        assert_eq!(hook.asset_id, Some(1));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Permission guard (code-level verification)
    // ═══════════════════════════════════════════════════════════════════════
    //
    // The `reopen_wo` IPC command in commands/wo.rs uses:
    //   require_permission!(state, &user, crate::rbac::permissions::OT_ADMIN, PermissionScope::Global);
    //
    // This is verified by code inspection only; domain-layer tests cannot
    // exercise the Tauri session/state model.
}
