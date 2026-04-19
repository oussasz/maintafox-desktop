//! Gap 06 sprint 03 — regression tests for close-out, integrity detectors, analytics contract.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::analytics_contract::list_contract_versions;
    use crate::data_integrity::detectors::run_data_integrity_detectors;
    use crate::errors::AppError;
    use crate::wo::closeout::{self, SaveFailureDetailInput, SaveVerificationInput, WoCloseInput};
    use crate::wo::costs;
    use crate::wo::domain::WoCreateInput;
    use crate::wo::execution::{
        self, WoAssignInput, WoMechCompleteInput, WoPlanInput, WoStartInput,
    };
    use crate::wo::labor::{self, AddLaborInput};
    use crate::wo::parts::{self, AddPartInput};
    use crate::wo::queries;

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
             VALUES ('gap06-verifier-sync', 'gap06verifier', 'Gap06 Verifier', 'local', \
                     'no-login-needed', 1, 0, 0, 0, ?, ?, 1)",
            [now.clone().into(), now.into()],
        ))
        .await
        .expect("insert second user");

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'gap06verifier' LIMIT 1".to_string(),
            ))
            .await
            .expect("query")
            .expect("verifier user should exist");
        row.try_get::<i64>("", "id").unwrap()
    }

    async fn create_wo_in_progress(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let actor = admin_id(db).await;

        let wo = queries::create_work_order(
            db,
            WoCreateInput {
                type_id: 1,
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
                title: "Gap06 regression WO".into(),
                description: Some("regression".into()),
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

    async fn advance_to_technically_verified(
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
                notes: Some("Test failure detail notes for cnd path".into()),
            },
        )
        .await
        .expect("save_failure_detail");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = ? WHERE id = ?",
            ["Root cause text for gap06.".into(), wo_id.into()],
        ))
        .await
        .expect("set root_cause_summary");

        let (_ver, wo) = closeout::save_verification(
            db,
            SaveVerificationInput {
                wo_id,
                verified_by_id: verifier,
                result: "pass".into(),
                return_to_service_confirmed: true,
                recurrence_risk_level: Some("low".into()),
                notes: Some("ok".into()),
                expected_row_version: rv,
            },
        )
        .await
        .expect("save_verification");

        wo.row_version
    }

    /// Ensures at least one mode and one cause row exist (lookup seed may be empty in minimal DBs).
    async fn ensure_failure_mode_and_cause_ids(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let hid: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM failure_hierarchies ORDER BY id LIMIT 1".to_string(),
            ))
            .await
            .expect("h")
            .expect("hierarchy")
            .try_get("", "id")
            .unwrap();

        let mode_ct: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM failure_codes WHERE code_type = 'mode'".to_string(),
            ))
            .await
            .expect("c1")
            .unwrap()
            .try_get("", "c")
            .unwrap();
        if mode_ct == 0 {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO failure_codes \
                 (entity_sync_id, hierarchy_id, parent_id, code, label, code_type, is_active, row_version) \
                 VALUES ('fc:gap06:mode', ?, NULL, 'GAP06_M', 'Gap06 mode', 'mode', 1, 1)",
                [hid.into()],
            ))
            .await
            .expect("ins mode");
        }

        let cause_ct: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM failure_codes WHERE code_type IN ('cause','mechanism')"
                    .to_string(),
            ))
            .await
            .expect("c2")
            .unwrap()
            .try_get("", "c")
            .unwrap();
        if cause_ct == 0 {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO failure_codes \
                 (entity_sync_id, hierarchy_id, parent_id, code, label, code_type, is_active, row_version) \
                 VALUES ('fc:gap06:cause', ?, NULL, 'GAP06_C', 'Gap06 cause', 'cause', 1, 1)",
                [hid.into()],
            ))
            .await
            .expect("ins cause");
        }

        let mid: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM failure_codes WHERE code_type = 'mode' LIMIT 1".to_string(),
            ))
            .await
            .expect("qm")
            .expect("mode")
            .try_get("", "id")
            .unwrap();
        let cid: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM failure_codes WHERE code_type IN ('cause','mechanism') LIMIT 1"
                    .to_string(),
            ))
            .await
            .expect("qc")
            .expect("cause")
            .try_get("", "id")
            .unwrap();
        (mid, cid)
    }

    #[tokio::test]
    async fn gap06_close_corrective_without_failure_mode_fails_under_default_policy() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let (wo_id, rv) = create_wo_in_progress(&db).await;
        let rv = advance_to_technically_verified(&db, wo_id, rv).await;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_order_failure_details SET cause_not_determined = 0, \
             failure_mode_id = NULL, failure_cause_id = NULL WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await
        .expect("strip failure coding");

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

        assert!(result.is_err(), "close must fail without failure mode when cause is determined");
        let errs = match result.unwrap_err() {
            AppError::ValidationFailed(e) => e,
            o => panic!("expected ValidationFailed, got {o:?}"),
        };
        let joined = errs.join(" ");
        assert!(
            joined.contains("Mode de defaillance") || joined.contains("Failure mode"),
            "expected failure mode gate: {joined}"
        );
    }

    #[tokio::test]
    async fn gap06_close_with_full_failure_coding_succeeds() {
        let db = setup().await;
        let actor = admin_id(&db).await;
        let verifier = create_second_user(&db).await;
        let (mode_id, cause_id) = ensure_failure_mode_and_cause_ids(&db).await;

        let (wo_id, rv) = create_wo_in_progress(&db).await;

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
        .expect("labor");

        let part = parts::add_planned_part(
            &db,
            AddPartInput {
                wo_id,
                article_id: None,
                article_ref: Some("GAP06-PART".into()),
                quantity_planned: 1.0,
                unit_cost: Some(10.0),
                stock_location_id: None,
                auto_reserve: Some(false),
                notes: None,
            },
        )
        .await
        .expect("part");

        parts::record_actual_usage(&db, part.id, 1.0, Some(10.0))
            .await
            .expect("usage");

        costs::update_service_cost(&db, wo_id, 5.0, actor)
            .await
            .expect("service");

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
        .expect("mech");
        let rv = wo.row_version;

        closeout::save_failure_detail(
            &db,
            SaveFailureDetailInput {
                wo_id,
                symptom_id: None,
                failure_mode_id: Some(mode_id),
                failure_cause_id: Some(cause_id),
                failure_effect_id: None,
                is_temporary_repair: false,
                is_permanent_repair: true,
                cause_not_determined: false,
                notes: Some("coded".into()),
            },
        )
        .await
        .expect("failure detail");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET root_cause_summary = ? WHERE id = ?",
            ["Misalignment led to bearing wear.".into(), wo_id.into()],
        ))
        .await
        .expect("rca");

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
        .expect("verify");
        let rv = wo.row_version;

        let closed = closeout::close_wo(
            &db,
            WoCloseInput {
                wo_id,
                actor_id: actor,
                expected_row_version: rv,
                ..Default::default()
            },
        )
        .await
        .expect("close with full coding must succeed");

        assert_eq!(closed.status_code.as_deref(), Some("closed"));
    }

    #[tokio::test]
    async fn gap06_integrity_detector_finds_negative_downtime_segment() {
        let db = setup().await;
        let (wo_id, _) = create_wo_in_progress(&db).await;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_order_downtime_segments \
             (work_order_id, started_at, ended_at, downtime_type) \
             VALUES (?, '2026-04-10T14:00:00Z', '2026-04-10T08:00:00Z', 'full')",
            [wo_id.into()],
        ))
        .await
        .expect("inject bad segment");

        run_data_integrity_detectors(&db).await.expect("detectors");

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM data_integrity_findings \
                 WHERE status = 'open' AND finding_code = 'WO_DOWNTIME_NEGATIVE_DURATION'",
                [],
            ))
            .await
            .expect("q")
            .expect("cnt");
        let c: i64 = row.try_get("", "c").unwrap();
        assert!(c >= 1, "expected at least one negative-duration finding");
    }

    #[tokio::test]
    async fn gap06_analytics_contract_version_seeded() {
        let db = setup().await;
        let rows = list_contract_versions(&db).await.expect("list");
        assert!(!rows.is_empty(), "seeded contract version expected");
        assert!(
            rows.iter()
                .any(|r| r.contract_id == "closeout_to_reliability_v1"),
            "default contract id missing"
        );
    }
}
