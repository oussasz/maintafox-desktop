//! Supervisor verification tests for Phase 2 SP04 File 01 Sprint S2.
//!
//! V1 — Create DI end-to-end: row + transition log.
//! V2 — Draft update guard: reject if status is not draft.
//! V3 — Optimistic concurrency: reject stale row_version.
//! V4 — Search filter: free-text search returns only matching DIs.
//! V5 — Recurrence query: recent similar DIs on same asset + symptom.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::di::domain::InterventionRequest;
    use crate::di::queries::{
        create_intervention_request, get_di_transition_log, get_intervention_request, get_recent_similar_dis,
        list_intervention_requests, update_di_draft_fields, DiCreateInput, DiDraftUpdateInput, DiListFilter,
    };

    /// In-memory SQLite with all migrations applied + seeded system data.
    async fn setup() -> sea_orm::DatabaseConnection {
        let db = crate::db::test_db::seeded().await;

        // Seed FK prerequisite data (equipment, org_nodes)
        seed_fk_data(&db).await;

        db
    }

    /// Insert minimal FK data: equipment + org_structure_models + org_node_types + org_nodes.
    async fn seed_fk_data(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'test-eq-001', 'EQ-TEST-001', 'Test Equipment', 'active_in_service', \
             datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert test equipment");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'test-model-001', 1, 'active', datetime('now'), datetime('now'));"
                .to_string(),
        ))
        .await
        .expect("insert test structure model");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'test-type-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'));".to_string(),
        ))
        .await
        .expect("insert test node type");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO org_nodes (id, entity_sync_id, code, name, node_type_id, status, created_at, updated_at, structure_model_id) \
             VALUES (1, 'test-org-001', 'SITE-001', 'Test Site', 1, 'active', \
             datetime('now'), datetime('now'), 1);".to_string(),
        ))
        .await
        .expect("insert test org_node");
    }

    /// Find the first user_accounts id (seeded by system seeder).
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

    /// Helper to build a standard DiCreateInput.
    async fn make_create_input(
        db: &sea_orm::DatabaseConnection,
        user_id: i64,
        title: &str,
        description: &str,
    ) -> DiCreateInput {
        DiCreateInput {
            equipment_id: 1,
            org_node_id: 1,
            title: title.to_string(),
            description: description.to_string(),
            origin_type: "operator".to_string(),
            request_type: "repair".to_string(),
            symptom_code_id: Some(seeded_symptom_id(db).await),
            impact_level: "unknown".to_string(),
            production_impact: false,
            safety_flag: false,
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: "normal".to_string(),
            observed_at: None,
            source_inspection_anomaly_id: None,
            submitter_id: user_id,
        }
    }

    #[tokio::test]
    async fn create_rejects_unknown_origin_code() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let mut input = make_create_input(&db, user_id, "Bad origin", "Desc").await;
        input.origin_type = "not-in-catalog".to_string();
        let err = create_intervention_request(&db, input)
            .await
            .expect_err("unknown origin must fail");
        assert!(matches!(err, crate::errors::AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn create_rejects_null_symptom() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let mut input = make_create_input(&db, user_id, "No symptom", "Desc").await;
        input.symptom_code_id = None;
        let err = create_intervention_request(&db, input)
            .await
            .expect_err("null symptom must fail");
        assert!(matches!(err, crate::errors::AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn create_rejects_unknown_symptom_id() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let mut input = make_create_input(&db, user_id, "Bad symptom", "Desc").await;
        input.symptom_code_id = Some(9_999_999);
        let err = create_intervention_request(&db, input)
            .await
            .expect_err("unknown symptom must fail");
        assert!(matches!(err, crate::errors::AppError::ValidationFailed(_)));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Create DI end-to-end
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_create_di_returns_submitted_with_code_and_timestamp() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(
                &db,
                user_id,
                "Pump vibration alert",
                "Excessive vibration on pump P-101",
            )
            .await,
        )
        .await
        .expect("create_di should succeed");

        let dev = crate::sync::device_code::this_device_code(&db)
            .await
            .expect("device code");
        assert_eq!(di.code, format!("DI-{dev}-0001"));
        assert_eq!(di.status, "submitted");
        assert_eq!(di.title, "Pump vibration alert");
        assert_eq!(di.origin_type, "operator");
        assert_eq!(di.impact_level, "unknown");
        assert_eq!(di.reported_urgency, "normal");
        assert!(!di.submitted_at.is_empty(), "submitted_at must be set");
        assert_eq!(di.row_version, 1);
        assert_eq!(di.submitter_id, user_id);
    }

    #[tokio::test]
    async fn v1_create_di_writes_transition_log() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id, "Test DI", "Description").await)
            .await
            .expect("create_di");

        let log = get_di_transition_log(&db, di.id).await.expect("log");

        assert_eq!(log.len(), 1, "exactly one transition log row on create");
        assert_eq!(log[0].from_status, "none");
        assert_eq!(log[0].to_status, "submitted");
        assert_eq!(log[0].action, "intake_submitted");
        assert_eq!(log[0].actor_id, Some(user_id));
    }

    #[tokio::test]
    async fn v1_get_di_returns_created_row() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let created =
            create_intervention_request(&db, make_create_input(&db, user_id, "Fetched DI", "Fetch test").await)
                .await
                .expect("create_di");

        let fetched = get_intervention_request(&db, created.id)
            .await
            .expect("get_di")
            .expect("DI must exist");

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.code, created.code);
        assert_eq!(fetched.title, "Fetched DI");
        assert!(
            fetched.submitter_display_name.is_some(),
            "submitter_display_name must be enriched"
        );
        assert!(
            fetched.asset_code.is_some() || fetched.asset_label.is_some(),
            "asset display fields must be enriched when equipment exists"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Draft update guard
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_update_draft_rejects_non_draft_status() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(&db, user_id, "Guard test", "Guard description").await,
        )
        .await
        .expect("create_di");

        // Forcibly set status to 'awaiting_approval' (a non-draft state)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'awaiting_approval' WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("force status");

        let result = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 1,
                title: Some("New title".into()),
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
            None,
        )
        .await;

        assert!(result.is_err(), "Should reject update on a DI awaiting approval");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("awaiting_approval"),
            "Error should mention current status: {err}"
        );
    }

    #[tokio::test]
    async fn v2_update_draft_allowed_on_submitted() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(&db, user_id, "Original title", "Original desc").await,
        )
        .await
        .expect("create_di");

        let updated = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 1,
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
            None,
        )
        .await
        .expect("update should succeed on submitted DI");

        assert_eq!(updated.title, "Updated title");
        assert_eq!(updated.row_version, 2, "row_version must increment");
    }

    #[tokio::test]
    async fn v2_update_draft_allowed_on_returned_for_clarification() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(&db, user_id, "RFC test", "RFC desc").await)
            .await
            .expect("create_di");

        // Forcibly set status to 'returned_for_clarification'
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'returned_for_clarification' WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("force status");

        let updated = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 1,
                title: Some("Clarified title".into()),
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
            None,
        )
        .await
        .expect("update should succeed on returned_for_clarification DI");

        assert_eq!(updated.title, "Clarified title");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Optimistic concurrency
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_stale_row_version_returns_error() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(&db, user_id, "Concurrency test", "Concurrency desc").await,
        )
        .await
        .expect("create_di");

        // First update succeeds
        update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 1,
                title: Some("Update 1".into()),
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
            None,
        )
        .await
        .expect("first update should succeed");

        // Second update with stale version (1 instead of 2) must fail
        let result = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 1, // stale
                title: Some("Update 2".into()),
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
            None,
        )
        .await;

        assert!(result.is_err(), "Stale row_version must return concurrency error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("version") || err.contains("Conflit"),
            "Error should mention version conflict: {err}"
        );
    }

    #[tokio::test]
    async fn v3_correct_row_version_succeeds_after_prior_update() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(&db, user_id, "Sequential update", "Seq desc").await,
        )
        .await
        .expect("create_di");

        // First update: v1 -> v2
        update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 1,
                title: Some("V2 title".into()),
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
            None,
        )
        .await
        .expect("v1->v2 update");

        // Second update: v2 -> v3 (correct version)
        let updated = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 2,
                title: Some("V3 title".into()),
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
            None,
        )
        .await
        .expect("v2->v3 update should succeed");

        assert_eq!(updated.title, "V3 title");
        assert_eq!(updated.row_version, 3);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 — Search filter
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v4_search_filter_returns_only_matching_di() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // Create 3 DIs with distinct titles
        create_intervention_request(
            &db,
            make_create_input(
                &db,
                user_id,
                "Motor overheating alert",
                "Motor M-200 temp above threshold",
            )
            .await,
        )
        .await
        .expect("di1");

        create_intervention_request(
            &db,
            make_create_input(&db, user_id, "Conveyor belt misalignment", "Belt CV-300 drifting").await,
        )
        .await
        .expect("di2");

        create_intervention_request(
            &db,
            make_create_input(
                &db,
                user_id,
                "Hydraulic leak on press",
                "Oil pooling under press HP-400",
            )
            .await,
        )
        .await
        .expect("di3");

        // Search for "conveyor" → should return only DI-0002
        let result = list_intervention_requests(
            &db,
            DiListFilter {
                search: Some("conveyor".to_string()),
                limit: 50,
                offset: 0,
                ..Default::default()
            },
        )
        .await
        .expect("search");

        assert_eq!(result.total, 1, "Search for 'conveyor' should match exactly 1 DI");
        assert_eq!(result.items[0].title, "Conveyor belt misalignment");
    }

    #[tokio::test]
    async fn v4_status_filter_returns_only_matching_status() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di1 = create_intervention_request(&db, make_create_input(&db, user_id, "DI-A", "A").await)
            .await
            .expect("di1");

        create_intervention_request(&db, make_create_input(&db, user_id, "DI-B", "B").await)
            .await
            .expect("di2");

        // Set di1 to awaiting approval
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'awaiting_approval' WHERE id = ?",
            [di1.id.into()],
        ))
        .await
        .expect("force status");

        // Filter by awaiting approval
        let result = list_intervention_requests(
            &db,
            DiListFilter {
                status: Some(vec!["awaiting_approval".to_string()]),
                limit: 50,
                offset: 0,
                ..Default::default()
            },
        )
        .await
        .expect("filter");

        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].status, "awaiting_approval");
    }

    #[tokio::test]
    async fn v4_list_returns_paginated_total() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        for i in 0..5 {
            create_intervention_request(
                &db,
                make_create_input(&db, user_id, &format!("DI {i}"), &format!("Desc {i}")).await,
            )
            .await
            .expect("create");
        }

        // Page 1: limit 2
        let page1 = list_intervention_requests(
            &db,
            DiListFilter {
                limit: 2,
                offset: 0,
                ..Default::default()
            },
        )
        .await
        .expect("page1");

        assert_eq!(page1.total, 5, "Total should be 5 regardless of page size");
        assert_eq!(page1.items.len(), 2, "Page 1 should return 2 items");

        // Page 2: limit 2, offset 2
        let page2 = list_intervention_requests(
            &db,
            DiListFilter {
                limit: 2,
                offset: 2,
                ..Default::default()
            },
        )
        .await
        .expect("page2");

        assert_eq!(page2.total, 5);
        assert_eq!(page2.items.len(), 2);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V5 — Recurrence query
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v5_recent_similar_dis_returns_same_asset() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // Create 2 DIs on asset 1
        create_intervention_request(
            &db,
            make_create_input(&db, user_id, "Recurrence A", "First occurrence").await,
        )
        .await
        .expect("di-a");

        create_intervention_request(
            &db,
            make_create_input(&db, user_id, "Recurrence B", "Second occurrence").await,
        )
        .await
        .expect("di-b");

        let similar = get_recent_similar_dis(&db, 1, None, 7).await.expect("similar query");

        assert_eq!(
            similar.len(),
            2,
            "Both DIs on same asset should appear in recent similar"
        );
        // Both created in same test instant, so we just verify both titles are present
        let titles: Vec<&str> = similar.iter().map(|s| s.title.as_str()).collect();
        assert!(
            titles.contains(&"Recurrence A") && titles.contains(&"Recurrence B"),
            "Both recurrence DIs must appear: {:?}",
            titles
        );
    }

    #[tokio::test]
    async fn v5_similar_dis_limited_to_5() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        for i in 0..8 {
            create_intervention_request(
                &db,
                make_create_input(&db, user_id, &format!("Similar {i}"), &format!("Desc {i}")).await,
            )
            .await
            .expect("create");
        }

        let similar = get_recent_similar_dis(&db, 1, None, 30).await.expect("similar query");

        assert!(
            similar.len() <= 5,
            "get_recent_similar_dis must return at most 5 rows, got {}",
            similar.len()
        );
    }

    // ─── Work queues, SLA state and the requester's answer ───────────────────

    async fn set_status(db: &sea_orm::DatabaseConnection, id: i64, status: &str) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = ?, row_version = row_version + 1 WHERE id = ?",
            [status.into(), id.into()],
        ))
        .await
        .expect("set status");
    }

    /// Moves the submission (and the frozen deadlines with it) `hours` into the past.
    async fn age_request(db: &sea_orm::DatabaseConnection, id: i64, hours: i64) {
        let shift = format!("-{hours} hours");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET \
                submitted_at = strftime('%Y-%m-%dT%H:%M:%SZ', submitted_at, ?), \
                sla_response_deadline = strftime('%Y-%m-%dT%H:%M:%SZ', sla_response_deadline, ?), \
                sla_resolution_deadline = strftime('%Y-%m-%dT%H:%M:%SZ', sla_resolution_deadline, ?) \
             WHERE id = ?",
            [
                shift.clone().into(),
                shift.clone().into(),
                shift.clone().into(),
                id.into(),
            ],
        ))
        .await
        .expect("age request");
        // The history moves with the request, so its pauses keep their place on the clock.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE di_state_transition_log SET acted_at = strftime('%Y-%m-%dT%H:%M:%SZ', acted_at, ?) \
             WHERE di_id = ?",
            [shift.into(), id.into()],
        ))
        .await
        .expect("age history");
    }

    /// Records a status change the way the review functions do (row + transition log entry).
    async fn move_to(db: &sea_orm::DatabaseConnection, id: i64, from: &str, to: &str) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = ?, row_version = row_version + 1 WHERE id = ?",
            [to.into(), id.into()],
        ))
        .await
        .expect("move status");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO di_state_transition_log (di_id, from_status, to_status, action, actor_id, acted_at) \
             VALUES (?, ?, ?, 'test', NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            [id.into(), from.into(), to.into()],
        ))
        .await
        .expect("log transition");
    }

    async fn sla_of(db: &sea_orm::DatabaseConnection, id: i64) -> crate::di::sla::DiSlaStatus {
        let di = get_intervention_request(db, id).await.expect("get").expect("row");
        crate::di::sla::compute_sla_status(db, &di).await.expect("sla")
    }

    #[tokio::test]
    async fn a_request_waiting_on_its_requester_does_not_breach_and_the_wait_extends_the_deadline() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let di = create_intervention_request(&db, make_create_input(&db, user_id, "Waiting", "d").await)
            .await
            .expect("create");
        move_to(&db, di.id, "submitted", "in_review").await;
        move_to(&db, di.id, "in_review", "returned_for_clarification").await;
        // Submitted, sent back at once, and still waiting a year later.
        age_request(&db, di.id, 24 * 365).await;

        let waiting = get_intervention_request(&db, di.id).await.expect("get").expect("row");
        assert_eq!(waiting.sla_state.as_deref(), Some("paused"));
        assert!(waiting.sla_paused_hours > 24.0 * 364.0, "{}", waiting.sla_paused_hours);
        let status = sla_of(&db, di.id).await;
        assert!(!status.is_response_breached && !status.is_resolution_breached);
        let ids = |q: &'static str| {
            let db = db.clone();
            async move {
                list_intervention_requests(&db, queue_filter(q))
                    .await
                    .expect("list")
                    .items
                    .iter()
                    .map(|d| d.id)
                    .collect::<Vec<_>>()
            }
        };
        assert!(ids("sla_at_risk").await.is_empty(), "a paused request is not at risk");

        // The requester answers: the clock runs again, the year of waiting is not counted.
        move_to(&db, di.id, "returned_for_clarification", "in_review").await;
        let back = get_intervention_request(&db, di.id).await.expect("get").expect("row");
        assert_eq!(back.sla_state.as_deref(), Some("on_track"));
        let status = sla_of(&db, di.id).await;
        let frozen = chrono::NaiveDateTime::parse_from_str(
            back.sla_response_deadline.as_deref().expect("frozen"),
            "%Y-%m-%dT%H:%M:%SZ",
        )
        .expect("parse");
        let extended = chrono::NaiveDateTime::parse_from_str(
            status.sla_deadline.as_deref().expect("deadline"),
            "%Y-%m-%dT%H:%M:%SZ",
        )
        .expect("parse");
        assert!(
            (extended - frozen).num_hours() >= 24 * 364,
            "the wait extends the deadline: {frozen} → {extended}"
        );
        assert!(status.paused_hours > 24.0 * 364.0);
    }

    #[tokio::test]
    async fn a_deferred_request_pauses_its_clock_too() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let di = create_intervention_request(&db, make_create_input(&db, user_id, "Deferred", "d").await)
            .await
            .expect("create");
        move_to(&db, di.id, "submitted", "in_review").await;
        move_to(&db, di.id, "in_review", "deferred").await;
        age_request(&db, di.id, 24 * 365).await;

        let deferred = get_intervention_request(&db, di.id).await.expect("get").expect("row");
        assert_eq!(deferred.sla_state.as_deref(), Some("paused"));
        let stats = crate::di::stats::get_di_stats(
            &db,
            crate::di::stats::DiStatsFilter {
                date_from: None,
                date_to: None,
                entity_id: None,
            },
        )
        .await
        .expect("stats");
        assert_eq!(stats.sla_open.paused, 1);
        assert_eq!(stats.sla_open.breached, 0);
        assert_eq!(stats.overdue, 0);

        move_to(&db, di.id, "deferred", "in_review").await;
        let back = get_intervention_request(&db, di.id).await.expect("get").expect("row");
        assert_eq!(back.sla_state.as_deref(), Some("on_track"));
    }

    #[tokio::test]
    async fn a_request_closed_before_its_deadline_meets_the_sla_even_if_never_screened() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let di = create_intervention_request(&db, make_create_input(&db, user_id, "Duplicate", "d").await)
            .await
            .expect("create");
        // Closed as a duplicate one hour after submission, without screening; that was long ago.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'closed', disposition_code = 'duplicate', \
                closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', submitted_at, '+1 hours') WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("close");
        age_request(&db, di.id, 24 * 30).await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', submitted_at, '+1 hours') \
             WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("closing time");

        let stats = crate::di::stats::get_di_stats(
            &db,
            crate::di::stats::DiStatsFilter {
                date_from: None,
                date_to: None,
                entity_id: None,
            },
        )
        .await
        .expect("stats");
        assert_eq!(stats.sla_met_count, 1, "closed within the response target");
        assert_eq!(stats.sla_breached_count, 0);
        assert_eq!(stats.closed_in_period, 1);
        assert_eq!(
            sla_of(&db, di.id).await.status,
            Some(crate::di::sla::DiSlaLifecycleStatus::Completed)
        );
    }

    #[tokio::test]
    async fn my_requests_list_and_count_agree() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let open = create_intervention_request(&db, make_create_input(&db, user_id, "Open", "d").await)
            .await
            .expect("open");
        let closed = create_intervention_request(&db, make_create_input(&db, user_id, "Closed", "d").await)
            .await
            .expect("closed");
        set_status(&db, closed.id, "closed").await;

        let mut filter = queue_filter("mine");
        filter.submitter_id = Some(user_id);
        let page = list_intervention_requests(&db, filter).await.expect("mine");
        let counts = crate::di::queries::count_di_queues(&db, user_id).await.expect("counts");
        assert_eq!(page.items.iter().map(|d| d.id).collect::<Vec<_>>(), vec![open.id]);
        assert_eq!(page.total, counts.mine);
    }

    fn queue_filter(queue: &str) -> DiListFilter {
        DiListFilter {
            queue: Some(queue.to_string()),
            limit: 50,
            offset: 0,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn work_queues_list_and_count_the_same_requests() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let mut ids = Vec::new();
        for title in ["Queue A", "Queue B", "Queue C", "Queue D", "Queue E"] {
            let di = create_intervention_request(&db, make_create_input(&db, user_id, title, "d").await)
                .await
                .expect("create");
            ids.push(di.id);
        }
        // A stays submitted; B returned for information; C approved without a work order;
        // D awaiting approval; E submitted long ago (SLA breached).
        set_status(&db, ids[1], "returned_for_clarification").await;
        set_status(&db, ids[2], "approved").await;
        set_status(&db, ids[3], "awaiting_approval").await;
        age_request(&db, ids[4], 24 * 365).await;

        let listed = |q: &'static str| {
            let db = db.clone();
            async move {
                list_intervention_requests(&db, queue_filter(q))
                    .await
                    .expect("list queue")
                    .items
                    .iter()
                    .map(|d| d.id)
                    .collect::<Vec<_>>()
            }
        };
        let to_screen = listed("to_screen").await;
        assert_eq!(to_screen.len(), 2);
        assert!(to_screen.contains(&ids[0]) && to_screen.contains(&ids[4]));
        assert_eq!(listed("awaiting_info").await, vec![ids[1]]);
        assert_eq!(listed("approved_not_converted").await, vec![ids[2]]);
        assert_eq!(listed("to_approve").await, vec![ids[3]]);
        assert_eq!(listed("sla_at_risk").await, vec![ids[4]]);

        // The SQL queue and the state computed on read agree.
        let aged = get_intervention_request(&db, ids[4]).await.expect("get").expect("row");
        assert_eq!(aged.sla_state.as_deref(), Some("breached"));
        let fresh = get_intervention_request(&db, ids[0]).await.expect("get").expect("row");
        assert_eq!(fresh.sla_state.as_deref(), Some("on_track"));

        let counts = crate::di::queries::count_di_queues(&db, user_id).await.expect("counts");
        assert_eq!(counts.to_screen, 2);
        assert_eq!(counts.to_review, 0);
        assert_eq!(counts.awaiting_info, 1);
        assert_eq!(counts.to_approve, 1);
        assert_eq!(counts.approved_not_converted, 1);
        assert_eq!(counts.sla_at_risk, 1);
        assert_eq!(counts.mine, 5);
        assert_eq!(counts.mine_awaiting_info, 1);

        let err = list_intervention_requests(&db, queue_filter("everything"))
            .await
            .expect_err("unknown queue refused");
        assert!(matches!(err, crate::errors::AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn requester_answers_and_resubmits_a_returned_request() {
        use crate::di::review::{resubmit_returned_di, DiResubmitInput};

        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let di = create_intervention_request(&db, make_create_input(&db, user_id, "Returned", "d").await)
            .await
            .expect("create");
        let input = |actor_id: i64, row_version: i64, note: &str| DiResubmitInput {
            di_id: di.id,
            actor_id,
            expected_row_version: row_version,
            response_note: note.to_string(),
        };

        // Not returned yet: refused.
        let err = resubmit_returned_di(&db, input(user_id, di.row_version, "answer"))
            .await
            .expect_err("only a returned request");
        assert!(matches!(err, crate::errors::AppError::ValidationFailed(_)));

        set_status(&db, di.id, "returned_for_clarification").await;
        let rv = di.row_version + 1;
        // An empty answer and another user are refused.
        assert!(resubmit_returned_di(&db, input(user_id, rv, "  ")).await.is_err());
        assert!(resubmit_returned_di(&db, input(user_id + 9999, rv, "answer"))
            .await
            .is_err());

        let updated = resubmit_returned_di(&db, input(user_id, rv, "Vibration at 120 Hz, photo added"))
            .await
            .expect("resubmit");
        assert_eq!(updated.status, "in_review");

        let log = get_di_transition_log(&db, di.id).await.expect("log");
        let last = log.last().expect("transition");
        assert_eq!(last.action, "re_submitted");
        assert_eq!(last.notes.as_deref(), Some("Vibration at 120 Hz, photo added"));
        let events = crate::di::review::get_review_events(&db, di.id).await.expect("events");
        assert!(events
            .iter()
            .any(|e| e.event_type == "resubmitted" && e.to_status == "in_review"));
    }

    #[tokio::test]
    async fn stats_measure_backlog_sla_and_conversion_from_the_requests() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let a = create_intervention_request(&db, make_create_input(&db, user_id, "Fresh", "d").await)
            .await
            .expect("a");
        let b = create_intervention_request(&db, make_create_input(&db, user_id, "Old", "d").await)
            .await
            .expect("b");
        age_request(&db, b.id, 24 * 365).await;

        let stats = crate::di::stats::get_di_stats(
            &db,
            crate::di::stats::DiStatsFilter {
                date_from: None,
                date_to: None,
                entity_id: None,
            },
        )
        .await
        .expect("stats");

        assert_eq!(stats.sla_open.breached, 1, "the old request is past its targets");
        assert_eq!(stats.sla_open.on_track, 1, "the fresh request is on track");
        assert_eq!(stats.overdue, 1);
        assert_eq!(stats.overdue_dis.len(), 1);
        assert_eq!(stats.overdue_dis[0].id, b.id);
        assert!(stats.overdue_dis[0].hours_overdue > 0.0);
        let submitted = stats
            .backlog_by_status
            .iter()
            .find(|s| s.status == "submitted")
            .expect("submitted backlog");
        assert_eq!(submitted.count, 2);
        assert!(submitted.max_age_days >= 365.0);
        assert_eq!(stats.converted_count, 0);
        assert_eq!(stats.avg_response_hours, None, "nothing screened yet");
        let _ = a;
    }
}
