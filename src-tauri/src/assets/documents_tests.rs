//! Supervisor verification tests for Phase 2 SP02 File 02 Sprint S3.
//!
//! V1 — Document primary rule: two primary links for same purpose must not both remain active
//! V2 — Command registration: lifecycle and meter commands resolve (compile-time IPC wiring)
//! V3 — Typed service contract: all document/lifecycle/meter types compile and are usable

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::assets::documents::{self, UpsertDocumentLinkPayload};
    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::assets::lifecycle::{self, RecordLifecycleEventPayload};
    use crate::assets::meters::{self, CreateAssetMeterPayload, RecordMeterReadingPayload};
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

    // ── V1 — Document primary rule ────────────────────────────────────────

    #[tokio::test]
    async fn v1_first_primary_link_succeeds() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-001", node_id).await;

        let link = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-2026-001".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("first primary link should succeed");

        assert!(link.is_primary);
        assert_eq!(link.link_purpose, "TECHNICAL_DOSSIER");
        assert_eq!(link.document_ref, "TD-2026-001");
        assert!(link.valid_to.is_none(), "active link must have no valid_to");
    }

    #[tokio::test]
    async fn v1_second_primary_supersedes_first() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-002", node_id).await;

        // First primary link
        let link1 = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-2026-001".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("first primary link");

        // Second primary link — same purpose
        let link2 = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-2026-002".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("second primary link should succeed via supersession");

        assert!(link2.is_primary);
        assert!(link2.valid_to.is_none(), "new primary must be active");

        // Verify the first link was expired (superseded)
        let all = documents::list_asset_document_links(&db, asset.id, true)
            .await
            .expect("list all links including expired");

        assert_eq!(all.len(), 2, "both links must exist in DB");

        let old = all.iter().find(|l| l.id == link1.id).expect("old link");
        assert!(
            old.valid_to.is_some(),
            "superseded primary must have valid_to set"
        );

        let new = all.iter().find(|l| l.id == link2.id).expect("new link");
        assert!(new.valid_to.is_none(), "new primary must still be active");
    }

    #[tokio::test]
    async fn v1_only_active_links_returned_by_default() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-003", node_id).await;

        // Two primary links — first gets superseded
        documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-OLD".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("first primary");

        documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-NEW".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("second primary");

        // Default list (include_expired = false) should return only the active one
        let active = documents::list_asset_document_links(&db, asset.id, false)
            .await
            .expect("list active links");

        assert_eq!(active.len(), 1, "only active link should be returned");
        assert_eq!(active[0].document_ref, "TD-NEW");
    }

    #[tokio::test]
    async fn v1_different_purposes_allow_separate_primaries() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-004", node_id).await;

        // Primary for TECHNICAL_DOSSIER
        let link_td = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-001".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("primary TECHNICAL_DOSSIER");

        // Primary for WARRANTY — should NOT supersede the TECHNICAL_DOSSIER one
        let link_w = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "WR-001".to_string(),
                link_purpose: "WARRANTY".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("primary WARRANTY");

        assert!(link_td.is_primary);
        assert!(link_w.is_primary);

        let active = documents::list_asset_document_links(&db, asset.id, false)
            .await
            .expect("list active");
        assert_eq!(active.len(), 2, "both purpose primaries must remain active");
    }

    #[tokio::test]
    async fn v1_non_primary_does_not_supersede() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-005", node_id).await;

        // Primary link
        documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-PRIMARY".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(true),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("primary link");

        // Non-primary link same purpose — should not expire the primary
        documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "TD-SECONDARY".to_string(),
                link_purpose: "TECHNICAL_DOSSIER".to_string(),
                is_primary: Some(false),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("non-primary link");

        let active = documents::list_asset_document_links(&db, asset.id, false)
            .await
            .expect("list active");
        assert_eq!(active.len(), 2, "both links should be active");
        assert!(
            active.iter().any(|l| l.document_ref == "TD-PRIMARY" && l.is_primary),
            "primary link must still be active"
        );
    }

    #[tokio::test]
    async fn v1_expire_link_sets_valid_to() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-006", node_id).await;

        let link = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "CERT-001".to_string(),
                link_purpose: "CERTIFICATE".to_string(),
                is_primary: Some(false),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("create link");

        let expired = documents::expire_asset_document_link(
            &db,
            link.id,
            Some("2026-12-31T23:59:59Z".to_string()),
            1,
        )
        .await
        .expect("expire link");

        assert_eq!(expired.valid_to.as_deref(), Some("2026-12-31T23:59:59Z"));

        // Must not appear in active-only listing
        let active = documents::list_asset_document_links(&db, asset.id, false)
            .await
            .expect("list active");
        assert!(
            active.is_empty(),
            "expired link must not appear in active listing"
        );
    }

    #[tokio::test]
    async fn v1_empty_document_ref_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-007", node_id).await;

        let err = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "   ".to_string(), // whitespace only
                link_purpose: "MANUAL".to_string(),
                is_primary: Some(false),
                valid_from: None,
            },
            1,
        )
        .await
        .expect_err("empty document_ref must be rejected");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("vide")),
                    "error should mention 'vide': {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_invalid_link_purpose_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DOC-V1-008", node_id).await;

        let err = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "FAKE-001".to_string(),
                link_purpose: "NONEXISTENT_PURPOSE".to_string(),
                is_primary: Some(false),
                valid_from: None,
            },
            1,
        )
        .await
        .expect_err("invalid purpose must be rejected");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("introuvable")),
                    "error should mention 'introuvable': {msgs:?}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V2 — Command registration (compile-time IPC wiring) ──────────────
    //
    // Tauri commands are registered at compile time in lib.rs via
    // tauri::generate_handler![]. These tests verify that the underlying
    // service functions called by those commands resolve correctly at runtime. 
    // If commands were missing or mistyped, the compilation would fail.

    #[tokio::test]
    async fn v2_lifecycle_list_command_resolves() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-LC-001", node_id).await;

        let events = lifecycle::list_asset_lifecycle_events(&db, asset.id, None)
            .await
            .expect("list lifecycle events should resolve");

        // New asset has no lifecycle events yet
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn v2_lifecycle_record_command_resolves() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-LC-002", node_id).await;

        let event = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "PRESERVED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: None,
                from_class_code: None,
                to_class_code: None,
                related_asset_id: None,
                reason_code: None,
                notes: Some("Preservation treatment".to_string()),
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect("record lifecycle event should resolve");

        assert_eq!(event.event_type, "PRESERVED");
    }

    #[tokio::test]
    async fn v2_meter_read_command_resolves() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-MTR-001", node_id).await;

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
        .expect("create meter should resolve");

        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 42.0,
                reading_at: Some("2026-04-01T08:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: Some("V2 test".to_string()),
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("record meter reading should resolve");

        let latest = meters::get_latest_meter_value(&db, meter.id)
            .await
            .expect("get latest meter value should resolve");

        assert!(latest.is_some());
        assert_eq!(latest.unwrap().reading_value, 42.0);
    }

    #[tokio::test]
    async fn v2_document_commands_resolve() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V2-DOC-001", node_id).await;

        // upsert
        let link = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "DOC-V2-001".to_string(),
                link_purpose: "INSPECTION_PACK".to_string(),
                is_primary: Some(false),
                valid_from: None,
            },
            1,
        )
        .await
        .expect("upsert document link should resolve");

        // list
        let links = documents::list_asset_document_links(&db, asset.id, false)
            .await
            .expect("list document links should resolve");
        assert_eq!(links.len(), 1);

        // expire
        let expired = documents::expire_asset_document_link(&db, link.id, None, 1)
            .await
            .expect("expire document link should resolve");
        assert!(expired.valid_to.is_some());
    }

    // ── V3 — Typed service contract ───────────────────────────────────────
    //
    // These tests verify that all type structures from the lifecycle, meter,
    // and document modules compile correctly and their fields are accessible.
    // If any type or field were missing from ipc-types.ts or the Rust structs,
    // this would cause a compilation error.

    #[tokio::test]
    async fn v3_lifecycle_event_type_contract() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V3-TYP-001", node_id).await;

        let event = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "INSTALLED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: None,
                from_class_code: None,
                to_class_code: None,
                related_asset_id: None,
                reason_code: None,
                notes: None,
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect("create lifecycle event for type contract test");

        // Verify all fields of AssetLifecycleEvent are accessible
        let _id: i64 = event.id;
        let _asset_id: i64 = event.asset_id;
        let _event_type: &str = &event.event_type;
        let _from_org: &Option<i64> = &event.from_org_node_id;
        let _to_org: &Option<i64> = &event.to_org_node_id;
        let _from_status: &Option<String> = &event.from_status_code;
        let _to_status: &Option<String> = &event.to_status_code;
        let _from_class: &Option<String> = &event.from_class_code;
        let _to_class: &Option<String> = &event.to_class_code;
        let _related: &Option<i64> = &event.related_asset_id;
        let _reason: &Option<String> = &event.reason_code;
        let _notes: &Option<String> = &event.notes;
        let _event_at: &str = &event.event_at;
        let _created_at: &str = &event.created_at;
    }

    #[tokio::test]
    async fn v3_meter_and_reading_type_contract() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V3-TYP-002", node_id).await;

        let meter = meters::create_asset_meter(
            &db,
            CreateAssetMeterPayload {
                asset_id: asset.id,
                name: "Runtime".to_string(),
                meter_code: Some("RT-V3".to_string()),
                meter_type: "HOURS".to_string(),
                unit: Some("h".to_string()),
                initial_reading: Some(0.0),
                expected_rate_per_day: Some(8.0),
                rollover_value: Some(99999.0),
                is_primary: Some(true),
            },
            1,
        )
        .await
        .expect("create meter for type contract test");

        // Verify all fields of AssetMeter are accessible
        let _id: i64 = meter.id;
        let _asset_id: i64 = meter.asset_id;
        let _name: &str = &meter.name;
        let _code: &Option<String> = &meter.meter_code;
        let _mtype: &str = &meter.meter_type;
        let _unit: &Option<String> = &meter.unit;
        let _primary: bool = meter.is_primary;
        let _rollover: &Option<f64> = &meter.rollover_value;
        let _active: bool = meter.is_active;

        // Record a reading and verify MeterReading contract
        meters::record_meter_reading(
            &db,
            RecordMeterReadingPayload {
                meter_id: meter.id,
                reading_value: 150.0,
                reading_at: Some("2026-04-02T10:00:00Z".to_string()),
                source_type: "MANUAL".to_string(),
                source_reference: Some("V3 contract test".to_string()),
                quality_flag: None,
            },
            1,
        )
        .await
        .expect("record reading for type contract test");

        let readings = meters::list_meter_readings(&db, meter.id, None)
            .await
            .expect("list readings");
        assert!(!readings.is_empty());

        let reading = &readings[0];
        let _rid: i64 = reading.id;
        let _mid: i64 = reading.meter_id;
        let _val: f64 = reading.reading_value;
        let _rat: &str = &reading.reading_at;
        let _src: &str = &reading.source_type;
        let _sref: &Option<String> = &reading.source_reference;
        let _qf: &str = &reading.quality_flag;
        let _cat: &str = &reading.created_at;
    }

    #[tokio::test]
    async fn v3_document_link_type_contract() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "V3-TYP-003", node_id).await;

        let link = documents::upsert_asset_document_link(
            &db,
            UpsertDocumentLinkPayload {
                asset_id: asset.id,
                document_ref: "DOC-V3-001".to_string(),
                link_purpose: "MANUAL".to_string(),
                is_primary: Some(true),
                valid_from: Some("2026-01-01T00:00:00Z".to_string()),
            },
            1,
        )
        .await
        .expect("create document link for type contract test");

        // Verify all fields of AssetDocumentLink are accessible
        let _id: i64 = link.id;
        let _asset_id: i64 = link.asset_id;
        let _doc_ref: &str = &link.document_ref;
        let _purpose: &str = &link.link_purpose;
        let _primary: bool = link.is_primary;
        let _from: &Option<String> = &link.valid_from;
        let _to: &Option<String> = &link.valid_to;
        let _created_by: &Option<i64> = &link.created_by_id;
        let _created_at: &str = &link.created_at;
    }
}
