//! Supervisor verification tests for Phase 2 SP04 File 01 Sprint S1.
//!
//! V1 — Migration 017 applies cleanly; both tables created with correct columns and indexes.
//! V2 — State machine coverage (covered by unit tests in domain.rs — re-verified here).
//! V3 — Code generation uniqueness: sequential DI codes with no duplicates.
//! V4 — Immutability flag (covered by unit tests in domain.rs — re-verified here).

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::di::domain::{
        generate_di_code, guard_transition, DiStatus,
    };

    /// In-memory SQLite with all migrations applied.
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
            .expect("migrations should apply cleanly (including 017)");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder should run cleanly");

        db
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Migration 017 applies cleanly
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_migration_017_creates_intervention_requests_table() {
        let db = setup().await;

        let exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='intervention_requests';".to_string(),
            ))
            .await
            .expect("query should succeed");

        assert!(
            exists.is_some(),
            "intervention_requests table must exist after migration 017"
        );
    }

    #[tokio::test]
    async fn v1_migration_017_creates_di_state_transition_log_table() {
        let db = setup().await;

        let exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='di_state_transition_log';".to_string(),
            ))
            .await
            .expect("query should succeed");

        assert!(
            exists.is_some(),
            "di_state_transition_log table must exist after migration 017"
        );
    }

    #[tokio::test]
    async fn v1_intervention_requests_has_all_columns() {
        let db = setup().await;

        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(intervention_requests);".to_string(),
            ))
            .await
            .expect("PRAGMA table_info should succeed");

        let column_names: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").unwrap())
            .collect();

        let expected_columns = [
            "id",
            "code",
            "asset_id",
            "sub_asset_ref",
            "org_node_id",
            "status",
            "title",
            "description",
            "origin_type",
            "symptom_code_id",
            "impact_level",
            "production_impact",
            "safety_flag",
            "environmental_flag",
            "quality_flag",
            "reported_urgency",
            "validated_urgency",
            "observed_at",
            "submitted_at",
            "review_team_id",
            "reviewer_id",
            "screened_at",
            "approved_at",
            "deferred_until",
            "declined_at",
            "closed_at",
            "archived_at",
            "converted_to_wo_id",
            "converted_at",
            "reviewer_note",
            "classification_code_id",
            "is_recurrence_flag",
            "recurrence_di_id",
            "row_version",
            "submitter_id",
            "created_at",
            "updated_at",
        ];

        for expected in &expected_columns {
            assert!(
                column_names.contains(&(*expected).to_string()),
                "intervention_requests is missing column '{}'. Present: {:?}",
                expected,
                column_names
            );
        }

        assert_eq!(
            column_names.len(),
            expected_columns.len(),
            "intervention_requests should have exactly {} columns, found {}: {:?}",
            expected_columns.len(),
            column_names.len(),
            column_names
        );
    }

    #[tokio::test]
    async fn v1_di_state_transition_log_has_all_columns() {
        let db = setup().await;

        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(di_state_transition_log);".to_string(),
            ))
            .await
            .expect("PRAGMA table_info should succeed");

        let column_names: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").unwrap())
            .collect();

        let expected_columns = [
            "id",
            "di_id",
            "from_status",
            "to_status",
            "action",
            "actor_id",
            "reason_code",
            "notes",
            "acted_at",
        ];

        for expected in &expected_columns {
            assert!(
                column_names.contains(&(*expected).to_string()),
                "di_state_transition_log is missing column '{}'. Present: {:?}",
                expected,
                column_names
            );
        }

        assert_eq!(
            column_names.len(),
            expected_columns.len(),
            "di_state_transition_log should have exactly {} columns, found {}: {:?}",
            expected_columns.len(),
            column_names.len(),
            column_names
        );
    }

    #[tokio::test]
    async fn v1_intervention_requests_indexes_exist() {
        let db = setup().await;

        let expected_indexes = [
            "idx_ir_status",
            "idx_ir_asset",
            "idx_ir_org_node",
            "idx_ir_submitter",
            "idx_ir_reviewer",
        ];

        for idx_name in &expected_indexes {
            let exists = db
                .query_one(Statement::from_string(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT name FROM sqlite_master WHERE type='index' AND name='{}';",
                        idx_name
                    ),
                ))
                .await
                .expect("index query should succeed");

            assert!(
                exists.is_some(),
                "index '{}' must exist on intervention_requests",
                idx_name
            );
        }
    }

    #[tokio::test]
    async fn v1_di_state_transition_log_index_exists() {
        let db = setup().await;

        let exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_dstl_di_id';".to_string(),
            ))
            .await
            .expect("index query should succeed");

        assert!(
            exists.is_some(),
            "index 'idx_dstl_di_id' must exist on di_state_transition_log"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — State machine coverage (integration re-verification)
    // ═══════════════════════════════════════════════════════════════════════

    /// Exhaustive check: every valid forward transition passes `guard_transition`.
    #[test]
    fn v2_all_valid_transitions_accepted() {
        let valid_pairs: &[(DiStatus, DiStatus)] = &[
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

        assert_eq!(valid_pairs.len(), 17, "PRD §6.4 defines exactly 17 forward transitions");

        for (from, to) in valid_pairs {
            assert!(
                guard_transition(from, to).is_ok(),
                "Expected valid: {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }

    /// At least 8 invalid transitions are rejected with descriptive error.
    #[test]
    fn v2_invalid_transitions_rejected() {
        let invalid_pairs: &[(DiStatus, DiStatus)] = &[
            (DiStatus::Submitted, DiStatus::ApprovedForPlanning),
            (DiStatus::Submitted, DiStatus::Archived),
            (DiStatus::Archived, DiStatus::Submitted),
            (DiStatus::Rejected, DiStatus::Submitted),
            (DiStatus::ConvertedToWorkOrder, DiStatus::Submitted),
            (DiStatus::Screened, DiStatus::Submitted),
            (DiStatus::Deferred, DiStatus::ApprovedForPlanning),
            (DiStatus::ClosedAsNonExecutable, DiStatus::Submitted),
        ];

        for (from, to) in invalid_pairs {
            let err = guard_transition(from, to).expect_err(&format!(
                "Should reject: {} -> {}",
                from.as_str(),
                to.as_str()
            ));
            assert!(
                err.contains(from.as_str()) && err.contains(to.as_str()),
                "Error should mention both states: {}",
                err
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Code generation uniqueness
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_first_generated_code_is_di_0001() {
        let db = setup().await;

        let code = generate_di_code(&db)
            .await
            .expect("code generation should succeed");

        assert_eq!(code, "DI-0001", "First DI code must be DI-0001");
    }

    #[tokio::test]
    async fn v3_sequential_codes_are_unique() {
        let db = setup().await;

        // Insert a DI row so the sequence advances.
        // We need prerequisite rows for FK constraints.
        seed_minimal_fk_data(&db).await;

        let code1 = generate_di_code(&db)
            .await
            .expect("first code gen");
        assert_eq!(code1, "DI-0001");

        // Insert the first DI with code1
        insert_stub_di(&db, &code1).await;

        let code2 = generate_di_code(&db)
            .await
            .expect("second code gen");
        assert_eq!(code2, "DI-0002");

        // Insert the second DI with code2
        insert_stub_di(&db, &code2).await;

        let code3 = generate_di_code(&db)
            .await
            .expect("third code gen");
        assert_eq!(code3, "DI-0003");

        // Verify uniqueness: all three are distinct
        assert_ne!(code1, code2);
        assert_ne!(code2, code3);
        assert_ne!(code1, code3);
    }

    #[tokio::test]
    async fn v3_code_generation_after_gap_continues_sequence() {
        let db = setup().await;
        seed_minimal_fk_data(&db).await;

        // Insert DI-0001
        insert_stub_di(&db, "DI-0001").await;
        // Insert DI-0005 (simulating a gap from import or manual)
        insert_stub_di(&db, "DI-0005").await;

        let next = generate_di_code(&db)
            .await
            .expect("code gen after gap");
        assert_eq!(
            next, "DI-0006",
            "Next code must follow the highest existing sequence, not fill gaps"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 — Immutability flag re-verification
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn v4_immutable_states_correct() {
        let immutable = [
            DiStatus::ConvertedToWorkOrder,
            DiStatus::ClosedAsNonExecutable,
            DiStatus::Rejected,
            DiStatus::Archived,
        ];
        for s in &immutable {
            assert!(
                s.is_immutable_after_conversion(),
                "{} must be immutable",
                s.as_str()
            );
        }
    }

    #[test]
    fn v4_mutable_states_correct() {
        let mutable = [
            DiStatus::Submitted,
            DiStatus::PendingReview,
            DiStatus::ReturnedForClarification,
            DiStatus::Screened,
            DiStatus::AwaitingApproval,
            DiStatus::ApprovedForPlanning,
            DiStatus::Deferred,
        ];
        for s in &mutable {
            assert!(
                !s.is_immutable_after_conversion(),
                "{} must be mutable (not immutable)",
                s.as_str()
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Test helpers — minimal FK data for DI insertion
    // ═══════════════════════════════════════════════════════════════════════

    /// Ensure the seeded data includes at least one equipment row and one org_node.
    /// The seeder only creates lookup domains/values, permissions, and user accounts —
    /// not equipment or org rows. We insert minimal stubs for FK satisfaction.
    async fn seed_minimal_fk_data(db: &sea_orm::DatabaseConnection) {
        // Check if equipment exists
        let equipment_exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM equipment LIMIT 1;".to_string(),
            ))
            .await
            .expect("equipment query");

        if equipment_exists.is_none() {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT INTO equipment (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
                 VALUES (1, 'test-eq-001', 'EQ-TEST-001', 'Test Equipment', 'active_in_service', \
                 datetime('now'), datetime('now'));".to_string(),
            ))
            .await
            .expect("insert test equipment");
        }

        // Check if org_node_types exists (required FK for org_nodes)
        let type_exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_node_types LIMIT 1;".to_string(),
            ))
            .await
            .expect("org_node_types query");

        let type_id: i64 = if let Some(row) = type_exists {
            row.try_get::<i64>("", "id").expect("id")
        } else {
            // Need a structure model first (FK for org_node_types)
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
            1
        };

        // Check if org_nodes exists
        let org_exists = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes LIMIT 1;".to_string(),
            ))
            .await
            .expect("org_nodes query");

        if org_exists.is_none() {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO org_nodes (id, sync_id, code, name, node_type_id, status, created_at, updated_at) \
                 VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', ?, 'active', \
                 datetime('now'), datetime('now'))",
                [type_id.into()],
            ))
            .await
            .expect("insert test org_node");
        }
    }

    /// Insert a minimal intervention_requests row referencing seeded FK data.
    async fn insert_stub_di(db: &sea_orm::DatabaseConnection, code: &str) {
        // Find valid FK ids
        let equipment_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM equipment LIMIT 1;".to_string(),
            ))
            .await
            .expect("query")
            .expect("equipment must exist")
            .try_get::<i64>("", "id")
            .expect("id");

        let org_node_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_nodes LIMIT 1;".to_string(),
            ))
            .await
            .expect("query")
            .expect("org_node must exist")
            .try_get::<i64>("", "id")
            .expect("id");

        let user_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts LIMIT 1;".to_string(),
            ))
            .await
            .expect("query")
            .expect("user must exist")
            .try_get::<i64>("", "id")
            .expect("id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO intervention_requests \
             (code, asset_id, org_node_id, status, title, description, origin_type, \
              impact_level, reported_urgency, submitted_at, submitter_id, created_at, updated_at) \
             VALUES (?, ?, ?, 'submitted', 'Test DI', 'Test description', 'operator', \
              'unknown', 'medium', datetime('now'), ?, datetime('now'), datetime('now'))",
            [
                code.into(),
                equipment_id.into(),
                org_node_id.into(),
                user_id.into(),
            ],
        ))
        .await
        .unwrap_or_else(|e| panic!("insert stub DI '{code}' failed: {e}"));
    }
}
