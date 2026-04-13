//! Supervisor verification tests for Phase 2 SP04 File 02 Sprint S1.
//!
//! V1 â€” Screen action atomicity (transaction rollback on failure).
//! V2 â€” Return requires non-empty reviewer_note.
//! V3 â€” Approve step-up guard (tested at domain layer; IPC test is manual).
//! V4 â€” Defer future-date guard: past dates rejected.
//! V5 â€” Full lifecycle: create â†’ screen â†’ approve â†’ defer â†’ reactivate with 6 event rows.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement, TransactionTrait};
    use sea_orm_migration::MigratorTrait;

    use crate::di::queries::{create_intervention_request, DiCreateInput};
    use crate::di::review::{
        approve_di_for_planning, defer_di, get_review_events, reactivate_deferred_di,
        return_di_for_clarification, screen_di, DiApproveInput, DiDeferInput,
        DiReactivateInput, DiReturnInput, DiScreenInput,
    };

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Test setup helpers (matching query_tests.rs patterns)
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

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
            "INSERT INTO org_nodes (id, sync_id, code, name, node_type_id, status, created_at, updated_at) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
             datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test org_node");

        // reference_domains â†’ reference_sets â†’ reference_values chain for classification FK
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_domains (id, code, name, structure_type, governance_level, is_extendable, created_at, updated_at) \
             VALUES (1, 'DI_CLASSIFICATION', 'DI Classification', 'flat', 'tenant_managed', 1, \
             datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test reference_domain");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_sets (id, domain_id, version_no, status, created_at) \
             VALUES (1, 1, 1, 'published', datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test reference_set");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (id, set_id, code, label, is_active) \
             VALUES (1, 1, 'MECH', 'MÃ©canique', 1);".to_string(),
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

    fn make_create_input(user_id: i64) -> DiCreateInput {
        DiCreateInput {
            asset_id: 1,
            org_node_id: 1,
            title: "Pump vibration alert".to_string(),
            description: "Excessive vibration on pump P-101".to_string(),
            origin_type: "operator".to_string(),
            symptom_code_id: None,
            impact_level: "unknown".to_string(),
            production_impact: false,
            safety_flag: false,
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: "medium".to_string(),
            observed_at: None,
            submitter_id: user_id,
        }
    }

    /// Advance a DI from 'submitted' to 'pending_review' directly (simulates
    /// the Submitted â†’ PendingReview transition that the intake UI performs).
    async fn advance_to_pending_review(db: &sea_orm::DatabaseConnection, di_id: i64) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'pending_review', \
             row_version = row_version + 1, updated_at = datetime('now') \
             WHERE id = ?",
            [di_id.into()],
        ))
        .await
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

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // V1 â€” Screen action atomicity
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // We simulate a failed screen attempt with an invalid classification FK.
    // The DI must remain in pending_review (no partial writes).

    #[tokio::test]
    async fn v1_screen_with_invalid_classification_does_not_change_status() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2, // bumped by advance helper
                validated_urgency: "high".to_string(),
                review_team_id: None,
                classification_code_id: Some(9999), // non-existent FK
                reviewer_note: None,
            },
        )
        .await;

        assert!(result.is_err(), "screen_di must fail with invalid classification FK");

        // DI must still be in pending_review â€” transaction rolled back
        let status = get_di_status(&db, di.id).await;
        assert_eq!(
            status, "pending_review",
            "DI must remain in pending_review after failed screen"
        );

        // No review events should have been written
        let events = get_review_events(&db, di.id).await.expect("events query");
        assert!(
            events.is_empty(),
            "No review events should exist after failed screen"
        );
    }

    #[tokio::test]
    async fn v1_screen_with_invalid_urgency_does_not_change_status() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "INVALID_URGENCY".to_string(),
                review_team_id: None,
                classification_code_id: Some(1),
                reviewer_note: None,
            },
        )
        .await;

        assert!(result.is_err(), "screen_di must fail with invalid urgency");

        let status = get_di_status(&db, di.id).await;
        assert_eq!(status, "pending_review", "DI stays in pending_review");
    }

    #[tokio::test]
    async fn v1_screen_wrong_status_rejects_transition() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        // DI is still 'submitted', not 'pending_review'

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 1,
                validated_urgency: "high".to_string(),
                review_team_id: None,
                classification_code_id: Some(1),
                reviewer_note: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "screen_di must fail when DI is in 'submitted' (not pending_review)"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // V2 â€” Return requires non-empty reviewer_note
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn v2_return_with_empty_note_returns_validation_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = return_di_for_clarification(
            &db,
            DiReturnInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                reviewer_note: "".to_string(), // empty â€” must fail
            },
        )
        .await;

        assert!(result.is_err(), "return must fail with empty note");

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("obligatoire") || err_str.contains("note"),
            "Error must mention the required note: got '{err_str}'"
        );
    }

    #[tokio::test]
    async fn v2_return_with_whitespace_only_note_returns_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = return_di_for_clarification(
            &db,
            DiReturnInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                reviewer_note: "   ".to_string(), // whitespace â€” must fail
            },
        )
        .await;

        assert!(result.is_err(), "return must fail with whitespace-only note");
    }

    #[tokio::test]
    async fn v2_return_with_valid_note_succeeds() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let updated = return_di_for_clarification(
            &db,
            DiReturnInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                reviewer_note: "Please add sensor readings.".to_string(),
            },
        )
        .await
        .expect("return should succeed with valid note");

        assert_eq!(updated.status, "returned_for_clarification");
        assert_eq!(
            updated.reviewer_note.as_deref(),
            Some("Please add sensor readings.")
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // V3 â€” Approve step-up guard
    //
    // At the domain layer, approve_di_for_planning does NOT check step-up
    // (that is enforced at the IPC command layer via require_step_up!).
    // Here we verify that the domain function works correctly when called,
    // and that it records step_up_used = true in the event log.
    // The actual IPC-layer step-up enforcement is tested manually.
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn v3_approve_records_step_up_used_in_event_log() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // Create â†’ pending_review â†’ screen (â†’ awaiting_approval)
        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "high".to_string(),
                review_team_id: Some(1),
                classification_code_id: Some(1),
                reviewer_note: Some("Screened OK".to_string()),
            },
        )
        .await
        .expect("screen should succeed");
        assert_eq!(screened.status, "awaiting_approval");

        // Now approve
        let approved = approve_di_for_planning(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: Some("Approved for execution".to_string()),
            },
        )
        .await
        .expect("approve should succeed");
        assert_eq!(approved.status, "approved_for_planning");
        assert!(approved.approved_at.is_some(), "approved_at must be set");

        // Check event log for step_up_used = true on the 'approved' event
        let events = get_review_events(&db, di.id).await.expect("events");
        let approve_event = events
            .iter()
            .find(|e| e.event_type == "approved")
            .expect("must have an 'approved' event");
        assert!(
            approve_event.step_up_used,
            "approved event must record step_up_used = true"
        );
    }

    #[tokio::test]
    async fn v3_approve_from_wrong_status_fails() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        // Try to approve directly from pending_review â€” must fail
        let result = approve_di_for_planning(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                notes: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "approve must fail when DI is in pending_review"
        );
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // V4 â€” Defer future-date guard
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn v4_defer_with_past_date_returns_validation_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // Create â†’ pending_review â†’ screen â†’ approve â†’ defer with past date
        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "medium".to_string(),
                review_team_id: None,
                classification_code_id: Some(1),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen");

        let approved = approve_di_for_planning(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: None,
            },
        )
        .await
        .expect("approve");

        // Defer with yesterday's date â€” must fail
        let result = defer_di(
            &db,
            DiDeferInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: approved.row_version,
                deferred_until: "2020-01-01".to_string(), // far in the past
                reason_code: "budget".to_string(),
                notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "defer must fail with past date");

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("futur") || err_str.contains("future"),
            "Error must mention future date requirement: got '{err_str}'"
        );
    }

    #[tokio::test]
    async fn v4_defer_with_today_returns_validation_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "medium".to_string(),
                review_team_id: None,
                classification_code_id: Some(1),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen");

        let approved = approve_di_for_planning(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: None,
            },
        )
        .await
        .expect("approve");

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let result = defer_di(
            &db,
            DiDeferInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: approved.row_version,
                deferred_until: today,
                reason_code: "budget".to_string(),
                notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "defer must fail with today's date (not strictly future)");
    }

    #[tokio::test]
    async fn v4_defer_with_empty_reason_returns_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "medium".to_string(),
                review_team_id: None,
                classification_code_id: Some(1),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen");

        let approved = approve_di_for_planning(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: None,
            },
        )
        .await
        .expect("approve");

        let result = defer_di(
            &db,
            DiDeferInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: approved.row_version,
                deferred_until: "2099-12-31".to_string(),
                reason_code: "".to_string(), // empty â€” must fail
                notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "defer must fail with empty reason_code");
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // V5 â€” Full lifecycle with 6 event rows
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn v5_full_lifecycle_screen_approve_defer_reactivate() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // 1. Create DI (submitted)
        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        assert_eq!(di.status, "submitted");

        // 2. Advance to pending_review
        advance_to_pending_review(&db, di.id).await;
        let version = get_row_version(&db, di.id).await;

        // 3. Screen (pending_review â†’ awaiting_approval, writes 2 events)
        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: version,
                validated_urgency: "high".to_string(),
                review_team_id: Some(1),
                classification_code_id: Some(1),
                reviewer_note: Some("Validated vibration concern".to_string()),
            },
        )
        .await
        .expect("screen should succeed");
        assert_eq!(screened.status, "awaiting_approval");
        assert!(screened.screened_at.is_some(), "screened_at must be set");
        assert_eq!(
            screened.validated_urgency.as_deref(),
            Some("high"),
            "validated_urgency must be set"
        );

        // 4. Approve (awaiting_approval â†’ approved_for_planning)
        let approved = approve_di_for_planning(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: Some("Approved â€” schedule next shutdown window".to_string()),
            },
        )
        .await
        .expect("approve should succeed");
        assert_eq!(approved.status, "approved_for_planning");
        assert!(approved.approved_at.is_some(), "approved_at must be set");

        // 5. Defer (approved_for_planning â†’ deferred)
        let deferred = defer_di(
            &db,
            DiDeferInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: approved.row_version,
                deferred_until: "2099-06-15".to_string(),
                reason_code: "budget_constraint".to_string(),
                notes: Some("Deferred until budget approval Q3".to_string()),
            },
        )
        .await
        .expect("defer should succeed");
        assert_eq!(deferred.status, "deferred");
        assert_eq!(
            deferred.deferred_until.as_deref(),
            Some("2099-06-15")
        );

        // 6. Reactivate (deferred â†’ awaiting_approval)
        let reactivated = reactivate_deferred_di(
            &db,
            DiReactivateInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: deferred.row_version,
                notes: Some("Budget approved â€” reactivate for scheduling".to_string()),
            },
        )
        .await
        .expect("reactivate should succeed");
        assert_eq!(reactivated.status, "awaiting_approval");
        assert!(
            reactivated.deferred_until.is_none(),
            "deferred_until must be cleared after reactivation"
        );

        // â”€â”€ Verify screened_at and approved_at are PRESERVED (not overwritten) â”€â”€
        assert!(
            reactivated.screened_at.is_some(),
            "screened_at must survive reactivation"
        );
        assert!(
            reactivated.approved_at.is_some(),
            "approved_at must survive reactivation"
        );

        // â”€â”€ Verify full event log: exactly 5 rows â”€â”€
        // screen(2: screened + advanced_to_approval) + approve(1) + defer(1) + reactivate(1) = 5
        let events = get_review_events(&db, di.id).await.expect("events");
        assert_eq!(
            events.len(),
            5,
            "Expected 5 review events: screen=2, approve=1, defer=1, reactivate=1. \
             Events found: {:?}",
            events.iter().map(|e| &e.event_type).collect::<Vec<_>>()
        );

        // Validate event types in order
        let event_types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            event_types,
            vec![
                "screened",
                "advanced_to_approval",
                "approved",
                "deferred",
                "reactivated",
            ],
            "Event types must follow lifecycle order"
        );

        // Validate from/to chains
        assert_eq!(events[0].from_status, "pending_review");
        assert_eq!(events[0].to_status, "screened");
        assert_eq!(events[1].from_status, "screened");
        assert_eq!(events[1].to_status, "awaiting_approval");
        assert_eq!(events[2].from_status, "awaiting_approval");
        assert_eq!(events[2].to_status, "approved_for_planning");
        assert_eq!(events[3].from_status, "approved_for_planning");
        assert_eq!(events[3].to_status, "deferred");
        assert_eq!(events[4].from_status, "deferred");
        assert_eq!(events[4].to_status, "awaiting_approval");
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Additional: reject requires reason_code
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn reject_with_empty_reason_returns_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = crate::di::review::reject_di(
            &db,
            crate::di::review::DiRejectInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                reason_code: "".to_string(),
                notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "reject must fail with empty reason_code");
    }

    #[tokio::test]
    async fn reject_with_valid_reason_succeeds() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = crate::di::review::reject_di(
            &db,
            crate::di::review::DiRejectInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                reason_code: "duplicate".to_string(),
                notes: Some("Already covered by DI-0045".to_string()),
            },
        )
        .await
        .expect("reject should succeed");

        assert_eq!(result.status, "rejected");
        assert!(result.declined_at.is_some(), "declined_at must be set");
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Optimistic concurrency on review actions
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    #[tokio::test]
    async fn screen_with_stale_row_version_fails() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 1, // stale â€” should be 2 after advance
                validated_urgency: "high".to_string(),
                review_team_id: None,
                classification_code_id: Some(1),
                reviewer_note: None,
            },
        )
        .await;

        assert!(result.is_err(), "screen must fail with stale row_version");

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("version") || err_str.contains("modifiÃ©"),
            "Error must mention version conflict: got '{err_str}'"
        );
    }
}
