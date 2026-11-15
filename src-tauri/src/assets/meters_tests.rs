//! Supervisor verification tests for Phase 2 SP02 File 02 Sprint S2.
//!
//! V1 — Primary meter uniqueness: only one primary meter per asset
//! V2 — Reading monotonic rule: reading values and timestamps must increase
//! V3 — Source metadata capture: source_type and source_reference are persisted

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::assets::meters::{self, CreateAssetMeterPayload, RecordMeterReadingPayload};
    use crate::errors::AppError;
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};

    /// In-memory SQLite with all migrations + seed data.
    async fn setup() -> sea_orm::DatabaseConnection {
        crate::db::test_db::seeded().await
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
                structure_model_id: model.id as i64,
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
        let entity_sync_id = uuid::Uuid::new_v4().to_string();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO equipment_classes
              (entity_sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
              VALUES (?, 'PUMP', 'Pompes', NULL, 'class', 1, ?, ?)",
            [entity_sync_id.into(), now.clone().into(), now.into()],
        ))
        .await
        .expect("insert equipment class");
    }

    /// Create a test asset with a unique code.
    async fn create_test_asset(db: &sea_orm::DatabaseConnection, code: &str, org_node_id: i64) -> identity::Asset {
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
                rams_schedule_reference_value_id: None,
                rams_utilization_factor: Some(1.0),
                org_node_id,
                commissioned_at: None,
                commercial: None,
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "KM").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: Some(
                    crate::reference::units::unit_id_by_code(&db, "CYCLE")
                        .await
                        .expect("unit"),
                ),
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
        let all = meters::list_asset_meters(&db, asset.id).await.expect("list meters");
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "KM").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
        let meters_list = meters::list_asset_meters(&db, asset.id).await.expect("list meters");
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
        assert_eq!(reading.source_reference.as_deref(), Some("Releve terrain operateur #7"));
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "KM").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: None,
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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
                unit_value_id: Some(crate::reference::units::unit_id_by_code(&db, "H").await.expect("unit")),
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

    // ── L2-01 — cancel a wrong reading, warn on abnormal jumps ───────────

    async fn hours_meter(db: &sea_orm::DatabaseConnection, code: &str, rate: Option<f64>) -> meters::AssetMeter {
        let node_id = setup_org_node(db).await;
        setup_equipment_class(db).await;
        let asset = create_test_asset(db, code, node_id).await;
        meters::create_asset_meter(
            db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Hours".to_string(),
                meter_code: None,
                meter_type: "HOURS".to_string(),
                unit_value_id: Some(crate::reference::units::unit_id_by_code(db, "H").await.expect("unit")),
                initial_reading: None,
                expected_rate_per_day: rate,
                rollover_value: None,
                is_primary: Some(false),
            },
            1,
        )
        .await
        .expect("create meter")
    }

    fn reading(meter_id: i64, value: f64, at: &str) -> RecordMeterReadingPayload {
        RecordMeterReadingPayload {
            meter_id,
            reading_value: value,
            reading_at: Some(at.to_string()),
            source_type: "MANUAL".to_string(),
            source_reference: None,
            quality_flag: None,
        }
    }

    #[tokio::test]
    async fn l2_01_jump_needs_confirmation_then_cancel_restores_the_meter() {
        let db = setup().await;
        let meter = hours_meter(&db, "L201-A", Some(10.0)).await;
        meters::record_meter_reading_checked(&db, reading(meter.id, 1000.0, "2026-04-01T08:00:00Z"), false, 1)
            .await
            .expect("first reading");

        // 12 450 typed instead of 1 245 over ten days of about 10 h/day.
        let err =
            meters::record_meter_reading_checked(&db, reading(meter.id, 12_450.0, "2026-04-11T08:00:00Z"), false, 1)
                .await
                .expect_err("abnormal jump is refused until confirmed");
        match err {
            AppError::ValidationFailed(m) => assert!(m.iter().any(|x| x.contains("GATE_ASSET_METER_JUMP_UNCONFIRMED"))),
            other => panic!("unexpected: {other:?}"),
        }
        let warning = meters::assess_meter_reading(&db, meter.id, 12_450.0, Some("2026-04-11T08:00:00Z".into()))
            .await
            .expect("assess")
            .expect("warning");
        assert!((warning.expected_delta - 100.0).abs() < 1e-6);
        assert!(
            meters::assess_meter_reading(&db, meter.id, 1_100.0, Some("2026-04-11T08:00:00Z".into()))
                .await
                .expect("assess")
                .is_none()
        );

        // Confirmed (the user really typed it), then found wrong: cancel with a reason.
        let wrong =
            meters::record_meter_reading_checked(&db, reading(meter.id, 12_450.0, "2026-04-11T08:00:00Z"), true, 1)
                .await
                .expect("confirmed jump");
        let cancelled = meters::cancel_meter_reading(&db, wrong.id, "  erreur de saisie  ", 1)
            .await
            .expect("cancel");
        assert!(cancelled.is_cancelled);
        assert_eq!(cancelled.cancel_reason.as_deref(), Some("erreur de saisie"));

        let m = meters::list_asset_meters(&db, meter.asset_id).await.expect("list")[0].clone();
        assert!(
            (m.current_reading - 1000.0).abs() < 1e-6,
            "meter back to the previous reading"
        );
        let latest = meters::get_latest_meter_value(&db, meter.id)
            .await
            .expect("latest")
            .expect("some");
        assert!((latest.reading_value - 1000.0).abs() < 1e-6);

        // The cancelled reading stays in the history, the marker row does not.
        let history = meters::list_meter_readings(&db, meter.id, None).await.expect("history");
        assert_eq!(history.len(), 2);
        assert!(history
            .iter()
            .any(|r| r.is_cancelled && (r.reading_value - 12_450.0).abs() < 1e-6));
        assert!(history.iter().all(|r| r.created_by_id == Some(1)));

        // The right value can now be recorded (a lower one than the cancelled value).
        meters::record_meter_reading_checked(&db, reading(meter.id, 1_245.0, "2026-04-11T09:00:00Z"), false, 1)
            .await
            .expect("correct reading after the cancellation");

        let audited: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = 'asset.meter_reading_cancelled'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "n")
            .expect("n");
        assert_eq!(audited, 1);
    }

    #[tokio::test]
    async fn l2_01_cancel_needs_a_reason_and_only_the_latest_reading() {
        let db = setup().await;
        let meter = hours_meter(&db, "L201-B", None).await;
        let first = meters::record_meter_reading(&db, reading(meter.id, 100.0, "2026-04-01T08:00:00Z"), 1)
            .await
            .expect("first");
        let second = meters::record_meter_reading(&db, reading(meter.id, 150.0, "2026-04-02T08:00:00Z"), 1)
            .await
            .expect("second");

        assert!(
            meters::cancel_meter_reading(&db, second.id, "   ", 1).await.is_err(),
            "reason required"
        );
        assert!(
            meters::cancel_meter_reading(&db, first.id, "motif", 1).await.is_err(),
            "an older reading is not the latest"
        );
        meters::cancel_meter_reading(&db, second.id, "motif", 1)
            .await
            .expect("cancel latest");
        assert!(
            meters::cancel_meter_reading(&db, second.id, "motif", 1).await.is_err(),
            "already cancelled"
        );
        // Now the first one is the latest valid reading, but also the only one: the meter's starting
        // value is not kept as a reading, so cancelling it would drop the meter to zero.
        let err = meters::cancel_meter_reading(&db, first.id, "motif", 1)
            .await
            .expect_err("the only reading cannot be cancelled");
        assert!(format!("{err:?}").contains("GATE_ASSET_METER_CANCEL_ONLY_READING"));
        let m = meters::list_asset_meters(&db, meter.asset_id).await.expect("list")[0].clone();
        assert!((m.current_reading - 100.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn l2_01_two_cancellations_of_one_reading_show_it_once() {
        let db = setup().await;
        let meter = hours_meter(&db, "L201-C", None).await;
        meters::record_meter_reading(&db, reading(meter.id, 100.0, "2026-04-01T08:00:00Z"), 1)
            .await
            .expect("first");
        let second = meters::record_meter_reading(&db, reading(meter.id, 150.0, "2026-04-02T08:00:00Z"), 1)
            .await
            .expect("second");
        meters::cancel_meter_reading(&db, second.id, "motif", 1)
            .await
            .expect("cancel");
        // A second device cancelled the same reading offline: its marker arrives through sync.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO asset_meter_readings \
             (meter_id, reading_value, reading_at, source_type, quality_flag, created_by_id, created_at, \
              reading_kind, entity_sync_id, cancels_sync_id, cancel_reason) \
             VALUES (?, 150.0, '2026-04-03T08:00:00Z', 'MANUAL', 'cancelled', 1, '2026-04-03T08:00:00Z', \
                     'cancellation', 'other-device-marker', ?, 'autre appareil')",
            [meter.id.into(), second.entity_sync_id.clone().into()],
        ))
        .await
        .expect("second marker");
        let history = meters::list_meter_readings(&db, meter.id, None).await.expect("history");
        assert_eq!(history.len(), 2, "each reading once, markers hidden");
        assert_eq!(history.iter().filter(|r| r.is_cancelled).count(), 1);
    }

    #[tokio::test]
    async fn l2_01_cancelled_quality_is_reserved_for_markers() {
        let db = setup().await;
        let meter = hours_meter(&db, "L201-D", None).await;
        let mut payload = reading(meter.id, 100.0, "2026-04-01T08:00:00Z");
        payload.quality_flag = Some("cancelled".to_string());
        let err = meters::record_meter_reading(&db, payload, 1)
            .await
            .expect_err("reserved quality");
        assert!(format!("{err:?}").contains("GATE_ASSET_METER_QUALITY_RESERVED"));
    }

    // ── L2-08 — alarm thresholds raise an intervention request ───────────

    #[test]
    fn l2_08_threshold_rules() {
        use meters::{checked_thresholds, crossed_level, ThresholdDirection, ThresholdLevel};
        // No threshold: nothing stored, nothing crossed.
        assert_eq!(
            checked_thresholds(None, None, Some("above")).expect("clear"),
            (None, None, None)
        );
        assert_eq!(crossed_level(1e9, None, None, None), None);
        // A threshold needs its direction.
        let err = checked_thresholds(Some(80.0), None, None).expect_err("direction");
        assert!(format!("{err:?}").contains("GATE_ASSET_METER_THRESHOLD_DIRECTION_REQUIRED"));
        // The alert comes before the critical threshold, on the side of the direction.
        let err = checked_thresholds(Some(90.0), Some(80.0), Some("above")).expect_err("order");
        assert!(format!("{err:?}").contains("GATE_ASSET_METER_THRESHOLD_ORDER"));
        let err = checked_thresholds(Some(10.0), Some(20.0), Some("below")).expect_err("order below");
        assert!(format!("{err:?}").contains("GATE_ASSET_METER_THRESHOLD_ORDER"));
        assert_eq!(
            checked_thresholds(Some(20.0), Some(10.0), Some("below")).expect("below"),
            (Some(20.0), Some(10.0), Some(ThresholdDirection::Below))
        );
        // Critical wins over alert; the threshold value itself crosses.
        let above = Some(ThresholdDirection::Above);
        assert_eq!(crossed_level(79.9, Some(80.0), Some(100.0), above), None);
        assert_eq!(
            crossed_level(80.0, Some(80.0), Some(100.0), above),
            Some(ThresholdLevel::Alert)
        );
        assert_eq!(
            crossed_level(150.0, Some(80.0), Some(100.0), above),
            Some(ThresholdLevel::Critical)
        );
        let below = Some(ThresholdDirection::Below);
        assert_eq!(
            crossed_level(5.0, Some(20.0), Some(10.0), below),
            Some(ThresholdLevel::Critical)
        );
        assert_eq!(
            crossed_level(15.0, Some(20.0), Some(10.0), below),
            Some(ThresholdLevel::Alert)
        );
        assert_eq!(crossed_level(25.0, Some(20.0), Some(10.0), below), None);
        // Only a critical threshold: below it nothing is raised.
        assert_eq!(crossed_level(90.0, None, Some(100.0), above), None);
    }

    async fn threshold_requests(db: &sea_orm::DatabaseConnection, meter_id: i64) -> Vec<(i64, String, String)> {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ir.source_meter_reading_id AS rid, ir.source_threshold_level AS lvl, ir.request_type AS rt \
               FROM intervention_requests ir JOIN asset_meter_readings r ON r.id = ir.source_meter_reading_id \
              WHERE r.meter_id = ? ORDER BY ir.id",
            [meter_id.into()],
        ))
        .await
        .expect("query")
        .iter()
        .map(|r| {
            (
                r.try_get::<i64>("", "rid").expect("rid"),
                r.try_get::<String>("", "lvl").expect("lvl"),
                r.try_get::<String>("", "rt").expect("rt"),
            )
        })
        .collect()
    }

    #[tokio::test]
    async fn l2_08_crossing_a_threshold_raises_one_request_per_level_while_open() {
        let db = setup().await;
        let meter = hours_meter(&db, "L208-A", None).await;
        let set = meters::set_meter_thresholds(
            &db,
            meters::SetMeterThresholdsPayload {
                meter_id: meter.id,
                alert_threshold: Some(100.0),
                critical_threshold: Some(200.0),
                threshold_direction: Some("above".into()),
            },
            1,
        )
        .await
        .expect("set thresholds");
        assert_eq!(set.alert_threshold, Some(100.0));
        assert_eq!(set.threshold_direction.as_deref(), Some("above"));

        let r = meters::record_meter_reading(&db, reading(meter.id, 50.0, "2026-05-01T08:00:00Z"), 1)
            .await
            .expect("below the thresholds");
        assert!(r.threshold_level.is_none() && r.raised_request_id.is_none());

        let alert = meters::record_meter_reading(&db, reading(meter.id, 150.0, "2026-05-02T08:00:00Z"), 1)
            .await
            .expect("alert reading");
        assert_eq!(alert.threshold_level.as_deref(), Some("alert"));
        assert!(alert.raised_request_code.is_some(), "{:?}", alert.threshold_problem);

        // Still open: the next reading above the alert threshold raises nothing new.
        let again = meters::record_meter_reading(&db, reading(meter.id, 160.0, "2026-05-03T08:00:00Z"), 1)
            .await
            .expect("second alert reading");
        assert!(again.raised_request_id.is_none());
        assert_eq!(again.open_request_code, alert.raised_request_code);

        // The critical level is a request of its own.
        let critical = meters::record_meter_reading(&db, reading(meter.id, 250.0, "2026-05-04T08:00:00Z"), 1)
            .await
            .expect("critical reading");
        assert_eq!(critical.threshold_level.as_deref(), Some("critical"));
        assert!(critical.raised_request_id.is_some());

        let raised = threshold_requests(&db, meter.id).await;
        assert_eq!(
            raised,
            vec![
                (alert.id, "alert".to_string(), "inspection".to_string()),
                (critical.id, "critical".to_string(), "inspection".to_string()),
            ],
            "each request records the reading that raised it"
        );

        // The history shows which reading raised which request.
        let history = meters::list_meter_readings(&db, meter.id, None).await.expect("history");
        let shown = history.iter().find(|h| h.id == critical.id).expect("critical reading");
        assert_eq!(shown.raised_request_code, critical.raised_request_code);
        assert_eq!(shown.threshold_level.as_deref(), Some("critical"));

        // Closing the alert request leaves the critical one open: no second critical request.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'closed' WHERE id = ?",
            [alert.raised_request_id.expect("alert id").into()],
        ))
        .await
        .expect("close");
        let critical_again = meters::record_meter_reading(&db, reading(meter.id, 260.0, "2026-05-05T08:00:00Z"), 1)
            .await
            .expect("critical again");
        assert!(
            critical_again.raised_request_id.is_none(),
            "critical request still open"
        );
        assert!(critical_again.open_request_code.is_some());

        // Once the critical request is closed, a new crossing raises a new one.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'closed' WHERE id = ?",
            [critical.raised_request_id.expect("critical id").into()],
        ))
        .await
        .expect("close critical");
        let critical_third = meters::record_meter_reading(&db, reading(meter.id, 270.0, "2026-05-06T08:00:00Z"), 1)
            .await
            .expect("critical after close");
        assert!(
            critical_third.raised_request_id.is_some(),
            "a closed request no longer blocks a new one"
        );
        assert_ne!(critical_third.raised_request_id, critical.raised_request_id);
    }

    #[tokio::test]
    async fn l2_08_a_request_that_cannot_be_raised_never_loses_the_reading() {
        let db = setup().await;
        let meter = hours_meter(&db, "L208-B", None).await;
        meters::set_meter_thresholds(
            &db,
            meters::SetMeterThresholdsPayload {
                meter_id: meter.id,
                alert_threshold: None,
                critical_threshold: Some(10.0),
                threshold_direction: Some("above".into()),
            },
            1,
        )
        .await
        .expect("set");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET installed_at_node_id = NULL WHERE id = ?",
            [meter.asset_id.into()],
        ))
        .await
        .expect("remove location");
        let r = meters::record_meter_reading(&db, reading(meter.id, 50.0, "2026-05-01T08:00:00Z"), 1)
            .await
            .expect("the reading is recorded");
        assert_eq!(r.threshold_level.as_deref(), Some("critical"));
        assert!(r.raised_request_id.is_none());
        assert!(r
            .threshold_problem
            .as_deref()
            .is_some_and(|p| p.contains("GATE_ASSET_METER_THRESHOLD_NO_LOCATION")));
        assert!(threshold_requests(&db, meter.id).await.is_empty());
        let m = meters::list_asset_meters(&db, meter.asset_id).await.expect("list")[0].clone();
        assert!((m.current_reading - 50.0).abs() < 1e-6);

        // Clearing the thresholds clears the direction too.
        let cleared = meters::set_meter_thresholds(
            &db,
            meters::SetMeterThresholdsPayload {
                meter_id: meter.id,
                alert_threshold: None,
                critical_threshold: None,
                threshold_direction: Some("above".into()),
            },
            1,
        )
        .await
        .expect("clear");
        assert!(cleared.critical_threshold.is_none() && cleared.threshold_direction.is_none());
    }
}
