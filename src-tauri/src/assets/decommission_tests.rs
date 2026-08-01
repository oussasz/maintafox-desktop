//! Domain tests for asset decommission orchestration.

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::assets::decommission::{self, DecommissionAssetPayload};
    use crate::assets::identity::{self, CreateAssetPayload};
    use crate::assets::lifecycle;
    use crate::errors::AppError;
    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};

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
                code: "SITE-DEC".to_string(),
                name: "Decommission Test Site".to_string(),
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
            },
            1,
        )
        .await
        .expect(&format!("create asset {code}"))
    }

    async fn insert_open_di(db: &sea_orm::DatabaseConnection, asset_id: i64, org_node_id: i64) {
        let user_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts ORDER BY id ASC LIMIT 1".to_string(),
            ))
            .await
            .expect("query")
            .expect("user")
            .try_get("", "id")
            .expect("id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO intervention_requests \
             (code, asset_id, org_node_id, status, title, description, origin_type, \
              impact_level, reported_urgency, submitted_at, submitter_id, created_at, updated_at) \
             VALUES ('DI-DEC-OPEN', ?, ?, 'submitted', 'Open DI', 'blocker', 'operator', \
              'unknown', 'medium', datetime('now'), ?, datetime('now'), datetime('now'))",
            [asset_id.into(), org_node_id.into(), user_id.into()],
        ))
        .await
        .expect("insert open DI");
    }

    async fn insert_open_wo(db: &sea_orm::DatabaseConnection, asset_id: i64) {
        let status_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM work_order_statuses WHERE code = 'draft' LIMIT 1".to_string(),
            ))
            .await
            .expect("query status")
            .expect("draft status")
            .try_get("", "id")
            .expect("status id");

        let type_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM work_order_types ORDER BY id ASC LIMIT 1".to_string(),
            ))
            .await
            .expect("query type")
            .expect("type")
            .try_get("", "id")
            .expect("type id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_orders \
             (code, type_id, status_id, equipment_id, title, row_version, created_at, updated_at) \
             VALUES ('WO-DEC-OPEN', ?, ?, ?, 'Open WO', 1, datetime('now'), datetime('now'))",
            [type_id.into(), status_id.into(), asset_id.into()],
        ))
        .await
        .expect("insert open WO");
    }

    #[tokio::test]
    async fn decommission_sets_status_and_returns_asset() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DEC-OK", node_id).await;

        let updated = decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "DECOMMISSIONED".to_string(),
                reason: "End of life".to_string(),
                notes: None,
            },
            1,
        )
        .await
        .expect("decommission should succeed");

        assert_eq!(updated.status_code, "DECOMMISSIONED");
        assert!(updated.decommissioned_at.is_some());
        assert!(updated.row_version > asset.row_version);

        let events = lifecycle::list_asset_lifecycle_events(&db, asset.id, Some(10))
            .await
            .expect("list events");
        assert!(
            events.iter().any(|e| e.event_type == "DECOMMISSIONED"),
            "lifecycle history must contain DECOMMISSIONED event"
        );
    }

    #[tokio::test]
    async fn decommission_scrapped_target_sets_scrapped_status() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DEC-SCR", node_id).await;

        let updated = decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "SCRAPPED".to_string(),
                reason: "Beyond repair".to_string(),
                notes: Some("Parts salvage complete".to_string()),
            },
            1,
        )
        .await
        .expect("scrapped decommission should succeed");

        assert_eq!(updated.status_code, "SCRAPPED");
        assert!(updated.decommissioned_at.is_some());
    }

    #[tokio::test]
    async fn decommission_rejects_empty_reason() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DEC-REASON", node_id).await;

        let err = decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "DECOMMISSIONED".to_string(),
                reason: "   ".to_string(),
                notes: None,
            },
            1,
        )
        .await
        .expect_err("empty reason must fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(!msgs.is_empty());
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn decommission_rejects_already_terminal() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DEC-TWICE", node_id).await;

        decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "DECOMMISSIONED".to_string(),
                reason: "First pass".to_string(),
                notes: None,
            },
            1,
        )
        .await
        .expect("first decommission");

        let err = decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "SCRAPPED".to_string(),
                reason: "Second pass".to_string(),
                notes: None,
            },
            1,
        )
        .await
        .expect_err("second decommission must fail");

        match err {
            AppError::ValidationFailed(_) => {}
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn decommission_rejects_open_di() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DEC-DI", node_id).await;
        insert_open_di(&db, asset.id, node_id).await;

        let err = decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "DECOMMISSIONED".to_string(),
                reason: "Should block".to_string(),
                notes: None,
            },
            1,
        )
        .await
        .expect_err("open DI must block");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(msgs.iter().any(|m| m.contains("intervention")));
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn decommission_rejects_open_wo() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DEC-WO", node_id).await;
        insert_open_wo(&db, asset.id).await;

        let err = decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "DECOMMISSIONED".to_string(),
                reason: "Should block".to_string(),
                notes: None,
            },
            1,
        )
        .await
        .expect_err("open WO must block");

        match err {
            AppError::ValidationFailed(msgs) => {
                assert!(msgs.iter().any(|m| m.contains("ordre") || m.contains("travail")));
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn decommission_rejects_invalid_target_status() {
        let db = setup().await;
        let node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;
        let asset = create_test_asset(&db, "DEC-BAD", node_id).await;

        let err = decommission::decommission_asset(
            &db,
            DecommissionAssetPayload {
                asset_id: asset.id,
                target_status: "RETIRED".to_string(),
                reason: "Invalid target".to_string(),
                notes: None,
            },
            1,
        )
        .await
        .expect_err("RETIRED must be rejected");

        match err {
            AppError::ValidationFailed(_) => {}
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }
}
