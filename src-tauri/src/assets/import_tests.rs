//! Supervisor verification tests for Phase 2 SP02 File 04 Sprint S1 & S2.
//!
//! V1 — Batch metadata captured: filename + SHA-256 stored in batch row.
//! V2 — Validation counts: valid/warning/error counts are populated.
//! V3 — Conflict classes visible: intentionally bad rows show explicit conflict category.
//! V4 — Apply creates/updates equipment rows from validated staging.
//! V5 — Idempotent replay: re-applying a batch is rejected.
//! V6 — Apply audit event records summary counts.
//! V7 — Permission gate: eq.import required for import commands.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::assets::governance::ConflictCategory;
    use crate::assets::import;
    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::auth::rbac::{self, PermissionScope};
    use crate::auth::session_manager::AuthenticatedUser;
    use crate::state::AppState;
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};

    /// In-memory SQLite with all migrations + seed data.
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

    /// Publish an org model and create an active root org node.
    /// Returns the org node id.
    async fn setup_org_node(db: &sea_orm::DatabaseConnection) -> i64 {
        let model = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("test model".to_string()),
            },
            1,
        )
        .await
        .expect("create model");

        let root_type = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "SITE".to_string(),
                label: "Site".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(0),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: true,
            },
        )
        .await
        .expect("create root type");

        let child_type = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "WORKSHOP".to_string(),
                label: "Atelier".to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("create child type");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: root_type.id,
                child_type_id: child_type.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("create rule");

        structure_model::publish_model(db, model.id, 1)
            .await
            .expect("publish model");

        let root = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "SITE-001".to_string(),
                name: "Test Site".to_string(),
                node_type_id: root_type.id.into(),
                parent_id: None,
                description: None,
                cost_center_code: None,
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
            },
            1,
        )
        .await
        .expect("create root node");

        root.id
    }

    /// Insert a test equipment class. Returns the class id.
    async fn setup_equipment_class(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        let sync_id = uuid::Uuid::new_v4().to_string();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO equipment_classes
              (sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
              VALUES (?, 'PUMP', 'Pompes', NULL, 'class', 1, ?, ?)",
            [sync_id.into(), now.clone().into(), now.into()],
        ))
        .await
        .expect("insert equipment class");

        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM equipment_classes WHERE code = 'PUMP'".to_string(),
            ))
            .await
            .expect("query class")
            .expect("class row");

        row.try_get::<i64>("", "id").expect("class id")
    }

    /// Build a test CSV with mixed valid and invalid rows.
    /// Row 1: valid new asset (will pass all checks)
    /// Row 2: unknown class code (should produce UnknownClassCode error)
    /// Row 3: nonexistent org node (should produce OrgNodeMissing error)
    /// Row 4: unknown criticality code (should produce UnknownCriticalityCode error)
    /// Row 5: missing asset_code and external_key (should produce MissingRequiredField error)
    fn build_test_csv(org_node_id: i64) -> Vec<u8> {
        format!(
            "asset_code,asset_name,class_code,criticality_code,status_code,org_node_id,manufacturer\n\
             PMP-2001,Pompe centrifuge P-201,PUMP,STANDARD,ACTIVE_IN_SERVICE,{org_node_id},KSB\n\
             PMP-2002,Pompe fantome,FAKE_CLASS,STANDARD,ACTIVE_IN_SERVICE,{org_node_id},KSB\n\
             PMP-2003,Pompe orpheline,PUMP,STANDARD,ACTIVE_IN_SERVICE,999999,KSB\n\
             PMP-2004,Pompe criticite,PUMP,NONEXISTENT_CRIT,ACTIVE_IN_SERVICE,{org_node_id},KSB\n\
             ,Pompe sans code,,STANDARD,ACTIVE_IN_SERVICE,{org_node_id},KSB\n"
        )
        .into_bytes()
    }

    /// Build a CSV that includes an existing asset with a different class (reclassification warning).
    fn build_reclassification_csv(org_node_id: i64) -> Vec<u8> {
        // Inserts a second class will be done in the test. This CSV updates PMP-3001
        // with a different class code.
        format!(
            "asset_code,asset_name,class_code,criticality_code,status_code,org_node_id\n\
             PMP-3001,Pompe modified,MOTOR,STANDARD,ACTIVE_IN_SERVICE,{org_node_id}\n"
        )
        .into_bytes()
    }

    /// Build a CSV that tries to decommission an active asset (forbidden transition).
    fn build_decommission_csv(org_node_id: i64) -> Vec<u8> {
        format!(
            "asset_code,asset_name,class_code,criticality_code,status_code,org_node_id\n\
             PMP-4001,Pompe to decom,PUMP,STANDARD,DECOMMISSIONED,{org_node_id}\n"
        )
        .into_bytes()
    }

    /// Compute SHA-256 hex digest of bytes (matches the import pipeline contract).
    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(data);
        hex::encode(hash)
    }

    // ── V1 — Batch metadata captured ──────────────────────────────────────

    #[tokio::test]
    async fn v1_batch_stores_filename_and_sha256() {
        let db = setup().await;

        let csv = b"asset_code,asset_name\nPMP-001,Test pump\n";
        let sha = sha256_hex(csv);

        let batch = import::create_import_batch(&db, "equipment_import.csv", &sha, Some(1))
            .await
            .expect("create batch should succeed");

        assert_eq!(batch.source_filename, "equipment_import.csv");
        assert_eq!(batch.source_sha256, sha);
        assert_eq!(batch.status, "uploaded");
        assert_eq!(batch.initiated_by_id, Some(1));
        assert_eq!(batch.total_rows, 0, "no rows staged yet");
    }

    #[tokio::test]
    async fn v1_batch_upload_event_recorded() {
        let db = setup().await;

        let csv = b"asset_code,asset_name\nPMP-001,Test\n";
        let sha = sha256_hex(csv);

        let batch = import::create_import_batch(&db, "test.csv", &sha, Some(1))
            .await
            .expect("create batch");

        let events = import::list_import_events(&db, batch.id)
            .await
            .expect("list events");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "uploaded");
        assert_eq!(events[0].created_by_id, Some(1));

        // Summary JSON should contain filename and SHA
        let summary = events[0].summary_json.as_ref().expect("summary_json");
        assert!(summary.contains("test.csv"), "summary should contain filename");
        assert!(summary.contains(&sha), "summary should contain sha256");
    }

    #[tokio::test]
    async fn v1_csv_staging_populates_total_rows() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "multi_row.csv", &sha, Some(1))
            .await
            .expect("create batch");

        let staged = import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage CSV");

        assert_eq!(staged.total_rows, 5, "CSV has 5 data rows");
    }

    // ── V2 — Validation counts ────────────────────────────────────────────

    #[tokio::test]
    async fn v2_validation_populates_summary_counts() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "counts.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        let validated = import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        assert_eq!(validated.status, "validated", "status should advance");
        assert_eq!(validated.total_rows, 5);
        // Row 1 is valid, rows 2-5 have errors
        assert!(
            validated.valid_rows >= 1,
            "at least 1 valid row expected, got {}",
            validated.valid_rows
        );
        assert!(
            validated.error_rows >= 3,
            "at least 3 error rows expected, got {}",
            validated.error_rows
        );
        // Total must add up
        assert_eq!(
            validated.valid_rows + validated.warning_rows + validated.error_rows,
            validated.total_rows,
            "counts must sum to total"
        );
    }

    #[tokio::test]
    async fn v2_validation_event_recorded_with_counts() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "event.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let events = import::list_import_events(&db, batch.id)
            .await
            .expect("list events");

        // Should have 2 events: uploaded + validated
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "uploaded");
        assert_eq!(events[1].event_type, "validated");

        let summary = events[1]
            .summary_json
            .as_ref()
            .expect("validated event should have summary");
        assert!(
            summary.contains("valid_rows"),
            "summary should contain valid_rows"
        );
        assert!(
            summary.contains("error_rows"),
            "summary should contain error_rows"
        );
    }

    #[tokio::test]
    async fn v2_double_validation_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "double.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("first validation");

        // Second validation should be rejected (status is now 'validated', not 'uploaded')
        let err = import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect_err("double validation should fail");

        match err {
            crate::errors::AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("validated"),
                    "should mention current status, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V3 — Conflict classes visible ─────────────────────────────────────

    #[tokio::test]
    async fn v3_unknown_class_code_produces_conflict_category() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "conflict.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let preview = import::get_import_preview(&db, batch.id)
            .await
            .expect("get preview");

        // Row 2 (row_no=2): FAKE_CLASS should produce UnknownClassCode
        let row2 = preview
            .rows
            .iter()
            .find(|r| r.row_no == 2)
            .expect("row 2 should exist in preview");

        assert_eq!(row2.validation_status, "error");
        assert!(
            row2.validation_messages
                .iter()
                .any(|m| m.category == ConflictCategory::UnknownClassCode),
            "row 2 should have UnknownClassCode conflict, got: {:?}",
            row2.validation_messages
        );
    }

    #[tokio::test]
    async fn v3_missing_org_node_produces_conflict_category() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "org.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let preview = import::get_import_preview(&db, batch.id)
            .await
            .expect("get preview");

        // Row 3 (row_no=3): org_node_id=999999 should produce OrgNodeMissing
        let row3 = preview
            .rows
            .iter()
            .find(|r| r.row_no == 3)
            .expect("row 3 should exist");

        assert_eq!(row3.validation_status, "error");
        assert!(
            row3.validation_messages
                .iter()
                .any(|m| m.category == ConflictCategory::OrgNodeMissing),
            "row 3 should have OrgNodeMissing conflict, got: {:?}",
            row3.validation_messages
        );
    }

    #[tokio::test]
    async fn v3_unknown_criticality_produces_conflict_category() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "crit.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let preview = import::get_import_preview(&db, batch.id)
            .await
            .expect("get preview");

        // Row 4 (row_no=4): NONEXISTENT_CRIT should produce UnknownCriticalityCode
        let row4 = preview
            .rows
            .iter()
            .find(|r| r.row_no == 4)
            .expect("row 4 should exist");

        assert_eq!(row4.validation_status, "error");
        assert!(
            row4.validation_messages
                .iter()
                .any(|m| m.category == ConflictCategory::UnknownCriticalityCode),
            "row 4 should have UnknownCriticalityCode conflict, got: {:?}",
            row4.validation_messages
        );
    }

    #[tokio::test]
    async fn v3_missing_required_fields_produces_conflict_category() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "missing.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let preview = import::get_import_preview(&db, batch.id)
            .await
            .expect("get preview");

        // Row 5 (row_no=5): empty asset_code + missing class_code
        let row5 = preview
            .rows
            .iter()
            .find(|r| r.row_no == 5)
            .expect("row 5 should exist");

        assert_eq!(row5.validation_status, "error");
        assert!(
            row5.validation_messages
                .iter()
                .any(|m| m.category == ConflictCategory::MissingRequiredField),
            "row 5 should have MissingRequiredField, got: {:?}",
            row5.validation_messages
        );
    }

    #[tokio::test]
    async fn v3_valid_row_produces_create_action() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_test_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "action.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let preview = import::get_import_preview(&db, batch.id)
            .await
            .expect("get preview");

        // Row 1 (row_no=1): fully valid, should propose "create"
        let row1 = preview
            .rows
            .iter()
            .find(|r| r.row_no == 1)
            .expect("row 1 should exist");

        assert_eq!(row1.validation_status, "valid");
        assert_eq!(
            row1.proposed_action.as_deref(),
            Some("create"),
            "valid new asset should propose create"
        );
        assert!(
            row1.validation_messages.is_empty(),
            "valid row should have no messages"
        );
    }

    #[tokio::test]
    async fn v3_reclassification_produces_warning() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Insert a second class (MOTOR) for reclassification detection
        let now = chrono::Utc::now().to_rfc3339();
        let sync_id = uuid::Uuid::new_v4().to_string();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO equipment_classes
              (sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
              VALUES (?, 'MOTOR', 'Moteurs', NULL, 'class', 1, ?, ?)",
            [sync_id.into(), now.clone().into(), now.into()],
        ))
        .await
        .expect("insert motor class");

        // Create an existing asset with class=PUMP
        identity::create_asset(
            &db,
            CreateAssetPayload {
                asset_code: "PMP-3001".to_string(),
                asset_name: "Pompe originale".to_string(),
                class_code: "PUMP".to_string(),
                family_code: None,
                subfamily_code: None,
                criticality_code: "STANDARD".to_string(),
                status_code: "ACTIVE_IN_SERVICE".to_string(),
                manufacturer: None,
                model: None,
                serial_number: None,
                maintainable_boundary: true,
                org_node_id,
                commissioned_at: None,
            },
            1,
        )
        .await
        .expect("create pre-existing asset");

        // Now import CSV with PMP-3001 but class=MOTOR
        let csv = build_reclassification_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "reclass.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let preview = import::get_import_preview(&db, batch.id)
            .await
            .expect("get preview");

        let row = &preview.rows[0];
        assert_eq!(row.validation_status, "warning");
        assert_eq!(
            row.proposed_action.as_deref(),
            Some("update"),
            "existing asset should propose update"
        );
        assert!(
            row.validation_messages
                .iter()
                .any(|m| m.category == ConflictCategory::ReclassificationRequiresReview),
            "should flag reclassification, got: {:?}",
            row.validation_messages
        );
    }

    #[tokio::test]
    async fn v3_decommission_via_import_is_forbidden() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Create an active asset
        identity::create_asset(
            &db,
            CreateAssetPayload {
                asset_code: "PMP-4001".to_string(),
                asset_name: "Pompe a decom".to_string(),
                class_code: "PUMP".to_string(),
                family_code: None,
                subfamily_code: None,
                criticality_code: "STANDARD".to_string(),
                status_code: "ACTIVE_IN_SERVICE".to_string(),
                manufacturer: None,
                model: None,
                serial_number: None,
                maintainable_boundary: true,
                org_node_id,
                commissioned_at: None,
            },
            1,
        )
        .await
        .expect("create asset");

        let csv = build_decommission_csv(org_node_id);
        let sha = sha256_hex(&csv);

        let batch = import::create_import_batch(&db, "decom.csv", &sha, Some(1))
            .await
            .expect("create batch");

        import::parse_and_stage_csv(&db, batch.id, &csv)
            .await
            .expect("stage");

        import::validate_import_batch(&db, batch.id, Some(1))
            .await
            .expect("validate");

        let preview = import::get_import_preview(&db, batch.id)
            .await
            .expect("get preview");

        let row = &preview.rows[0];
        assert_eq!(row.validation_status, "error");
        assert!(
            row.validation_messages
                .iter()
                .any(|m| m.category == ConflictCategory::ForbiddenStatusTransition),
            "should flag forbidden decommission, got: {:?}",
            row.validation_messages
        );
    }

    // ── V4 — Apply creates/updates equipment ──────────────────────────────

    /// Build a minimal all-valid CSV for apply tests.
    fn build_apply_csv(org_node_id: i64) -> Vec<u8> {
        format!(
            "asset_code,asset_name,class_code,criticality_code,status_code,org_node_id,manufacturer\n\
             IMP-5001,Imported Pump A,PUMP,STANDARD,ACTIVE_IN_SERVICE,{org_node_id},KSB\n\
             IMP-5002,Imported Pump B,PUMP,STANDARD,ACTIVE_IN_SERVICE,{org_node_id},Sulzer\n"
        )
        .into_bytes()
    }

    /// Helper: stage + validate a CSV and return the batch id.
    async fn stage_and_validate(
        db: &sea_orm::DatabaseConnection,
        csv: &[u8],
        filename: &str,
    ) -> i64 {
        let sha = sha256_hex(csv);
        let batch = import::create_import_batch(db, filename, &sha, Some(1))
            .await
            .expect("create batch");
        import::parse_and_stage_csv(db, batch.id, csv)
            .await
            .expect("stage");
        import::validate_import_batch(db, batch.id, Some(1))
            .await
            .expect("validate");
        batch.id
    }

    #[tokio::test]
    async fn v4_apply_creates_new_equipment_rows() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_apply_csv(org_node_id);
        let batch_id = stage_and_validate(&db, &csv, "apply_create.csv").await;

        let policy = import::ApplyPolicy {
            include_warnings: false,
            external_system_code: None,
        };
        let result = import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect("apply should succeed");

        assert_eq!(result.created, 2, "should have created 2 equipment rows");
        assert_eq!(result.updated, 0);
        assert_eq!(result.batch.status, "applied");

        // Verify equipment rows exist in the registry
        let asset = identity::get_asset_by_id(
            &db,
            find_equipment_id_by_code(&db, "IMP-5001").await,
        )
        .await
        .expect("IMP-5001 should exist");
        assert_eq!(asset.asset_code, "IMP-5001");
        assert_eq!(asset.asset_name, "Imported Pump A");
    }

    /// Find an equipment row's id by its asset_id_code.
    async fn find_equipment_id_by_code(db: &sea_orm::DatabaseConnection, code: &str) -> i64 {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM equipment WHERE asset_id_code = ? AND deleted_at IS NULL",
                [code.into()],
            ))
            .await
            .expect("query equipment")
            .expect("equipment row should exist");
        row.try_get::<i64>("", "id").expect("equipment id")
    }

    #[tokio::test]
    async fn v4_apply_updates_existing_equipment() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Pre-create asset IMP-6001
        identity::create_asset(
            &db,
            CreateAssetPayload {
                asset_code: "IMP-6001".to_string(),
                asset_name: "Original name".to_string(),
                class_code: "PUMP".to_string(),
                family_code: None,
                subfamily_code: None,
                criticality_code: "STANDARD".to_string(),
                status_code: "ACTIVE_IN_SERVICE".to_string(),
                manufacturer: None,
                model: None,
                serial_number: None,
                maintainable_boundary: true,
                org_node_id,
                commissioned_at: None,
            },
            1,
        )
        .await
        .expect("create pre-existing asset");

        // CSV that updates IMP-6001's name and manufacturer
        let csv = format!(
            "asset_code,asset_name,class_code,criticality_code,status_code,org_node_id,manufacturer\n\
             IMP-6001,Updated name,PUMP,STANDARD,ACTIVE_IN_SERVICE,{org_node_id},NewManufacturer\n"
        )
        .into_bytes();
        let batch_id = stage_and_validate(&db, &csv, "apply_update.csv").await;

        let policy = import::ApplyPolicy {
            include_warnings: false,
            external_system_code: None,
        };
        let result = import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect("apply should succeed");

        assert_eq!(result.created, 0);
        assert_eq!(result.updated, 1, "should have updated 1 row");

        let asset = identity::get_asset_by_id(
            &db,
            find_equipment_id_by_code(&db, "IMP-6001").await,
        )
        .await
        .expect("IMP-6001 should exist");
        assert_eq!(asset.asset_name, "Updated name");
    }

    #[tokio::test]
    async fn v4_apply_skips_error_rows() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Mixed CSV: row 1 valid, rows 2-5 have errors
        let csv = build_test_csv(org_node_id);
        let batch_id = stage_and_validate(&db, &csv, "apply_mixed.csv").await;

        let policy = import::ApplyPolicy {
            include_warnings: false,
            external_system_code: None,
        };
        let result = import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect("apply should succeed");

        assert_eq!(result.created, 1, "only 1 valid row should be created");
        assert!(result.skipped >= 4, "error rows should be skipped");
        assert_eq!(result.batch.status, "applied");
    }

    #[tokio::test]
    async fn v4_apply_with_external_key_creates_link() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = format!(
            "asset_code,external_key,asset_name,class_code,criticality_code,status_code,org_node_id\n\
             IMP-7001,ERP-123,Linked pump,PUMP,STANDARD,ACTIVE_IN_SERVICE,{org_node_id}\n"
        )
        .into_bytes();
        let batch_id = stage_and_validate(&db, &csv, "apply_ext.csv").await;

        let policy = import::ApplyPolicy {
            include_warnings: false,
            external_system_code: Some("ERP".to_string()),
        };
        let result = import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect("apply should succeed");

        assert_eq!(result.created, 1);

        // Verify external_id link was created
        let link_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT external_id, system_code FROM asset_external_ids WHERE external_id = 'ERP-123'".to_string(),
            ))
            .await
            .expect("query external_ids")
            .expect("external_id row should exist");
        let system: String = link_row.try_get("", "system_code").expect("system_code");
        assert_eq!(system, "ERP");
    }

    // ── V5 — Idempotent replay ────────────────────────────────────────────

    #[tokio::test]
    async fn v5_double_apply_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_apply_csv(org_node_id);
        let batch_id = stage_and_validate(&db, &csv, "idempotent.csv").await;

        let policy = import::ApplyPolicy {
            include_warnings: false,
            external_system_code: None,
        };

        // First apply succeeds
        import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect("first apply");

        // Second apply should be rejected
        let err = import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect_err("double apply should fail");

        match err {
            crate::errors::AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("déjà été appliqué"),
                    "should mention already applied, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v5_apply_from_wrong_status_rejected() {
        let db = setup().await;

        let csv = b"asset_code,asset_name\nX-001,Test\n";
        let sha = sha256_hex(csv);

        // Create batch in 'uploaded' state (not yet validated)
        let batch = import::create_import_batch(&db, "wrong_status.csv", &sha, Some(1))
            .await
            .expect("create batch");

        let policy = import::ApplyPolicy {
            include_warnings: false,
            external_system_code: None,
        };
        let err = import::apply_import_batch(&db, batch.id, &policy, Some(1))
            .await
            .expect_err("apply from uploaded should fail");

        match err {
            crate::errors::AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("uploaded"),
                    "should mention current status, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V6 — Apply audit event ────────────────────────────────────────────

    #[tokio::test]
    async fn v6_apply_records_audit_event_with_counts() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let csv = build_apply_csv(org_node_id);
        let batch_id = stage_and_validate(&db, &csv, "audit.csv").await;

        let policy = import::ApplyPolicy {
            include_warnings: false,
            external_system_code: None,
        };
        import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect("apply");

        let events = import::list_import_events(&db, batch_id)
            .await
            .expect("list events");

        // Should have 3 events: uploaded + validated + applied
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].event_type, "applied");
        assert_eq!(events[2].created_by_id, Some(1));

        let summary = events[2]
            .summary_json
            .as_ref()
            .expect("applied event should have summary");

        let parsed: serde_json::Value =
            serde_json::from_str(summary).expect("summary should be valid JSON");
        assert_eq!(parsed["created"], 2);
        assert_eq!(parsed["updated"], 0);
    }

    #[tokio::test]
    async fn v6_apply_includes_warnings_policy() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Insert second class for reclassification warning
        let now = chrono::Utc::now().to_rfc3339();
        let sync_id = uuid::Uuid::new_v4().to_string();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO equipment_classes
              (sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
              VALUES (?, 'MOTOR', 'Moteurs', NULL, 'class', 1, ?, ?)",
            [sync_id.into(), now.clone().into(), now.into()],
        ))
        .await
        .expect("insert motor class");

        // Create existing asset with PUMP class
        identity::create_asset(
            &db,
            CreateAssetPayload {
                asset_code: "IMP-8001".to_string(),
                asset_name: "Pompe originale".to_string(),
                class_code: "PUMP".to_string(),
                family_code: None,
                subfamily_code: None,
                criticality_code: "STANDARD".to_string(),
                status_code: "ACTIVE_IN_SERVICE".to_string(),
                manufacturer: None,
                model: None,
                serial_number: None,
                maintainable_boundary: true,
                org_node_id,
                commissioned_at: None,
            },
            1,
        )
        .await
        .expect("create asset");

        // Import with class=MOTOR → reclassification warning
        let csv = format!(
            "asset_code,asset_name,class_code,criticality_code,status_code,org_node_id\n\
             IMP-8001,Reclassified,MOTOR,STANDARD,ACTIVE_IN_SERVICE,{org_node_id}\n"
        )
        .into_bytes();
        let batch_id = stage_and_validate(&db, &csv, "warn_apply.csv").await;

        // Apply WITH include_warnings
        let policy = import::ApplyPolicy {
            include_warnings: true,
            external_system_code: None,
        };
        let result = import::apply_import_batch(&db, batch_id, &policy, Some(1))
            .await
            .expect("apply with warnings should succeed");

        assert_eq!(result.updated, 1, "warning row should be applied");

        let events = import::list_import_events(&db, batch_id)
            .await
            .expect("list events");
        let applied_event = events.iter().find(|e| e.event_type == "applied").expect("applied event");
        let summary: serde_json::Value =
            serde_json::from_str(applied_event.summary_json.as_ref().unwrap()).unwrap();
        assert_eq!(summary["include_warnings"], true);
    }

    // ── V7 — Permission gate ──────────────────────────────────────────────

    #[tokio::test]
    async fn v7_user_without_eq_import_is_denied() {
        let db = setup().await;

        // User 999 has NO user_scope_assignments → check_permission returns false
        let has_perm = rbac::check_permission(&db, 999, "eq.import", &PermissionScope::Global)
            .await
            .expect("check_permission should not error");

        assert!(
            !has_perm,
            "user without role assignment should NOT have eq.import"
        );
    }

    #[tokio::test]
    async fn v7_admin_with_role_assignment_has_eq_import() {
        let db = setup().await;

        // Assign user 1 the Administrator role at tenant scope
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, created_at, updated_at) \
             VALUES ('test-assign', 1, \
               (SELECT id FROM roles WHERE name = 'Administrator'), \
               'tenant', ?, ?)",
            [now.clone().into(), now.into()],
        ))
        .await
        .expect("insert user_scope_assignment");

        let has_perm = rbac::check_permission(&db, 1, "eq.import", &PermissionScope::Global)
            .await
            .expect("check_permission should not error");

        assert!(
            has_perm,
            "Administrator should have eq.import permission"
        );
    }

    #[tokio::test]
    async fn v7_operator_role_does_not_have_eq_import() {
        let db = setup().await;

        // Assign user 2 the Operator role
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, created_at, updated_at) \
             VALUES ('test-op', 2, \
               (SELECT id FROM roles WHERE name = 'Operator'), \
               'tenant', ?, ?)",
            [now.clone().into(), now.into()],
        ))
        .await
        .expect("insert operator assignment");

        let has_perm = rbac::check_permission(&db, 2, "eq.import", &PermissionScope::Global)
            .await
            .expect("check_permission should not error");

        assert!(
            !has_perm,
            "Operator should NOT have eq.import permission"
        );
    }
}
