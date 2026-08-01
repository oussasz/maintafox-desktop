//! Review workflow tests (lifecycle redesign, 7-state model).
//!
//! V1 – Screen action atomicity (transaction rollback on failure).
//! V2 – Return requires non-empty reviewer_note.
//! V3 – Approve records step_up_used in event log.
//! V4 – Defer future-date guard: past dates rejected.
//! V5 – Full lifecycle: create → screen → approve → defer → reactivate with correct event chain.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::di::queries::{create_intervention_request, DiCreateInput};
    use crate::di::review::{
        approve_di, close_di, defer_di, get_review_events, reactivate_deferred_di, return_di_for_clarification,
        screen_di, DiApproveInput, DiCloseInput, DiDeferInput, DiReactivateInput, DiReturnInput, DiScreenInput,
    };

    // ═══════════════════════════════════════════════════════════════════════
    // Setup helpers
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

        seed_fk_data(&db).await;

        db
    }

    async fn seed_fk_data(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO equipment (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'test-eq-001', 'EQ-TEST-001', 'Test Equipment', 'active_in_service', \
             datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert test equipment");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO org_structure_models (id, sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));"
                .to_string(),
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
             VALUES (900001, 900001, 1, 'published', datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert test reference_set");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (id, set_id, code, label, is_active) \
             VALUES (900001, 900001, 'MECH', 'Mécanique', 1);"
                .to_string(),
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

    // ═══════════════════════════════════════════════════════════════════════
    // V1 – Screen action atomicity
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_screen_with_invalid_classification_does_not_change_status() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "high".to_string(),
                review_team_id: None,
                classification_code_id: Some(9999), // non-existent FK
                reviewer_note: None,
            },
        )
        .await;

        assert!(result.is_err(), "screen_di must fail with invalid classification FK");

        let status = get_di_status(&db, di.id).await;
        assert_eq!(status, "in_review", "DI must remain in in_review after failed screen");

        let events = get_review_events(&db, di.id).await.expect("events query");
        assert!(
            events.iter().all(|e| e.event_type == "sla_initialized"),
            "No screening review events should exist after failed screen; found: {:?}",
            events.iter().map(|e| &e.event_type).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn v1_screen_with_invalid_urgency_does_not_change_status() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "INVALID_URGENCY".to_string(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: None,
            },
        )
        .await;

        assert!(result.is_err(), "screen_di must fail with invalid urgency");

        let status = get_di_status(&db, di.id).await;
        assert_eq!(status, "in_review", "DI stays in in_review");
    }

    #[tokio::test]
    async fn v1_screen_wrong_status_rejects_transition() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        // DI is still 'submitted', not 'in_review'

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 1,
                validated_urgency: "high".to_string(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "screen_di must fail when DI is in 'submitted' (not in_review)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 – Return requires non-empty reviewer_note
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_return_with_empty_note_returns_validation_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let result = return_di_for_clarification(
            &db,
            DiReturnInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                reviewer_note: "".to_string(),
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

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let result = return_di_for_clarification(
            &db,
            DiReturnInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                reviewer_note: "   ".to_string(),
            },
        )
        .await;

        assert!(result.is_err(), "return must fail with whitespace-only note");
    }

    #[tokio::test]
    async fn v2_return_with_valid_note_succeeds() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

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
        assert_eq!(updated.reviewer_note.as_deref(), Some("Please add sensor readings."));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 – Approve records step_up_used in event log
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_approve_records_step_up_used_in_event_log() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "high".to_string(),
                review_team_id: Some(1),
                classification_code_id: Some(900001),
                reviewer_note: Some("Screened OK".to_string()),
            },
        )
        .await
        .expect("screen should succeed");
        assert_eq!(screened.status, "awaiting_approval");

        let approved = approve_di(
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
        assert_eq!(approved.status, "approved");
        assert!(approved.approved_at.is_some(), "approved_at must be set");

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

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        // Try to approve directly from in_review — must fail
        let result = approve_di(
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
            "approve must fail when DI is in in_review (not awaiting_approval)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 – Defer future-date guard
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v4_defer_with_past_date_returns_validation_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "medium".to_string(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen");

        let approved = approve_di(
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
                deferred_until: "2020-01-01".to_string(),
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

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "medium".to_string(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen");

        let approved = approve_di(
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

        assert!(
            result.is_err(),
            "defer must fail with today's date (not strictly future)"
        );
    }

    #[tokio::test]
    async fn v4_defer_with_empty_reason_returns_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "medium".to_string(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen");

        let approved = approve_di(
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
                reason_code: "".to_string(),
                notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "defer must fail with empty reason_code");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V5 – Full lifecycle with correct 7-state event chain
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v5_full_lifecycle_screen_approve_defer_reactivate() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // 1. Create DI (submitted)
        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        assert_eq!(di.status, "submitted");

        // 2. Advance to in_review
        advance_to_in_review(&db, di.id).await;
        let version = get_row_version(&db, di.id).await;

        // 3. Screen (in_review → awaiting_approval)
        let screened = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: version,
                validated_urgency: "high".to_string(),
                review_team_id: Some(1),
                classification_code_id: Some(900001),
                reviewer_note: Some("Validated vibration concern".to_string()),
            },
        )
        .await
        .expect("screen should succeed");
        assert_eq!(screened.status, "awaiting_approval");
        assert!(screened.screened_at.is_some(), "screened_at must be set");
        assert_eq!(screened.validated_urgency.as_deref(), Some("high"));

        // 4. Approve (awaiting_approval → approved)
        let approved = approve_di(
            &db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: Some("Approved — schedule next shutdown window".to_string()),
            },
        )
        .await
        .expect("approve should succeed");
        assert_eq!(approved.status, "approved");
        assert!(approved.approved_at.is_some(), "approved_at must be set");

        // 5. Defer (approved → deferred, stores deferred_from_status = "approved")
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
        assert_eq!(deferred.deferred_until.as_deref(), Some("2099-06-15"));
        assert_eq!(deferred.deferred_from_status.as_deref(), Some("approved"));

        // 6. Reactivate (deferred → approved via deferred_from_status)
        let reactivated = reactivate_deferred_di(
            &db,
            DiReactivateInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: deferred.row_version,
                notes: Some("Budget approved — reactivate for scheduling".to_string()),
            },
        )
        .await
        .expect("reactivate should succeed");
        assert_eq!(reactivated.status, "approved");
        assert!(
            reactivated.deferred_until.is_none(),
            "deferred_until must be cleared after reactivation"
        );

        // screened_at and approved_at survive reactivation
        assert!(
            reactivated.screened_at.is_some(),
            "screened_at must survive reactivation"
        );
        assert!(
            reactivated.approved_at.is_some(),
            "approved_at must survive reactivation"
        );

        // Event log: sla_initialized + screen + approve + defer + reactivate
        let events = get_review_events(&db, di.id).await.expect("events");
        let event_types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            event_types,
            vec!["sla_initialized", "screened", "approved", "deferred", "reactivated"],
            "Event types must follow lifecycle order"
        );

        // Status chain (skip sla_initialized intake event)
        let lifecycle: Vec<_> = events.iter().filter(|e| e.event_type != "sla_initialized").collect();
        assert_eq!(lifecycle[0].from_status, "in_review");
        assert_eq!(lifecycle[0].to_status, "awaiting_approval");
        assert_eq!(lifecycle[1].from_status, "awaiting_approval");
        assert_eq!(lifecycle[1].to_status, "approved");
        assert_eq!(lifecycle[2].from_status, "approved");
        assert_eq!(lifecycle[2].to_status, "deferred");
        assert_eq!(lifecycle[3].from_status, "deferred");
        assert_eq!(lifecycle[3].to_status, "approved");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Close with disposition
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn close_with_no_work_required_succeeds() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let result = close_di(
            &db,
            DiCloseInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv,
                disposition_code: "no_work_required".to_string(),
                notes: Some("Equipment was functioning normally on inspection.".to_string()),
                related_di_id: None,
            },
        )
        .await
        .expect("close with no_work_required should succeed");

        assert_eq!(result.status, "closed");
        assert_eq!(result.disposition_code.as_deref(), Some("no_work_required"));
        assert_eq!(result.closed_by_id, Some(user_id));
        assert!(result.closed_at.is_some());

        // Verify review event
        let events = get_review_events(&db, di.id).await.expect("events");
        let close_event = events.iter().find(|e| e.event_type == "closed").expect("closed event");
        assert_eq!(close_event.reason_code.as_deref(), Some("no_work_required"));
    }

    #[tokio::test]
    async fn close_with_duplicate_requires_related_di_id() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let result = close_di(
            &db,
            DiCloseInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv,
                disposition_code: "duplicate".to_string(),
                notes: None,
                related_di_id: None, // required for duplicate
            },
        )
        .await;

        assert!(result.is_err(), "close with duplicate but no related_di_id must fail");
    }

    #[tokio::test]
    async fn close_with_other_requires_non_empty_notes() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let result = close_di(
            &db,
            DiCloseInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv,
                disposition_code: "other".to_string(),
                notes: None,
                related_di_id: None,
            },
        )
        .await;

        assert!(result.is_err(), "close with other and no notes must fail");
    }

    #[tokio::test]
    async fn close_with_converted_to_wo_via_close_path_is_rejected() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;
        let rv = get_row_version(&db, di.id).await;

        let result = close_di(
            &db,
            DiCloseInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: rv,
                disposition_code: "converted_to_wo".to_string(),
                notes: Some("Should use convert path".to_string()),
                related_di_id: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "close with converted_to_wo disposition via close path must be rejected"
        );
    }

    #[tokio::test]
    async fn close_from_submitted_is_rejected_use_cancel_own() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");

        let result = close_di(
            &db,
            DiCloseInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: di.row_version,
                disposition_code: "cancelled_by_requester".to_string(),
                notes: None,
                related_di_id: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "close_di from submitted must fail (use cancel_own_di instead)"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Optimistic concurrency on review actions
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn screen_with_stale_row_version_fails() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id).await)
            .await
            .expect("create DI");
        advance_to_in_review(&db, di.id).await;

        let result = screen_di(
            &db,
            DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 1, // stale — should be 2 after advance
                validated_urgency: "high".to_string(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: None,
            },
        )
        .await;

        assert!(result.is_err(), "screen must fail with stale row_version");

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("version") || err_str.contains("modifié"),
            "Error must mention version conflict: got '{err_str}'"
        );
    }
}
