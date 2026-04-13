//! Supervisor verification tests for Phase 2 SP04 File 03 Sprint S2.
//!
//! V1 — Conversion atomicity (row-version conflict → transaction rollback).
//! V2 — State-machine guard (only approved_for_planning can convert).
//! V3 — Missing classification → descriptive error.
//! V4 — Successful conversion → WO stub row with source_di_id + draft status.
//! V5 — DI locked after conversion → update_di_draft_fields must fail.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::di::conversion::{convert_di_to_work_order, WoConversionInput};
    use crate::di::queries::{
        create_intervention_request, update_di_draft_fields, DiCreateInput, DiDraftUpdateInput,
    };
    use crate::di::review::{
        approve_di_for_planning, get_review_events, screen_di, DiApproveInput, DiScreenInput,
    };

    // ═══════════════════════════════════════════════════════════════════════
    // Test setup helpers (matching review_tests.rs patterns)
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
             VALUES (1, 1, 'MECH', 'Mécanique', 1);".to_string(),
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

    /// Advance a DI through: create → pending_review → screen → approve.
    /// Returns the DI id and final row_version after approval.
    async fn advance_to_approved(db: &sea_orm::DatabaseConnection, user_id: i64) -> (i64, i64) {
        let di = create_intervention_request(db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(db, di.id).await;

        let screened = screen_di(
            db,
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

        let approved = approve_di_for_planning(
            db,
            DiApproveInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                notes: Some("Approved for planning".to_string()),
            },
        )
        .await
        .expect("approve should succeed");

        assert_eq!(approved.status, "approved_for_planning");
        (di.id, approved.row_version)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Conversion atomicity (row-version conflict → rollback)
    //
    // We pass an incorrect expected_row_version so the UPDATE hits
    // rows_affected=0 and check_concurrency fails. The entire transaction
    // (including any WO stub INSERT already executed) must be rolled back.
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_row_version_conflict_rolls_back_entire_transaction() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let (di_id, correct_version) = advance_to_approved(&db, user_id).await;

        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: correct_version + 999, // stale version
                conversion_notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "conversion must fail with stale row_version");

        // DI must still be approved_for_planning — no partial state change
        let status = get_di_status(&db, di_id).await;
        assert_eq!(
            status, "approved_for_planning",
            "DI must remain approved_for_planning after failed conversion"
        );

        // No WO should exist — entire transaction rolled back
        let wo_count = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM work_orders WHERE source_di_id = ?",
                [di_id.into()],
            ))
            .await
            .expect("query")
            .expect("row");
        let cnt: i64 = wo_count.try_get("", "cnt").expect("cnt");
        assert_eq!(
            cnt, 0,
            "No WO must exist after rolled-back conversion"
        );

        // No transition log for 'convert' action should exist
        let log_count = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM di_state_transition_log WHERE di_id = ? AND action = 'convert'",
                [di_id.into()],
            ))
            .await
            .expect("query")
            .expect("row");
        let log_cnt: i64 = log_count.try_get("", "cnt").expect("cnt");
        assert_eq!(
            log_cnt, 0,
            "No transition log for 'convert' must exist after rollback"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — State-machine guard: only approved_for_planning can convert
    //
    // Step-up reauthentication is enforced at the IPC command layer via
    // require_step_up!. At the domain layer we verify that the state
    // transition guard rejects conversion from all non-approved states.
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_conversion_from_submitted_fails() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");

        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: di.row_version,
                conversion_notes: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "conversion must fail when DI is in 'submitted'"
        );
    }

    #[tokio::test]
    async fn v2_conversion_from_pending_review_fails() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        let di = create_intervention_request(&db, make_create_input(user_id))
            .await
            .expect("create DI");
        advance_to_pending_review(&db, di.id).await;

        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                conversion_notes: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "conversion must fail when DI is in 'pending_review'"
        );
    }

    #[tokio::test]
    async fn v2_conversion_from_awaiting_approval_fails() {
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
                validated_urgency: "high".to_string(),
                review_team_id: None,
                classification_code_id: Some(1),
                reviewer_note: None,
            },
        )
        .await
        .expect("screen");

        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: screened.row_version,
                conversion_notes: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "conversion must fail when DI is in 'awaiting_approval'"
        );
    }

    #[tokio::test]
    async fn v2_conversion_permission_requires_step_up() {
        let db = setup().await;

        // Verify that di.convert permission exists and requires step-up
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                ["di.convert".into()],
            ))
            .await
            .expect("query");

        // If the permission doesn't exist yet in seed data, the conversion
        // command uses di.approve which does require step-up. We check both.
        if let Some(row) = row {
            let requires: i64 = row.try_get("", "requires_step_up").expect("column");
            assert_eq!(requires, 1, "di.convert must require step-up authentication");
        } else {
            // Fallback: verify di.approve requires step-up (which is used for conversion)
            let approve_row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT requires_step_up FROM permissions WHERE name = ?",
                    ["di.approve".into()],
                ))
                .await
                .expect("query")
                .expect("di.approve permission must exist");
            let requires: i64 = approve_row.try_get("", "requires_step_up").expect("column");
            assert_eq!(requires, 1, "di.approve (used for conversion step-up) must require step-up");
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Missing classification → descriptive error
    //
    // A DI without classification_code_id set cannot be converted.
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_missing_classification_prevents_conversion() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let (di_id, version) = advance_to_approved(&db, user_id).await;

        // Remove classification_code_id (set to NULL) to simulate missing classification
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET classification_code_id = NULL WHERE id = ?",
            [di_id.into()],
        ))
        .await
        .expect("clear classification");

        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: version,
                conversion_notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "conversion must fail with missing classification");

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("Classification") || err_str.contains("classification"),
            "Error must mention classification requirement: got '{err_str}'"
        );

        // DI must remain approved_for_planning
        let status = get_di_status(&db, di_id).await;
        assert_eq!(status, "approved_for_planning");
    }

    #[tokio::test]
    async fn v3_missing_asset_prevents_conversion() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let (di_id, version) = advance_to_approved(&db, user_id).await;

        // Set asset_id to 0 to simulate missing asset context
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET asset_id = 0 WHERE id = ?",
            [di_id.into()],
        ))
        .await
        .expect("clear asset_id");

        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: version,
                conversion_notes: None,
            },
        )
        .await;

        assert!(result.is_err(), "conversion must fail with missing asset");

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("actif") || err_str.contains("asset"),
            "Error must mention asset requirement: got '{err_str}'"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V4 — Successful conversion creates WO stub
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v4_successful_conversion_creates_wo_stub() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let (di_id, version) = advance_to_approved(&db, user_id).await;

        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: version,
                conversion_notes: Some("Converting for scheduled shutdown".to_string()),
            },
        )
        .await
        .expect("conversion should succeed");

        // DI status must be converted_to_work_order
        assert_eq!(result.di.status, "converted_to_work_order");
        assert!(result.di.converted_at.is_some(), "converted_at must be set");
        assert_eq!(
            result.di.converted_to_wo_id,
            Some(result.wo_id),
            "DI.converted_to_wo_id must reference the WO stub"
        );

        // WO code must match OT-NNNN pattern
        assert!(
            result.wo_code.starts_with("OT-"),
            "WO code must start with 'OT-': got '{}'",
            result.wo_code
        );

        // WO row must exist with correct data in work_orders
        let wo_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT w.id, w.code, w.source_di_id, w.equipment_id, s.code AS status \
                 FROM work_orders w \
                 JOIN work_order_statuses s ON s.id = w.status_id \
                 WHERE w.id = ?",
                [result.wo_id.into()],
            ))
            .await
            .expect("query")
            .expect("WO must exist");

        let wo_source: i64 = wo_row.try_get("", "source_di_id").expect("source_di_id");
        let wo_status: String = wo_row.try_get("", "status").expect("status");
        let wo_code: String = wo_row.try_get("", "code").expect("code");

        assert_eq!(wo_source, di_id, "WO must reference the source DI");
        assert_eq!(wo_status, "draft", "WO status must be 'draft'");
        assert_eq!(wo_code, result.wo_code, "WO code must match result");

        // row_version must have incremented
        let rv = get_row_version(&db, di_id).await;
        assert_eq!(rv, version + 1, "row_version must increment by 1");
    }

    #[tokio::test]
    async fn v4_conversion_records_event_and_transition_log() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let (di_id, version) = advance_to_approved(&db, user_id).await;

        convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: version,
                conversion_notes: Some("Test notes".to_string()),
            },
        )
        .await
        .expect("conversion should succeed");

        // Review event: must have a 'converted' event with step_up_used = true
        let events = get_review_events(&db, di_id).await.expect("events");
        let convert_event = events
            .iter()
            .find(|e| e.event_type == "converted")
            .expect("must have a 'converted' event");
        assert!(
            convert_event.step_up_used,
            "converted event must record step_up_used = true"
        );
        assert_eq!(
            convert_event.from_status.as_str(),
            "approved_for_planning"
        );
        assert_eq!(
            convert_event.to_status.as_str(),
            "converted_to_work_order"
        );

        // State transition log: must have a 'convert' action row
        let log_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT from_status, to_status, action, notes \
                 FROM di_state_transition_log WHERE di_id = ? AND action = 'convert'",
                [di_id.into()],
            ))
            .await
            .expect("query")
            .expect("transition log must exist for 'convert'");

        let from: String = log_row.try_get("", "from_status").expect("from_status");
        let to: String = log_row.try_get("", "to_status").expect("to_status");
        let notes: Option<String> = log_row.try_get("", "notes").ok();

        assert_eq!(from, "approved_for_planning");
        assert_eq!(to, "converted_to_work_order");
        assert_eq!(notes.as_deref(), Some("Test notes"));
    }

    #[tokio::test]
    async fn v4_wo_code_increments_sequentially() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;

        // First conversion
        let (di_id_1, v1) = advance_to_approved(&db, user_id).await;
        let r1 = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id: di_id_1,
                actor_id: user_id,
                expected_row_version: v1,
                conversion_notes: None,
            },
        )
        .await
        .expect("first conversion");

        // Second conversion
        let (di_id_2, v2) = advance_to_approved(&db, user_id).await;
        let r2 = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id: di_id_2,
                actor_id: user_id,
                expected_row_version: v2,
                conversion_notes: None,
            },
        )
        .await
        .expect("second conversion");

        assert_eq!(r1.wo_code, "OT-0001", "First WO code must be OT-0001");
        assert_eq!(r2.wo_code, "OT-0002", "Second WO code must be OT-0002");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V5 — DI locked after conversion: update_di_draft_fields must fail
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v5_converted_di_rejects_draft_field_updates() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let (di_id, version) = advance_to_approved(&db, user_id).await;

        // Convert the DI
        let converted = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: version,
                conversion_notes: None,
            },
        )
        .await
        .expect("conversion should succeed");

        assert_eq!(converted.di.status, "converted_to_work_order");

        // Attempt to update draft fields — must fail
        let result = update_di_draft_fields(
            &db,
            DiDraftUpdateInput {
                id: di_id,
                expected_row_version: converted.di.row_version,
                title: Some("Modified title after conversion".to_string()),
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
            "update_di_draft_fields must fail on a converted DI"
        );

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("submitted") || err_str.contains("returned_for_clarification"),
            "Error must mention allowed statuses: got '{err_str}'"
        );
    }

    #[tokio::test]
    async fn v5_double_conversion_fails() {
        let db = setup().await;
        let user_id = get_user_id(&db).await;
        let (di_id, version) = advance_to_approved(&db, user_id).await;

        // First conversion succeeds
        let converted = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: version,
                conversion_notes: None,
            },
        )
        .await
        .expect("first conversion should succeed");

        // Second conversion attempt must fail (DI is now converted_to_work_order)
        let result = convert_di_to_work_order(
            &db,
            WoConversionInput {
                di_id,
                actor_id: user_id,
                expected_row_version: converted.di.row_version,
                conversion_notes: None,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "second conversion attempt must fail — DI is already converted"
        );
    }
}
