//! Sprint S2 â€” Full DI test suite.
//!
//! Phase 2 â€“ Sub-phase 04 â€“ File 04.
//!
//! Covers: state machine (tests 01â€“03), DI code generation (test 04),
//! SLA engine (tests 05â€“07), optimistic locking (tests 08â€“09),
//! full lifecycle integration (test 10), return/resubmit (test 11),
//! and rejection path (test 12).

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
        approve_di_for_planning, reject_di, return_di_for_clarification, screen_di,
        DiApproveInput, DiRejectInput, DiReturnInput, DiScreenInput,
    };
    use crate::di::sla::{compute_sla_status, resolve_sla_rule, DiSlaLifecycleStatus};
    use crate::di::sla_poller::run_sla_poll_tick;
    use crate::di::review::get_review_events;

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Setup helpers
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    /// Create an in-memory SQLite database with all migrations and seed data applied.
    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            // SAFETY: in-memory SQLite always connects in test context
            .expect("in-memory SQLite should connect");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        // SAFETY: PRAGMA is a valid SQLite statement
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(&db, None)
            .await
            // SAFETY: migrations must apply cleanly or the test is invalid
            .expect("migrations should apply cleanly");

        crate::db::seeder::seed_system_data(&db)
            .await
            // SAFETY: seeder must run after migrations
            .expect("seeder should run cleanly");

        seed_fk_data(&db).await;

        db
    }

    /// Seed the minimum FK data required by DI creation: equipment, org_nodes,
    /// reference chain (domain â†’ set â†’ value) for classification.
    async fn seed_fk_data(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO equipment (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'test-eq-001', 'EQ-TEST-001', 'Test Equipment', 'active_in_service', \
             datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        // SAFETY: test fixture insert
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
             VALUES (900001, 900001, 'MECH', 'MÃ©canique', 1);".to_string(),
        ))
        .await
        .expect("insert test reference_value");
    }

    /// Get the seeded admin user's id.
    async fn get_user_id(db: &sea_orm::DatabaseConnection) -> i64 {
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts LIMIT 1;".to_string(),
        ))
        .await
        // SAFETY: seeder always inserts at least one user
        .expect("query")
        .expect("user must exist")
        .try_get::<i64>("", "id")
        .expect("id")
    }

    /// Build a standard DiCreateInput for testing.
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

    /// Advance a DI from `submitted` to `pending_review` via direct SQL
    /// (simulates the Submitted â†’ PendingReview intake UI trigger).
    async fn advance_to_pending_review(db: &sea_orm::DatabaseConnection, di_id: i64) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'pending_review', \
             row_version = row_version + 1, updated_at = datetime('now') \
             WHERE id = ?",
            [di_id.into()],
        ))
        .await
        // SAFETY: DI must exist before calling this helper
        .expect("advance to pending_review");
    }

    /// Read the current status of a DI directly from the database.
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

    /// Read the current row_version of a DI.
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

    /// No-op retained for call-site clarity: criticality comes from `equipment`.
    async fn ensure_equipment_criticality_path(_db: &sea_orm::DatabaseConnection) {
        // Production SLA lookup uses equipment + lookup/reference JOINs.
        // Test fixtures already create equipment rows; missing criticality simply
        // falls through to urgency-only SLA rules.
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 01 â€” All valid transitions
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_01_all_valid_transitions() {
        // PRD Â§6.4 complete transition table
        let valid_pairs: Vec<(DiStatus, DiStatus)> = vec![
            (DiStatus::Submitted, DiStatus::PendingReview),
            (DiStatus::PendingReview, DiStatus::Screened),
            (DiStatus::PendingReview, DiStatus::ReturnedForClarification),
            (DiStatus::PendingReview, DiStatus::Rejected),
            (DiStatus::ReturnedForClarification, DiStatus::PendingReview),
            (DiStatus::Screened, DiStatus::AwaitingApproval),
            (DiStatus::Screened, DiStatus::Rejected),
            (DiStatus::AwaitingApproval, DiStatus::ApprovedForPlanning),
            (DiStatus::AwaitingApproval, DiStatus::Deferred),
            (DiStatus::AwaitingApproval, DiStatus::Rejected),
            (DiStatus::ApprovedForPlanning, DiStatus::ConvertedToWorkOrder),
            (DiStatus::ApprovedForPlanning, DiStatus::Deferred),
            (DiStatus::ApprovedForPlanning, DiStatus::ClosedAsNonExecutable),
            (DiStatus::Deferred, DiStatus::AwaitingApproval),
            (DiStatus::ConvertedToWorkOrder, DiStatus::Archived),
            (DiStatus::ClosedAsNonExecutable, DiStatus::Archived),
            (DiStatus::Rejected, DiStatus::Archived),
        ];

        for (from, to) in &valid_pairs {
            let result = guard_transition(from, to);
            assert!(
                result.is_ok(),
                "Transition {} â†’ {} should be valid, got: {:?}",
                from.as_str(),
                to.as_str(),
                result.err()
            );
        }

        // Verify we covered all transitions defined in the state machine
        assert_eq!(
            valid_pairs.len(),
            17,
            "PRD Â§6.4 defines exactly 17 valid transitions"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 02 â€” Invalid transitions
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_02_invalid_transitions() {
        let invalid_pairs: Vec<(DiStatus, DiStatus)> = vec![
            (DiStatus::Submitted, DiStatus::ApprovedForPlanning),
            (DiStatus::Submitted, DiStatus::Archived),
            (DiStatus::AwaitingApproval, DiStatus::Submitted),
            (DiStatus::ConvertedToWorkOrder, DiStatus::PendingReview),
            (DiStatus::Archived, DiStatus::Submitted),
            // Additional invalid pairs for completeness
            (DiStatus::Rejected, DiStatus::PendingReview),
            (DiStatus::Submitted, DiStatus::ConvertedToWorkOrder),
            (DiStatus::PendingReview, DiStatus::ApprovedForPlanning),
        ];

        for (from, to) in &invalid_pairs {
            let result = guard_transition(from, to);
            assert!(
                result.is_err(),
                "Transition {} â†’ {} should be INVALID, but guard_transition returned Ok(())",
                from.as_str(),
                to.as_str()
            );
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 03 â€” Immutable states
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_03_immutable_states() {
        // States where is_immutable_after_conversion returns true
        let immutable = [
            DiStatus::ConvertedToWorkOrder,
            DiStatus::ClosedAsNonExecutable,
            DiStatus::Rejected,
            DiStatus::Archived,
        ];
        for status in &immutable {
            assert!(
                status.is_immutable_after_conversion(),
                "{} must be immutable, but is_immutable_after_conversion returned false",
                status.as_str()
            );
        }

        // States where is_immutable_after_conversion returns false
        let mutable = [
            DiStatus::Submitted,
            DiStatus::PendingReview,
            DiStatus::ReturnedForClarification,
            DiStatus::Screened,
            DiStatus::AwaitingApproval,
            DiStatus::ApprovedForPlanning,
            DiStatus::Deferred,
        ];
        for status in &mutable {
            assert!(
                !status.is_immutable_after_conversion(),
                "{} must NOT be immutable, but is_immutable_after_conversion returned true",
                status.as_str()
            );
        }
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 04 â€” DI code generation
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_04_di_code_generation() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di1 = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            // SAFETY: first DI insert must succeed in a clean DB
            .expect("create DI #1");
        let di2 = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI #2");
        let di3 = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI #3");

        assert_eq!(di1.code, "DI-0001", "First DI code must be DI-0001");
        assert_eq!(di2.code, "DI-0002", "Second DI code must be DI-0002");
        assert_eq!(di3.code, "DI-0003", "Third DI code must be DI-0003");

        // Verify no duplicates
        let codes: std::collections::HashSet<&str> =
            [di1.code.as_str(), di2.code.as_str(), di3.code.as_str()]
                .into_iter()
                .collect();
        assert_eq!(codes.len(), 3, "All DI codes must be unique");
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 05 â€” SLA rule priority
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_05_sla_rule_priority() {
        let db = setup().await;

        // Clear seeded SLA rules to avoid interference with test-specific rules
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM di_sla_rules;".to_string(),
        ))
        .await
        // SAFETY: clearing seed data for isolated SLA rule testing
        .expect("clear seeded SLA rules");

        // Insert specific rule: high + iot â†’ response 2h
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('High+IoT', 'high', 'iot', NULL, 2, 24, 1, 1)",
            [],
        ))
        .await
        // SAFETY: test fixture insert
        .expect("insert high+iot SLA rule");

        // Insert broad rule: high + NULL â†’ response 8h
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('High+Any', 'high', NULL, NULL, 8, 48, 4, 1)",
            [],
        ))
        .await
        .expect("insert high+NULL SLA rule");

        // Priority 2 match: high + iot should return the specific 2h rule
        let rule_iot = resolve_sla_rule(&db, "high", "iot", None)
            .await
            // SAFETY: query must succeed
            .expect("resolve SLA for high+iot")
            // SAFETY: at least the broad rule should match
            .expect("a matching rule must exist for high+iot");
        assert_eq!(
            rule_iot.target_response_hours, 2,
            "high+iot should resolve to 2h response rule"
        );

        // Priority 3 match: high + operator â†’ no specific rule, falls back to broad 8h
        let rule_operator = resolve_sla_rule(&db, "high", "operator", None)
            .await
            .expect("resolve SLA for high+operator")
            .expect("a matching rule must exist for high+operator");
        assert_eq!(
            rule_operator.target_response_hours, 8,
            "high+operator should resolve to 8h response rule (broad fallback)"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 06 â€” SLA breach detection
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_06_sla_breach_detection() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        ensure_equipment_criticality_path(&db).await;

        // Insert a critical rule: 1h response
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Critical', 'critical', NULL, NULL, 1, 8, 1, 1)",
            [],
        ))
        .await
        .expect("insert critical SLA rule");

        // Create a DI with urgency=critical
        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "critical".to_string();
        let di = create_intervention_request(&db, input)
            .await
            .expect("create critical DI");

        // Backdate submitted_at to 10 hours ago so it breaches the 1h response target.
        // Use strftime with ISO 8601 'T' separator â€” the SLA parser expects this format.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 hours') WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("backdate submitted_at");

        // Re-read the DI with updated submitted_at
        let di_updated = crate::di::queries::get_intervention_request(&db, di.id)
            .await
            .expect("re-read DI")
            .expect("DI must exist");

        let sla_status = compute_sla_status(&db, &di_updated)
            .await
            .expect("compute SLA status");

        assert!(
            sla_status.is_response_breached,
            "DI submitted 10h ago with 1h target and screened_at=NULL must be response-breached"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 07 â€” SLA no breach when screened
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_07_sla_no_breach_when_screened() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        ensure_equipment_criticality_path(&db).await;

        // Insert a critical rule: 1h response
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Critical', 'critical', NULL, NULL, 1, 8, 1, 1)",
            [],
        ))
        .await
        .expect("insert critical SLA rule");

        // Create a DI with urgency=critical
        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "critical".to_string();
        let di = create_intervention_request(&db, input)
            .await
            .expect("create critical DI");

        // Set submitted_at to 10h ago but screened_at to 30 minutes ago.
        // Use strftime with ISO 8601 'T' separator â€” the SLA parser expects this format.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
             submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 hours'), \
             screened_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 minutes') \
             WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("set timestamps");

        let di_updated = crate::di::queries::get_intervention_request(&db, di.id)
            .await
            .expect("re-read DI")
            .expect("DI must exist");

        let sla_status = compute_sla_status(&db, &di_updated)
            .await
            .expect("compute SLA status");

        assert!(
            !sla_status.is_response_breached,
            "DI with screened_at set must NOT be response-breached (response milestone reached)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07b — SLA At Risk via escalation_threshold_hours
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07b_sla_at_risk_via_escalation_threshold() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM di_sla_rules;".to_string(),
        ))
        .await
        .expect("clear rules");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Medium', 'medium', NULL, NULL, 24, 72, 4, 1)",
            [],
        ))
        .await
        .expect("insert rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "medium".to_string();
        let di = create_intervention_request(&db, input)
            .await
            .expect("create DI");

        // 6h elapsed: past escalation (4h) but under response target (24h)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
             submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-6 hours') \
             WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("backdate");

        let di_updated = crate::di::queries::get_intervention_request(&db, di.id)
            .await
            .expect("re-read")
            .expect("exists");

        let sla_status = compute_sla_status(&db, &di_updated)
            .await
            .expect("compute");

        assert!(!sla_status.is_response_breached);
        assert_eq!(sla_status.status, Some(DiSlaLifecycleStatus::AtRisk));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07c — create persists sla_initialized review event
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07c_create_persists_sla_initialized_event() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");

        let events = get_review_events(&db, di.id).await.expect("events");
        let init = events
            .iter()
            .find(|e| e.event_type == "sla_initialized")
            .expect("sla_initialized event must exist");
        assert!(
            init.sla_target_hours.is_some(),
            "sla_target_hours must be populated"
        );
        assert!(
            init.sla_deadline.is_some(),
            "sla_deadline must be populated"
        );
        assert!(
            init.sla_resolution_target_hours.is_some(),
            "sla_resolution_target_hours must be populated"
        );
        assert!(
            init.sla_resolution_deadline.is_some(),
            "sla_resolution_deadline must be populated"
        );
        assert!(
            di.sla_response_deadline.is_some(),
            "DI must have frozen sla_response_deadline"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07c2 — frozen SLA is immutable after rule edit
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07c2_sla_snapshot_immutable_after_rule_change() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM di_sla_rules;".to_string(),
        ))
        .await
        .expect("clear rules");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('High', 'high', NULL, NULL, 4, 24, 2, 1)",
            [],
        ))
        .await
        .expect("insert 4h rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "high".to_string();
        let di = create_intervention_request(&db, input)
            .await
            .expect("create DI");

        assert_eq!(di.sla_target_response_hours, Some(4));
        let frozen_deadline = di.sla_response_deadline.clone().expect("deadline");

        // Change live rule to 2h — must not rewrite existing DI snapshot.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE di_sla_rules SET target_response_hours = 2 WHERE urgency_level = 'high'",
            [],
        ))
        .await
        .expect("update rule");

        let di2 = crate::di::queries::get_intervention_request(&db, di.id)
            .await
            .expect("re-read")
            .expect("exists");
        let status = compute_sla_status(&db, &di2).await.expect("status");

        assert_eq!(status.target_response_hours, Some(4));
        assert_eq!(status.sla_deadline.as_deref(), Some(frozen_deadline.as_str()));
        assert_eq!(di2.sla_target_response_hours, Some(4));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 07d — SLA poller emits once (dedupe)
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_07d_sla_poller_dedupes_breach_notification() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM di_sla_rules;".to_string(),
        ))
        .await
        .expect("clear rules");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules (name, urgency_level, origin_type, asset_criticality_class, \
             target_response_hours, target_resolution_hours, escalation_threshold_hours, is_active) \
             VALUES ('Critical', 'critical', NULL, NULL, 1, 8, 1, 1)",
            [],
        ))
        .await
        .expect("insert rule");

        let mut input = make_create_input(&db, user_id).await;
        input.reported_urgency = "critical".to_string();
        let di = create_intervention_request(&db, input)
            .await
            .expect("create DI");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
             submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 hours'), \
             sla_response_deadline = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-9 hours'), \
             sla_resolution_deadline = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-2 hours') \
             WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("backdate");

        run_sla_poll_tick(&db).await.expect("first tick");
        run_sla_poll_tick(&db).await.expect("second tick");

        let di_after = crate::di::queries::get_intervention_request(&db, di.id)
            .await
            .expect("re-read")
            .expect("exists");
        assert!(
            di_after.sla_response_breach_notified_at.is_some(),
            "response breach must set notify timestamp"
        );
        assert!(
            di_after.sla_resolution_breach_notified_at.is_some(),
            "resolution breach must set notify timestamp"
        );

        let response_count: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM notification_events \
                 WHERE dedupe_key = ?",
                [format!("di-sla-response-breach-{}", di.id).into()],
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "c")
            .expect("c");

        assert_eq!(
            response_count, 1,
            "response breach must emit once across ticks"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 08 — Optimistic lock on draft update
    // ═══════════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_08_optimistic_lock_on_draft_update() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");

        // DI starts with row_version=1; try update with stale version 0
        let result = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 0, // stale!
                title: Some("Updated title".into()),
                description: None,
                request_type: None,
                symptom_code_id: None,
                impact_level: None,
                production_impact: None,
                safety_flag: None,
                environmental_flag: None,
                quality_flag: None,
                reported_urgency: None,
                observed_at: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "update_di_draft with stale row_version=0 must fail"
        );

        // Verify DI was not modified
        let current = crate::di::queries::get_intervention_request(&db, di.id)
            .await
            .expect("re-read DI")
            .expect("DI must exist");
        assert_eq!(
            current.title, di.title,
            "Title must be unchanged after failed optimistic lock"
        );
        assert_eq!(
            current.row_version, 1,
            "row_version must be unchanged after failed update"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 09 â€” Optimistic lock on screen
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_09_optimistic_lock_on_screen() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");

        // Advance to pending_review (row_version goes to 2)
        advance_to_pending_review(&db, di.id).await;

        // Try screen with stale row_version=999
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
        )
        .await;

        assert!(
            result.is_err(),
            "screen_di with stale row_version=999 must fail"
        );

        // DI must still be in pending_review
        let status = get_di_status(&db, di.id).await;
        assert_eq!(
            status, "pending_review",
            "DI status must remain pending_review after failed screen"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 10 â€” Full DI lifecycle (integration)
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_10_full_di_lifecycle() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // â”€â”€ Phase A: Submission â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");

        assert_eq!(di.status, "submitted", "Initial status must be 'submitted'");

        // Assert 1 row in di_state_transition_log (action = intake_submitted)
        let transitions = get_di_transition_log(&db, di.id)
            .await
            .expect("get transition log");
        assert_eq!(
            transitions.len(),
            1,
            "Exactly 1 transition log row after submission"
        );
        assert_eq!(transitions[0].action, "intake_submitted");
        assert_eq!(transitions[0].to_status, "submitted");

        // Record a submission change event (the command layer would do this)
        audit::record_di_change_event(
            &db,
            audit::DiAuditInput {
                di_id: Some(di.id),
                action: "submitted".into(),
                actor_id: Some(user_id),
                summary: Some("DI submitted".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        // â”€â”€ Phase B: Review (screen) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        advance_to_pending_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv,
                validated_urgency: "high".into(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: Some("Validated by reviewer".into()),
            },
        )
        .await
        .expect("screen_di should succeed");

        // screen_di auto-advances to awaiting_approval
        assert_eq!(
            screened.status, "awaiting_approval",
            "Status after screening must be awaiting_approval"
        );
        assert!(
            screened.screened_at.is_some(),
            "screened_at must be set after screening"
        );
        assert_eq!(
            screened.reviewer_id,
            Some(user_id),
            "reviewer_id must be set"
        );

        // Record screen audit event
        audit::record_di_change_event(
            &db,
            audit::DiAuditInput {
                di_id: Some(di.id),
                action: "screened".into(),
                actor_id: Some(user_id),
                summary: Some("DI screened".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        // â”€â”€ Phase C: Approval â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let approved = approve_di_for_planning(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: Some("Approved for planning".into()),
            },
        )
        .await
        .expect("approve_di_for_planning should succeed");

        assert_eq!(
            approved.status, "approved_for_planning",
            "Status after approval must be approved_for_planning"
        );
        assert!(
            approved.approved_at.is_some(),
            "approved_at must be set after approval"
        );

        // Record approval audit event
        audit::record_di_change_event(
            &db,
            audit::DiAuditInput {
                di_id: Some(di.id),
                action: "approved".into(),
                actor_id: Some(user_id),
                summary: Some("DI approved for planning".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        // â”€â”€ Phase D: Conversion â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let conversion = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: approved.row_version,
                conversion_notes: Some("Converting to WO".into()),
            },
        )
        .await
        .expect("convert_di_to_work_order should succeed");

        assert_eq!(
            conversion.di.status, "converted_to_work_order",
            "Status after conversion must be converted_to_work_order"
        );
        assert!(
            conversion.di.converted_to_wo_id.is_some(),
            "converted_to_wo_id must NOT be NULL after conversion"
        );
        assert_eq!(
            conversion.di.converted_to_wo_id,
            Some(conversion.wo_id),
            "converted_to_wo_id must match the WO stub id"
        );
        assert!(
            conversion.di.converted_at.is_some(),
            "converted_at must be set after conversion"
        );
        assert!(
            conversion
                .di
                .converted_to_wo_code
                .as_deref()
                .is_some_and(|c| !c.is_empty()),
            "converted_to_wo_code must be enriched after conversion"
        );
        assert!(
            conversion
                .di
                .converted_to_wo_title
                .as_deref()
                .is_some_and(|t| !t.is_empty()),
            "converted_to_wo_title must be enriched after conversion"
        );

        // Verify WO exists with source_di_id
        let wo_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT source_di_id FROM work_orders WHERE id = ?",
                [conversion.wo_id.into()],
            ))
            .await
            .expect("query WO")
            .expect("WO must exist");
        let source_di_id: i64 = wo_row.try_get("", "source_di_id").expect("source_di_id");
        assert_eq!(source_di_id, di.id, "WO must link back to source DI");

        // Record conversion audit event
        audit::record_di_change_event(
            &db,
            audit::DiAuditInput {
                di_id: Some(di.id),
                action: "converted".into(),
                actor_id: Some(user_id),
                summary: Some("DI converted to WO".into()),
                details_json: Some(
                    format!(
                        r#"{{"wo_id":{},"wo_code":"{}"}}"#,
                        conversion.wo_id, conversion.wo_code
                    ),
                ),
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        // â”€â”€ Phase E: Immutability â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let update_result = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: conversion.di.row_version,
                title: Some("Should fail".into()),
                description: None,
                request_type: None,
                symptom_code_id: None,
                impact_level: None,
                production_impact: None,
                safety_flag: None,
                environmental_flag: None,
                quality_flag: None,
                reported_urgency: None,
                observed_at: None,
            },
        )
        .await;

        assert!(
            update_result.is_err(),
            "update_di_draft on converted DI must fail"
        );

        // Verify fields unchanged
        let final_di = crate::di::queries::get_intervention_request(&db, di.id)
            .await
            .expect("re-read DI")
            .expect("DI must exist");
        assert_eq!(
            final_di.title, "Pump vibration alert",
            "Title must remain unchanged after failed update on converted DI"
        );

        // â”€â”€ Phase F: Audit completeness â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let events = audit::list_di_change_events(&db, di.id, 100)
            .await
            .expect("list change events");

        let actions: Vec<&str> = events.iter().map(|e| e.action.as_str()).collect();
        assert!(
            actions.contains(&"submitted"),
            "Audit trail must include 'submitted' event"
        );
        assert!(
            actions.contains(&"screened"),
            "Audit trail must include 'screened' event"
        );
        assert!(
            actions.contains(&"approved"),
            "Audit trail must include 'approved' event"
        );
        assert!(
            actions.contains(&"converted"),
            "Audit trail must include 'converted' event"
        );
        assert!(
            events.len() >= 4,
            "At minimum 4 audit events expected, found {}",
            events.len()
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 11 â€” Return and resubmit
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_11_return_and_resubmit() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // Create and advance to pending_review
        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        // Return for clarification
        let returned = return_di_for_clarification(
            &db,
            DiReturnInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv,
                reviewer_note: "Need more details on vibration frequency".into(),
            },
        )
        .await
        .expect("return_for_clarification should succeed");

        assert_eq!(
            returned.status, "returned_for_clarification",
            "Status after return must be returned_for_clarification"
        );

        // Update draft with new description (allowed in returned_for_clarification)
        let updated = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: returned.row_version,
                title: None,
                description: Some("Updated: vibration at 120Hz on bearing DE".into()),
                request_type: None,
                symptom_code_id: None,
                impact_level: None,
                production_impact: None,
                safety_flag: None,
                environmental_flag: None,
                quality_flag: None,
                reported_urgency: None,
                observed_at: None,
            },
        )
        .await
        .expect("update_di_draft in returned state must succeed");

        assert_eq!(
            updated.description, "Updated: vibration at 120Hz on bearing DE",
            "Description must be updated"
        );

        // Resubmit: returned_for_clarification â†’ pending_review
        advance_to_pending_review(&db, di.id).await;
        let rv2 = get_row_version(&db, di.id).await;

        // Screen again
        let re_screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv2,
                validated_urgency: "high".into(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: Some("Re-screened after clarification".into()),
            },
        )
        .await
        .expect("re-screen should succeed after resubmission");

        assert_eq!(
            re_screened.status, "awaiting_approval",
            "Status after re-screen must be awaiting_approval"
        );
        assert!(
            re_screened.screened_at.is_some(),
            "screened_at must be updated after re-screening"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // TEST 12 â€” Rejection path
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn test_12_rejection_path() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // Create DI and advance to awaiting_approval (for speed, bypass screened)
        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        // Screen: pending_review â†’ awaiting_approval
        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv,
                validated_urgency: "medium".into(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen should succeed");

        // Reject from awaiting_approval
        let rejected = reject_di(
            &db,
            DiRejectInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                reason_code: "duplicate".into(),
                notes: Some("Already reported as DI-0001".into()),
            },
        )
        .await
        .expect("reject_di should succeed from awaiting_approval");

        assert_eq!(
            rejected.status, "rejected",
            "Status after rejection must be 'rejected'"
        );
        assert!(
            rejected.declined_at.is_some(),
            "declined_at must be set after rejection"
        );

        // Guard: rejected â†’ pending_review must fail
        let invalid = guard_transition(&DiStatus::Rejected, &DiStatus::PendingReview);
        assert!(
            invalid.is_err(),
            "Transition rejected â†’ pending_review must be illegal"
        );

        // Guard: rejected â†’ archived must succeed
        let valid = guard_transition(&DiStatus::Rejected, &DiStatus::Archived);
        assert!(
            valid.is_ok(),
            "Transition rejected â†’ archived must be legal"
        );
    }
}
