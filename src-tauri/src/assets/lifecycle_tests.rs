//! Supervisor verification tests for Phase 2 SP02 File 02 Sprint S1.
//!
//! V1 — Move event integrity: both old and new org node ids are saved
//! V2 — Replacement traceability: related_asset_id linkage preserved
//! V3 — Decommission behavior: status changes and historical row preserved

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::assets::lifecycle::{self, RecordLifecycleEventPayload};
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
                structure_model_id: model.id as i64,
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
        let entity_sync_id = uuid::Uuid::new_v4().to_string();
        let node_type_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM org_node_types WHERE code = 'SITE' LIMIT 1".to_string(),
            ))
            .await
            .expect("query node type")
            .expect("SITE type row")
            .try_get("", "id")
            .expect("node type id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO org_nodes \
             (entity_sync_id, code, name, node_type_id, parent_id, status, \
              created_at, updated_at, row_version, structure_model_id) \
             VALUES (?, 'SITE-002', 'Second Site', ?, NULL, 'active', ?, ?, 1, \
                     (SELECT id FROM org_structure_models WHERE status = 'active' ORDER BY id DESC LIMIT 1))",
            [
                entity_sync_id.into(),
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

    /// Create an asset with a unique code and return its full record.
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
                expected_row_version: Some(asset.row_version),
            },
            1,
        )
        .await
        .expect("move event should succeed");

        assert_eq!(event.event_type, "MOVED");
        assert_eq!(event.from_org_node_id, Some(node_id), "from_org_node_id must be saved");
        assert_eq!(event.to_org_node_id, Some(node_id_2), "to_org_node_id must be saved");

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
                expected_row_version: Some(asset.row_version),
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
                expected_row_version: Some(asset.row_version),
            },
            1,
        )
        .await
        .expect("move event");

        // Re-fetch asset — installed_at_node_id must be updated
        let updated = identity::get_asset_by_id(&db, asset.id).await.expect("re-fetch asset");
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
                expected_row_version: None,
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

    fn move_payload(asset_id: i64, to: i64, expected_row_version: Option<i64>) -> RecordLifecycleEventPayload {
        RecordLifecycleEventPayload {
            asset_id,
            event_type: "MOVED".to_string(),
            event_at: None,
            from_org_node_id: None,
            to_org_node_id: Some(to),
            from_status_code: None,
            to_status_code: None,
            from_class_code: None,
            to_class_code: None,
            related_asset_id: None,
            reason_code: None,
            notes: None,
            approved_by_id: None,
            expected_row_version,
        }
    }

    async fn moved_events(db: &sea_orm::DatabaseConnection, asset_id: i64) -> i64 {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM equipment_lifecycle_events WHERE equipment_id = ? AND event_type = 'MOVED'",
            [asset_id.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "c")
        .expect("c")
    }

    fn refused_with(err: AppError, code: &str) -> bool {
        matches!(err, AppError::ValidationFailed(lines) if lines.iter().any(|l| l.starts_with(code)))
    }

    /// AU-77: a MOVED event goes through the move action's checks (version the person saw, asset
    /// not retired, target node active); a refused move records no event and moves nothing.
    #[tokio::test]
    async fn au77_a_move_event_goes_through_the_move_checks() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "MV-AU77", node_id).await;

        // No version: refused, with the code pointing at the missing version.
        let err = lifecycle::record_lifecycle_event(&db, move_payload(asset.id, node_id_2, None), 1)
            .await
            .expect_err("version required");
        assert!(refused_with(err, "GATE_ASSET_LIFECYCLE_MOVED_VERSION"));

        // A version that is no longer the asset's: refused.
        let err =
            lifecycle::record_lifecycle_event(&db, move_payload(asset.id, node_id_2, Some(asset.row_version - 1)), 1)
                .await
                .expect_err("stale version");
        assert!(refused_with(err, "GATE_RECORD_CHANGED_ELSEWHERE"));

        // An inactive target node: refused like the move action.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET status = 'inactive' WHERE id = ?",
            [node_id_2.into()],
        ))
        .await
        .expect("deactivate node");
        lifecycle::record_lifecycle_event(&db, move_payload(asset.id, node_id_2, Some(asset.row_version)), 1)
            .await
            .expect_err("inactive target node");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET status = 'active' WHERE id = ?",
            [node_id_2.into()],
        ))
        .await
        .expect("reactivate node");

        // A retired asset (status tagged `retired` in the published list): refused like the move action.
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED' WHERE id = ?",
            [asset.id.into()],
        ))
        .await
        .expect("retire asset");
        let err = lifecycle::record_lifecycle_event(&db, move_payload(asset.id, node_id_2, Some(asset.row_version)), 1)
            .await
            .expect_err("retired asset");
        assert!(refused_with(err, "GATE_ASSET_HIER_MOVE_STATUS"));

        let unchanged = identity::get_asset_by_id(&db, asset.id).await.expect("asset");
        assert_eq!(unchanged.org_node_id, Some(node_id), "nothing moved");
        assert_eq!(unchanged.row_version, asset.row_version);
        assert_eq!(moved_events(&db, asset.id).await, 0, "no event recorded");
    }

    /// AU-77, review S5: the origin of a MOVED event is the asset's current node, never a value
    /// sent by the caller.
    #[tokio::test]
    async fn au77_a_move_event_records_the_asset_s_actual_origin() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "MV-ORIGIN", node_id).await;

        let mut payload = move_payload(asset.id, node_id_2, Some(asset.row_version));
        payload.from_org_node_id = Some(node_id_2); // not where the asset is
        let event = lifecycle::record_lifecycle_event(&db, payload, 1)
            .await
            .expect("move event");
        assert_eq!(event.from_org_node_id, Some(node_id), "the asset's actual origin");
        assert_eq!(event.to_org_node_id, Some(node_id_2));
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
                expected_row_version: None,
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

        let related_db: Option<i64> = row.try_get("", "related_asset_id").expect("related_asset_id");
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
                expected_row_version: None,
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
                expected_row_version: None,
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

        let event = record_unguarded(
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
                expected_row_version: None,
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
        let updated = identity::get_asset_by_id(&db, asset.id).await.expect("re-fetch asset");

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

        record_unguarded(
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
                expected_row_version: None,
            },
            1,
        )
        .await
        .expect("decommission event");

        // The event row must be preserved (append-only: no deleted_at on this table)
        let events = lifecycle::list_asset_lifecycle_events(&db, asset.id, None)
            .await
            .expect("list events");

        assert!(!events.is_empty(), "at least one lifecycle event must exist");
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
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");

        let asset = create_test_asset(&db, "REC-A", node_id).await;

        // First: decommission
        record_unguarded(
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
                expected_row_version: None,
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
                expected_row_version: Some(decom.row_version),
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

    #[tokio::test]
    async fn recommission_needs_a_retired_asset_a_reason_a_service_status_and_the_current_version() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");
        let asset = create_test_asset(&db, "REC-G", node_id).await;
        let event = |event_type: &str, to: Option<&str>, notes: Option<&str>, version: Option<i64>| {
            RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: event_type.to_string(),
                event_at: None,
                from_org_node_id: None,
                to_org_node_id: None,
                from_status_code: None,
                to_status_code: to.map(str::to_string),
                from_class_code: None,
                to_class_code: None,
                related_asset_id: None,
                reason_code: None,
                notes: notes.map(str::to_string),
                approved_by_id: None,
                expected_row_version: version,
            }
        };
        let code = |r: Result<lifecycle::AssetLifecycleEvent, AppError>| match r {
            Err(AppError::ValidationFailed(m)) => m[0].clone(),
            other => panic!("expected a refusal, got {other:?}"),
        };
        let reason = Some("Back after overhaul");

        // In service: nothing to recommission.
        assert_eq!(
            code(
                lifecycle::record_lifecycle_event(
                    &db,
                    event(
                        "RECOMMISSIONED",
                        Some("ACTIVE_IN_SERVICE"),
                        reason,
                        Some(asset.row_version)
                    ),
                    1
                )
                .await
            ),
            lifecycle::RECOMMISSION_NOT_RETIRED
        );

        record_unguarded(&db, event("DECOMMISSIONED", Some("DECOMMISSIONED"), None, None), 1)
            .await
            .expect("decommission");
        let retired = identity::get_asset_by_id(&db, asset.id).await.expect("fetch");

        assert_eq!(
            code(
                lifecycle::record_lifecycle_event(
                    &db,
                    event(
                        "RECOMMISSIONED",
                        Some("ACTIVE_IN_SERVICE"),
                        Some("  "),
                        Some(retired.row_version)
                    ),
                    1
                )
                .await
            ),
            lifecycle::RECOMMISSION_REASON_REQUIRED
        );
        assert_eq!(
            code(
                lifecycle::record_lifecycle_event(
                    &db,
                    event("RECOMMISSIONED", Some("SCRAPPED"), reason, Some(retired.row_version)),
                    1
                )
                .await
            ),
            lifecycle::RECOMMISSION_TARGET_RETIRED
        );
        assert_eq!(
            code(
                lifecycle::record_lifecycle_event(
                    &db,
                    event("RECOMMISSIONED", Some("ACTIVE_IN_SERVICE"), reason, None),
                    1
                )
                .await
            ),
            lifecycle::RECOMMISSION_VERSION_REQUIRED
        );
        // Someone changed the asset meanwhile: refused, nothing written.
        assert_eq!(
            code(
                lifecycle::record_lifecycle_event(
                    &db,
                    event(
                        "RECOMMISSIONED",
                        Some("ACTIVE_IN_SERVICE"),
                        reason,
                        Some(retired.row_version - 1)
                    ),
                    1
                )
                .await
            ),
            lifecycle::ASSET_STALE_VERSION
        );
        let still = identity::get_asset_by_id(&db, asset.id).await.expect("fetch");
        assert_eq!(still.status_code, "DECOMMISSIONED");

        lifecycle::record_lifecycle_event(
            &db,
            event(
                "RECOMMISSIONED",
                Some("ACTIVE_IN_SERVICE"),
                reason,
                Some(retired.row_version),
            ),
            1,
        )
        .await
        .expect("recommission");
        let back = identity::get_asset_by_id(&db, asset.id).await.expect("fetch");
        assert_eq!(back.status_code, "ACTIVE_IN_SERVICE");
        assert!(back.decommissioned_at.is_none());
    }

    /// Records a retirement directly (the path `decommission_asset` uses inside its transaction).
    async fn record_unguarded(
        db: &sea_orm::DatabaseConnection,
        payload: RecordLifecycleEventPayload,
        actor_id: i32,
    ) -> Result<lifecycle::AssetLifecycleEvent, AppError> {
        use sea_orm::TransactionTrait;
        let txn = db.begin().await?;
        let event = lifecycle::record_lifecycle_event_in(&txn, payload, actor_id).await?;
        txn.commit().await?;
        Ok(event)
    }

    #[tokio::test]
    async fn retirement_is_recorded_only_by_declasser() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");
        let asset = create_test_asset(&db, "DEC-G", node_id).await;
        let retire = |to: &str| RecordLifecycleEventPayload {
            asset_id: asset.id,
            event_type: "DECOMMISSIONED".to_string(),
            event_at: None,
            from_org_node_id: None,
            to_org_node_id: None,
            from_status_code: None,
            to_status_code: Some(to.to_string()),
            from_class_code: None,
            to_class_code: None,
            related_asset_id: None,
            reason_code: None,
            notes: Some("Fin de vie".to_string()),
            approved_by_id: None,
            expected_row_version: None,
        };
        // The generic lifecycle command never retires an asset.
        let refused = lifecycle::record_lifecycle_event(&db, retire("SCRAPPED"), 1).await;
        assert!(
            matches!(refused, Err(AppError::ValidationFailed(ref m)) if m[0] == lifecycle::DECOMMISSION_THROUGH_DECLASSER),
            "{refused:?}"
        );
        let unchanged = identity::get_asset_by_id(&db, asset.id).await.expect("fetch");
        assert_eq!(unchanged.status_code, "ACTIVE_IN_SERVICE");

        // Déclasser retires once; a second retirement (DECOMMISSIONED -> SCRAPPED) is refused and
        // the end-of-life date stays.
        let first = crate::assets::decommission::decommission_asset(
            &db,
            crate::assets::decommission::DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "DECOMMISSIONED".into(),
                reason: "Remplacée par une pompe neuve".into(),
                notes: None,
            },
            1,
        )
        .await
        .expect("decommission");
        let again = crate::assets::decommission::decommission_asset(
            &db,
            crate::assets::decommission::DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "SCRAPPED".into(),
                reason: "Mise au rebut".into(),
                notes: None,
            },
            1,
        )
        .await;
        assert!(again.is_err(), "already retired");
        let after = identity::get_asset_by_id(&db, asset.id).await.expect("fetch");
        assert_eq!(after.status_code, "DECOMMISSIONED");
        assert_eq!(after.decommissioned_at, first.decommissioned_at);
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
                expected_row_version: None,
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
                expected_row_version: None,
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
                expected_row_version: None,
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
                expected_row_version: None,
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
                expected_row_version: None,
            },
            1,
        )
        .await
        .expect("preserved event");

        let events = lifecycle::list_asset_lifecycle_events(&db, asset.id, None)
            .await
            .expect("list events");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "PRESERVED", "newest event should be first");
        assert_eq!(events[1].event_type, "INSTALLED");
    }
}
