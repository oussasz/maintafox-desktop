//! Supervisor verification tests for Phase 2 SP05 File 03 — Sprint S1.
//!
//! V1 — Multi-gate failure list: close_wo on corrective WO with no labor, no parts,
//!       no failure detail must return 3+ blocking errors in one response.
//! V2 — Self-verification guard: save_verification with verified_by_id =
//!       primary_responsible_id must fail.
//! V3 — Cost roll-up: labor 220 + parts 100 + service 50 = total 370.
//! V4 — Reopen window: closed_at 10 days ago → reopen must fail.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::wo::closeout::{
        self, SaveFailureDetailInput, SaveVerificationInput, WoCloseInput, WoReopenInput,
    };
    use crate::wo::costs;
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{
        self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput,
    };
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts::{self, AddPartInput};
    use crate::wo::queries;

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

    /// Get the admin user id (always 1 after seed).
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

    /// Create a second test user for self-verification tests.
    /// Returns the new user's id.
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

    /// Create a corrective WO and advance it to in_progress.
    /// Returns (wo_id, row_version).
    async fn create_wo_in_progress(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let actor = admin_id(db).await;

        let wo = queries::create_work_order(
            db,
            WoCreateInput {
                type_id: 1, // corrective
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
                title: "Closeout test WO".into(),
                description: Some("Integration test for closeout".into()),
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

        let wo_id = wo.id;
        let mut rv = wo.row_version;

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
        rv = wo.row_version;

        // Advance: planned → ready_to_schedule (direct SQL)
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

        // Assign: ready_to_schedule → assigned
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

        // Start: assigned → in_progress
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

        (wo_id, wo.row_version)
    }

    /// Advance a WO from in_progress → mechanically_complete → technically_verified.
    /// This is the minimum path needed before close_wo can be called.
    /// Adds labor, confirms parts, completes mechanically, then verifies.
    /// Returns the updated row_version.
    async fn advance_to_technically_verified(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        rv: i64,
    ) -> i64 {
        let actor = admin_id(db).await;
        let verifier = create_second_user(db).await;

        // Add a labor entry (required for close gate)
        labor::add_labor_entry(
            db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T10:00:00Z".into()),
                hours_worked: None,
                hourly_rate: Some(50.0),
                notes: None,
            },
        )
        .await
        .expect("add labor");

        // Confirm parts (so the parts gate passes)
        parts::confirm_no_parts_used(db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        // Complete mechanically: in_progress → mechanically_complete
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

        // Save failure detail (required for corrective WO close gate)
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
                notes: Some("Test failure detail".into()),
            },
        )
        .await
        .expect("save_failure_detail");

        // Set root_cause_summary (required for corrective close gate)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = ? WHERE id = ?",
            ["Bearing failure due to misalignment".into(), wo_id.into()],
        ))
        .await
        .expect("set root_cause_summary");

        // Verify: mechanically_complete → technically_verified
        let (_ver, wo) = closeout::save_verification(
            db,
            SaveVerificationInput {
                wo_id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("low".into()),
                notes: Some("All checks passed".into()),
                expected_row_version: rv,
            },
        )
        .await
        .expect("save_verification");

        wo.row_version
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Multi-gate failure list
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_multi_gate_failure_list() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        // Create a corrective WO and advance to in_progress
        let (wo_id, rv) = create_wo_in_progress(&db).await;

        // Confirm parts to pass the mech-complete parts gate only
        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        // Complete mechanically (no labor, no parts, no failure detail, no verification)
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
        .expect("complete_wo_mechanically");
        let rv = wo.row_version;

        // Advance to technically_verified via direct SQL (bypass verification logic)
        // so we can test the close_wo quality gate directly
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = \
             (SELECT id FROM work_order_statuses WHERE code = 'technically_verified'), \
             technically_verified_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
             row_version = row_version + 1 \
             WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("advance to technically_verified");
        let rv = rv + 1;

        // Now try close_wo — should fail with at least 3 blocking errors:
        //   1. Labor actuals required (no interveners with hours)
        //   2. Parts actuals required (parts_actuals_confirmed=1 but no used parts — wait,
        //      we confirmed. Let me re-check: confirm_no_parts_used sets
        //      parts_actuals_confirmed=1, which means gate (b) passes.
        //      We need to NOT confirm parts for this test.)
        // Actually, we already called confirm_no_parts_used above for the mech-complete gate.
        // Let's reset it to 0 to ensure the close gate also catches it:
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET parts_actuals_confirmed = 0 WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("reset parts_actuals_confirmed");

        let result = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await;

        assert!(result.is_err(), "close_wo must fail when quality gates are not met");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                assert!(
                    errors.len() >= 3,
                    "Expected at least 3 blocking errors, got {}: {:?}",
                    errors.len(),
                    errors
                );

                let joined = errors.join(" | ");
                // Check for specific error categories
                assert!(
                    joined.contains("Labor") || joined.contains("main-d'oeuvre"),
                    "Errors must include labor actuals: {joined}"
                );
                assert!(
                    joined.contains("Parts") || joined.contains("pieces") || joined.contains("pièces"),
                    "Errors must include parts actuals: {joined}"
                );
                // For corrective WO: failure coding or root cause or verification
                assert!(
                    joined.contains("Failure") || joined.contains("defaillance")
                        || joined.contains("Root") || joined.contains("cause racine")
                        || joined.contains("verification") || joined.contains("Verification"),
                    "Errors must include failure/root-cause/verification gate: {joined}"
                );
            }
            other => panic!("Expected ValidationFailed, got: {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Self-verification guard
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_self_verification_guard() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        // Create WO and advance to in_progress
        let (wo_id, rv) = create_wo_in_progress(&db).await;

        // Confirm parts for mech-complete gate
        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        // Complete mechanically: in_progress → mechanically_complete
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
        .expect("complete_wo_mechanically");

        // The primary_responsible_id is set to `actor` (admin) by assign_wo.
        // Try to verify with the same actor — should fail with self-verification error.
        let result = closeout::save_verification(
            &db,
            SaveVerificationInput {
                wo_id,
                verified_by_id: actor, // same as primary_responsible_id!
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: None,
                notes: None,
                expected_row_version: wo.row_version,
            },
        )
        .await;

        assert!(result.is_err(), "save_verification must fail on self-verification");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                let joined = errors.join(" | ");
                assert!(
                    joined.contains("auto-verification") || joined.contains("Self-verification"),
                    "Error must mention self-verification: {joined}"
                );
            }
            other => panic!("Expected ValidationFailed, got: {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Cost roll-up
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_cost_roll_up() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_second_user(&db).await;

        // Create WO and advance to in_progress
        let (wo_id, rv) = create_wo_in_progress(&db).await;

        // Add 2 labor entries: 2h × 50 = 100, 3h × 40 = 120 → labor_cost = 220
        labor::add_labor_entry(
            &db,
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
            &db,
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

        // Add 1 planned part, then record actual usage: 5 × 20 = 100 → parts_cost = 100
        let part = parts::add_planned_part(
            &db,
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

        parts::record_actual_usage(&db, part.id, 5.0, Some(20.0))
            .await
            .expect("record actual usage");

        // Set service_cost_input = 50
        costs::update_service_cost(&db, wo_id, 50.0, actor)
            .await
            .expect("update_service_cost");

        // Complete mechanically: in_progress → mechanically_complete
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
        .expect("complete_wo_mechanically");
        let rv = wo.row_version;

        // Save failure detail (required for corrective WO)
        closeout::save_failure_detail(
            &db,
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

        // Set root_cause_summary (required for corrective)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = ? WHERE id = ?",
            ["Bearing failure due to misalignment".into(), wo_id.into()],
        ))
        .await
        .expect("set root_cause_summary");

        // Verify: mechanically_complete → technically_verified
        let (_ver, wo) = closeout::save_verification(
            &db,
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

        assert_eq!(
            wo.status_code.as_deref(),
            Some("technically_verified"),
            "WO must be technically_verified before close"
        );

        // Close WO — should succeed and compute costs
        let closed_wo = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await
        .expect("close_wo should succeed when all gates pass");

        assert_eq!(
            closed_wo.status_code.as_deref(),
            Some("closed"),
            "WO must be closed"
        );

        // Verify cost roll-up
        assert!(
            (closed_wo.labor_cost.unwrap_or(0.0) - 220.0).abs() < 0.01,
            "labor_cost expected 220, got {:?}",
            closed_wo.labor_cost
        );
        assert!(
            (closed_wo.parts_cost.unwrap_or(0.0) - 100.0).abs() < 0.01,
            "parts_cost expected 100, got {:?}",
            closed_wo.parts_cost
        );
        assert!(
            (closed_wo.service_cost.unwrap_or(0.0) - 50.0).abs() < 0.01,
            "service_cost expected 50, got {:?}",
            closed_wo.service_cost
        );
        assert!(
            (closed_wo.total_cost.unwrap_or(0.0) - 370.0).abs() < 0.01,
            "total_cost expected 370, got {:?}",
            closed_wo.total_cost
        );

        // Also verify via get_cost_summary
        let summary = costs::get_cost_summary(&db, wo_id)
            .await
            .expect("get_cost_summary");
        assert!(
            (summary.total_cost - 370.0).abs() < 0.01,
            "cost summary total expected 370, got {}",
            summary.total_cost
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 — Reopen window
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v4_reopen_window_expired() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_second_user(&db).await;

        // Create WO and advance to in_progress
        let (wo_id, rv) = create_wo_in_progress(&db).await;

        // Add labor entry (required for close gate)
        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-01T08:00:00Z".into()),
                ended_at: Some("2026-04-01T10:00:00Z".into()),
                hours_worked: Some(2.0),
                hourly_rate: Some(50.0),
                notes: None,
            },
        )
        .await
        .expect("add labor");

        // Confirm parts
        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        // Complete mechanically
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
        .expect("complete_wo_mechanically");
        let rv = wo.row_version;

        // Save failure detail (corrective WO)
        closeout::save_failure_detail(
            &db,
            SaveFailureDetailInput {
                wo_id,
                symptom_id: None,
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: None,
            },
        )
        .await
        .expect("save_failure_detail");

        // Set root_cause_summary
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = ? WHERE id = ?",
            ["Root cause identified".into(), wo_id.into()],
        ))
        .await
        .expect("set root_cause_summary");

        // Verify: mechanically_complete → technically_verified
        let (_ver, wo) = closeout::save_verification(
            &db,
            SaveVerificationInput {
                wo_id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("none".into()),
                notes: None,
                expected_row_version: rv,
            },
        )
        .await
        .expect("save_verification");
        let rv = wo.row_version;

        // Close the WO
        let closed_wo = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await
        .expect("close_wo");
        let rv = closed_wo.row_version;

        assert_eq!(closed_wo.status_code.as_deref(), Some("closed"));

        // Backdate closed_at to 10 days ago via direct DB update
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 days') \
             WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("backdate closed_at");

        // Try to reopen — should fail because outside the 7-day window
        let result = closeout::reopen_wo(
            &db,
            WoReopenInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                reason: "Need to add more data".into(),
            },
        )
        .await;

        assert!(result.is_err(), "reopen_wo must fail after reopen window expired");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                let joined = errors.join(" | ");
                assert!(
                    joined.contains("fenetre") || joined.contains("window")
                        || joined.contains("depassee") || joined.contains("exceeded"),
                    "Error must mention reopen window: {joined}"
                );
            }
            other => panic!("Expected ValidationFailed, got: {other:?}"),
        }
    }
}
