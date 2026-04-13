//! Consolidated WO test suite (14 tests) for Phase 2 SP05 File 04 Sprint S2.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::di::queries::{create_intervention_request, DiCreateInput};
    use crate::errors::AppError;
    use crate::wo::analytics;
    use crate::wo::audit;
    use crate::wo::closeout::{self, SaveFailureDetailInput, SaveVerificationInput, UpdateWoRcaInput, WoCloseInput, WoReopenInput};
    use crate::wo::costs;
    use crate::wo::delay::{self, OpenDowntimeInput};
    use crate::wo::domain::{guard_wo_transition, WoCreateInput, WoStatus};
    use crate::wo::execution::{self, WoAssignInput, WoMechCompleteInput, WoPauseInput, WoPlanInput, WoResumeInput, WoStartInput};
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts::{self, AddPartInput};
    use crate::wo::queries;
    use crate::wo::tasks::{self, AddTaskInput};

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
            .expect("query")
            .expect("admin user must exist");
        row.try_get::<i64>("", "id").expect("decode admin id")
    }

    async fn create_verifier(db: &sea_orm::DatabaseConnection, username: &str) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        let sync_id = format!("{username}-sync");
        let display_name = format!("{username} display");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts \
             (sync_id, username, display_name, identity_mode, password_hash, \
              is_active, is_admin, force_password_change, failed_login_attempts, \
              created_at, updated_at, row_version) \
             VALUES (?, ?, ?, 'local', 'no-login-needed', 1, 0, 0, 0, ?, ?, 1)",
            [
                sync_id.into(),
                username.to_string().into(),
                display_name.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert verifier user");

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = ? LIMIT 1",
                [username.to_string().into()],
            ))
            .await
            .expect("query verifier")
            .expect("verifier should exist");
        row.try_get::<i64>("", "id").expect("decode verifier id")
    }

    async fn create_wo_draft(db: &sea_orm::DatabaseConnection, title: &str) -> crate::wo::domain::WorkOrder {
        let actor = admin_id(db).await;
        queries::create_work_order(
            db,
            WoCreateInput {
                type_id: 1,
                equipment_id: None,
                location_id: None,
                source_di_id: None,
                entity_id: None,
                planner_id: None,
                urgency_id: Some(3),
                title: title.to_string(),
                description: Some("test WO".into()),
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: Some(8.0),
                creator_id: actor,
            },
        )
        .await
        .expect("create_work_order")
    }

    async fn transition_planned_assigned_in_progress(
        db: &sea_orm::DatabaseConnection,
        wo_id: i64,
        start_rv: i64,
        actor: i64,
    ) -> crate::wo::domain::WorkOrder {
        let wo = execution::plan_wo(
            db,
            WoPlanInput {
                wo_id,
                actor_id: actor,
                expected_row_version: start_rv,
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

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
             status_id = (SELECT id FROM work_order_statuses WHERE code = 'ready_to_schedule'), \
             row_version = row_version + 1, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
             WHERE id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("advance to ready_to_schedule");

        let wo = execution::assign_wo(
            db,
            WoAssignInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version + 1,
                assigned_group_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
            },
        )
        .await
        .expect("assign_wo");

        execution::start_wo(
            db,
            WoStartInput {
                wo_id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("start_wo")
    }

    async fn seed_di_fk_data(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO equipment \
             (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'test-eq-001', 'EQ-TEST-001', 'Test Equipment', 'active_in_service', \
                     datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert equipment");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_structure_models \
             (id, sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert structure model");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_node_types \
             (id, sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert node type");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_nodes \
             (id, sync_id, code, name, node_type_id, status, created_at, updated_at) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert org node");
    }

    fn approx_eq(a: f64, b: f64, tolerance: f64) {
        let delta = (a - b).abs();
        assert!(
            delta <= tolerance,
            "expected {a} ~= {b} within {tolerance}, delta={delta}"
        );
    }

    #[tokio::test]
    async fn test_wo_01_all_valid_transitions() {
        let states = [
            WoStatus::Draft,
            WoStatus::AwaitingApproval,
            WoStatus::Planned,
            WoStatus::ReadyToSchedule,
            WoStatus::Assigned,
            WoStatus::WaitingForPrerequisite,
            WoStatus::InProgress,
            WoStatus::Paused,
            WoStatus::MechanicallyComplete,
            WoStatus::TechnicallyVerified,
            WoStatus::Closed,
            WoStatus::Cancelled,
        ];

        for from in states {
            for to in from.allowed_transitions() {
                let res = guard_wo_transition(&from, to);
                assert!(
                    res.is_ok(),
                    "expected valid transition {} -> {}, got {res:?}",
                    from.as_str(),
                    to.as_str()
                );
            }
        }
    }

    #[tokio::test]
    async fn test_wo_02_invalid_transitions() {
        let invalid = [
            (WoStatus::Draft, WoStatus::InProgress),
            (WoStatus::Draft, WoStatus::Closed),
            (WoStatus::Planned, WoStatus::InProgress),
            (WoStatus::Closed, WoStatus::Draft),
            (WoStatus::Cancelled, WoStatus::Draft),
            (WoStatus::TechnicallyVerified, WoStatus::InProgress),
        ];

        for (from, to) in invalid {
            let res = guard_wo_transition(&from, &to);
            assert!(
                res.is_err(),
                "expected invalid transition {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }

    #[tokio::test]
    async fn test_wo_03_terminal_states() {
        assert!(WoStatus::Closed.is_terminal());
        assert!(WoStatus::Cancelled.is_terminal());

        for s in [
            WoStatus::Draft,
            WoStatus::AwaitingApproval,
            WoStatus::Planned,
            WoStatus::ReadyToSchedule,
            WoStatus::Assigned,
            WoStatus::WaitingForPrerequisite,
            WoStatus::InProgress,
            WoStatus::Paused,
            WoStatus::MechanicallyComplete,
            WoStatus::TechnicallyVerified,
        ] {
            assert!(!s.is_terminal(), "{s:?} must not be terminal");
        }
    }

    #[tokio::test]
    async fn test_wo_04_cancelled_reachability() {
        let from_states = [
            WoStatus::Draft,
            WoStatus::AwaitingApproval,
            WoStatus::Planned,
            WoStatus::ReadyToSchedule,
            WoStatus::Assigned,
            WoStatus::WaitingForPrerequisite,
            WoStatus::InProgress,
            WoStatus::Paused,
            WoStatus::MechanicallyComplete,
        ];

        for from in from_states {
            assert!(
                from.allowed_transitions().contains(&WoStatus::Cancelled),
                "cancelled must be reachable from {:?}",
                from
            );
        }
    }

    #[tokio::test]
    async fn test_wo_05_wo_code_generation() {
        let db = setup().await;

        let wo1 = create_wo_draft(&db, "WO-1").await;
        let wo2 = create_wo_draft(&db, "WO-2").await;
        let wo3 = create_wo_draft(&db, "WO-3").await;

        assert_eq!(wo1.code, "WOR-0001");
        assert_eq!(wo2.code, "WOR-0002");
        assert_eq!(wo3.code, "WOR-0003");
    }

    #[tokio::test]
    async fn test_wo_06_plan_requires_dates() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let wo = create_wo_draft(&db, "Plan invalid dates").await;

        let res = execution::plan_wo(
            &db,
            WoPlanInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                planner_id: actor,
                planned_start: "2026-04-10T16:00:00Z".into(),
                planned_end: "2026-04-10T08:00:00Z".into(),
                shift: None,
                expected_duration_hours: Some(8.0),
                urgency_id: None,
            },
        )
        .await;

        let errs = match res {
            Err(AppError::ValidationFailed(e)) => e,
            other => panic!("expected ValidationFailed, got {other:?}"),
        };
        let joined = errs.join(" | ").to_lowercase();
        assert!(joined.contains("planned_end"));
    }

    #[tokio::test]
    async fn test_wo_07_assign_requires_assignee() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let wo = create_wo_draft(&db, "Assign without assignee").await;

        let wo = execution::plan_wo(
            &db,
            WoPlanInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
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

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'ready_to_schedule'), row_version = row_version + 1 WHERE id = ?",
            [wo.id.into()],
        ))
        .await
        .expect("move ready_to_schedule");

        let res = execution::assign_wo(
            &db,
            WoAssignInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version + 1,
                assigned_group_id: None,
                primary_responsible_id: None,
                scheduled_at: None,
            },
        )
        .await;

        let errs = match res {
            Err(AppError::ValidationFailed(e)) => e,
            other => panic!("expected ValidationFailed, got {other:?}"),
        };
        let joined = errs.join(" | ").to_lowercase();
        assert!(joined.contains("assigned_group_id") || joined.contains("au moins un"));
    }

    #[tokio::test]
    async fn test_wo_08_pause_requires_delay_reason() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let wo = create_wo_draft(&db, "Pause invalid reason").await;
        let wo = transition_planned_assigned_in_progress(&db, wo.id, wo.row_version, actor).await;

        let res = execution::pause_wo(
            &db,
            WoPauseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                delay_reason_id: 999_999,
                comment: Some("invalid reason".into()),
            },
        )
        .await;

        let errs = match res {
            Err(AppError::ValidationFailed(e)) => e,
            other => panic!("expected ValidationFailed, got {other:?}"),
        };
        let joined = errs.join(" | ").to_lowercase();
        assert!(joined.contains("delay_reason") || joined.contains("délai") || joined.contains("delai"));
    }

    #[tokio::test]
    async fn test_wo_09_close_all_gates() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_verifier(&db, "wo09verifier").await;
        let wo = create_wo_draft(&db, "Close all gates").await;
        let wo = transition_planned_assigned_in_progress(&db, wo.id, wo.row_version, actor).await;

        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id: wo.id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T10:00:00Z".into()),
                hours_worked: None,
                hourly_rate: Some(60.0),
                notes: None,
            },
        )
        .await
        .expect("add_labor");

        let part = parts::add_planned_part(
            &db,
            AddPartInput {
                wo_id: wo.id,
                article_id: None,
                article_ref: Some("P-001".into()),
                quantity_planned: 2.0,
                unit_cost: Some(45.0),
                notes: None,
            },
        )
        .await
        .expect("add_planned_part");

        parts::record_actual_usage(&db, part.id, 2.0, Some(45.0))
            .await
            .expect("record_actual_usage");

        let wo = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: Some("mechanical done".into()),
            },
        )
        .await
        .expect("complete_wo_mechanically");

        closeout::save_failure_detail(
            &db,
            SaveFailureDetailInput {
                wo_id: wo.id,
                symptom_id: None,
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("failure mode captured".into()),
            },
        )
        .await
        .expect("save_failure_detail");

        closeout::update_wo_rca(
            &db,
            UpdateWoRcaInput {
                wo_id: wo.id,
                root_cause_summary: Some("Root cause summary".into()),
                corrective_action_summary: Some("Corrective action summary".into()),
            },
        )
        .await
        .expect("update_wo_rca");

        let (_v, wo) = closeout::save_verification(
            &db,
            SaveVerificationInput {
                wo_id: wo.id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("low".into()),
                notes: None,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("save_verification");

        let wo = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("close_wo");

        assert_eq!(wo.status_code.as_deref(), Some("closed"));
    }

    #[tokio::test]
    async fn test_wo_10_close_missing_failure_coding() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_verifier(&db, "wo10verifier").await;
        let wo = create_wo_draft(&db, "Missing failure coding").await;
        let wo = transition_planned_assigned_in_progress(&db, wo.id, wo.row_version, actor).await;

        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id: wo.id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T10:00:00Z".into()),
                hours_worked: None,
                hourly_rate: Some(60.0),
                notes: None,
            },
        )
        .await
        .expect("add_labor");

        parts::confirm_no_parts_used(&db, wo.id, actor)
            .await
            .expect("confirm_no_parts_used");

        let wo = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await
        .expect("complete_wo_mechanically");

        let (_v, wo) = closeout::save_verification(
            &db,
            SaveVerificationInput {
                wo_id: wo.id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("low".into()),
                notes: None,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("save_verification");

        let res = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await;

        let errs = match res {
            Err(AppError::ValidationFailed(e)) => e,
            other => panic!("expected ValidationFailed, got {other:?}"),
        };
        let joined = errs.join(" | ").to_lowercase();
        assert!(joined.contains("failure coding") || joined.contains("defaillance"));
    }

    #[tokio::test]
    async fn test_wo_11_close_missing_verification() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let wo = create_wo_draft(&db, "Missing verification").await;
        let wo = transition_planned_assigned_in_progress(&db, wo.id, wo.row_version, actor).await;

        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id: wo.id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: Some("2026-04-10T10:00:00Z".into()),
                hours_worked: None,
                hourly_rate: Some(60.0),
                notes: None,
            },
        )
        .await
        .expect("add_labor");

        parts::confirm_no_parts_used(&db, wo.id, actor)
            .await
            .expect("confirm_no_parts");

        let wo = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await
        .expect("complete_wo_mechanically");

        closeout::save_failure_detail(
            &db,
            SaveFailureDetailInput {
                wo_id: wo.id,
                symptom_id: None,
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("failure detail saved".into()),
            },
        )
        .await
        .expect("save_failure_detail");

        closeout::update_wo_rca(
            &db,
            UpdateWoRcaInput {
                wo_id: wo.id,
                root_cause_summary: Some("Root cause".into()),
                corrective_action_summary: Some("Action".into()),
            },
        )
        .await
        .expect("update rca");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
             status_id = (SELECT id FROM work_order_statuses WHERE code = 'technically_verified'), \
             technically_verified_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
             row_version = row_version + 1 \
             WHERE id = ?",
            [wo.id.into()],
        ))
        .await
        .expect("force technically_verified");

        let res = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version + 1,
            },
        )
        .await;

        let errs = match res {
            Err(AppError::ValidationFailed(e)) => e,
            other => panic!("expected ValidationFailed, got {other:?}"),
        };
        let joined = errs.join(" | ").to_lowercase();
        assert!(joined.contains("technical verification") || joined.contains("verification technique"));
    }

    #[tokio::test]
    async fn test_wo_12_cost_roll_up() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_verifier(&db, "wo12verifier").await;
        let wo = create_wo_draft(&db, "Cost roll-up").await;
        let wo = transition_planned_assigned_in_progress(&db, wo.id, wo.row_version, actor).await;

        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id: wo.id,
                intervener_id: actor,
                skill_id: None,
                started_at: None,
                ended_at: None,
                hours_worked: Some(3.0),
                hourly_rate: Some(60.0),
                notes: None,
            },
        )
        .await
        .expect("labor1");

        labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id: wo.id,
                intervener_id: actor,
                skill_id: None,
                started_at: None,
                ended_at: None,
                hours_worked: Some(2.0),
                hourly_rate: Some(50.0),
                notes: None,
            },
        )
        .await
        .expect("labor2");

        let part = parts::add_planned_part(
            &db,
            AddPartInput {
                wo_id: wo.id,
                article_id: None,
                article_ref: Some("P-ROLLUP".into()),
                quantity_planned: 2.0,
                unit_cost: Some(45.0),
                notes: None,
            },
        )
        .await
        .expect("add part");

        parts::record_actual_usage(&db, part.id, 2.0, Some(45.0))
            .await
            .expect("record usage");

        costs::update_service_cost(&db, wo.id, 30.0, actor)
            .await
            .expect("update service cost");

        let wo = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                actual_end: None,
                actual_duration_hours: None,
                conclusion: None,
            },
        )
        .await
        .expect("complete mech");

        closeout::save_failure_detail(
            &db,
            SaveFailureDetailInput {
                wo_id: wo.id,
                symptom_id: None,
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("cost closeout".into()),
            },
        )
        .await
        .expect("save failure detail");

        closeout::update_wo_rca(
            &db,
            UpdateWoRcaInput {
                wo_id: wo.id,
                root_cause_summary: Some("root cause".into()),
                corrective_action_summary: Some("corrective action".into()),
            },
        )
        .await
        .expect("update rca");

        let (_ver, wo) = closeout::save_verification(
            &db,
            SaveVerificationInput {
                wo_id: wo.id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("low".into()),
                notes: None,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("save verification");

        closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("close_wo");

        let summary = costs::get_cost_summary(&db, wo.id)
            .await
            .expect("get_cost_summary");

        approx_eq(summary.labor_cost, 280.0, 0.01);
        approx_eq(summary.parts_cost, 90.0, 0.01);
        approx_eq(summary.service_cost, 30.0, 0.01);
        approx_eq(summary.total_cost, 400.0, 0.01);
    }

    #[tokio::test]
    async fn test_wo_13_optimistic_lock_on_plan() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let wo = create_wo_draft(&db, "Optimistic lock plan").await;

        let res = execution::plan_wo(
            &db,
            WoPlanInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: 0,
                planner_id: actor,
                planned_start: "2026-04-10T08:00:00Z".into(),
                planned_end: "2026-04-10T16:00:00Z".into(),
                shift: None,
                expected_duration_hours: Some(8.0),
                urgency_id: None,
            },
        )
        .await;

        let errs = match res {
            Err(AppError::ValidationFailed(e)) => e,
            other => panic!("expected ValidationFailed, got {other:?}"),
        };
        let joined = errs.join(" | ").to_lowercase();
        assert!(joined.contains("conflit de version") || joined.contains("version"));
    }

    #[tokio::test]
    async fn test_wo_14_full_wo_lifecycle() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_verifier(&db, "wo14verifier").await;

        // Phase A - Create WO from DI mock
        seed_di_fk_data(&db).await;
        let di = create_intervention_request(
            &db,
            DiCreateInput {
                asset_id: 1,
                org_node_id: 1,
                title: "Mock DI for WO lifecycle".into(),
                description: "DI created for test_wo_14".into(),
                origin_type: "operator".into(),
                symptom_code_id: None,
                impact_level: "major".into(),
                production_impact: true,
                safety_flag: false,
                environmental_flag: false,
                quality_flag: false,
                reported_urgency: "high".into(),
                observed_at: Some("2026-04-10T07:30:00Z".into()),
                submitter_id: actor,
            },
        )
        .await
        .expect("create_intervention_request");

        let mut wo = queries::create_work_order(
            &db,
            WoCreateInput {
                type_id: 1,
                equipment_id: Some(1),
                location_id: None,
                source_di_id: Some(di.id),
                entity_id: Some(1),
                planner_id: None,
                urgency_id: Some(4),
                title: "WO from DI".into(),
                description: Some("Full lifecycle".into()),
                notes: None,
                planned_start: None,
                planned_end: None,
                shift: None,
                expected_duration_hours: Some(8.0),
                creator_id: actor,
            },
        )
        .await
        .expect("create_work_order from DI");

        assert_eq!(wo.status_code.as_deref(), Some("draft"));
        assert_eq!(wo.code, "WOR-0001");
        assert_eq!(wo.source_di_id, Some(di.id));
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "created".into(),
                actor_id: Some(actor),
                summary: Some("created".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        // Phase B - Plan
        wo = execution::plan_wo(
            &db,
            WoPlanInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
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
        assert_eq!(wo.status_code.as_deref(), Some("planned"));
        assert!(wo.planned_start.is_some());
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "planned".into(),
                actor_id: Some(actor),
                summary: Some("planned".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        // Phase C - Assign (planned -> ready_to_schedule -> assigned)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET status_id = (SELECT id FROM work_order_statuses WHERE code = 'ready_to_schedule'), row_version = row_version + 1 WHERE id = ?",
            [wo.id.into()],
        ))
        .await
        .expect("advance to ready_to_schedule");

        // Add mandatory task while assigned/planning states are still valid for task insertion.
        let wo_assigned = execution::assign_wo(
            &db,
            WoAssignInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version + 1,
                assigned_group_id: None,
                primary_responsible_id: Some(actor),
                scheduled_at: None,
            },
        )
        .await
        .expect("assign_wo");
        assert_eq!(wo_assigned.status_code.as_deref(), Some("assigned"));
        assert_eq!(wo_assigned.primary_responsible_id, Some(actor));
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "assigned".into(),
                actor_id: Some(actor),
                summary: Some("assigned".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        let task = tasks::add_task(
            &db,
            AddTaskInput {
                wo_id: wo.id,
                task_description: "Mandatory execution checklist".into(),
                sequence_order: 1,
                is_mandatory: true,
                estimated_minutes: Some(30),
            },
        )
        .await
        .expect("add_task mandatory");

        // Phase D - Execute
        let mut wo = execution::start_wo(
            &db,
            WoStartInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo_assigned.row_version,
            },
        )
        .await
        .expect("start_wo");
        assert_eq!(wo.status_code.as_deref(), Some("in_progress"));
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "started".into(),
                actor_id: Some(actor),
                summary: Some("started".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        let labor_row = labor::add_labor_entry(
            &db,
            AddLaborInput {
                wo_id: wo.id,
                intervener_id: actor,
                skill_id: None,
                started_at: Some("2026-04-10T08:00:00Z".into()),
                ended_at: None,
                hours_worked: None,
                hourly_rate: Some(60.0),
                notes: None,
            },
        )
        .await
        .expect("add_labor in progress");

        let part = parts::add_planned_part(
            &db,
            AddPartInput {
                wo_id: wo.id,
                article_id: None,
                article_ref: Some("FULL-14".into()),
                quantity_planned: 2.0,
                unit_cost: Some(45.0),
                notes: None,
            },
        )
        .await
        .expect("add_part");

        tasks::complete_task(&db, task.id, actor, "ok".into(), Some("done".into()))
            .await
            .expect("complete_task");

        let dt = delay::open_downtime_segment(
            &db,
            OpenDowntimeInput {
                wo_id: wo.id,
                downtime_type: "partial".into(),
                comment: Some("brief stop".into()),
                actor_id: actor,
            },
        )
        .await
        .expect("open_downtime");
        delay::close_downtime_segment(&db, dt.id, Some("2026-04-10T10:00:00Z".into()))
            .await
            .expect("close_downtime");

        wo = execution::pause_wo(
            &db,
            WoPauseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                delay_reason_id: 1,
                comment: Some("waiting part".into()),
            },
        )
        .await
        .expect("pause_wo");
        assert_eq!(wo.status_code.as_deref(), Some("paused"));
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "paused".into(),
                actor_id: Some(actor),
                summary: Some("paused".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_delay_segments SET started_at = '2026-04-10T09:00:00Z' WHERE work_order_id = ? AND ended_at IS NULL",
            [wo.id.into()],
        ))
        .await
        .expect("backdate open delay segment");

        wo = execution::resume_wo(
            &db,
            WoResumeInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("resume_wo");
        assert_eq!(wo.status_code.as_deref(), Some("in_progress"));
        assert!(wo.total_waiting_hours.unwrap_or(0.0) > 0.0);
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "resumed".into(),
                actor_id: Some(actor),
                summary: Some("resumed".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        // Phase E - Mechanical completion
        parts::record_actual_usage(&db, part.id, 2.0, Some(45.0))
            .await
            .expect("record_part_usage");

        labor::close_labor_entry(&db, labor_row.id, "2026-04-10T10:00:00Z".into(), actor)
            .await
            .expect("close_labor");

        let wo = execution::complete_wo_mechanically(
            &db,
            WoMechCompleteInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                actual_end: Some("2026-04-10T10:00:00Z".into()),
                actual_duration_hours: Some(2.0),
                conclusion: Some("mechanical complete".into()),
            },
        )
        .await
        .expect("complete_wo_mechanically");
        assert!(wo.mechanically_completed_at.is_some());
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "mechanically_completed".into(),
                actor_id: Some(actor),
                summary: Some("mech complete".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        // Phase F - Verification and close
        closeout::save_failure_detail(
            &db,
            SaveFailureDetailInput {
                wo_id: wo.id,
                symptom_id: None,
                failure_mode_id: None,
                failure_cause_id: None,
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: true,
                notes: Some("failure detail".into()),
            },
        )
        .await
        .expect("save_failure_detail");

        let (_ver, wo) = closeout::save_verification(
            &db,
            SaveVerificationInput {
                wo_id: wo.id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("low".into()),
                notes: Some("ok".into()),
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("save_verification");
        assert_eq!(wo.status_code.as_deref(), Some("technically_verified"));
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "verification_saved".into(),
                actor_id: Some(actor),
                summary: Some("verified".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        closeout::update_wo_rca(
            &db,
            UpdateWoRcaInput {
                wo_id: wo.id,
                root_cause_summary: Some("Root cause summary lifecycle".into()),
                corrective_action_summary: Some("Corrective action lifecycle".into()),
            },
        )
        .await
        .expect("update rca");

        let mut wo = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("close_wo");
        assert_eq!(wo.status_code.as_deref(), Some("closed"));
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "closed".into(),
                actor_id: Some(actor),
                summary: Some("closed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        // Phase G - Analytics snapshot
        let snap = analytics::get_wo_analytics_snapshot(&db, wo.id)
            .await
            .expect("get_wo_analytics_snapshot");
        assert_eq!(snap.failure_details.len(), 1);
        assert_eq!(snap.verifications.len(), 1);
        assert!(snap.total_cost > 0.0);
        assert!(snap.was_planned);
        assert_eq!(snap.reopen_count, 0);

        // Phase H - Reopen and re-close
        wo = closeout::reopen_wo(
            &db,
            WoReopenInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
                reason: "Need final adjustment".into(),
            },
        )
        .await
        .expect("reopen_wo");
        assert_eq!(wo.status_code.as_deref(), Some("technically_verified"));
        assert_eq!(wo.reopen_count, 1);
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "reopened".into(),
                actor_id: Some(actor),
                summary: Some("reopened".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        wo = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id: wo.id,
                actor_id: actor,
                expected_row_version: wo.row_version,
            },
        )
        .await
        .expect("re-close");
        assert_eq!(wo.status_code.as_deref(), Some("closed"));
        assert_eq!(wo.reopen_count, 1);
        audit::record_wo_change_event(
            &db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "closed".into(),
                actor_id: Some(actor),
                summary: Some("closed again".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        // Phase I - Audit trail
        let events = audit::list_wo_change_events(&db, wo.id, 100)
            .await
            .expect("list_wo_change_events");
        let actions: Vec<String> = events.iter().map(|e| e.action.clone()).collect();

        for expected in [
            "created",
            "planned",
            "assigned",
            "started",
            "paused",
            "resumed",
            "mechanically_completed",
            "verification_saved",
            "closed",
            "reopened",
        ] {
            assert!(
                actions.iter().any(|a| a == expected),
                "missing expected action '{expected}' in {actions:?}"
            );
        }

        let closed_count = actions.iter().filter(|a| a.as_str() == "closed").count();
        assert!(closed_count >= 2, "expected at least two 'closed' events");
        assert!(events.len() >= 11, "expected at least 11 events, got {}", events.len());
    }
}
