//! Supervisor verification tests for Phase 2 SP02 File 02 Sprint S2.
//!
//! V1 — Primary meter uniqueness: only one primary meter per asset
//! V2 — Reading monotonic rule: reading values and timestamps must increase
//! V3 — Source metadata capture: source_type and source_reference are persisted

#[cfg(test)]
mod tests {
    use sea_orm::{Database, DbBackend, ConnectionTrait, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::assets::meters::{
        self, CreateAssetMeterPayload, RecordMeterReadingPayload,
    };
    use crate::errors::AppError;
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

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: root_type.id,
                child_type_id: root_type.id,
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

    /// Insert a test equipment class.
    async fn setup_equipment_class(db: &sea_orm::DatabaseConnection) {
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
    }

    /// Create a test asset with a unique code.
    async fn create_test_asset(
        db: &sea_orm::DatabaseConnection,
        code: &str,
        org_node_id: i64,
    ) -> identity::Asset {
        identity::create_asset(
            db,
            CreateAssetPayload {
                asset_code: code.to_string(),
                asset_name: format!("Test asset {code}"),
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
        .expect(&format!("create asset {code}"))
    }

    // ── V1 — Primary meter uniqueness ─────────────────────────────────────

    #[tokio::test]
    async fn v1_first_primary_meter_succeeds() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V1-PRIM-001", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Run Hours".to_string(),
                meter_code: Some("RH-001".to_string()),
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: Some(0.0),
                expected_rate_per_day: Some(24.0),
                rollover_value: None,
                is_primary: Some(true),
            },
            1,
        )
        .await
        .expect("first primary meter should succeed");

        assert!(meter.is_primary);
        assert_eq!(meter.meter_code.as_deref(), Some("RH-001"));
        assert_eq!(meter.meter_type, "HOURS");
    }

    #[tokio::test]
    async fn v1_second_primary_meter_fails() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V1-PRIM-002", node_id).await;

        // First primary — OK
        meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Run Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(true),
            },
            1,
        )
        .await
        .expect("first primary OK");

        // Second primary — must fail
        let err = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Odometer".to_string(),
                meter_code: None,
                meter_type: "DISTANCE".to_string(),
                unit: Some("km".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(true),
            },
            1,
        )
        .await
        .expect_err("second primary must fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("primaire")),
                    "error should mention 'primaire': {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_non_primary_meter_allowed_alongside_primary() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V1-PRIM-003", node_id).await;

        // Primary meter
        meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Run Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(true),
            },
            1,
        )
        .await
        .expect("primary OK");

        // Non-primary meter — should succeed
        let m2 = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Cycle Count".to_string(),
                meter_code: None,
                meter_type: "CYCLES".to_string(),
                unit: Some("cycles".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("non-primary alongside primary should succeed");

        assert!(!m2.is_primary);

        // Verify list returns both
        let all = meters::list_asset_meters(&db, asset.id)
            .await
            .expect("list meters");
        assert_eq!(all.len(), 2);
        // Primary should be first (ordered by is_primary DESC)
        assert!(all[0].is_primary);
    }

    // ── V2 — Reading monotonic rule ───────────────────────────────────────

    #[tokio::test]
    async fn v2_decreasing_value_without_rollover_fails() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-MONO-001", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        // First reading: 100
        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 100.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("first reading OK");

        // Second reading: 90 — must fail (decrease without rollover)
        let err = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 90.0,
                reading_at: Some("2026-04-01T09:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect_err("decreasing value without rollover must fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("inferieur")),
                    "error should mention 'inferieur': {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v2_earlier_timestamp_fails() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-MONO-002", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        // Reading at T1
        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 50.0,
                reading_at: Some("2026-04-01T10:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("first reading OK");

        // Reading at T0 (before T1) — must fail
        let err = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 60.0,
                reading_at: Some("2026-04-01T09:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect_err("earlier timestamp must fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("posterieur")),
                    "error should mention 'posterieur': {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v2_decreasing_value_with_rollover_succeeds() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-MONO-003", node_id).await;

        // Meter with rollover at 9999
        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Odometer".to_string(),
                meter_code: None,
                meter_type: "DISTANCE".to_string(),
                unit: Some("km".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: Some(9999.0),
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter with rollover");

        assert_eq!(meter.rollover_value, Some(9999.0));

        // Reading: 9500
        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 9500.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("reading 9500 OK");

        // Reading: 200 (rollover) — should succeed because rollover_value is set
        let r2 = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 200.0,
                reading_at: Some("2026-04-01T10:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("rollover reading should succeed");

        assert_eq!(r2.reading_value, 200.0);
    }

    #[tokio::test]
    async fn v2_corrected_reading_bypasses_monotonic_checks() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-MONO-004", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        // Reading: 100
        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 100.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("first reading OK");

        // Corrected reading at earlier timestamp with lower value — should succeed
        let corrected = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 80.0,
                reading_at: Some("2026-04-01T07:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: Some("correction dossier #42".to_string()),
                quality_flag: Some("corrected".to_string()),
            },
            1,
        )
        .await
        .expect("corrected reading should bypass monotonic check");

        assert_eq!(corrected.quality_flag, "corrected");
        assert_eq!(corrected.reading_value, 80.0);
    }

    #[tokio::test]
    async fn v2_reading_updates_current_value_on_meter() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-MONO-005", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: Some(0.0),
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        assert_eq!(meter.current_reading, 0.0);

        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 150.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("record reading");

        // Reload meters and verify current_reading was updated
        let meters_list = meters::list_asset_meters(&db, asset.id)
            .await
            .expect("list meters");
        assert_eq!(meters_list.len(), 1);
        assert_eq!(meters_list[0].current_reading, 150.0);
        assert!(meters_list[0].last_read_at.is_some());
    }

    // ── V3 — Source metadata capture ──────────────────────────────────────

    #[tokio::test]
    async fn v3_manual_reading_captures_source_metadata() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V3-SRC-001", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Run Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        let reading = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 42.5,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: Some("Releve terrain operateur #7".to_string()),
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("manual reading");

        assert_eq!(reading.source_type, "MANUAL");
        assert_eq!(
            reading.source_reference.as_deref(),
            Some("Releve terrain operateur #7")
        );
        assert_eq!(reading.quality_flag, "accepted");
    }

    #[tokio::test]
    async fn v3_import_reading_captures_source_metadata() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V3-SRC-002", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Odometer".to_string(),
                meter_code: None,
                meter_type: "DISTANCE".to_string(),
                unit: Some("km".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        let reading = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 1234.0,
                reading_at: Some("2026-04-02T12:00:00Z".to_string()),
                source_type: "IMPORT".to_string(),
                source_reference: Some("batch-2026-04-02-erp.csv:row42".to_string()),
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("import reading");

        assert_eq!(reading.source_type, "IMPORT");
        assert_eq!(
            reading.source_reference.as_deref(),
            Some("batch-2026-04-02-erp.csv:row42")
        );
        assert_eq!(reading.quality_flag, "accepted");
    }

    #[tokio::test]
    async fn v3_readings_persisted_via_list_and_latest() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V3-SRC-003", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        // Insert two readings in order
        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 10.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: Some("first".to_string()),
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("reading 1");

        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 25.0,
                reading_at: Some("2026-04-01T12:00:00Z".to_string()),
                source_type: "IMPORT".to_string(),
                source_reference: Some("second".to_string()),
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("reading 2");

        // list_meter_readings: newest-first
        let readings = meters::list_meter_readings(&db, meter.id, None)
            .await
            .expect("list readings");
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0].reading_value, 25.0);
        assert_eq!(readings[0].source_type, "IMPORT");
        assert_eq!(readings[1].reading_value, 10.0);
        assert_eq!(readings[1].source_type, "MANUAL");

        // get_latest_meter_value: returns the most recent accepted
        let latest = meters::get_latest_meter_value(&db, meter.id)
            .await
            .expect("get latest")
            .expect("should have a latest reading");
        assert_eq!(latest.reading_value, 25.0);
        assert_eq!(latest.source_reference.as_deref(), Some("second"));
    }

    // ── Additional governance tests ───────────────────────────────────────

    #[tokio::test]
    async fn invalid_meter_type_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "GOV-MT-001", node_id).await;

        let err = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Bad Meter".to_string(),
                meter_code: None,
                meter_type: "NONEXISTENT".to_string(),
                unit: None,
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect_err("invalid meter_type must fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("equipment.meter_type")),
                    "error should reference domain: {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_source_type_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "GOV-ST-001", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        let err = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 10.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "FAKE_SOURCE".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect_err("invalid source_type must fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("reading_source_type")),
                    "error should reference domain: {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn negative_reading_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "GOV-NEG-001", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: None,
                expected_rate_per_day: None,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter");

        let err = meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: -5.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: None,
                quality_flag: None,
            },
            1,
        )
        .await
        .expect_err("negative reading must fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("negative")),
                    "error should mention 'negative': {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }
}
