//! Supervisor verification tests for Phase 2 SP02 File 01 Sprint S1.
//!
//! V1 — Asset creation and uniqueness: duplicate `asset_code` rejected
//! V2 — Org linkage guard: inactive org node rejected
//! V3 — Lookup governance guard: unknown criticality_code rejected

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::assets::identity::{self, CreateAssetPayload};
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
    /// Returns the org node id for linking assets.
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

    /// Insert a test equipment class so class_code can be resolved.
    /// Returns the class id.
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

    /// Standard payload for a valid asset creation.
    fn valid_payload(org_node_id: i64) -> CreateAssetPayload {
        CreateAssetPayload {
            asset_code: "PMP-1001".to_string(),
            asset_name: "Pompe centrifuge P-101".to_string(),
            class_code: "PUMP".to_string(),
            family_code: None,
            criticality_code: "STANDARD".to_string(),
            status_code: "ACTIVE_IN_SERVICE".to_string(),
            manufacturer: Some("KSB".to_string()),
            model: Some("Etanorm 50-200".to_string()),
            serial_number: Some("SN-2024-00123".to_string()),
            maintainable_boundary: true,
            org_node_id,
            commissioned_at: Some("2024-03-15T00:00:00Z".to_string()),
        }
    }

    // ── V1 — Asset creation and uniqueness ────────────────────────────────

    #[tokio::test]
    async fn v1_create_asset_then_duplicate_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // First insert succeeds
        let asset = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect("first create should succeed");

        assert_eq!(asset.asset_code, "PMP-1001");
        assert_eq!(asset.row_version, 1);
        assert!(asset.maintainable_boundary);
        assert_eq!(asset.status_code, "ACTIVE_IN_SERVICE");
        assert!(asset.org_node_id.is_some());

        // Second insert with same code must fail
        let err = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect_err("duplicate code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("PMP-1001"),
                    "error should name the duplicate code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v1_create_asset_row_version_is_1() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let asset = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect("create should succeed");

        assert_eq!(asset.row_version, 1, "new asset must start at row_version 1");
    }

    // ── V2 — Org linkage guard ────────────────────────────────────────────

    #[tokio::test]
    async fn v2_create_asset_with_nonexistent_org_node_rejected() {
        let db = setup().await;
        setup_equipment_class(&db).await;

        // Use a node id that does not exist
        let mut payload = valid_payload(999_999);
        payload.org_node_id = 999_999;

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("nonexistent org node should fail");

        match err {
            AppError::NotFound { entity, id } => {
                assert_eq!(entity, "org_node");
                assert_eq!(id, "999999");
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v2_create_asset_with_inactive_org_node_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // Deactivate the org node directly
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET status = 'inactive' WHERE id = ?",
            [org_node_id.into()],
        ))
        .await
        .expect("deactivate node");

        let payload = valid_payload(org_node_id);

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("inactive org node should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("actif") || joined.contains("inactive"),
                    "error should mention inactive status, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    // ── V3 — Lookup governance guard ──────────────────────────────────────

    #[tokio::test]
    async fn v3_create_asset_with_unknown_criticality_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let mut payload = valid_payload(org_node_id);
        payload.criticality_code = "DOES_NOT_EXIST".to_string();

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("unknown criticality code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("DOES_NOT_EXIST"),
                    "error should name the bad code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v3_create_asset_with_unknown_status_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        let mut payload = valid_payload(org_node_id);
        payload.status_code = "FANTASY_STATUS".to_string();

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("unknown status code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("FANTASY_STATUS"),
                    "error should name the bad code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v3_create_asset_with_unknown_class_code_rejected() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        // NOTE: we do NOT call setup_equipment_class here

        let payload = valid_payload(org_node_id);

        let err = identity::create_asset(&db, payload, 1)
            .await
            .expect_err("unknown class code should fail");

        match err {
            AppError::ValidationFailed(msgs) => {
                let joined = msgs.join(" ");
                assert!(
                    joined.contains("PUMP"),
                    "error should name the bad class code, got: {joined}"
                );
            }
            other => panic!("expected ValidationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn v3_valid_lookup_codes_succeed() {
        let db = setup().await;
        let org_node_id = setup_org_node(&db).await;
        setup_equipment_class(&db).await;

        // All seeded codes: STANDARD criticality, ACTIVE_IN_SERVICE status, PUMP class
        let asset = identity::create_asset(&db, valid_payload(org_node_id), 1)
            .await
            .expect("valid lookup codes should succeed");

        assert!(asset.criticality_code.as_deref() == Some("STANDARD"));
        assert_eq!(asset.status_code, "ACTIVE_IN_SERVICE");
        assert!(asset.class_code.as_deref() == Some("PUMP"));
    }
}
