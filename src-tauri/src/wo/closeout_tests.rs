//! Supervisor verification tests for Phase 2 SP05 File 03 — Sprint S1. Option B lifecycle.
//!
//! V1 — Multi-gate failure list: close_wo on corrective WO with no labor, no parts,
//!       no failure detail must return 3+ blocking errors in one response.
//! V2 — Self-verification guard: save_verification with verified_by_id =
//!       primary_responsible_id must fail.
//! V3 — Cost roll-up: labor 220 + parts 100 + service 50 = total 370.
//! V4 — Reopen from closed must fail (closed is a terminal state; reopen_wo requires completed).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::wo::closeout::{
        self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput,
        WoReopenInput,
    };
    use crate::wo::costs;
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput};
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts::{self, AddPartInput};
    use crate::wo::queries;
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
             VALUES (1, 'test-eq-co-001', 'EQ-CO-001', 'Closeout Test Equipment', \
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

    /// Strip RAMS fields so close_wo quality gates can still assert missing failure coding.
    async fn clear_rams_for_close_gate(db: &sea_orm::DatabaseConnection, wo_id: i64) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM work_order_failure_details WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("clear failure details");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = NULL, corrective_action_summary = NULL WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("clear root_cause_summary");
    }

    /// Create a corrective WO and advance it to in_progress via Option B lifecycle.
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

        (wo_id, wo.row_version)
    }

    /// Advance a WO from in_progress to completed+verified with all quality gates satisfied.
    /// In Option B, save_verification does NOT change WO status — stays completed.
    /// Returns the updated row_version.
    async fn advance_to_completed_verified(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        rv: i64,
    ) -> i64 {
        let actor = admin_id(db).await;
        let verifier = create_second_user(db).await;

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

        parts::confirm_no_parts_used(db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

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

        // WO is now completed
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
                notes: Some("All checks passed".into()),
                expected_row_version: rv,
            },
        )
        .await
        .expect("save_verification");

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        wo.row_version
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Multi-gate failure list
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_multi_gate_failure_list() {
        let db = setup().await;
        let actor = admin_id(&db).await;

        let (wo_id, rv) = create_wo_in_progress(&db).await;

        // Confirm parts so the mech-complete parts gate passes
        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts for mech complete");

        seed_rams_for_complete(&db, wo_id).await;

        // Complete mechanically: in_progress → completed
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

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        // Reset parts + strip RAMS so close gates re-fire
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET parts_actuals_confirmed = 0 WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("reset parts_actuals_confirmed");
        clear_rams_for_close_gate(&db, wo_id).await;

        // close_wo on a completed WO with no labor, no parts confirmed, no failure detail, no verification
        let result = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
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
                assert!(
                    joined.contains("Labor") || joined.contains("main-d'oeuvre"),
                    "Errors must include labor actuals: {joined}"
                );
                assert!(
                    joined.contains("Parts") || joined.contains("pieces") || joined.contains("pièces"),
                    "Errors must include parts actuals: {joined}"
                );
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

        let (wo_id, rv) = create_wo_in_progress(&db).await;

        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        seed_rams_for_complete(&db, wo_id).await;

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

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        // Try to verify with the same actor as primary_responsible_id — should fail
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

        let (wo_id, rv) = create_wo_in_progress(&db).await;

        // 2h × 50 = 100, 3h × 40 = 120 → labor_cost = 220
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

        // 5 × 20 = 100 → parts_cost = 100
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
                origin: None,
            },
        )
        .await
        .expect("add planned part");

        parts::record_actual_usage(&db, part.id, 5.0, Some(20.0))
            .await
            .expect("record actual usage");

        costs::update_service_cost(&db, wo_id, 50.0, actor)
            .await
            .expect("update_service_cost");

        seed_rams_for_complete(&db, wo_id).await;

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

        assert_eq!(wo.status_code.as_deref(), Some("completed"));

        // save_verification: stamps technically_verified_at, status stays completed
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
            Some("completed"),
            "WO must be completed after save_verification in Option B"
        );

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

        assert_eq!(closed_wo.status_code.as_deref(), Some("closed"));

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
    // V4 — Reopen from closed must fail (closed is terminal)
    // ═══════════════════════════════════════════════════════════════════════
    //
    // In Option B, reopen_wo only operates on completed WOs.
    // closed is a terminal state; attempting reopen_wo from closed must fail
    // with an invalid transition error.

    #[tokio::test]
    async fn v4_reopen_from_closed_is_rejected() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_second_user(&db).await;

        let (wo_id, rv) = create_wo_in_progress(&db).await;

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

        parts::confirm_no_parts_used(&db, wo_id, actor)
            .await
            .expect("confirm_no_parts");

        seed_rams_for_complete(&db, wo_id).await;

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

        assert_eq!(closed_wo.status_code.as_deref(), Some("closed"));

        // Attempt reopen_wo from closed — must fail because closed is terminal.
        // reopen_wo only works from completed in Option B.
        let result = closeout::reopen_wo(
            &db,
            WoReopenInput {
                wo_id,
                actor_id: actor,
                expected_row_version: closed_wo.row_version,
                reason: "Need to add more data".into(),
                target_status: None,
            },
        )
        .await;

        assert!(result.is_err(), "reopen_wo must fail from closed state (terminal)");
        match result.unwrap_err() {
            AppError::ValidationFailed(errors) => {
                let joined = errors.join(" | ");
                assert!(
                    joined.to_lowercase().contains("closed")
                        || joined.to_lowercase().contains("statut")
                        || joined.to_lowercase().contains("status")
                        || joined.to_lowercase().contains("transition")
                        || joined.to_lowercase().contains("completed"),
                    "Error must mention closed/status/transition: {joined}"
                );
            }
            other => panic!("Expected ValidationFailed, got: {other:?}"),
        }
    }
}
