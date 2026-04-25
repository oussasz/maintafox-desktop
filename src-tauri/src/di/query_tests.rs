//! Supervisor verification tests for Phase 2 SP04 File 01 Sprint S2.
//!
//! V1 — Create DI end-to-end: row + transition log.
//! V2 — Draft update guard: reject if status is not draft.
//! V3 — Optimistic concurrency: reject stale row_version.
//! V4 — Search filter: free-text search returns only matching DIs.
//! V5 — Recurrence query: recent similar DIs on same asset + symptom.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::di::domain::InterventionRequest;
    use crate::di::queries::{
        create_intervention_request, get_di_transition_log, get_intervention_request,
        get_recent_similar_dis, list_intervention_requests, update_di_draft_fields,
        DiCreateInput, DiDraftUpdateInput, DiListFilter,
    };

    /// In-memory SQLite with all migrations applied + seeded system data.
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

        // Seed FK prerequisite data (equipment, org_nodes)
        seed_fk_data(&db).await;

        db
    }

    /// Insert minimal FK data: equipment + org_structure_models + org_node_types + org_nodes.
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

    /// Helper to build a standard DiCreateInput.
    fn make_create_input(user_id: i64, title: &str, description: &str) -> DiCreateInput {
        DiCreateInput {
            asset_id: 1,
            org_node_id: 1,
            title: title.to_string(),
            description: description.to_string(),
            origin_type: "operator".to_string(),
            symptom_code_id: None,
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

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Create DI end-to-end
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_create_di_returns_submitted_with_code_and_timestamp() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(user_id, "Pump vibration alert", "Excessive vibration on pump P-101"),
        )
        .await
        .expect("create_di should succeed");

        assert_eq!(di.code, "DI-0001");
        assert_eq!(di.status, "submitted");
        assert_eq!(di.title, "Pump vibration alert");
        assert_eq!(di.origin_type, "operator");
        assert_eq!(di.impact_level, "unknown");
        assert_eq!(di.reported_urgency, "medium");
        assert!(!di.submitted_at.is_empty(), "submitted_at must be set");
        assert_eq!(di.row_version, 1);
        assert_eq!(di.submitter_id, user_id);
    }

    #[tokio::test]
    async fn v1_create_di_writes_transition_log() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(user_id, "Test DI", "Description"),
        )
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

        let created = create_intervention_request(
            &db,
            make_create_input(user_id, "Fetched DI", "Fetch test"),
        )
        .await
        .expect("create_di");

        let fetched = get_intervention_request(&db, created.id)
            .await
            .expect("get_di")
            .expect("DI must exist");

        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.code, created.code);
        assert_eq!(fetched.title, "Fetched DI");
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
            make_create_input(user_id, "Guard test", "Guard description"),
        )
        .await
        .expect("create_di");

        // Forcibly set status to 'screened' (a non-draft state)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'screened' WHERE id = ?",
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

        assert!(result.is_err(), "Should reject update on screened DI");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("screened"),
            "Error should mention current status: {err}"
        );
    }

    #[tokio::test]
    async fn v2_update_draft_allowed_on_submitted() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(user_id, "Original title", "Original desc"),
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
        .expect("update should succeed on submitted DI");

        assert_eq!(updated.title, "Updated title");
        assert_eq!(updated.row_version, 2, "row_version must increment");
    }

    #[tokio::test]
    async fn v2_update_draft_allowed_on_returned_for_clarification() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(
            &db,
            make_create_input(user_id, "RFC test", "RFC desc"),
        )
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
            make_create_input(user_id, "Concurrency test", "Concurrency desc"),
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
        .expect("first update should succeed");

        // Second update with stale version (1 instead of 2) must fail
        let result = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 1, // stale
                title: Some("Update 2".into()),
                description: None,
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
            "Stale row_version must return concurrency error"
        );
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
            make_create_input(user_id, "Sequential update", "Seq desc"),
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
        .expect("v1->v2 update");

        // Second update: v2 -> v3 (correct version)
        let updated = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di.id,
                expected_row_version: 2,
                title: Some("V3 title".into()),
                description: None,
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
            make_create_input(user_id, "Motor overheating alert", "Motor M-200 temp above threshold"),
        )
        .await
        .expect("di1");

        create_intervention_request(
            &db,
            make_create_input(user_id, "Conveyor belt misalignment", "Belt CV-300 drifting"),
        )
        .await
        .expect("di2");

        create_intervention_request(
            &db,
            make_create_input(user_id, "Hydraulic leak on press", "Oil pooling under press HP-400"),
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

        let di1 = create_intervention_request(
            &db,
            make_create_input(user_id, "DI-A", "A"),
        )
        .await
        .expect("di1");

        create_intervention_request(
            &db,
            make_create_input(user_id, "DI-B", "B"),
        )
        .await
        .expect("di2");

        // Set di1 to screened
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'screened' WHERE id = ?",
            [di1.id.into()],
        ))
        .await
        .expect("force status");

        // Filter by screened
        let result = list_intervention_requests(
            &db,
            DiListFilter {
                status: Some(vec!["screened".to_string()]),
                limit: 50,
                offset: 0,
                ..Default::default()
            },
        )
        .await
        .expect("filter");

        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].status, "screened");
    }

    #[tokio::test]
    async fn v4_list_returns_paginated_total() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        for i in 0..5 {
            create_intervention_request(
                &db,
                make_create_input(user_id, &format!("DI {i}"), &format!("Desc {i}")),
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
            make_create_input(user_id, "Recurrence A", "First occurrence"),
        )
        .await
        .expect("di-a");

        create_intervention_request(
            &db,
            make_create_input(user_id, "Recurrence B", "Second occurrence"),
        )
        .await
        .expect("di-b");

        let similar = get_recent_similar_dis(&db, 1, None, 7)
            .await
            .expect("similar query");

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
                make_create_input(user_id, &format!("Similar {i}"), &format!("Desc {i}")),
            )
            .await
            .expect("create");
        }

        let similar = get_recent_similar_dis(&db, 1, None, 30)
            .await
            .expect("similar query");

        assert!(
            similar.len() <= 5,
            "get_recent_similar_dis must return at most 5 rows, got {}",
            similar.len()
        );
    }
}
