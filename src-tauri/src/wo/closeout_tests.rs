//! Supervisor verification tests for Phase 2 SP05 File 03 — Sprint S1. Option B lifecycle.
//!
//! V1 — Multi-gate failure list: close_wo on corrective WO with no labor, no parts,
//!       no failure detail must return 3+ blocking errors in one response.
//! V2 — Self-verification guard: save_verification with verified_by_id =
//!       primary_responsible_id must fail.
//! V3 — Cost roll-up: labor 220 + parts 100 + service 50 = total 370.
//! V4 — Reopen from closed: allowed within the tenant reopen window (PRD §6.5), keeping the
//!      verification evidence; refused once the window has passed.

#[cfg(test)]
pub(crate) mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::errors::AppError;
    use crate::wo::closeout::{
        self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput, WoReopenInput,
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

    pub(crate) async fn setup() -> sea_orm::DatabaseConnection {
        crate::db::test_db::seeded().await
    }

    pub(crate) async fn admin_id(db: &sea_orm::DatabaseConnection) -> i64 {
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

    pub(crate) async fn create_second_user(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO user_accounts \
             (entity_sync_id, username, display_name, identity_mode, password_hash, \
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

    pub(crate) async fn seed_test_equipment(db: &sea_orm::DatabaseConnection) {
        // Ensure org scaffolding for installed_at_node_id FK
        let _ = db
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO org_structure_models \
                 (id, entity_sync_id, version_number, status, created_at, updated_at) \
                 VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));"
                    .to_string(),
            ))
            .await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_node_types \
             (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert org_node_types");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_nodes \
             (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
                     datetime('now'), datetime('now'), 1);"
                .to_string(),
        ))
        .await
        .expect("insert org_nodes");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO equipment \
             (id, entity_sync_id, asset_id_code, name, lifecycle_status, installed_at_node_id, \
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
    pub(crate) async fn seed_rams_for_complete(db: &sea_orm::DatabaseConnection, wo_id: i64) -> i64 {
        let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("lookup symptom")
            .expect("seeded DI.SYMPTOM vibration");

        closeout::save_failure_detail(
            db,
            SaveFailureDetailInput {
                wo_id,
                expected_row_version: crate::wo::closeout::wo_row_version(db, wo_id)
                    .await
                    .expect("wo row_version"),
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
                expected_row_version: crate::wo::closeout::wo_row_version(db, wo_id)
                    .await
                    .expect("wo row_version"),
                root_cause_summary: Some("Test root cause".into()),
                corrective_action_summary: Some("Test corrective action".into()),
            },
        )
        .await
        .expect("seed_rams_for_complete: update_wo_rca")
    }

    /// Strip RAMS fields so close_wo quality gates can still assert missing failure coding.
    pub(crate) async fn clear_rams_for_close_gate(db: &sea_orm::DatabaseConnection, wo_id: i64) {
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
    pub(crate) async fn create_wo_in_progress(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
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
                urgency_code: Some("normal".into()),
                subtype_code: None,
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
                planned_start: Some("2026-04-10T08:00:00Z".to_string()),
                planned_end: Some("2026-04-10T16:00:00Z".to_string()),
                shift: None,
                shift_class_version_id: None,
                clear_shift: false,
                expected_duration_hours: Some(8.0),
                planned_crew_size: None,
                clear_crew_size: false,
                planned_downtime_hours: None,
                urgency_code: None,
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
                assigned_team_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
                override_reason: "test assign".to_string(),
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
    pub(crate) async fn advance_to_completed_verified(db: &sea_orm::DatabaseConnection, wo_id: i64, rv: i64) -> i64 {
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

        let rv = seed_rams_for_complete(db, wo_id).await;

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

        let rv = seed_rams_for_complete(&db, wo_id).await;

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
                    joined.contains("Failure")
                        || joined.contains("defaillance")
                        || joined.contains("Root")
                        || joined.contains("cause racine")
                        || joined.contains("verification")
                        || joined.contains("Verification"),
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

        let rv = seed_rams_for_complete(&db, wo_id).await;

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
                is_required: true,
            },
        )
        .await
        .expect("add planned part");

        parts::record_actual_usage(&db, part.id, 5.0, Some(20.0))
            .await
            .expect("record actual usage");

        costs::update_service_cost(
            &db,
            wo_id,
            50.0,
            crate::wo::closeout::wo_row_version(&db, wo_id)
                .await
                .expect("wo row_version"),
            actor,
        )
        .await
        .expect("update_service_cost");

        let rv = seed_rams_for_complete(&db, wo_id).await;

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

        let summary = costs::get_cost_summary(&db, wo_id).await.expect("get_cost_summary");
        assert!(
            (summary.total_cost - 370.0).abs() < 0.01,
            "cost summary total expected 370, got {}",
            summary.total_cost
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 — Reopen from closed: inside the window only, evidence preserved
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v4_reopen_from_closed_respects_window_and_keeps_evidence() {
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

        let rv = seed_rams_for_complete(&db, wo_id).await;

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

        // Outside the window (closed 40 days ago, window 30): refused.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-40 days') WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("age closure");
        let reopen = |rv: i64| WoReopenInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            reason: "Leak came back".into(),
            target_status: None,
            estimated_remaining_hours: None,
        };
        match closeout::reopen_wo(&db, reopen(closed_wo.row_version)).await {
            Err(AppError::ValidationFailed(errors)) => assert!(
                errors.join(" ").contains("reopen window"),
                "expected the window message: {errors:?}"
            ),
            other => panic!("expected the window refusal, got {other:?}"),
        }

        // Inside the window: reopened to execution, verification evidence kept.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-2 days') WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("recent closure");

        // Reopening to execution restarts work: the permit gate applies. The OT requires a
        // permit and its only permit is closed → refused.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET requires_permit = 1 WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("require a permit");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_permits (entity_sync_id, code, linked_work_order_id, permit_type_id, asset_id, entity_id, status, row_version)
             SELECT 'reopen-closed-permit', 'PTW-REOPEN', wo.id, pt.id, COALESCE(wo.equipment_id, (SELECT MIN(id) FROM equipment)), COALESCE(wo.entity_id, 1), 'closed', 1
               FROM work_orders wo, permit_types pt WHERE wo.id = ? AND pt.code = 'hot_work'",
            [wo_id.into()],
        ))
        .await
        .expect("closed permit");
        let refused = closeout::reopen_wo(&db, reopen(closed_wo.row_version)).await;
        assert!(
            format!("{refused:?}").contains("PERMIT_REQUIRED_TO_START"),
            "reopen to execution without a live permit: {refused:?}"
        );
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET requires_permit = 0 WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("no permit required");

        let reopened = closeout::reopen_wo(&db, reopen(closed_wo.row_version))
            .await
            .expect("reopen within window");
        assert_eq!(reopened.status_code.as_deref(), Some("in_progress"));
        assert!(reopened.technically_verified_at.is_some(), "verification kept");
        assert!(reopened.mechanically_completed_at.is_some(), "completion evidence kept");
        assert!(reopened.closed_at.is_none());
        assert!(reopened.last_closed_at.is_some());
        assert_eq!(reopened.reopen_count, 1);

        // A missing setting is an error, never a guessed window.
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM app_settings WHERE setting_key = 'wo.reopen_window_days'".to_string(),
        ))
        .await
        .expect("drop setting");
        assert!(closeout::reopen_window_days(&db).await.is_err());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Override signer password: counted toward the signer's account lockout
    // ═══════════════════════════════════════════════════════════════════════

    const SIGNER_PASSWORD: &str = "Signer-Pass-2026!";

    async fn scalar(db: &sea_orm::DatabaseConnection, sql: &str, id: i64) -> i64 {
        db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [id.into()]))
            .await
            .expect("query")
            .expect("row")
            .try_get::<i64>("", "v")
            .expect("decode")
    }

    /// A completed, verified corrective WO whose failure mode has a critical FMECA item with a
    /// suggested part that was not used: closing it needs the supervisor override (reason +
    /// signer password). Returns (wo id, row version, signer id).
    async fn wo_needing_signer_override(db: &sea_orm::DatabaseConnection) -> (i64, i64, i64) {
        let (wo_id, rv) = create_wo_in_progress(db).await;
        let rv = advance_to_completed_verified(db, wo_id, rv).await;
        let now = chrono::Utc::now().to_rfc3339();
        let hash = crate::auth::password::hash_password(SIGNER_PASSWORD).expect("hash");
        // The failure mode and the suggested article are plain ids here (no catalogue rows).
        db.execute_unprepared("PRAGMA foreign_keys = OFF")
            .await
            .expect("fk off");
        for sql in [
            "UPDATE closeout_validation_policies SET require_fmeca_parts_for_critical = 1, \
             fmeca_parts_override_allowed_roles_json = '[\"Administrator\"]'"
                .to_string(),
            format!("UPDATE work_order_failure_details SET failure_mode_id = 9101 WHERE work_order_id = {wo_id}"),
            "INSERT INTO fmeca_analyses (entity_sync_id, equipment_id, title, created_at, updated_at) \
             VALUES ('fa-signer-test', 1, 'Pompe', datetime('now'), datetime('now'))"
                .to_string(),
            "INSERT INTO fmeca_items (entity_sync_id, analysis_id, failure_mode_id, severity, occurrence, \
             detectability, rpn, review_required, updated_at) \
             SELECT 'fi-signer-test', id, 9101, 10, 10, 10, 1000, 0, datetime('now') FROM fmeca_analyses \
             WHERE entity_sync_id = 'fa-signer-test'"
                .to_string(),
            "INSERT INTO fmeca_item_parts (fmeca_item_id, article_id) \
             SELECT id, 9202 FROM fmeca_items WHERE entity_sync_id = 'fi-signer-test'"
                .to_string(),
        ] {
            db.execute_unprepared(&sql).await.expect("fixture");
        }
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts \
             (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin, \
              force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version) \
             VALUES ('signer-sync', 'chef_equipe', 'local', ?, 1, 0, 0, ?, 0, ?, ?, 1)",
            [hash.into(), now.clone().into(), now.clone().into(), now.clone().into()],
        ))
        .await
        .expect("signer");
        let signer = scalar(
            db,
            "SELECT id AS v FROM user_accounts WHERE username = 'chef_equipe' AND id > ?",
            0,
        )
        .await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (entity_sync_id, user_id, role_id, scope_type, scope_reference, created_at, updated_at, row_version) \
             SELECT 'signer-assignment', ?, id, 'tenant', NULL, ?, ?, 1 FROM roles WHERE name = 'Administrator'",
            [signer.into(), now.clone().into(), now.into()],
        ))
        .await
        .expect("signer role");
        (wo_id, rv, signer)
    }

    async fn close_with_signer(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        rv: i64,
        actor: i64,
        signer: i64,
        password: &str,
    ) -> Result<(), Vec<String>> {
        match closeout::close_wo(
            db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                fmeca_parts_override_reason: Some("Piece suggeree indisponible, montage equivalent".into()),
                fmeca_parts_override_signed_by_id: Some(signer),
                fmeca_parts_override_signer_password: Some(password.into()),
                ..Default::default()
            },
        )
        .await
        {
            Ok(_) => Ok(()),
            Err(AppError::ValidationFailed(errors)) => Err(errors),
            Err(other) => panic!("unexpected close-out error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn wrong_override_signer_passwords_lock_the_signer_account() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv, signer) = wo_needing_signer_override(&db).await;
        let max = i64::from(crate::auth::lockout::LockoutPolicy::load(&db).await.max_attempts);
        let failed = "SELECT failed_login_attempts AS v FROM user_accounts WHERE id = ?";
        let locked = "SELECT COUNT(*) AS v FROM user_accounts WHERE id = ? AND locked_until IS NOT NULL";

        for attempt in 1..=max {
            let errors = close_with_signer(&db, wo_id, rv, actor, signer, "mauvais")
                .await
                .unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|e| e == "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNER_AUTH_FAILED"),
                "attempt {attempt}: {errors:?}"
            );
        }
        assert_eq!(
            scalar(&db, locked, signer).await,
            1,
            "the signer's account is locked out"
        );
        assert_eq!(scalar(&db, failed, actor).await, 0, "the user closing is not counted");
        // Each wrong signer password is audited: actor = the user closing, target = the signer.
        let audited = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS v FROM audit_events WHERE action_code = ? AND actor_id = ? \
                 AND target_type = 'user_account' AND target_id = ?",
                [
                    crate::audit::event_type::STEP_UP_FAILURE.into(),
                    actor.into(),
                    signer.to_string().into(),
                ],
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get::<i64>("", "v")
            .expect("decode");
        assert_eq!(audited, max);

        // Locked out: even the right password is refused, without being checked or counted.
        let attempts_before = scalar(&db, failed, signer).await;
        let errors = close_with_signer(&db, wo_id, rv, actor, signer, SIGNER_PASSWORD)
            .await
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e == "GATE_CLOSEOUT_FMECA_PARTS_OVERRIDE_SIGNER_LOCKED"),
            "{errors:?}"
        );
        assert_eq!(scalar(&db, failed, signer).await, attempts_before);

        // Once the lockout ends, the right password signs and the count starts afresh.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET locked_until = '2000-01-01T00:00:00Z' WHERE id = ?",
            [signer.into()],
        ))
        .await
        .expect("lockout over");
        if let Err(errors) = close_with_signer(&db, wo_id, rv, actor, signer, SIGNER_PASSWORD).await {
            assert!(
                // No code of the FMECA-parts gate (a prefix here, not a code: the frontend
                // gate-code scan reads quoted `GATE_…` literals).
                !errors.iter().any(|e| e.contains("CLOSEOUT_FMECA_PARTS")),
                "the override is accepted: {errors:?}"
            );
        }
        assert_eq!(scalar(&db, failed, signer).await, 0);
        assert_eq!(scalar(&db, locked, signer).await, 0);
    }

    #[tokio::test]
    async fn a_committed_close_stays_closed_when_the_signer_outcome_cannot_be_recorded() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv, signer) = wo_needing_signer_override(&db).await;
        // Earlier failures to reset, and a database that refuses the reset after the commit.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET failed_login_attempts = 2 WHERE id = ?",
            [signer.into()],
        ))
        .await
        .expect("failures");
        db.execute_unprepared(
            "CREATE TRIGGER block_signer_reset BEFORE UPDATE OF failed_login_attempts ON user_accounts \
             BEGIN SELECT RAISE(ABORT, 'blocked for the test'); END",
        )
        .await
        .expect("trigger");

        let closed = close_with_signer(&db, wo_id, rv, actor, signer, SIGNER_PASSWORD).await;
        assert!(
            closed.is_ok(),
            "the close is committed and reported as such: {closed:?}"
        );
        let wo = queries::get_work_order(&db, wo_id).await.expect("read").expect("wo");
        assert_eq!(wo.status_code.as_deref(), Some("closed"));
    }

    /// Self-signing allowed by the policy: the closer's own password is checked, so a lockout it
    /// causes ends the closer's session (ACCOUNT_LOCKED → sign-in), as every password check in a
    /// session does.
    #[tokio::test]
    async fn a_self_signed_override_locks_out_like_any_password_check_in_the_session() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv, _signer) = wo_needing_signer_override(&db).await;
        let (state, session_id) = self_signing_session(&db, actor).await;

        let max = i64::from(crate::auth::lockout::LockoutPolicy::load(&db).await.max_attempts);
        let close = |password: &'static str| self_signed_close(wo_id, rv, actor, password);
        for attempt in 1..max {
            let refused = crate::commands::wo::close_wo_impl(&state, close("mauvais"))
                .await
                .unwrap_err();
            assert!(
                matches!(refused, AppError::ValidationFailed(_)),
                "attempt {attempt}: {refused:?}"
            );
            assert!(state.session.read().await.current.is_some());
        }
        let refused = crate::commands::wo::close_wo_impl(&state, close("mauvais"))
            .await
            .unwrap_err();
        assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
        assert!(
            state.session.read().await.current.is_none(),
            "the closer's session ended"
        );
        assert_eq!(session_revoked(&db, &session_id).await, 1);
    }

    /// The signer password is checked and counted even when matched parts make the override
    /// unnecessary, so the close goes through: a lockout it causes still ends the session, and
    /// the committed close is still reported.
    #[tokio::test]
    async fn a_self_signed_lockout_ends_the_session_even_when_the_close_goes_through() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv, _signer) = wo_needing_signer_override(&db).await;
        let (state, session_id) = self_signing_session(&db, actor).await;
        let max = i64::from(crate::auth::lockout::LockoutPolicy::load(&db).await.max_attempts);
        for sql in [
            format!(
                "INSERT INTO work_order_parts (work_order_id, article_id, quantity_planned, quantity_used, \
                 unit_cost, is_required) VALUES ({wo_id}, 9202, 1.0, 1.0, 0, 1)"
            ),
            format!(
                "UPDATE user_accounts SET failed_login_attempts = {} WHERE id = {actor}",
                max - 1
            ),
        ] {
            db.execute_unprepared(&sql).await.expect("fixture");
        }

        let closed = crate::commands::wo::close_wo_impl(&state, self_signed_close(wo_id, rv, actor, "mauvais")).await;
        assert!(closed.is_ok(), "matched parts: the close goes through: {closed:?}");
        assert!(
            state.session.read().await.current.is_none(),
            "the closer's account is locked: the session ended"
        );
        assert_eq!(session_revoked(&db, &session_id).await, 1);
    }

    fn self_signed_close(wo_id: i64, rv: i64, actor: i64, password: &str) -> WoCloseInput {
        WoCloseInput {
            wo_id,
            actor_id: actor,
            expected_row_version: rv,
            fmeca_parts_override_reason: Some("Piece suggeree indisponible, montage equivalent".into()),
            fmeca_parts_override_signed_by_id: Some(actor),
            fmeca_parts_override_signer_password: Some(password.into()),
            ..Default::default()
        }
    }

    async fn session_revoked(db: &sea_orm::DatabaseConnection, session_id: &str) -> i64 {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
            [session_id.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "v")
        .expect("decode")
    }

    /// Self-signing allowed by the policy, and `actor` signed in with a stored session.
    async fn self_signing_session(db: &sea_orm::DatabaseConnection, actor: i64) -> (crate::state::AppState, String) {
        db.execute_unprepared(
            "UPDATE closeout_validation_policies SET fmeca_parts_override_require_distinct_signer = 0",
        )
        .await
        .expect("self-signing allowed");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET force_password_change = 0 WHERE id = ?",
            [actor.into()],
        ))
        .await
        .expect("password chosen");

        let state = crate::state::AppState::new(db.clone());
        let session_id = {
            let mut guard = state.session.write().await;
            guard.create_session(crate::auth::session_manager::AuthenticatedUser {
                user_id: i32::try_from(actor).expect("fits"),
                username: "admin".into(),
                display_name: None,
                is_admin: true,
                force_password_change: false,
                tenant_id: "tenant-test".into(),
                token_tenant_id: "tenant-test".into(),
            });
            let current = guard.current.as_ref().expect("session");
            (current.session_db_id.clone(), current.expires_at.to_rfc3339())
        };
        crate::auth::session_manager::create_session_record(
            db,
            &session_id.0,
            i32::try_from(actor).expect("fits"),
            &session_id.1,
        )
        .await
        .expect("stored session");
        (state, session_id.0)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AU-18 — close-out coding, root cause and service cost are version-checked
    // ═══════════════════════════════════════════════════════════════════════

    fn assert_changed_elsewhere(err: AppError) {
        match err {
            AppError::ValidationFailed(lines) => assert!(
                lines.iter().any(|l| l.starts_with("GATE_RECORD_CHANGED_ELSEWHERE")),
                "expected the stale-version refusal, got {lines:?}"
            ),
            other => panic!("expected the stale-version refusal, got {other:?}"),
        }
    }

    fn failure_input(wo_id: i64, expected_row_version: i64, notes: &str) -> SaveFailureDetailInput {
        SaveFailureDetailInput {
            wo_id,
            expected_row_version,
            symptom_id: None,
            failure_mode_id: None,
            failure_cause_id: None,
            failure_effect_id: None,
            is_temporary_repair: false,
            is_permanent_repair: true,
            cause_not_determined: true,
            notes: Some(notes.into()),
        }
    }

    #[tokio::test]
    async fn au18_failure_detail_saved_on_a_stale_version_is_refused_and_writes_nothing() {
        let db = setup().await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;

        // First person saves at the version both screens loaded.
        let saved = closeout::save_failure_detail(&db, failure_input(wo_id, rv, "first"))
            .await
            .expect("first save");
        assert_eq!(saved.wo_row_version, rv + 1);
        assert_eq!(closeout::wo_row_version(&db, wo_id).await.expect("rv"), rv + 1);

        // Second person still holds the old version: refused, the first coding is kept.
        let err = closeout::save_failure_detail(&db, failure_input(wo_id, rv, "second"))
            .await
            .expect_err("stale save must be refused");
        assert_changed_elsewhere(err);
        let details = closeout::get_failure_details(&db, wo_id).await.expect("details");
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].notes.as_deref(), Some("first"));
        assert_eq!(closeout::wo_row_version(&db, wo_id).await.expect("rv"), rv + 1);

        // The coding path (mechanism / detection) is refused the same way, in one transaction.
        let err = closeout::save_failure_coding(
            &db,
            closeout::SaveFailureCodingInput {
                base: failure_input(wo_id, rv, "third"),
                failure_mechanism_id: None,
                detection_method_id: None,
            },
        )
        .await
        .expect_err("stale coding must be refused");
        assert_changed_elsewhere(err);
        let details = closeout::get_failure_details(&db, wo_id).await.expect("details");
        assert_eq!(details[0].notes.as_deref(), Some("first"));

        // Reloaded version: accepted and bumped again.
        let saved = closeout::save_failure_coding(
            &db,
            closeout::SaveFailureCodingInput {
                base: failure_input(wo_id, rv + 1, "after reload"),
                failure_mechanism_id: None,
                detection_method_id: None,
            },
        )
        .await
        .expect("save after reload");
        assert_eq!(saved.wo_row_version, rv + 2);
        assert_eq!(saved.detail.notes.as_deref(), Some("after reload"));
    }

    #[tokio::test]
    async fn au18_root_cause_saved_on_a_stale_version_is_refused() {
        let db = setup().await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;

        let new_rv = closeout::update_wo_rca(
            &db,
            UpdateWoRcaInput {
                wo_id,
                expected_row_version: rv,
                root_cause_summary: Some("first cause".into()),
                corrective_action_summary: Some("first action".into()),
            },
        )
        .await
        .expect("first rca");
        assert_eq!(new_rv, rv + 1);

        let err = closeout::update_wo_rca(
            &db,
            UpdateWoRcaInput {
                wo_id,
                expected_row_version: rv,
                root_cause_summary: Some("second cause".into()),
                corrective_action_summary: None,
            },
        )
        .await
        .expect_err("stale rca must be refused");
        assert_changed_elsewhere(err);

        let wo = queries::get_work_order(&db, wo_id).await.expect("get").expect("wo");
        assert_eq!(wo.root_cause_summary.as_deref(), Some("first cause"));
        assert_eq!(wo.row_version, rv + 1);
    }

    #[tokio::test]
    async fn au18_service_cost_saved_on_a_stale_version_is_refused() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;

        let new_rv = costs::update_service_cost(&db, wo_id, 120.0, rv, actor)
            .await
            .expect("first cost");
        assert_eq!(new_rv, rv + 1);

        let err = costs::update_service_cost(&db, wo_id, 999.0, rv, actor)
            .await
            .expect_err("stale cost must be refused");
        assert_changed_elsewhere(err);

        let cost = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT service_cost_input, row_version FROM work_orders WHERE id = ?",
                [wo_id.into()],
            ))
            .await
            .expect("query")
            .expect("row");
        assert_eq!(cost.try_get::<f64>("", "service_cost_input").expect("cost"), 120.0);
        assert_eq!(cost.try_get::<i64>("", "row_version").expect("rv"), rv + 1);
    }
}
