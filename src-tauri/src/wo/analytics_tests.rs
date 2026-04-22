//! Sprint S2 verification tests for Phase 2 SP05 File 03.
//!
//! V1 — Analytics snapshot completeness: close WO with 2 labor, 1 part, 1 task,
//!       1 failure detail, 1 verification → counts > 0, costs match.
//! V2 — Cost posting hook: wo_code, total_cost, type_code, entity_id populated.
//! V3 — Permission guard (code inspection): reopen_wo IPC command requires ot.admin.
//!       This is verified by inspecting commands/wo.rs (require_permission! macro)
//!       and cannot be tested at the domain layer. See note at bottom of file.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::wo::analytics;
    use crate::wo::closeout::{
        self, SaveFailureDetailInput, SaveVerificationInput, WoCloseInput,
    };
    use crate::wo::costs;
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{
        self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput,
    };
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts::{self, AddPartInput};
    use crate::wo::queries;
    use crate::wo::tasks::{self, AddTaskInput};

    // ═══════════════════════════════════════════════════════════════════════
    // Setup (same as closeout_tests)
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
            "INSERT INTO user_accounts \
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

    /// Create a corrective WO in draft state. Returns (wo_id, row_version).
    async fn create_wo_draft(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let actor = admin_id(db).await;
        let wo = queries::create_work_order(
            db,
            WoCreateInput {
                type_code: "corrective".into(),
                equipment_id: None,
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

    /// Advance a WO from draft → planned → ready_to_schedule → assigned → in_progress.
    /// Returns the updated row_version.
    async fn advance_to_in_progress(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        rv: i64,
    ) -> i64 {
        let actor = admin_id(db).await;

        // Plan: draft → planned
        let wo = execution::plan_wo(
            db,
            WoPlanInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                planner_id: actor,
                planned_start: "2026-04-10T08:00:00Z".into(),
                planned_end: "2026-04-10T16:00:00Z".into(),
                shift: None,
                expected_duration_hours: Some(8.0),
                urgency_id: None,
            },
        )
        .await
        .expect("plan_wo");
        let mut rv = wo.row_version;

        // ready_to_schedule (direct SQL)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = \
             (SELECT id FROM work_order_statuses WHERE code = 'ready_to_schedule'), \
             row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
             WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("advance to ready_to_schedule");
        rv += 1;

        // Assign
        let wo = execution::assign_wo(
            db,
            WoAssignInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                assigned_group_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
            },
        )
        .await
        .expect("assign_wo");
        rv = wo.row_version;

        // Start
        let wo = execution::start_wo(
            db,
            WoStartInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
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

        // 2 labor entries: 2h×50=100, 3h×40=120 → labor=220
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

        // 1 planned part: 5×20=100
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
            },
        )
        .await
        .expect("add planned part");

        parts::record_actual_usage(db, part.id, 5.0, Some(20.0))
            .await
            .expect("record actual usage");

        // Service cost = 50
        costs::update_service_cost(db, wo_id, 50.0, actor)
            .await
            .expect("update_service_cost");

        // Complete mechanically
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

        // Save failure detail
        closeout::save_failure_detail(
            db,
            SaveFailureDetailInput {
                wo_id,
                symptom_id: None,
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("Bearing replacement".into()),
            },
        )
        .await
        .expect("save_failure_detail");

        // Set root_cause_summary
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = ? WHERE id = ?",
            ["Bearing failure due to misalignment".into(), wo_id.into()],
        ))
        .await
        .expect("set root_cause_summary");

        // Verify
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

        // Close
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

        assert_eq!(
            closed_wo.status_code.as_deref(),
            Some("closed"),
            "WO must be closed"
        );

        closed_wo.row_version
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Analytics snapshot completeness
    // ═══════════════════════════════════════════════════════════════════════

    /// Close a WO with 2 labor, 1 part, 1 task, 1 failure detail, 1 verification,
    /// then verify the analytics snapshot returns complete data.
    #[tokio::test]
    async fn v1_analytics_snapshot_completeness() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        // Create WO in draft
        let (wo_id, rv) = create_wo_draft(&db).await;

        // Add 1 task while WO is still in draft (add_task requires draft/planned/ready/assigned)
        let task = tasks::add_task(
            &db,
            AddTaskInput {
                wo_id,
                task_description: "Inspect bearing alignment".into(),
                sequence_order: 1,
                is_mandatory: true,
                estimated_minutes: Some(30),
            },
        )
        .await
        .expect("add_task in draft");

        // Advance to in_progress
        let rv = advance_to_in_progress(&db, wo_id, rv).await;

        // Complete the task
        tasks::complete_task(&db, task.id, actor, "ok".into(), Some("Done".into()))
            .await
            .expect("complete_task");

        // Close with full data (2 labor + 1 part + service + failure + verification)
        let _rv = close_wo_with_data(&db, wo_id, rv).await;

        // ── Get analytics snapshot ────────────────────────────────────────
        let snap = analytics::get_wo_analytics_snapshot(&db, wo_id)
            .await
            .expect("get_wo_analytics_snapshot should succeed on closed WO");

        // Basic identity
        assert_eq!(snap.wo_id, wo_id);
        assert!(!snap.wo_code.is_empty(), "wo_code must not be empty");
        assert_eq!(snap.type_code, "corrective");

        // Sub-entity counts
        assert_eq!(snap.labor_entries_count, 2, "expected 2 labor entries");
        assert_eq!(snap.parts_entries_count, 1, "expected 1 parts entry");
        assert_eq!(snap.task_count, 1, "expected 1 task");
        assert_eq!(snap.mandatory_task_count, 1, "expected 1 mandatory task");
        assert_eq!(snap.completed_task_count, 1, "expected 1 completed task");

        // Close-out evidence
        assert_eq!(
            snap.failure_details.len(),
            1,
            "expected 1 failure detail"
        );
        assert_eq!(
            snap.verifications.len(),
            1,
            "expected 1 verification"
        );
        assert_eq!(
            snap.recurrence_risk_level.as_deref(),
            Some("low"),
            "recurrence_risk_level should be 'low'"
        );
        assert!(
            snap.root_cause_summary.is_some(),
            "root_cause_summary should be set"
        );

        // Costs: labor=220, parts=100, service=50, total=370
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

        // Timestamps
        assert!(snap.submitted_at.is_some(), "submitted_at must be set");
        assert!(snap.actual_start.is_some(), "actual_start must be set");
        assert!(snap.closed_at.is_some(), "closed_at must be set");
        assert!(
            snap.technically_verified_at.is_some(),
            "technically_verified_at must be set"
        );

        // Planning quality
        assert!(snap.was_planned, "WO was planned so was_planned must be true");
        // parts_actuals_confirmed is false — record_actual_usage doesn't set this flag;
        // only confirm_no_parts_used does.
        assert!(!snap.parts_actuals_confirmed, "parts actuals not explicitly confirmed");

        // Reopen count should be 0
        assert_eq!(snap.reopen_count, 0, "no reopens happened");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Cost posting hook
    // ═══════════════════════════════════════════════════════════════════════

    /// Close a WO with known costs, then verify the cost posting hook payload.
    #[tokio::test]
    async fn v2_cost_posting_hook() {
        let db = setup().await;

        // Create WO in draft
        let (wo_id, rv) = create_wo_draft(&db).await;

        // Advance to in_progress
        let rv = advance_to_in_progress(&db, wo_id, rv).await;

        // Close with full data
        let _rv = close_wo_with_data(&db, wo_id, rv).await;

        // ── Get cost posting hook ─────────────────────────────────────────
        let hook = costs::get_cost_posting_hook(&db, wo_id)
            .await
            .expect("get_cost_posting_hook should succeed on closed WO");

        assert_eq!(hook.wo_id, wo_id);
        assert!(
            hook.wo_code.starts_with("WOR-"),
            "wo_code should start with 'WOR-', got '{}'",
            hook.wo_code
        );
        assert_eq!(
            hook.type_code, "corrective",
            "type_code should be 'corrective'"
        );
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
        assert!(
            hook.closed_at.is_some(),
            "closed_at should be set on closed WO"
        );
        // entity_id / asset_id are None because we didn't set them in create
        assert_eq!(
            hook.entity_id, None,
            "entity_id should be None (not set)"
        );
        assert_eq!(
            hook.asset_id, None,
            "asset_id should be None (not set)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Permission guard (code-level verification)
    // ═══════════════════════════════════════════════════════════════════════
    //
    // The `reopen_wo` IPC command in commands/wo.rs uses:
    //   require_permission!(state, &user, "ot.admin", PermissionScope::Global);
    //
    // This macro runs inside the Tauri session/state model and cannot be
    // exercised from pure domain-layer integration tests. The permission
    // guard is verified by code inspection:
    //
    //   commands/wo.rs line ~536:
    //     let user = require_session!(state);
    //     require_permission!(state, &user, "ot.admin", PermissionScope::Global);
    //     require_step_up!(state);
    //     closeout::reopen_wo(&state.db, input).await
    //
    // A non-admin user (with only ot.edit) calling reopen_wo will hit the
    // require_permission! macro which returns PermissionDenied before
    // the domain function is ever invoked.
    //
    // If a full Tauri integration test harness is available in the future,
    // add a test here that creates a non-admin user session and tries to
    // invoke reopen_wo, expecting PermissionDenied.
}
