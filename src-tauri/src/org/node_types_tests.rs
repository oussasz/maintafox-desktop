//! Supervisor verification tests for Sprint S2 — Node Types and Relationship Rules.
//!
//! V4 — Node type CRUD + capability flags round-trip
//! V5 — Root type uniqueness constraint
//! V6 — Relationship rule CRUD and guards on published models

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::errors::AppError;
    use crate::org::node_types::{self, CreateNodeTypePayload};
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

    /// Helper: create a draft model and return its id.
    async fn create_draft_model(db: &sea_orm::DatabaseConnection) -> i32 {
        let m = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("test model".to_string()),
            },
            1,
        )
        .await
        .expect("create_model should work");
        m.id
    }

    /// Helper: build a basic node type payload.
    fn site_payload(model_id: i32) -> CreateNodeTypePayload {
        CreateNodeTypePayload {
            structure_model_id: model_id,
            code: "SITE".to_string(),
            label: "Site".to_string(),
            icon_key: Some("building".to_string()),
            depth_hint: Some(0),
            can_host_assets: true,
            can_own_work: true,
            can_carry_cost_center: true,
            can_aggregate_kpis: true,
            can_receive_permits: false,
            is_root_type: true,
        }
    }

    fn zone_payload(model_id: i32) -> CreateNodeTypePayload {
        CreateNodeTypePayload {
            structure_model_id: model_id,
            code: "ZONE".to_string(),
            label: "Zone".to_string(),
            icon_key: None,
            depth_hint: Some(1),
            can_host_assets: true,
            can_own_work: false,
            can_carry_cost_center: false,
            can_aggregate_kpis: false,
            can_receive_permits: true,
            is_root_type: false,
        }
    }

    // ── V4 — Node type creation and capability flag round-trip ────────────

    #[tokio::test]
    async fn v4_create_node_type_in_draft_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let nt = node_types::create_node_type(&db, site_payload(model_id))
            .await
            .expect("should create node type");

        assert_eq!(nt.code, "SITE");
        assert_eq!(nt.structure_model_id, model_id);
        assert!(nt.is_active);
    }

    #[tokio::test]
    async fn v4_capability_flags_all_true_round_trip() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let all_true = CreateNodeTypePayload {
            structure_model_id: model_id,
            code: "ALL_TRUE".to_string(),
            label: "All True".to_string(),
            icon_key: None,
            depth_hint: None,
            can_host_assets: true,
            can_own_work: true,
            can_carry_cost_center: true,
            can_aggregate_kpis: true,
            can_receive_permits: true,
            is_root_type: false,
        };
        let nt = node_types::create_node_type(&db, all_true)
            .await
            .expect("should create node type");

        // Read it back to confirm DB round-trip
        let fetched = node_types::get_node_type_by_id(&db, nt.id).await.unwrap();
        assert!(fetched.can_host_assets);
        assert!(fetched.can_own_work);
        assert!(fetched.can_carry_cost_center);
        assert!(fetched.can_aggregate_kpis);
        assert!(fetched.can_receive_permits);
    }

    #[tokio::test]
    async fn v4_capability_flags_all_false_round_trip() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let all_false = CreateNodeTypePayload {
            structure_model_id: model_id,
            code: "ALL_FALSE".to_string(),
            label: "All False".to_string(),
            icon_key: None,
            depth_hint: None,
            can_host_assets: false,
            can_own_work: false,
            can_carry_cost_center: false,
            can_aggregate_kpis: false,
            can_receive_permits: false,
            is_root_type: false,
        };
        let nt = node_types::create_node_type(&db, all_false)
            .await
            .expect("should create node type");

        let fetched = node_types::get_node_type_by_id(&db, nt.id).await.unwrap();
        assert!(!fetched.can_host_assets);
        assert!(!fetched.can_own_work);
        assert!(!fetched.can_carry_cost_center);
        assert!(!fetched.can_aggregate_kpis);
        assert!(!fetched.can_receive_permits);
    }

    #[tokio::test]
    async fn v4_list_node_types_for_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();

        let list = node_types::list_node_types(&db, model_id)
            .await
            .expect("list should work");
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn v4_get_node_type_by_id() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let created = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let fetched = node_types::get_node_type_by_id(&db, created.id)
            .await
            .expect("should fetch by id");
        assert_eq!(fetched.code, "SITE");
        assert_eq!(fetched.sync_id, created.sync_id);
    }

    #[tokio::test]
    async fn v4_cannot_add_node_type_to_published_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        structure_model::publish_model(&db, model_id, 1)
            .await
            .expect("publish should work");

        let err = node_types::create_node_type(&db, site_payload(model_id))
            .await
            .expect_err("should reject on published model");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v4_duplicate_code_rejected() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();

        let err = node_types::create_node_type(&db, site_payload(model_id))
            .await
            .expect_err("duplicate code should be rejected");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // ── V5 — Root type uniqueness ─────────────────────────────────────────

    #[tokio::test]
    async fn v5_only_one_root_type_per_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        node_types::create_node_type(&db, site_payload(model_id))
            .await
            .expect("first root should succeed");

        // Attempt a second root type with a different code
        let second_root = CreateNodeTypePayload {
            structure_model_id: model_id,
            code: "CAMPUS".to_string(),
            label: "Campus".to_string(),
            icon_key: None,
            depth_hint: Some(0),
            can_host_assets: false,
            can_own_work: false,
            can_carry_cost_center: false,
            can_aggregate_kpis: false,
            can_receive_permits: false,
            is_root_type: true,
        };
        let err = node_types::create_node_type(&db, second_root)
            .await
            .expect_err("second root should be rejected");
        match &err {
            AppError::ValidationFailed(msgs) => {
                assert!(msgs[0].contains("root"), "message should mention root: {}", msgs[0]);
            }
            other => panic!("expected ValidationFailed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn v5_non_root_types_are_unlimited() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;

        // Two non-root types with different codes should both succeed
        node_types::create_node_type(&db, zone_payload(model_id))
            .await
            .expect("first non-root ok");

        let second = CreateNodeTypePayload {
            structure_model_id: model_id,
            code: "UNIT".to_string(),
            label: "Unit".to_string(),
            icon_key: None,
            depth_hint: Some(2),
            can_host_assets: false,
            can_own_work: true,
            can_carry_cost_center: false,
            can_aggregate_kpis: false,
            can_receive_permits: false,
            is_root_type: false,
        };
        node_types::create_node_type(&db, second)
            .await
            .expect("second non-root ok");
    }

    // ── V4 continued — Deactivation ──────────────────────────────────────

    #[tokio::test]
    async fn v4_deactivate_node_type_with_no_nodes() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let nt = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();

        let deactivated = node_types::deactivate_node_type(&db, nt.id)
            .await
            .expect("deactivation should succeed");
        assert!(!deactivated.is_active);
    }

    // ── V6 — Relationship rules ───────────────────────────────────────────

    #[tokio::test]
    async fn v6_create_relationship_rule_in_draft_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let site = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let zone = node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();

        let rule = relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_id,
                parent_type_id: site.id,
                child_type_id: zone.id,
                min_children: Some(1),
                max_children: Some(10),
            },
        )
        .await
        .expect("should create rule");

        assert_eq!(rule.parent_type_id, site.id);
        assert_eq!(rule.child_type_id, zone.id);
        assert_eq!(rule.min_children, Some(1));
        assert_eq!(rule.max_children, Some(10));
    }

    #[tokio::test]
    async fn v6_duplicate_rule_rejected() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let site = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let zone = node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();

        let payload = CreateRelationshipRulePayload {
            structure_model_id: model_id,
            parent_type_id: site.id,
            child_type_id: zone.id,
            min_children: None,
            max_children: None,
        };
        relationship_rules::create_rule(&db, payload)
            .await
            .expect("first rule ok");

        let dup = CreateRelationshipRulePayload {
            structure_model_id: model_id,
            parent_type_id: site.id,
            child_type_id: zone.id,
            min_children: None,
            max_children: None,
        };
        let err = relationship_rules::create_rule(&db, dup)
            .await
            .expect_err("duplicate should be rejected");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v6_cannot_add_rule_to_published_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let site = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let zone = node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();
        structure_model::publish_model(&db, model_id, 1).await.unwrap();

        let err = relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_id,
                parent_type_id: site.id,
                child_type_id: zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect_err("should reject on published model");
        match &err {
            AppError::ValidationFailed(msgs) => {
                assert!(msgs[0].contains("draft"), "message should mention draft: {}", msgs[0]);
            }
            other => panic!("expected ValidationFailed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn v6_delete_rule_from_draft_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let site = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let zone = node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();

        let rule = relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_id,
                parent_type_id: site.id,
                child_type_id: zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .unwrap();

        relationship_rules::delete_rule(&db, rule.id)
            .await
            .expect("delete should succeed on draft model");

        // Confirm it's gone
        let err = relationship_rules::get_rule_by_id(&db, rule.id)
            .await
            .expect_err("rule should be gone");
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn v6_cannot_delete_rule_from_published_model() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let site = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let zone = node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();

        let rule = relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_id,
                parent_type_id: site.id,
                child_type_id: zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .unwrap();

        structure_model::publish_model(&db, model_id, 1).await.unwrap();

        let err = relationship_rules::delete_rule(&db, rule.id)
            .await
            .expect_err("delete should fail on published model");
        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v6_list_rules_returns_labels() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let site = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let zone = node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_id,
                parent_type_id: site.id,
                child_type_id: zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .unwrap();

        let rules = relationship_rules::list_rules(&db, model_id)
            .await
            .expect("list_rules should work");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].parent_type_label.as_deref(), Some("Site"));
        assert_eq!(rules[0].child_type_label.as_deref(), Some("Zone"));
    }

    #[tokio::test]
    async fn v6_is_allowed_check() {
        let db = setup().await;
        let model_id = create_draft_model(&db).await;
        let site = node_types::create_node_type(&db, site_payload(model_id)).await.unwrap();
        let zone = node_types::create_node_type(&db, zone_payload(model_id)).await.unwrap();

        // Not allowed yet
        let allowed = relationship_rules::is_allowed(&db, model_id, site.id, zone.id)
            .await
            .unwrap();
        assert!(!allowed);

        // Add the rule
        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_id,
                parent_type_id: site.id,
                child_type_id: zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .unwrap();

        let allowed = relationship_rules::is_allowed(&db, model_id, site.id, zone.id)
            .await
            .unwrap();
        assert!(allowed);
    }
}
