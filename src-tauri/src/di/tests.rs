//! Sprint S2 – Full DI test suite (lifecycle redesign, migration 139).
//!
//! Covers: state machine (tests 01–03), DI code generation (test 04),
//! SLA engine (tests 05–07), optimistic locking (tests 08–09),
//! full lifecycle integration (test 10), return/resubmit (test 11),
//! and close path (test 12).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::di::audit;
    use crate::di::conversion::{convert_di_to_work_order, WoConversionInput};
    use crate::di::domain::{guard_transition, DiStatus};
    use crate::di::queries::{
        create_intervention_request, get_di_transition_log, update_di_draft_fields, DiCreateInput,
        DiDraftUpdateInput,
    };
    use crate::di::review::{
        approve_di, close_di, return_di_for_clarification, screen_di,
        DiApproveInput, DiCloseInput, DiReturnInput, DiScreenInput,
    };
    use crate::di::sla::{compute_sla_status, resolve_sla_rule, DiSlaLifecycleStatus};
    use crate::di::sla_poller::run_sla_poll_tick;
    use crate::di::review::get_review_events;

    // ═══════════════════════════════════════════════════════════════════════════
    // Setup helpers
    // ═══════════════════════════════════════════════════════════════════════════

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

        seed_fk_data(&db).await;

        db
    }

    async fn seed_fk_data(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO equipment (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'test-eq-001', 'EQ-TEST-001', 'Test Equipment', 'active_in_service', \
             datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test equipment");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO org_structure_models (id, sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test structure model");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO org_node_types (id, sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test node type");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO org_nodes (id, sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
             datetime('now'), datetime('now'), 1);".to_string(),
        ))
        .await
        .expect("insert test org_node");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_domains (id, code, name, structure_type, governance_level, is_extendable, created_at, updated_at) \
             VALUES (900001, 'DI_CLASSIFICATION', 'DI Classification', 'flat', 'tenant_managed', 1, \
             datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test reference_domain");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_sets (id, domain_id, version_no, status, created_at) \
             VALUES (900001, 900001, 1, 'published', datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test reference_set");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (id, set_id, code, label, is_active) \
             VALUES (900001, 900001, 'MECH', 'Mécanique', 1);".to_string(),
        ))
        .await
        .expect("insert test reference_value");
    }

    async fn get_user_id(db: &sea_orm::DatabaseConnection) -> i64 {
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts LIMIT 1;".to_string(),
        ))
        .await
        .expect("query")
        .expect("user must exist")
        .try_get::<i64>("", "id")
        .expect("id")
    }

    async fn seeded_symptom_id(db: &sea_orm::DatabaseConnection) -> i64 {
        crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("symptom lookup")
            .expect("seeded DI.SYMPTOM vibration")
    }

    async fn make_create_input(db: &sea_orm::DatabaseConnection, user_id: i64) -> DiCreateInput {
        DiCreateInput {
            asset_id: 1,
            org_node_id: 1,
            title: "Pump vibration alert".to_string(),
            description: "Excessive vibration on pump P-101".to_string(),
            origin_type: "operator".to_string(),
            request_type: "repair".to_string(),
            symptom_code_id: Some(seeded_symptom_id(db).await),
            impact_level: "unknown".to_string(),
            production_impact: false,
            safety_flag: false,
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: "medium".to_string(),
            observed_at: None,
            source_inspection_anomaly_id: None,
            submitter_id: user_id,
        }
    }

    /// Advance a DI from `submitted` to `in_review` via direct SQL.
    async fn advance_to_in_review(db: &sea_orm::DatabaseConnection, di_id: i64) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'in_review', \
             row_version = row_version + 1, updated_at = datetime('now') \
             WHERE id = ?",
            [di_id.into()],
        ))
        .await
        .expect("advance to in_review");
    }

    async fn get_di_status(db: &sea_orm::DatabaseConnection, di_id: i64) -> String {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM intervention_requests WHERE id = ?",
            [di_id.into()],
        ))
        .await
        .expect("query")
        .expect("DI must exist")
        .try_get::<String>("", "status")
        .expect("status")
    }

    async fn get_row_version(db: &sea_orm::DatabaseConnection, di_id: i64) -> i64 {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version FROM intervention_requests WHERE id = ?",
            [di_id.into()],
        ))
        .await
        .expect("query")
        .expect("DI must exist")
        .try_get::<i64>("", "row_version")
        .expect("row_version")
    }

    async fn ensure_equipment_criticality_path(_db: &sea_orm::DatabaseConnection) {}

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 01 – All valid transitions (7-state model)
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_01_all_valid_transitions() {
        let valid_pairs: Vec<(DiStatus, DiStatus)> = vec![
            // Submitted
            (DiStatus::Submitted, DiStatus::InReview),
            (DiStatus::Submitted, DiStatus::Closed),
            // InReview
            (DiStatus::InReview, DiStatus::ReturnedForClarification),
            (DiStatus::InReview, DiStatus::AwaitingApproval),
            (DiStatus::InReview, DiStatus::Closed),
            (DiStatus::InReview, DiStatus::Deferred),
            // ReturnedForClarification
            (DiStatus::ReturnedForClarification, DiStatus::InReview),
            (DiStatus::ReturnedForClarification, DiStatus::Closed),
            // AwaitingApproval
            (DiStatus::AwaitingApproval, DiStatus::Approved),
            (DiStatus::AwaitingApproval, DiStatus::Closed),
            (DiStatus::AwaitingApproval, DiStatus::Deferred),
            // Approved
            (DiStatus::Approved, DiStatus::Closed),
            (DiStatus::Approved, DiStatus::Deferred),
            // Deferred
            (DiStatus::Deferred, DiStatus::InReview),
            (DiStatus::Deferred, DiStatus::AwaitingApproval),
            (DiStatus::Deferred, DiStatus::Approved),
        ];

        for (from, to) in &valid_pairs {
            let result = guard_transition(from, to);
            assert!(
                result.is_ok(),
                "Transition {} → {} should be valid, got: {:?}",
                from.as_str(), to.as_str(), result.err()
            );
        }

        assert_eq!(valid_pairs.len(), 16, "7-state model defines 16 valid transitions");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 02 – Invalid transitions
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_02_invalid_transitions() {
        let invalid_pairs: Vec<(DiStatus, DiStatus)> = vec![
            (DiStatus::Submitted, DiStatus::Approved),
            (DiStatus::Submitted, DiStatus::Deferred),
            (DiStatus::Submitted, DiStatus::AwaitingApproval),
            (DiStatus::AwaitingApproval, DiStatus::Submitted),
            (DiStatus::Closed, DiStatus::Submitted),
            (DiStatus::Closed, DiStatus::InReview),
            (DiStatus::Closed, DiStatus::Approved),
            (DiStatus::Approved, DiStatus::InReview),
        ];

        for (from, to) in &invalid_pairs {
            let result = guard_transition(from, to);
            assert!(
                result.is_err(),
                "Transition {} → {} should be INVALID",
                from.as_str(), to.as_str()
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 03 – Immutable states
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_03_immutable_states() {
        // Only Closed is immutable
        assert!(DiStatus::Closed.is_immutable(), "Closed must be immutable");

        let mutable = [
            DiStatus::Submitted,
            DiStatus::InReview,
            DiStatus::ReturnedForClarification,
            DiStatus::AwaitingApproval,
            DiStatus::Approved,
            DiStatus::Deferred,
        ];
        for status in &mutable {
            assert!(
                !status.is_immutable(),
                "{} must NOT be immutable",
                status.as_str()
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 04 – DI code generation
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_04_di_code_generation() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di1 = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI #1");
        let di2 = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI #2");
        let di3 = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI #3");

        assert_eq!(di1.code, "DI-0001");
        assert_eq!(di2.code, "DI-0002");
        assert_eq!(di3.code, "DI-0003");

        let codes: std::collections::HashSet<&str> =
            [di1.code.as_str(), di2.code.as_str(), di3.code.as_str()]
                .into_iter().collect();
        assert_eq!(codes.len(), 3, "All DI codes must be unique");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 05 – SLA rule priority
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_05_sla_rule_priority() {
        let db = setup().await;

        db.execute(Statement::from_string(DbBackend::Sqlite, "DELETE FROM di_sla_rules;".to_string()))
            .await.expect("clear seeded SLA rules");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('High+IoT', 'high', 'iot', NULL, 2, 24, 1, 1)", [],
        )).await.expect("insert high+iot SLA rule");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('High+Any', 'high', NULL, NULL, 8, 48, 4, 1)", [],
        )).await.expect("insert high+NULL SLA rule");

        let rule_iot = resolve_sla_rule(&db, "high", "iot", None).await
            .expect("resolve").expect("must exist");
        assert_eq!(rule_iot.target_response_hours, 2);

        let rule_operator = resolve_sla_rule(&db, "high", "operator", None).await
            .expect("resolve").expect("must exist");
        assert_eq!(rule_operator.target_response_hours, 8);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 06 – SLA breach detection
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_06_sla_breach_detection() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        ensure_equipment_criticality_path(&db).await;

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Critical', 'critical', NULL, NULL, 1, 8, 1, 1)", [],
        )).await.expect("insert critical SLA rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "critical".to_string();
        let di = create_intervention_request(&db, input).await.expect("create critical DI");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "UPDATE intervention_requests SET submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 hours') WHERE id = ?",
            [di.id.into()],
        )).await.expect("backdate submitted_at");

        let di_updated = crate::di::queries::get_intervention_request(&db, di.id)
            .await.expect("re-read DI").expect("DI must exist");
        let sla_status = compute_sla_status(&db, &di_updated).await.expect("compute SLA status");

        assert!(sla_status.is_response_breached, "DI submitted 10h ago with 1h target must be response-breached");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07 – SLA no breach when screened
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07_sla_no_breach_when_screened() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        ensure_equipment_criticality_path(&db).await;

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Critical', 'critical', NULL, NULL, 1, 8, 1, 1)", [],
        )).await.expect("insert critical SLA rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "critical".to_string();
        let di = create_intervention_request(&db, input).await.expect("create critical DI");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
             submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 hours'), \
             screened_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 minutes') \
             WHERE id = ?",
            [di.id.into()],
        )).await.expect("set timestamps");

        let di_updated = crate::di::queries::get_intervention_request(&db, di.id)
            .await.expect("re-read DI").expect("DI must exist");
        let sla_status = compute_sla_status(&db, &di_updated).await.expect("compute SLA status");

        assert!(!sla_status.is_response_breached, "DI with screened_at set must NOT be response-breached");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07b – SLA At Risk via escalation_threshold_hours
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07b_sla_at_risk_via_escalation_threshold() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        db.execute(Statement::from_string(DbBackend::Sqlite, "DELETE FROM di_sla_rules;".to_string()))
            .await.expect("clear rules");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Medium', 'medium', NULL, NULL, 24, 72, 4, 1)", [],
        )).await.expect("insert rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "medium".to_string();
        let di = create_intervention_request(&db, input).await.expect("create DI");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
             submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-6 hours') \
             WHERE id = ?",
            [di.id.into()],
        )).await.expect("backdate");

        let di_updated = crate::di::queries::get_intervention_request(&db, di.id)
            .await.expect("re-read").expect("exists");
        let sla_status = compute_sla_status(&db, &di_updated).await.expect("compute");

        assert!(!sla_status.is_response_breached);
        assert_eq!(sla_status.status, Some(DiSlaLifecycleStatus::AtRisk));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07c – create persists sla_initialized review event
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07c_create_persists_sla_initialized_event() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI");

        let events = get_review_events(&db, di.id).await.expect("events");
        let init = events.iter().find(|e| e.event_type == "sla_initialized")
            .expect("sla_initialized event must exist");
        assert!(init.sla_target_hours.is_some(), "sla_target_hours must be populated");
        assert!(init.sla_deadline.is_some(), "sla_deadline must be populated");
        assert!(di.sla_response_deadline.is_some(), "DI must have frozen sla_response_deadline");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07c2 – frozen SLA is immutable after rule edit
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07c2_sla_snapshot_immutable_after_rule_change() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        db.execute(Statement::from_string(DbBackend::Sqlite, "DELETE FROM di_sla_rules;".to_string()))
            .await.expect("clear rules");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('High', 'high', NULL, NULL, 4, 24, 2, 1)", [],
        )).await.expect("insert 4h rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "high".to_string();
        let di = create_intervention_request(&db, input).await.expect("create DI");

        assert_eq!(di.sla_target_response_hours, Some(4));
        let frozen_deadline = di.sla_response_deadline.clone().expect("deadline");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "UPDATE di_sla_rules SET target_response_hours = 2 WHERE urgency_level = 'high'", [],
        )).await.expect("update rule");

        let di2 = crate::di::queries::get_intervention_request(&db, di.id)
            .await.expect("re-read").expect("exists");
        let status = compute_sla_status(&db, &di2).await.expect("status");

        assert_eq!(status.target_response_hours, Some(4));
        assert_eq!(status.sla_deadline.as_deref(), Some(frozen_deadline.as_str()));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07d – SLA poller emits once (dedupe)
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07d_sla_poller_dedupes_breach_notification() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        db.execute(Statement::from_string(DbBackend::Sqlite, "DELETE FROM di_sla_rules;".to_string()))
            .await.expect("clear rules");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Critical', 'critical', NULL, NULL, 1, 8, 1, 1)", [],
        )).await.expect("insert rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "critical".to_string();
        let di = create_intervention_request(&db, input).await.expect("create DI");

        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
             submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 hours'), \
             sla_response_deadline = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-9 hours'), \
             sla_resolution_deadline = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-2 hours') \
             WHERE id = ?",
            [di.id.into()],
        )).await.expect("backdate");

        run_sla_poll_tick(&db).await.expect("first tick");
        run_sla_poll_tick(&db).await.expect("second tick");

        let di_after = crate::di::queries::get_intervention_request(&db, di.id)
            .await.expect("re-read").expect("exists");
        assert!(di_after.sla_response_breach_notified_at.is_some());
        assert!(di_after.sla_resolution_breach_notified_at.is_some());

        let response_count: i64 = db
            .query_one(Statement::from_sql_and_values(DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM notification_events WHERE dedupe_key = ?",
                [format!("di-sla-response-breach-{}", di.id).into()],
            ))
            .await.expect("query").expect("row").try_get("", "c").expect("c");

        assert_eq!(response_count, 1, "response breach must emit once across ticks");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 08 – Optimistic lock on draft update
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_08_optimistic_lock_on_draft_update() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI");

        let result = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 0, // stale!
                title: Some("Updated title".into()),
                description: None, request_type: None, symptom_code_id: None,
                impact_level: None, production_impact: None, safety_flag: None,
                environmental_flag: None, quality_flag: None, reported_urgency: None,
                observed_at: None,
            },
        ).await;

        assert!(result.is_err(), "update_di_draft with stale row_version=0 must fail");

        let current = crate::di::queries::get_intervention_request(&db, di.id)
            .await.expect("re-read DI").expect("DI must exist");
        assert_eq!(current.title, di.title);
        assert_eq!(current.row_version, 1);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 09 – Optimistic lock on screen
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_09_optimistic_lock_on_screen() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI");

        advance_to_in_review(&db, di.id).await;

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 999, // wildly stale
                validated_urgency: "high".into(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: Some("Test".into()),
            },
        ).await;

        assert!(result.is_err(), "screen_di with stale row_version=999 must fail");

        let status = get_di_status(&db, di.id).await;
        assert_eq!(status, "in_review", "DI status must remain in_review after failed screen");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 10 – Full DI lifecycle (integration)
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_10_full_di_lifecycle() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // Phase A: Submission
        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI");
        assert_eq!(di.status, "submitted");

        let transitions = get_di_transition_log(&db, di.id).await.expect("get transition log");
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].action, "intake_submitted");
        assert_eq!(transitions[0].to_status, "submitted");

        audit::record_di_change_event(&db, audit::DiAuditInput {
            di_id: Some(di.id), action: "submitted".into(), actor_id: Some(user_id),
            summary: Some("DI submitted".into()), details_json: None,
            requires_step_up: false, apply_result: "applied".into(),
        }).await;

        // Phase B: Triage → screen
        advance_to_in_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let screened = screen_di(&db, DiScreenInput {
            di_id: di.id, actor_id: user_id, expected_row_version: rv,
            validated_urgency: "high".into(), review_team_id: None,
            classification_code_id: Some(900001),
            reviewer_note: Some("Validated by reviewer".into()),
        }).await.expect("screen_di should succeed");

        assert_eq!(screened.status, "awaiting_approval");
        assert!(screened.screened_at.is_some());
        assert_eq!(screened.reviewer_id, Some(user_id));

        audit::record_di_change_event(&db, audit::DiAuditInput {
            di_id: Some(di.id), action: "screened".into(), actor_id: Some(user_id),
            summary: Some("DI screened".into()), details_json: None,
            requires_step_up: false, apply_result: "applied".into(),
        }).await;

        // Phase C: Approval
        let approved = approve_di(&db, DiApproveInput {
            di_id: di.id, actor_id: user_id,
            expected_row_version: screened.row_version,
            notes: Some("Approved for planning".into()),
        }).await.expect("approve_di should succeed");

        assert_eq!(approved.status, "approved");
        assert!(approved.approved_at.is_some());

        audit::record_di_change_event(&db, audit::DiAuditInput {
            di_id: Some(di.id), action: "approved".into(), actor_id: Some(user_id),
            summary: Some("DI approuvée.".into()), details_json: None,
            requires_step_up: true, apply_result: "applied".into(),
        }).await;

        // Phase D: Conversion
        let conversion = convert_di_to_work_order(&db, WoConversionInput {
            di_id: di.id, actor_id: user_id,
            expected_row_version: approved.row_version,
            conversion_notes: Some("Converting to WO".into()),
        }).await.expect("convert_di_to_work_order should succeed");

        assert_eq!(conversion.di.status, "closed", "Status after conversion must be closed");
        assert_eq!(conversion.di.disposition_code.as_deref(), Some("converted_to_wo"));
        assert!(conversion.di.converted_to_wo_id.is_some());
        assert_eq!(conversion.di.converted_to_wo_id, Some(conversion.wo_id));
        assert!(conversion.di.converted_at.is_some());
        assert!(conversion.di.closed_at.is_some());

        // Phase E: Immutability (closed DI cannot be draft-edited)
        let update_result = update_di_draft_fields(&db, DiDraftUpdateInput {
            id: di.id, expected_row_version: conversion.di.row_version,
            title: Some("Should fail".into()), description: None, request_type: None,
            symptom_code_id: None, impact_level: None, production_impact: None,
            safety_flag: None, environmental_flag: None, quality_flag: None,
            reported_urgency: None, observed_at: None,
        }).await;
        assert!(update_result.is_err(), "update_di_draft on closed DI must fail");

        // Phase F: Audit completeness
        let events = audit::list_di_change_events(&db, di.id, 100)
            .await.expect("list change events");
        let actions: Vec<&str> = events.iter().map(|e| e.action.as_str()).collect();
        assert!(actions.contains(&"submitted"));
        assert!(actions.contains(&"screened"));
        assert!(actions.contains(&"approved"));
        assert!(events.len() >= 3);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 11 – Return and resubmit
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_11_return_and_resubmit() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI");
        advance_to_in_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let returned = return_di_for_clarification(&db, DiReturnInput {
            di_id: di.id, actor_id: user_id, expected_row_version: rv,
            reviewer_note: "Need more details on vibration frequency".into(),
        }).await.expect("return_for_clarification should succeed");

        assert_eq!(returned.status, "returned_for_clarification");

        let updated = update_di_draft_fields(&db, DiDraftUpdateInput {
            id: di.id, expected_row_version: returned.row_version,
            title: None,
            description: Some("Updated: vibration at 120Hz on bearing DE".into()),
            request_type: None, symptom_code_id: None, impact_level: None,
            production_impact: None, safety_flag: None, environmental_flag: None,
            quality_flag: None, reported_urgency: None, observed_at: None,
        }).await.expect("update_di_draft in returned state must succeed");
        assert_eq!(updated.description, "Updated: vibration at 120Hz on bearing DE");

        // Resubmit: returned_for_clarification → in_review
        advance_to_in_review(&db, di.id).await;
        let rv2 = get_row_version(&db, di.id).await;

        let re_screened = screen_di(&db, DiScreenInput {
            di_id: di.id, actor_id: user_id, expected_row_version: rv2,
            validated_urgency: "high".into(), review_team_id: None,
            classification_code_id: Some(900001),
            reviewer_note: Some("Re-screened after clarification".into()),
        }).await.expect("re-screen should succeed");

        assert_eq!(re_screened.status, "awaiting_approval");
        assert!(re_screened.screened_at.is_some());
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 12 – Close path (replaces old rejection test)
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_12_close_path() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await.expect("create DI");
        advance_to_in_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let screened = screen_di(&db, DiScreenInput {
            di_id: di.id, actor_id: user_id, expected_row_version: rv,
            validated_urgency: "medium".into(), review_team_id: None,
            classification_code_id: Some(900001), reviewer_note: None,
        }).await.expect("screen should succeed");

        // Close from awaiting_approval with disposition duplicate
        let closed = close_di(&db, DiCloseInput {
            di_id: di.id, actor_id: user_id,
            expected_row_version: screened.row_version,
            disposition_code: "duplicate".into(),
            notes: Some("Already reported as DI-0001".into()),
            related_di_id: None,
        }).await;

        // duplicate requires related_di_id → should fail
        assert!(closed.is_err(), "close with duplicate but no related_di_id must fail");

        // Now close with rejected_invalid disposition (no related_di_id required)
        let closed_ok = close_di(&db, DiCloseInput {
            di_id: di.id, actor_id: user_id,
            expected_row_version: screened.row_version,
            disposition_code: "rejected_invalid".into(),
            notes: Some("Request does not meet criteria".into()),
            related_di_id: None,
        }).await.expect("close with rejected_invalid should succeed");

        assert_eq!(closed_ok.status, "closed");
        assert!(closed_ok.closed_at.is_some());
        assert_eq!(closed_ok.disposition_code.as_deref(), Some("rejected_invalid"));
        assert_eq!(closed_ok.closed_by_id, Some(user_id));

        // Guard: closed is immutable
        let invalid = guard_transition(&DiStatus::Closed, &DiStatus::InReview);
        assert!(invalid.is_err(), "Closed → InReview must be illegal");
    }
}
