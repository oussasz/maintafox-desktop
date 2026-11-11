//! Supervisor verification tests for Phase 2 SP02 File 01 Sprint S2.
//!
//! V1 — Cycle prevention: A→B then B→A must fail
//! V2 — Effective dating: unlink sets effective_to, row persists
//! V3 — Move version increment: move org node increments row_version by 1

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::assets::hierarchy::{self, LinkAssetPayload};
    use crate::assets::identity::{self, CreateAssetPayload};
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

    /// Create a second active org node under the same model as the first.
    async fn setup_second_org_node(db: &sea_orm::DatabaseConnection) -> i64 {
        // Re-use the existing child type for the second node.
        // The root already exists from setup_org_node — create a sibling root.
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

    // ── V1 — Cycle prevention ─────────────────────────────────────────────

    #[tokio::test]
    async fn v1_cycle_a_to_b_then_b_to_a_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "ASSET-A", node_id).await;
        let b = create_test_asset(&db, "ASSET-B", node_id).await;

        // A → B succeeds
        hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: b.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect("A→B should succeed");

        // B → A must fail (would create cycle)
        let err = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: b.id,
                child_asset_id: a.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect_err("B→A should fail with cycle");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(joined.contains("cycle"), "error should mention cycle, got: {joined}");
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_self_reference_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "ASSET-X", node_id).await;

        let err = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: a.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect_err("self-reference should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("propre parent"),
                    "error should mention self-reference, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_transitive_cycle_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "CYC-A", node_id).await;
        let b = create_test_asset(&db, "CYC-B", node_id).await;
        let c = create_test_asset(&db, "CYC-C", node_id).await;

        // A → B
        hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: b.id,
                relation_type: "DRIVES".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect("A→B");

        // B → C
        hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: b.id,
                child_asset_id: c.id,
                relation_type: "DRIVES".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect("B→C");

        // C → A must fail (transitive cycle)
        let err = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: c.id,
                child_asset_id: a.id,
                relation_type: "DRIVES".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect_err("C→A should fail with cycle");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(joined.contains("cycle"), "error should mention cycle, got: {joined}");
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V1b — Single-parent enforcement ───────────────────────────────────

    #[tokio::test]
    async fn v1_single_parent_rule_for_parent_child_type() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "SP-A", node_id).await;
        let b = create_test_asset(&db, "SP-B", node_id).await;
        let c = create_test_asset(&db, "SP-C", node_id).await;

        // A → C (PARENT_CHILD)
        hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: c.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect("A→C should succeed");

        // B → C (PARENT_CHILD) must fail — C already has an active parent
        let err = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: b.id,
                child_asset_id: c.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect_err("second parent should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("déjà un parent actif"),
                    "error should mention existing parent, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V2 — Effective dating behavior ────────────────────────────────────

    #[tokio::test]
    async fn v2_unlink_sets_effective_to_row_persists() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "EFF-A", node_id).await;
        let b = create_test_asset(&db, "EFF-B", node_id).await;

        // Link A → B
        let link = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: b.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect("link A→B");

        assert!(
            link.effective_to.is_none(),
            "newly created link should have no effective_to"
        );

        // Unlink
        let unlinked = hierarchy::unlink_asset_hierarchy(&db, link.relation_id, None, 1)
            .await
            .expect("unlink should succeed");

        assert!(
            unlinked.effective_to.is_some(),
            "unlinked relation must have effective_to set"
        );

        // Row must still exist in the table (not hard-deleted)
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, effective_to FROM equipment_hierarchy WHERE id = ?",
                [link.relation_id.into()],
            ))
            .await
            .expect("query")
            .expect("row must still exist after unlink");

        let eff_to: Option<String> = row.try_get("", "effective_to").expect("effective_to column");
        assert!(eff_to.is_some(), "effective_to must be set in DB");
    }

    #[tokio::test]
    async fn v2_unlink_already_ended_relation_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "END-A", node_id).await;
        let b = create_test_asset(&db, "END-B", node_id).await;

        let link = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: b.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect("link");

        // Unlink once — succeeds
        hierarchy::unlink_asset_hierarchy(&db, link.relation_id, None, 1)
            .await
            .expect("first unlink");

        // Unlink again — must fail
        let err = hierarchy::unlink_asset_hierarchy(&db, link.relation_id, None, 1)
            .await
            .expect_err("double unlink should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("déjà terminée"),
                    "error should mention already ended, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V3 — Move version increment ───────────────────────────────────────

    #[tokio::test]
    async fn v3_move_org_node_increments_row_version() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "MOV-A", node_id).await;
        assert_eq!(asset.row_version, 1);
        assert_eq!(asset.org_node_id, Some(node_id));

        // Move to second org node
        let moved = hierarchy::move_asset_org_node(
            &db, asset.id, node_id_2, 1, // expected_row_version
            1,
        )
        .await
        .expect("move should succeed");

        assert_eq!(moved.row_version, 2, "row_version must increment by 1 after move");
        assert_eq!(moved.org_node_id, Some(node_id_2), "org_node_id must be updated");
    }

    #[tokio::test]
    async fn v3_move_decommissioned_asset_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "DEC-A", node_id).await;
        // Retired statuses are the ones tagged `retired` in the published list.
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");

        // Decommission the asset directly
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED' WHERE id = ?",
            [asset.id.into()],
        ))
        .await
        .expect("decommission");

        let err = hierarchy::move_asset_org_node(&db, asset.id, node_id_2, 1, 1)
            .await
            .expect_err("move decommissioned should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("DECOMMISSIONED"),
                    "error should mention status, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v3_move_with_wrong_row_version_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = create_test_asset(&db, "VER-A", node_id).await;

        let err = hierarchy::move_asset_org_node(
            &db, asset.id, node_id_2, 99, // wrong version
            1,
        )
        .await
        .expect_err("wrong version should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("version"),
                    "error should mention version conflict, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── Domain governance — relation type validation ──────────────────────

    #[tokio::test]
    async fn link_with_unknown_relation_type_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "REL-A", node_id).await;
        let b = create_test_asset(&db, "REL-B", node_id).await;

        let err = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: b.id,
                relation_type: "MADE_UP_TYPE".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect_err("unknown relation type should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("MADE_UP_TYPE"),
                    "error should name the bad type, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── Decommissioned asset linkage guard ────────────────────────────────

    #[tokio::test]
    async fn link_decommissioned_asset_rejected() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let a = create_test_asset(&db, "DLK-A", node_id).await;
        let b = create_test_asset(&db, "DLK-B", node_id).await;

        // Decommission asset B
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED' WHERE id = ?",
            [b.id.into()],
        ))
        .await
        .expect("decommission");

        let err = hierarchy::link_asset_hierarchy(
            &db,
            LinkAssetPayload {
                parent_asset_id: a.id,
                child_asset_id: b.id,
                relation_type: "PARENT_CHILD".to_string(),
                effective_from: None,
            },
            1,
        )
        .await
        .expect_err("link to decommissioned should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("DECOMMISSIONED"),
                    "error should mention status, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── D19 — a move is recorded ("Déplacé") ──────────────────────────────

    async fn moved_events(
        db: &sea_orm::DatabaseConnection,
        asset_id: i64,
    ) -> Vec<(Option<i64>, Option<i64>, Option<i64>)> {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT from_node_id, to_node_id, performed_by_id FROM equipment_lifecycle_events \
             WHERE equipment_id = ? AND event_type = 'MOVED' ORDER BY id",
            [asset_id.into()],
        ))
        .await
        .expect("events")
        .iter()
        .map(|r| {
            (
                r.try_get("", "from_node_id").expect("from"),
                r.try_get("", "to_node_id").expect("to"),
                r.try_get("", "performed_by_id").expect("by"),
            )
        })
        .collect()
    }

    async fn move_audits(db: &sea_orm::DatabaseConnection, asset_id: i64) -> Vec<(Option<i64>, String)> {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT actor_id, details_json FROM audit_events WHERE action_code = ? AND target_id = ? ORDER BY id",
            [
                crate::assets::lifecycle::ASSET_MOVED_EVENT.into(),
                asset_id.to_string().into(),
            ],
        ))
        .await
        .expect("audits")
        .iter()
        .map(|r| {
            (
                r.try_get("", "actor_id").expect("actor"),
                r.try_get("", "details_json").expect("details"),
            )
        })
        .collect()
    }

    #[tokio::test]
    async fn d19_move_action_records_moved_event_and_audit() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "D19-A", node_id).await;

        hierarchy::move_asset_org_node(&db, asset.id, node_id_2, asset.row_version, 1)
            .await
            .expect("move");

        assert_eq!(
            moved_events(&db, asset.id).await,
            vec![(Some(node_id), Some(node_id_2), Some(1))]
        );
        let audits = move_audits(&db, asset.id).await;
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].0, Some(1));
        assert!(
            audits[0].1.contains(&format!("\"to_org_node_id\":{node_id_2}")),
            "{}",
            audits[0].1
        );
    }

    #[tokio::test]
    async fn d19_edit_form_save_moves_in_the_same_transaction() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "D19-B", node_id).await;

        let saved = identity::update_asset_identity(
            &db,
            asset.id,
            identity::UpdateAssetIdentityPayload {
                asset_name: Some("Pompe déplacée".into()),
                org_node_id: Some(node_id_2),
                ..Default::default()
            },
            asset.row_version,
            1,
        )
        .await
        .expect("edit and move");

        assert_eq!(saved.asset_name, "Pompe déplacée");
        assert_eq!(saved.org_node_id, Some(node_id_2));
        assert_eq!(
            saved.row_version,
            asset.row_version + 2,
            "one version for the edit, one for the move"
        );
        assert_eq!(
            moved_events(&db, asset.id).await,
            vec![(Some(node_id), Some(node_id_2), Some(1))]
        );
        assert_eq!(move_audits(&db, asset.id).await.len(), 1);
    }

    #[tokio::test]
    async fn d19_same_location_records_no_move() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "D19-C", node_id).await;

        let saved = identity::update_asset_identity(
            &db,
            asset.id,
            identity::UpdateAssetIdentityPayload {
                asset_name: Some("Renommée".into()),
                org_node_id: Some(node_id),
                ..Default::default()
            },
            asset.row_version,
            1,
        )
        .await
        .expect("edit");

        assert_eq!(saved.row_version, asset.row_version + 1);
        assert!(moved_events(&db, asset.id).await.is_empty());
        assert!(move_audits(&db, asset.id).await.is_empty());
    }

    #[tokio::test]
    async fn d19_refused_move_leaves_the_edit_unsaved() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "D19-D", node_id).await;

        identity::update_asset_identity(
            &db,
            asset.id,
            identity::UpdateAssetIdentityPayload {
                asset_name: Some("Ne doit pas rester".into()),
                org_node_id: Some(999_999),
                ..Default::default()
            },
            asset.row_version,
            1,
        )
        .await
        .expect_err("unknown location refused");

        let stored = identity::get_asset_by_id(&db, asset.id).await.expect("asset");
        assert_eq!(
            stored.asset_name, asset.asset_name,
            "the edit is rolled back with the refused move"
        );
        assert_eq!(stored.row_version, asset.row_version);
        assert_eq!(stored.org_node_id, Some(node_id));
        assert!(moved_events(&db, asset.id).await.is_empty());
        assert!(move_audits(&db, asset.id).await.is_empty());
    }

    #[tokio::test]
    async fn d19_org_screen_assignment_is_a_recorded_move() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "D19-E", node_id).await;

        crate::org::equipment_assignment::assign_equipment_to_node(
            &db,
            crate::org::AssignEquipmentPayload {
                equipment_id: i32::try_from(asset.id).expect("id"),
                node_id: node_id_2,
            },
            1,
        )
        .await
        .expect("assign");

        assert_eq!(
            moved_events(&db, asset.id).await,
            vec![(Some(node_id), Some(node_id_2), Some(1))]
        );
        assert_eq!(move_audits(&db, asset.id).await.len(), 1);
        let stored = identity::get_asset_by_id(&db, asset.id).await.expect("asset");
        assert_eq!(stored.row_version, asset.row_version + 1);
    }

    #[tokio::test]
    async fn d19_first_placement_is_audited_without_a_move_event() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "D19-F", node_id).await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET installed_at_node_id = NULL WHERE id = ?",
            [asset.id.into()],
        ))
        .await
        .expect("no location");

        let placed = hierarchy::move_asset_org_node(&db, asset.id, node_id, asset.row_version, 1)
            .await
            .expect("place");

        assert_eq!(placed.org_node_id, Some(node_id));
        assert!(
            moved_events(&db, asset.id).await.is_empty(),
            "a placement has no origin"
        );
        let audits = move_audits(&db, asset.id).await;
        assert_eq!(audits.len(), 1);
        assert!(audits[0].1.contains("\"from_org_node_id\":null"), "{}", audits[0].1);
    }

    #[tokio::test]
    async fn d19_explicit_moved_event_is_audited() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        let node_id_2 = setup_second_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "D19-G", node_id).await;

        crate::assets::lifecycle::record_lifecycle_event(
            &db,
            crate::assets::lifecycle::RecordLifecycleEventPayload {
                asset_id: asset.id,
                event_type: "MOVED".into(),
                to_org_node_id: Some(node_id_2),
                expected_row_version: Some(asset.row_version),
                ..Default::default()
            },
            1,
        )
        .await
        .expect("event");

        assert_eq!(
            moved_events(&db, asset.id).await,
            vec![(Some(node_id), Some(node_id_2), Some(1))]
        );
        assert_eq!(move_audits(&db, asset.id).await.len(), 1, "one audit entry, not two");
    }

    // ── L2-03 — "Déplacer le composant" ───────────────────────────────────

    use crate::assets::hierarchy::{MoveAssetComponentPayload, MoveAssetComponentResult};
    use crate::auth::scope::AccessScope;

    /// Moves `component` (at its current version) under `new_parent` (`None`: to stock).
    async fn move_component(
        db: &sea_orm::DatabaseConnection,
        component: i64,
        new_parent: Option<i64>,
        effective_date: Option<String>,
        notes: Option<&str>,
    ) -> crate::errors::AppResult<MoveAssetComponentResult> {
        let version = identity::get_asset_by_id(db, component)
            .await
            .expect("component")
            .row_version;
        hierarchy::move_asset_component(
            db,
            MoveAssetComponentPayload {
                asset_id: component,
                new_parent_asset_id: new_parent,
                effective_date,
                notes: notes.map(str::to_string),
                expected_row_version: version,
            },
            &AccessScope::Everywhere,
            1,
        )
        .await
    }

    fn gate_text(err: AppError) -> String {
        match err {
            AppError::ValidationFailed(msgs) => msgs.join(" "),
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    fn days_ago(days: i64) -> String {
        (crate::clock::now_local().date_naive() - chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string()
    }

    /// A motor `M` and two machines `A` and `B` on the same site.
    async fn motor_and_machines(db: &sea_orm::DatabaseConnection, prefix: &str) -> (i64, i64, i64, i64) {
        let node_id = setup_org_node(db).await;
        setup_equipment_class(db).await;
        let motor = create_test_asset(db, &format!("{prefix}-M"), node_id).await;
        let a = create_test_asset(db, &format!("{prefix}-A"), node_id).await;
        let b = create_test_asset(db, &format!("{prefix}-B"), node_id).await;
        (node_id, motor.id, a.id, b.id)
    }

    async fn hierarchy_rows(
        db: &sea_orm::DatabaseConnection,
        child: i64,
    ) -> Vec<(i64, Option<String>, Option<String>)> {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT parent_equipment_id, effective_from, effective_to FROM equipment_hierarchy WHERE child_equipment_id = ? ORDER BY id",
            [child.into()],
        ))
        .await
        .expect("rows")
        .iter()
        .map(|r| {
            (
                r.try_get("", "parent_equipment_id").expect("parent"),
                r.try_get("", "effective_from").expect("from"),
                r.try_get("", "effective_to").expect("to"),
            )
        })
        .collect()
    }

    #[tokio::test]
    async fn l2_03_first_install_move_and_removal_keep_the_same_record() {
        let db = setup().await;
        let (_, motor, a, b) = motor_and_machines(&db, "L23A").await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET serial_number = 'SN-4711' WHERE id = ?",
            [motor.into()],
        ))
        .await
        .expect("serial");

        // First installation: no current parent.
        let first = move_component(&db, motor, Some(a), Some(days_ago(10)), None)
            .await
            .expect("first install");
        assert!(first.ended_relation.is_none());
        assert_eq!(first.new_relation.as_ref().map(|r| r.parent_asset_id), Some(a));
        assert_eq!(
            first.new_relation.as_ref().map(|r| r.relation_type.as_str()),
            Some("PARENT_CHILD")
        );

        // Move from A to B, with a note.
        let moved = move_component(&db, motor, Some(b), Some(days_ago(3)), Some("Échange standard"))
            .await
            .expect("move");
        let ended = moved.ended_relation.expect("A relation ended");
        assert_eq!(ended.parent_asset_id, a);
        assert!(ended
            .effective_to
            .as_deref()
            .is_some_and(|d| d.starts_with(&days_ago(3))));
        let new = moved.new_relation.expect("B relation");
        assert_eq!(new.parent_asset_id, b);
        assert_eq!(new.effective_from, ended.effective_to, "one effective date for both");
        assert_eq!(moved.event.event_type, "PARENT_CHANGED");
        assert_eq!(moved.event.related_asset_id, Some(b));
        assert_eq!(moved.event.notes.as_deref(), Some("Échange standard"));
        assert_eq!(moved.event.created_by_id, Some(1));

        // Removal to stock: no new parent.
        let removed = move_component(&db, motor, None, None, None).await.expect("removal");
        assert_eq!(removed.ended_relation.map(|r| r.parent_asset_id), Some(b));
        assert!(removed.new_relation.is_none());

        // Same record, same serial number; every relation kept, all ended.
        let stored = identity::get_asset_by_id(&db, motor).await.expect("motor");
        assert_eq!(stored.serial_number.as_deref(), Some("SN-4711"));
        let rows = hierarchy_rows(&db, motor).await;
        assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), vec![a, b]);
        assert!(rows.iter().all(|r| r.2.is_some()), "{rows:?}");
        assert!(hierarchy::list_asset_parents(&db, motor)
            .await
            .expect("parents")
            .is_empty());

        // One audit entry per move.
        let audits: i64 = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM audit_events WHERE action_code = ? AND target_id = ?",
                [
                    crate::assets::lifecycle::ASSET_PARENT_CHANGED_EVENT.into(),
                    motor.to_string().into(),
                ],
            ))
            .await
            .expect("audit")
            .expect("row")
            .try_get("", "cnt")
            .expect("cnt");
        assert_eq!(audits, 3);
    }

    #[tokio::test]
    async fn l2_03_history_shows_installed_and_removed_with_the_parent() {
        let db = setup().await;
        let (_, motor, a, b) = motor_and_machines(&db, "L23H").await;
        move_component(&db, motor, Some(a), Some(days_ago(5)), None)
            .await
            .expect("install");
        move_component(&db, motor, Some(b), None, Some("Roulement HS"))
            .await
            .expect("move");

        let history = |asset_id: i64| {
            crate::assets::history::list_asset_history_events(
                &db,
                crate::assets::history::AssetHistoryQuery {
                    asset_id,
                    event_types: Some(vec!["parent_changed".into(), "child_changed".into()]),
                    period: None,
                    search: None,
                    limit: None,
                    offset: None,
                },
            )
        };
        let entries: Vec<(String, String, String)> = history(motor)
            .await
            .expect("motor history")
            .into_iter()
            .map(|e| {
                (
                    e.event_type,
                    e.metadata["movement"].as_str().expect("movement").to_string(),
                    e.metadata["parent_code"].as_str().expect("parent").to_string(),
                )
            })
            .collect();
        let entry = |movement: &str, code: &str| ("parent_changed".to_string(), movement.to_string(), code.to_string());
        // Newest first: posé sur B, déposé de A, posé sur A.
        assert_eq!(
            entries,
            vec![
                entry("installed", "L23H-B"),
                entry("removed", "L23H-A"),
                entry("installed", "L23H-A")
            ]
        );

        // Each machine shows the motor coming and going.
        let on_a: Vec<String> = history(a)
            .await
            .expect("A history")
            .into_iter()
            .map(|e| {
                assert_eq!(e.event_type, "child_changed");
                assert_eq!(e.metadata["component_code"].as_str(), Some("L23H-M"));
                e.metadata["movement"].as_str().expect("movement").to_string()
            })
            .collect();
        assert_eq!(on_a, vec!["removed".to_string(), "installed".to_string()]);
        let on_b = history(b).await.expect("B history");
        assert_eq!(on_b.len(), 1);
        assert_eq!(on_b[0].metadata["movement"].as_str(), Some("installed"));
        assert_eq!(on_b[0].description.as_deref(), Some("Roulement HS"));
        assert_eq!(on_b[0].r#ref.as_ref().and_then(|r| r.entity_id), Some(motor));
    }

    #[tokio::test]
    async fn l2_03_cycle_and_self_parent_refused() {
        let db = setup().await;
        let (_, motor, a, _) = motor_and_machines(&db, "L23C").await;
        move_component(&db, motor, Some(a), None, None).await.expect("install");

        // A under its own component: a loop.
        let err = move_component(&db, a, Some(motor), None, None)
            .await
            .expect_err("cycle");
        assert!(gate_text(err).contains("GATE_ASSET_HIER_CYCLE"));
        let err = move_component(&db, motor, Some(motor), None, None)
            .await
            .expect_err("self");
        assert!(gate_text(err).contains("GATE_ASSET_HIER_SELF_PARENT"));
        assert_eq!(hierarchy_rows(&db, a).await.len(), 0, "nothing written");
    }

    #[tokio::test]
    async fn l2_03_same_parent_refused() {
        let db = setup().await;
        let (_, motor, a, _) = motor_and_machines(&db, "L23S").await;
        // Already in stock: a removal changes nothing.
        let err = move_component(&db, motor, None, None, None)
            .await
            .expect_err("stock to stock");
        assert!(gate_text(err).contains("GATE_ASSET_MOVE_SAME_PARENT"));
        move_component(&db, motor, Some(a), None, None).await.expect("install");
        let err = move_component(&db, motor, Some(a), None, None)
            .await
            .expect_err("same parent");
        assert!(gate_text(err).contains("GATE_ASSET_MOVE_SAME_PARENT"));
        assert_eq!(hierarchy_rows(&db, motor).await.len(), 1);
    }

    #[tokio::test]
    async fn l2_03_date_before_installation_or_in_future_refused() {
        let db = setup().await;
        let (_, motor, a, b) = motor_and_machines(&db, "L23D").await;
        move_component(&db, motor, Some(a), Some(days_ago(5)), None)
            .await
            .expect("install");

        let err = move_component(&db, motor, Some(b), Some(days_ago(10)), None)
            .await
            .expect_err("before install");
        let text = gate_text(err);
        assert!(text.contains("GATE_ASSET_MOVE_DATE_BEFORE_INSTALL"), "{text}");
        assert!(text.contains(&format!("installed_on={}", days_ago(5))), "{text}");

        let err = move_component(&db, motor, Some(b), Some(days_ago(-2)), None)
            .await
            .expect_err("future");
        assert!(gate_text(err).contains("GATE_ASSET_MOVE_DATE_FUTURE"));
        let err = move_component(&db, motor, Some(b), Some("11/05/2026".into()), None)
            .await
            .expect_err("unreadable");
        assert!(gate_text(err).contains("GATE_ASSET_MOVE_DATE_INVALID"));

        // The installation day itself is accepted.
        move_component(&db, motor, Some(b), Some(days_ago(5)), None)
            .await
            .expect("same day as installation");
    }

    #[tokio::test]
    async fn l2_03_stale_version_and_retired_parent_refused() {
        let db = setup().await;
        let (_, motor, a, b) = motor_and_machines(&db, "L23V").await;
        crate::assets::taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(&db)
            .await
            .expect("status tags");
        let seen = identity::get_asset_by_id(&db, motor).await.expect("motor").row_version;
        move_component(&db, motor, Some(a), None, None).await.expect("install");

        let err = hierarchy::move_asset_component(
            &db,
            MoveAssetComponentPayload {
                asset_id: motor,
                new_parent_asset_id: Some(b),
                effective_date: None,
                notes: None,
                expected_row_version: seen,
            },
            &AccessScope::Everywhere,
            1,
        )
        .await
        .expect_err("stale version");
        assert!(gate_text(err).contains("GATE_RECORD_CHANGED_ELSEWHERE"));

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET lifecycle_status = 'DECOMMISSIONED' WHERE id = ?",
            [b.into()],
        ))
        .await
        .expect("retire B");
        let err = move_component(&db, motor, Some(b), None, None)
            .await
            .expect_err("retired parent");
        assert!(gate_text(err).contains("GATE_ASSET_MOVE_RETIRED"));
    }

    #[tokio::test]
    async fn l2_03_new_parent_outside_the_persons_sites_refused() {
        let db = setup().await;
        let (node_id, motor, a, _) = motor_and_machines(&db, "L23P").await;
        let node_id_2 = setup_second_org_node(&db).await;
        let elsewhere = create_test_asset(&db, "L23P-X", node_id_2).await;
        let version = identity::get_asset_by_id(&db, motor).await.expect("motor").row_version;
        let only_first_site = AccessScope::Subtrees([node_id].into_iter().collect());

        let payload = |parent: i64, version: i64| MoveAssetComponentPayload {
            asset_id: motor,
            new_parent_asset_id: Some(parent),
            effective_date: None,
            notes: None,
            expected_row_version: version,
        };
        hierarchy::move_asset_component(&db, payload(elsewhere.id, version), &only_first_site, 1)
            .await
            .expect_err("new parent on another site");
        assert!(hierarchy_rows(&db, motor).await.is_empty(), "nothing written");

        hierarchy::move_asset_component(&db, payload(a, version), &only_first_site, 1)
            .await
            .expect("same site allowed");
    }

    #[tokio::test]
    async fn l2_03_parent_change_is_not_a_bare_lifecycle_event() {
        let db = setup().await;
        let (_, motor, a, _) = motor_and_machines(&db, "L23E").await;
        let err = crate::assets::lifecycle::record_lifecycle_event(
            &db,
            crate::assets::lifecycle::RecordLifecycleEventPayload {
                asset_id: motor,
                event_type: "PARENT_CHANGED".into(),
                related_asset_id: Some(a),
                ..Default::default()
            },
            1,
        )
        .await
        .expect_err("refused");
        assert!(gate_text(err).contains("GATE_ASSET_PARENT_CHANGE_THROUGH_MOVE"));
    }
}
