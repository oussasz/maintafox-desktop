//! Supervisor verification tests for Phase 2 SP02 File 02 Sprint S1.
//!
//! V1 — Move event integrity: both old and new org node ids are saved
//! V2 — Replacement traceability: related_asset_id linkage preserved
//! V3 — Decommission behavior: status changes and historical row preserved

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::assets::lifecycle::{self, RecordLifecycleEventPayload};
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

    /// Create a second active org node (sibling root).
    async fn setup_second_org_node(db: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        let sync_id = uuid::Uuid::new_v4().to_string();
        let node_type_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_node_types WHERE code = 'SITE' LIMIT 1"
                    .to_string(),
            ))
            .await
            .expect("query node type")
            .expect("SITE type row")
            .try_get("", "id")
            .expect("node type id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO org_nodes \
             (sync_id, code, name, node_type_id, parent_id, status, \
              created_at, updated_at, row_version) \
             VALUES (?, 'SITE-002', 'Second Site', ?, NULL, 'active', ?, ?, 1)",
            [
                sync_id.into(),
                node_type_id.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert second org node");

        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE code = 'SITE-002'".to_string(),
        ))
        .await
        .expect("query node")
        .expect("node row")
        .try_get::<i64>("", "id")
        .expect("node id")
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

    /// Create an asset with a unique code and return its full record.
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

    // ── V1 — Move event integrity ─────────────────────────────────────────

    #[tokio::test]
    async fn v1_move_event_saves_both_org_node_ids() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "MV-A", node_id).await;

        let event = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "MOVED".to_string(),
                event_at: None,
                from_org_node_id: Some(node_id),
                to_org_node_id: Some(node_id_2),
                from_status_code: None,
                to_status_code: None,
                from_class_code: None,
                to_class_code: None,
                related_asset_id: None,
                reason_code: None,
                notes: Some("Transfert vers site 2".to_string()),
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect("move event should succeed");

        assert_eq!(event.event_type, "MOVED");
        assert_eq!(
            event.from_org_node_id,
            Some(node_id),
            "from_org_node_id must be saved"
        );
        assert_eq!(
            event.to_org_node_id,
            Some(node_id_2),
            "to_org_node_id must be saved"
        );

        // Verify the DB row directly
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT from_node_id, to_node_id \
                 FROM equipment_lifecycle_events WHERE id = ?",
                [event.id.into()],
            ))
            .await
            .expect("query")
            .expect("event row must exist");

        let from_db: Option<i64> = row.try_get("", "from_node_id").expect("from_node_id");
        let to_db: Option<i64> = row.try_get("", "to_node_id").expect("to_node_id");
        assert_eq!(from_db, Some(node_id));
        assert_eq!(to_db, Some(node_id_2));
    }

    #[tokio::test]
    async fn v1_move_auto_captures_current_org_node_as_from() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "MV-B", node_id).await;

        // Omit from_org_node_id — service should auto-capture it
        let event = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "MOVED".to_string(),
                event_at: None,
                from_org_node_id: None, // auto-capture
                to_org_node_id: Some(node_id_2),
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
        .expect("move event with auto-capture should succeed");

        assert_eq!(
            event.from_org_node_id,
            Some(node_id),
            "from_org_node_id must be auto-captured from current asset state"
        );
        assert_eq!(event.to_org_node_id, Some(node_id_2));
    }

    #[tokio::test]
    async fn v1_move_updates_installed_at_node() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "MV-C", node_id).await;
        assert_eq!(asset.org_node_id, Some(node_id));

        lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "MOVED".to_string(),
                event_at: None,
                from_org_node_id: Some(node_id),
                to_org_node_id: Some(node_id_2),
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
        .expect("move event");

        // Re-fetch asset — installed_at_node_id must be updated
        let updated = identity::get_asset_by_id(&db, asset.id)
            .await
            .expect("re-fetch asset");
        assert_eq!(
            updated.org_node_id,
            Some(node_id_2),
            "asset installed_at_node_id must reflect the move"
        );
    }

    #[tokio::test]
    async fn v1_move_without_to_org_node_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "MV-D", node_id).await;

        let err = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "MOVED".to_string(),
                event_at: None,
                from_org_node_id: Some(node_id),
                to_org_node_id: None, // missing required field
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
        .expect_err("move without to_org_node_id should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("to_org_node_id"),
                    "error should mention missing to_org_node_id, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V2 — Replacement traceability ─────────────────────────────────────

    #[tokio::test]
    async fn v2_replacement_event_stores_related_asset_id() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let old_asset = create_test_asset(&db, "RPL-OLD", node_id).await;
        let new_asset = create_test_asset(&db, "RPL-NEW", node_id).await;

        let event = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: old_asset.id,
                event_type: "REPLACED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: None,
                from_class_code: None,
                to_class_code: None,
                related_asset_id: Some(new_asset.id),
                reason_code: None,
                notes: Some("Remplacement pompe usee".to_string()),
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect("replacement event should succeed");

        assert_eq!(event.event_type, "REPLACED");
        assert_eq!(
            event.related_asset_id,
            Some(new_asset.id),
            "related_asset_id must be persisted"
        );

        // Verify DB row directly
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT related_asset_id FROM equipment_lifecycle_events WHERE id = ?",
                [event.id.into()],
            ))
            .await
            .expect("query")
            .expect("event row");

        let related_db: Option<i64> = row
            .try_get("", "related_asset_id")
            .expect("related_asset_id");
        assert_eq!(related_db, Some(new_asset.id));
    }

    #[tokio::test]
    async fn v2_replacement_without_related_asset_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "RPL-X", node_id).await;

        let err = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "REPLACED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: None,
                from_class_code: None,
                to_class_code: None,
                related_asset_id: None, // missing
                reason_code: None,
                notes: None,
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect_err("replacement without related_asset_id should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("related_asset_id"),
                    "error should mention missing related_asset_id, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v2_replacement_with_nonexistent_related_asset_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "RPL-Y", node_id).await;

        let err = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "REPLACED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: None,
                from_class_code: None,
                to_class_code: None,
                related_asset_id: Some(999_999),
                reason_code: None,
                notes: None,
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect_err("replacement with nonexistent related asset should fail");

        match err {
            AppError::NotFound { entity, .. } => {
                assert!(
                    entity.contains("related_asset"),
                    "error entity should mention related_asset, got: {entity}"
                );
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    // ── V3 — Decommission behavior ────────────────────────────────────────

    #[tokio::test]
    async fn v3_decommission_event_changes_status_and_sets_decommissioned_at() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "DEC-A", node_id).await;
        assert_eq!(asset.status_code, "ACTIVE_IN_SERVICE");
        assert!(asset.decommissioned_at.is_none());

        let event = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "DECOMMISSIONED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: Some("DECOMMISSIONED".to_string()),
                from_class_code: None,
                to_class_code: None,
                related_asset_id: None,
                reason_code: None,
                notes: Some("Fin de vie".to_string()),
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect("decommission event should succeed");

        assert_eq!(event.event_type, "DECOMMISSIONED");
        // from_status auto-captured from current state
        assert_eq!(
            event.from_status_code,
            Some("ACTIVE_IN_SERVICE".to_string()),
            "from_status_code must reflect pre-event state"
        );

        // Re-fetch the asset — status must be updated
        let updated = identity::get_asset_by_id(&db, asset.id)
            .await
            .expect("re-fetch asset");

        assert_eq!(
            updated.status_code, "DECOMMISSIONED",
            "lifecycle_status must be DECOMMISSIONED after event"
        );
        assert!(
            updated.decommissioned_at.is_some(),
            "decommissioned_at must be set after decommission event"
        );
        assert!(
            updated.row_version > asset.row_version,
            "row_version must increment after status change"
        );
    }

    #[tokio::test]
    async fn v3_decommission_preserves_historical_event_row() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "DEC-B", node_id).await;

        lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "DECOMMISSIONED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: Some("DECOMMISSIONED".to_string()),
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
        .expect("decommission event");

        // The event row must be preserved (append-only: no deleted_at on this table)
        let events = lifecycle::list_asset_lifecycle_events(&db, asset.id, None)
            .await
            .expect("list events");

        assert!(
            !events.is_empty(),
            "at least one lifecycle event must exist"
        );
        let decom = events
            .iter()
            .find(|e| e.event_type == "DECOMMISSIONED")
            .expect("decommission event must be in history");
        assert_eq!(decom.asset_id, asset.id);
    }

    #[tokio::test]
    async fn v3_recommission_restores_status_and_clears_decommissioned_at() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "REC-A", node_id).await;

        // First: decommission
        lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "DECOMMISSIONED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: Some("DECOMMISSIONED".to_string()),
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
        .expect("decommission");

        let decom = identity::get_asset_by_id(&db, asset.id)
            .await
            .expect("fetch decommissioned");
        assert_eq!(decom.status_code, "DECOMMISSIONED");

        // Then: recommission
        lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "RECOMMISSIONED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: Some("ACTIVE_IN_SERVICE".to_string()),
                from_class_code: None,
                to_class_code: None,
                related_asset_id: None,
                reason_code: None,
                notes: Some("Remise en service apres revision".to_string()),
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect("recommission event");

        let restored = identity::get_asset_by_id(&db, asset.id)
            .await
            .expect("fetch recommissioned");

        assert_eq!(
            restored.status_code, "ACTIVE_IN_SERVICE",
            "status must be restored after recommission"
        );
        assert!(
            restored.decommissioned_at.is_none(),
            "decommissioned_at must be cleared after recommission"
        );
    }

    // ── Additional: event type governance ─────────────────────────────────

    #[tokio::test]
    async fn unknown_event_type_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "UNK-A", node_id).await;

        let err = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "TELEPORTED".to_string(),
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
        .expect_err("unknown event type should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("TELEPORTED"),
                    "error should name the bad event type, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn deleted_asset_lifecycle_event_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "DEL-A", node_id).await;

        // Soft-delete the asset
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET deleted_at = datetime('now') WHERE id = ?",
            [asset.id.into()],
        ))
        .await
        .expect("soft delete");

        let err = lifecycle::record_lifecycle_event(
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
        .expect_err("event on deleted asset should fail");

        match err {
            AppError::NotFound { entity, .. } => {
                assert_eq!(entity, "equipment");
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    // ── Additional: reclassification validation ───────────────────────────

    #[tokio::test]
    async fn reclassify_without_class_codes_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "RCL-A", node_id).await;

        let err = lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "RECLASSIFIED".to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: None,
                from_class_code: None, // missing
                to_class_code: None,   // missing
                related_asset_id: None,
                reason_code: None,
                notes: None,
                approved_by_id: None,
            },
            1,
        )
        .await
        .expect_err("reclassify without class codes should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("from_class_code") && joined.contains("to_class_code"),
                    "error should mention both missing class codes, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── Additional: list events ───────────────────────────────────────────

    #[tokio::test]
    async fn list_events_returns_newest_first() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "LST-A", node_id).await;

        // Record two events
        lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "INSTALLED".to_string(),
                event_at: Some("2026-01-01T00:00:00Z".to_string()),
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
        .expect("installed event");

        lifecycle::record_lifecycle_event(
            &db,
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "PRESERVED".to_string(),
                event_at: Some("2026-06-01T00:00:00Z".to_string()),
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
        .expect("preserved event");

        let events = lifecycle::list_asset_lifecycle_events(&db, asset.id, None)
            .await
            .expect("list events");

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].event_type, "PRESERVED",
            "newest event should be first"
        );
        assert_eq!(events[1].event_type, "INSTALLED");
    }
}
