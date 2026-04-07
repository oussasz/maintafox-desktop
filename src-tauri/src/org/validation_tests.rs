//! Supervisor verification tests for Sprint S1 — Publish Validation and Remap.
//!
//! V1 — Missing root type blocks validation
//! V2 — Unreachable type blocks validation
//! V3 — Missing type-code mapping blocks publish
//! V4 — Valid draft publishes and remaps live nodes

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::org::node_types::{self, CreateNodeTypePayload};
    use crate::org::nodes::{self, CreateOrgNodePayload};
    use crate::org::relationship_rules::{self, CreateRelationshipRulePayload};
    use crate::org::structure_model::{self, CreateStructureModelPayload};
    use crate::org::validation;

    // ── Test helpers ──────────────────────────────────────────────────────

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

    /// Create and publish a base model with three types and two rules.
    ///
    /// Types: SITE (root), PLANT, ZONE
    /// Rules: SITE→PLANT, PLANT→ZONE
    ///
    /// Returns `(model_id, site_type_id, plant_type_id, zone_type_id)`.
    async fn create_base_active_model(
        db: &sea_orm::DatabaseConnection,
    ) -> (i32, i32, i32, i32) {
        let model = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("Base model v1".to_string()),
            },
            1,
        )
        .await
        .expect("create base model");

        let site = node_types::create_node_type(
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
        .expect("create SITE type");

        let plant = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "PLANT".to_string(),
                label: "Plant".to_string(),
                icon_key: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("create PLANT type");

        let zone = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "ZONE".to_string(),
                label: "Zone".to_string(),
                icon_key: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("create ZONE type");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: site.id,
                child_type_id: plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("create SITE→PLANT rule");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: model.id,
                parent_type_id: plant.id,
                child_type_id: zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("create PLANT→ZONE rule");

        // First publish — no live nodes, no remap needed.
        structure_model::publish_model(db, model.id, 1)
            .await
            .expect("publish base model");

        (model.id, site.id, plant.id, zone.id)
    }

    /// Create live nodes using the active model's type IDs.
    ///
    /// Creates: HQ (SITE), PLT1 (PLANT, cost_center_code=CC-001), ZN1 (ZONE).
    ///
    /// Returns `(site_node_id, plant_node_id, zone_node_id)`.
    async fn create_live_nodes(
        db: &sea_orm::DatabaseConnection,
        site_type_id: i32,
        plant_type_id: i32,
        zone_type_id: i32,
    ) -> (i64, i64, i64) {
        let site_node = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "HQ".to_string(),
                name: "Headquarters".to_string(),
                node_type_id: site_type_id as i64,
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
        .expect("create HQ node");

        let plant_node = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "PLT1".to_string(),
                name: "Plant 1".to_string(),
                node_type_id: plant_type_id as i64,
                parent_id: Some(site_node.id),
                description: None,
                cost_center_code: Some("CC-001".to_string()),
                external_reference: None,
                effective_from: None,
                erp_reference: None,
                notes: None,
            },
            1,
        )
        .await
        .expect("create PLT1 node");

        let zone_node = nodes::create_org_node(
            db,
            CreateOrgNodePayload {
                code: "ZN1".to_string(),
                name: "Zone 1".to_string(),
                node_type_id: zone_type_id as i64,
                parent_id: Some(plant_node.id),
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
        .expect("create ZN1 node");

        (site_node.id, plant_node.id, zone_node.id)
    }

    /// Helper to build a full draft model v2 with the same three types and rules.
    ///
    /// Returns `(draft_model_id, new_site_type_id, new_plant_type_id, new_zone_type_id)`.
    async fn create_matching_draft_model(
        db: &sea_orm::DatabaseConnection,
    ) -> (i32, i32, i32, i32) {
        let draft = structure_model::create_model(
            db,
            CreateStructureModelPayload {
                description: Some("Model v2".to_string()),
            },
            1,
        )
        .await
        .expect("create draft v2");

        let new_site = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("new SITE type");

        let new_plant = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".to_string(),
                label: "Plant v2".to_string(),
                icon_key: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("new PLANT type");

        let new_zone = node_types::create_node_type(
            db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "ZONE".to_string(),
                label: "Zone v2".to_string(),
                icon_key: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("new ZONE type");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_site.id,
                child_type_id: new_plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("SITE→PLANT rule v2");

        relationship_rules::create_rule(
            db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_plant.id,
                child_type_id: new_zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("PLANT→ZONE rule v2");

        (draft.id, new_site.id, new_plant.id, new_zone.id)
    }

    // ── V1 — Missing root type blocks publish ─────────────────────────────

    #[tokio::test]
    async fn v1_missing_root_type_blocks_publish() {
        let db = setup().await;

        let model = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("No root".to_string()),
            },
            1,
        )
        .await
        .expect("create draft");

        // A non-root type only — no root type declared.
        node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "PLANT".to_string(),
                label: "Plant".to_string(),
                icon_key: None,
                depth_hint: None,
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("create non-root type");

        let result = validation::validate_draft_model_for_publish(&db, model.id as i64)
            .await
            .expect("validate should return result");

        assert!(!result.can_publish, "validation must block publish");
        assert!(
            result.issues.iter().any(|i| i.code == "NO_ROOT_TYPE"),
            "expected NO_ROOT_TYPE issue"
        );
    }

    // ── V2 — Unreachable type blocks publish ──────────────────────────────

    #[tokio::test]
    async fn v2_unreachable_type_blocks_publish() {
        let db = setup().await;

        let model = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("Orphan test".to_string()),
            },
            1,
        )
        .await
        .expect("create draft");

        node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "ROOT".to_string(),
                label: "Root".to_string(),
                icon_key: None,
                depth_hint: Some(0),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: true,
            },
        )
        .await
        .expect("root type");

        // Orphan type — no relationship rule connects ROOT to ORPHAN.
        node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model.id,
                code: "ORPHAN".to_string(),
                label: "Orphan".to_string(),
                icon_key: None,
                depth_hint: None,
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("orphan type");

        let result = validation::validate_draft_model_for_publish(&db, model.id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish, "validation must block publish");
        assert!(
            result.issues.iter().any(|i| i.code == "UNREACHABLE_TYPE"),
            "expected UNREACHABLE_TYPE issue"
        );
    }

    // ── V3 — Missing type-code mapping blocks publish ─────────────────────

    #[tokio::test]
    async fn v3_missing_type_code_mapping_blocks_publish() {
        let db = setup().await;

        // Active model v1 with SITE, PLANT, ZONE + live nodes.
        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Draft v2 omits ZONE — live ZONE node has no target type.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("Missing ZONE".to_string()),
            },
            1,
        )
        .await
        .expect("create draft");

        let new_site = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("SITE v2");

        let new_plant = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".to_string(),
                label: "Plant v2".to_string(),
                icon_key: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("PLANT v2");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_site.id,
                child_type_id: new_plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule v2");

        let result = validation::validate_draft_model_for_publish(&db, draft.id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish, "validation must block — ZONE is missing");
        assert!(
            result.issues.iter().any(|i| i.code == "MISSING_TYPE_CODE"),
            "expected MISSING_TYPE_CODE issue for the live ZONE node"
        );
    }

    // ── V4 — Valid draft publishes and remaps live nodes ──────────────────

    #[tokio::test]
    async fn v4_valid_draft_publish_remaps_live_nodes() {
        let db = setup().await;

        // Active model v1 + live nodes.
        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        let (site_node_id, plant_node_id, zone_node_id) =
            create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Verify pre-remap: nodes reference v1 type IDs.
        let before = nodes::get_org_node_by_id(&db, site_node_id)
            .await
            .expect("get site node before");
        assert_eq!(before.node_type_id, site_id as i64);

        // Draft v2 — same codes, new rows (different IDs), updated labels.
        let (draft_id, new_site_id, new_plant_id, new_zone_id) =
            create_matching_draft_model(&db).await;

        // Publish with remap.
        let result = validation::publish_model_with_remap(&db, draft_id as i64, 1)
            .await
            .expect("publish with remap should succeed");

        assert!(result.can_publish);
        assert_eq!(result.remap_count, 3, "three type-to-type mappings");

        // Verify post-remap: nodes reference v2 type IDs.
        let site_node = nodes::get_org_node_by_id(&db, site_node_id)
            .await
            .expect("get site");
        let plant_node = nodes::get_org_node_by_id(&db, plant_node_id)
            .await
            .expect("get plant");
        let zone_node = nodes::get_org_node_by_id(&db, zone_node_id)
            .await
            .expect("get zone");

        assert_eq!(site_node.node_type_id, new_site_id as i64);
        assert_eq!(plant_node.node_type_id, new_plant_id as i64);
        assert_eq!(zone_node.node_type_id, new_zone_id as i64);

        // Old type IDs are no longer referenced.
        assert_ne!(site_node.node_type_id, site_id as i64);

        // Active model is now the draft we just published.
        let active = structure_model::get_active_model(&db)
            .await
            .expect("get active")
            .expect("active model must exist");
        assert_eq!(active.id, draft_id);
        assert_eq!(active.status, "active");

        // The old model is superseded.
        let models = structure_model::list_models(&db)
            .await
            .expect("list models");
        let superseded_count = models.iter().filter(|m| m.status == "superseded").count();
        assert_eq!(superseded_count, 1);
    }

    // ── V5 — Publish fails and transaction rolls back ─────────────────────

    #[tokio::test]
    async fn v5_publish_with_remap_fails_and_rolls_back() {
        let db = setup().await;

        // Active model v1 + live nodes.
        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Draft v2 omits ZONE — will fail validation.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("Incomplete draft".to_string()),
            },
            1,
        )
        .await
        .expect("draft");

        let new_site = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("site");

        let new_plant = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".to_string(),
                label: "Plant v2".to_string(),
                icon_key: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("plant");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_site.id,
                child_type_id: new_plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule");

        // Attempt publish — must fail.
        let err = validation::publish_model_with_remap(&db, draft.id as i64, 1)
            .await
            .expect_err("publish should fail");

        // Verify error is ValidationFailed.
        assert!(
            matches!(err, crate::errors::AppError::ValidationFailed(_)),
            "expected ValidationFailed error"
        );

        // Draft model must still be in draft status (transaction rolled back).
        let model = structure_model::get_model_by_id(&db, draft.id)
            .await
            .expect("get draft model");
        assert_eq!(model.status, "draft", "draft status must be preserved");

        // Active model must still be v1.
        let active = structure_model::get_active_model(&db)
            .await
            .expect("get active")
            .expect("active model must still exist");
        assert_eq!(active.version_number, 1);
    }

    // ── V6 — Parent-child rule drift blocks publish ───────────────────────

    #[tokio::test]
    async fn v6_parent_child_rule_drift_blocks_publish() {
        let db = setup().await;

        // Active model v1 + live nodes (SITE → PLANT → ZONE).
        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Draft v2: all three types exist, but remove the PLANT→ZONE rule.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("Missing rule".to_string()),
            },
            1,
        )
        .await
        .expect("draft");

        let new_site = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("site");

        let new_plant = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".to_string(),
                label: "Plant v2".to_string(),
                icon_key: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: true,
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("plant");

        let _new_zone = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "ZONE".to_string(),
                label: "Zone v2".to_string(),
                icon_key: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("zone");

        // Only SITE→PLANT rule — deliberately omitting PLANT→ZONE.
        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_site.id,
                child_type_id: new_plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule");

        // ZONE is unreachable AND the live PLANT→ZONE arrangement is no longer allowed.
        let result = validation::validate_draft_model_for_publish(&db, draft.id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.code == "PARENT_CHILD_NOT_ALLOWED"),
            "expected PARENT_CHILD_NOT_ALLOWED for the live ZONE node under PLANT"
        );

        // Also unreachable since no rule points to ZONE at all.
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.code == "UNREACHABLE_TYPE" && i.message.contains("ZONE")),
            "expected UNREACHABLE_TYPE for ZONE"
        );
    }

    // ── V7 — Cost-center incompatibility blocks publish ───────────────────

    #[tokio::test]
    async fn v7_cost_center_incompatibility_blocks_publish() {
        let db = setup().await;

        // Active model + live nodes (PLT1 has cost_center_code=CC-001).
        let (_, site_id, plant_id, zone_id) = create_base_active_model(&db).await;
        create_live_nodes(&db, site_id, plant_id, zone_id).await;

        // Draft v2: PLANT type loses can_carry_cost_center.
        let draft = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("Cost center break".to_string()),
            },
            1,
        )
        .await
        .expect("draft");

        let new_site = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("site");

        let new_plant = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "PLANT".to_string(),
                label: "Plant v2".to_string(),
                icon_key: None,
                depth_hint: Some(1),
                can_host_assets: true,
                can_own_work: true,
                can_carry_cost_center: false, // ← removed capability
                can_aggregate_kpis: true,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("plant no cc");

        let new_zone = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft.id,
                code: "ZONE".to_string(),
                label: "Zone v2".to_string(),
                icon_key: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("zone");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_site.id,
                child_type_id: new_plant.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft.id,
                parent_type_id: new_plant.id,
                child_type_id: new_zone.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("rule2");

        let result = validation::validate_draft_model_for_publish(&db, draft.id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.code == "COST_CENTER_INCOMPATIBLE"),
            "expected COST_CENTER_INCOMPATIBLE for PLT1 node"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Supervisor Verification Tests (File 04, Sprint S1)
    // ═══════════════════════════════════════════════════════════════════════

    // ── SV1 — Missing type-code mapping (WORKSHOP) blocks publish ─────────

    /// Supervisor V1: Create an active model with a live node of type WORKSHOP.
    /// Create a draft model that omits WORKSHOP. Validation must fail and
    /// `can_publish` must be `false`.
    #[tokio::test]
    async fn sv1_missing_workshop_type_code_blocks_publish() {
        let db = setup().await;

        // ── Active model v1: CAMPUS (root) → WORKSHOP ────────────────────
        let model_v1 = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("SV1 base with WORKSHOP".to_string()),
            },
            1,
        )
        .await
        .expect("create model v1");

        let campus = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "CAMPUS".to_string(),
                label: "Campus".to_string(),
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
        .expect("CAMPUS type");

        let workshop = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "WORKSHOP".to_string(),
                label: "Workshop".to_string(),
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
        .expect("WORKSHOP type");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_v1.id,
                parent_type_id: campus.id,
                child_type_id: workshop.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("CAMPUS→WORKSHOP rule");

        // Publish v1 (no live nodes yet — first publish).
        structure_model::publish_model(&db, model_v1.id, 1)
            .await
            .expect("publish v1");

        // ── Create live nodes using v1 types ──────────────────────────────
        let campus_node = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "MAIN_CAMPUS".to_string(),
                name: "Main Campus".to_string(),
                node_type_id: campus.id as i64,
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
        .expect("create campus node");

        nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "WS1".to_string(),
                name: "Workshop 1".to_string(),
                node_type_id: workshop.id as i64,
                parent_id: Some(campus_node.id),
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
        .expect("create workshop node");

        // ── Draft v2: only CAMPUS — omits WORKSHOP ────────────────────────
        let draft_v2 = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("SV1 draft omits WORKSHOP".to_string()),
            },
            1,
        )
        .await
        .expect("create draft v2");

        node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft_v2.id,
                code: "CAMPUS".to_string(),
                label: "Campus v2".to_string(),
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
        .expect("CAMPUS v2");

        // No WORKSHOP type, no rules — just the root type.

        // ── Validate: must fail ───────────────────────────────────────────
        let result = validation::validate_draft_model_for_publish(&db, draft_v2.id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish, "SV1: can_publish must be false");
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.code == "MISSING_TYPE_CODE"
                    && i.message.contains("WORKSHOP")),
            "SV1: expected MISSING_TYPE_CODE issue referencing WORKSHOP"
        );
    }

    // ── SV2 — Parent-child rule drift blocks publish ──────────────────────

    /// Supervisor V2: Create live nodes whose arrangement is valid under the
    /// active model. In the draft model, remove the rule that allows that
    /// arrangement. Validation must report a blocking PARENT_CHILD_NOT_ALLOWED
    /// issue referencing the affected live node or type pair.
    #[tokio::test]
    async fn sv2_parent_child_rule_drift_blocks_publish() {
        let db = setup().await;

        // ── Active model v1: SITE → BUILDING → FLOOR ─────────────────────
        let model_v1 = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("SV2 base".to_string()),
            },
            1,
        )
        .await
        .expect("create model v1");

        let site = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
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
        .expect("SITE type");

        let building = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "BUILDING".to_string(),
                label: "Building".to_string(),
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
        .expect("BUILDING type");

        let floor = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "FLOOR".to_string(),
                label: "Floor".to_string(),
                icon_key: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("FLOOR type");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_v1.id,
                parent_type_id: site.id,
                child_type_id: building.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("SITE→BUILDING rule");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_v1.id,
                parent_type_id: building.id,
                child_type_id: floor.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("BUILDING→FLOOR rule");

        structure_model::publish_model(&db, model_v1.id, 1)
            .await
            .expect("publish v1");

        // ── Create live nodes: SITE → BUILDING → FLOOR ───────────────────
        let site_node = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "S1".to_string(),
                name: "Site Alpha".to_string(),
                node_type_id: site.id as i64,
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
        .expect("site node");

        let building_node = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "B1".to_string(),
                name: "Building One".to_string(),
                node_type_id: building.id as i64,
                parent_id: Some(site_node.id),
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
        .expect("building node");

        nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "F1".to_string(),
                name: "Floor One".to_string(),
                node_type_id: floor.id as i64,
                parent_id: Some(building_node.id),
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
        .expect("floor node");

        // ── Draft v2: all three types, but REMOVE the BUILDING→FLOOR rule ─
        let draft_v2 = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("SV2 drift — no BUILDING→FLOOR rule".to_string()),
            },
            1,
        )
        .await
        .expect("draft v2");

        let new_site = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft_v2.id,
                code: "SITE".to_string(),
                label: "Site v2".to_string(),
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
        .expect("SITE v2");

        let new_building = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft_v2.id,
                code: "BUILDING".to_string(),
                label: "Building v2".to_string(),
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
        .expect("BUILDING v2");

        let new_floor = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft_v2.id,
                code: "FLOOR".to_string(),
                label: "Floor v2".to_string(),
                icon_key: None,
                depth_hint: Some(2),
                can_host_assets: true,
                can_own_work: false,
                can_carry_cost_center: false,
                can_aggregate_kpis: false,
                can_receive_permits: false,
                is_root_type: false,
            },
        )
        .await
        .expect("FLOOR v2");

        // Only SITE→BUILDING, deliberately omitting BUILDING→FLOOR.
        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft_v2.id,
                parent_type_id: new_site.id,
                child_type_id: new_building.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("SITE→BUILDING rule v2");

        // Also add SITE→FLOOR so FLOOR is reachable (isolate the parent-child check).
        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft_v2.id,
                parent_type_id: new_site.id,
                child_type_id: new_floor.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("SITE→FLOOR rule v2 (reachability only)");

        // ── Validate: must fail with PARENT_CHILD_NOT_ALLOWED ─────────────
        let result = validation::validate_draft_model_for_publish(&db, draft_v2.id as i64)
            .await
            .expect("validate");

        assert!(!result.can_publish, "SV2: can_publish must be false");

        let pc_issues: Vec<&crate::org::validation::OrgValidationIssue> = result
            .issues
            .iter()
            .filter(|i| i.code == "PARENT_CHILD_NOT_ALLOWED")
            .collect();

        assert!(
            !pc_issues.is_empty(),
            "SV2: expected at least one PARENT_CHILD_NOT_ALLOWED issue"
        );
        assert!(
            pc_issues.iter().any(|i| i.message.contains("BUILDING")
                && i.message.contains("FLOOR")),
            "SV2: the issue must reference the BUILDING/FLOOR type pair — got: {:?}",
            pc_issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
    }

    // ── SV3 — Remap after publish (labels differ, IDs change) ─────────────

    /// Supervisor V3: Publish a valid draft where type codes are preserved but
    /// labels differ. Query `org_nodes.node_type_id` before and after publish.
    /// The IDs must change to the new draft rows while the nodes remain intact.
    #[tokio::test]
    async fn sv3_remap_after_publish_preserves_nodes_changes_ids() {
        let db = setup().await;

        // ── Active model v1: REGION (root) → DEPOT ────────────────────────
        let model_v1 = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("SV3 base".to_string()),
            },
            1,
        )
        .await
        .expect("create model v1");

        let region = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "REGION".to_string(),
                label: "Region".to_string(),
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
        .expect("REGION type");

        let depot = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: model_v1.id,
                code: "DEPOT".to_string(),
                label: "Depot".to_string(),
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
        .expect("DEPOT type");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: model_v1.id,
                parent_type_id: region.id,
                child_type_id: depot.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("REGION→DEPOT rule");

        structure_model::publish_model(&db, model_v1.id, 1)
            .await
            .expect("publish v1");

        let old_region_type_id = region.id as i64;
        let old_depot_type_id = depot.id as i64;

        // ── Create live nodes ─────────────────────────────────────────────
        let region_node = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "REG_NORTH".to_string(),
                name: "North Region".to_string(),
                node_type_id: old_region_type_id,
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
        .expect("region node");

        let depot_node = nodes::create_org_node(
            &db,
            CreateOrgNodePayload {
                code: "DPT_01".to_string(),
                name: "Depot 01".to_string(),
                node_type_id: old_depot_type_id,
                parent_id: Some(region_node.id),
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
        .expect("depot node");

        // ── Capture pre-publish type IDs ──────────────────────────────────
        let pre_region = nodes::get_org_node_by_id(&db, region_node.id)
            .await
            .expect("pre region");
        let pre_depot = nodes::get_org_node_by_id(&db, depot_node.id)
            .await
            .expect("pre depot");

        assert_eq!(pre_region.node_type_id, old_region_type_id);
        assert_eq!(pre_depot.node_type_id, old_depot_type_id);

        // ── Draft v2: same codes, DIFFERENT labels ────────────────────────
        let draft_v2 = structure_model::create_model(
            &db,
            CreateStructureModelPayload {
                description: Some("SV3 relabelled draft".to_string()),
            },
            1,
        )
        .await
        .expect("draft v2");

        let new_region = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft_v2.id,
                code: "REGION".to_string(),
                label: "Regional Hub".to_string(), // label changed
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
        .expect("REGION v2");

        let new_depot = node_types::create_node_type(
            &db,
            CreateNodeTypePayload {
                structure_model_id: draft_v2.id,
                code: "DEPOT".to_string(),
                label: "Distribution Depot".to_string(), // label changed
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
        .expect("DEPOT v2");

        relationship_rules::create_rule(
            &db,
            CreateRelationshipRulePayload {
                structure_model_id: draft_v2.id,
                parent_type_id: new_region.id,
                child_type_id: new_depot.id,
                min_children: None,
                max_children: None,
            },
        )
        .await
        .expect("REGION→DEPOT rule v2");

        let new_region_type_id = new_region.id as i64;
        let new_depot_type_id = new_depot.id as i64;

        // IDs must differ between v1 and v2 rows.
        assert_ne!(old_region_type_id, new_region_type_id, "type IDs must differ between model versions");
        assert_ne!(old_depot_type_id, new_depot_type_id, "type IDs must differ between model versions");

        // ── Publish with remap ────────────────────────────────────────────
        let result = validation::publish_model_with_remap(&db, draft_v2.id as i64, 1)
            .await
            .expect("publish with remap");

        assert!(result.can_publish, "SV3: publish must succeed");
        assert_eq!(result.remap_count, 2, "SV3: two type-to-type remaps");

        // ── Verify post-publish: IDs changed, nodes intact ────────────────
        let post_region = nodes::get_org_node_by_id(&db, region_node.id)
            .await
            .expect("post region");
        let post_depot = nodes::get_org_node_by_id(&db, depot_node.id)
            .await
            .expect("post depot");

        // IDs must now point to v2 type rows.
        assert_eq!(
            post_region.node_type_id, new_region_type_id,
            "SV3: region node must reference new REGION type ID"
        );
        assert_eq!(
            post_depot.node_type_id, new_depot_type_id,
            "SV3: depot node must reference new DEPOT type ID"
        );

        // Old IDs must no longer be referenced.
        assert_ne!(post_region.node_type_id, old_region_type_id);
        assert_ne!(post_depot.node_type_id, old_depot_type_id);

        // Nodes themselves are intact — same code, name, hierarchy.
        assert_eq!(post_region.code, "REG_NORTH");
        assert_eq!(post_region.name, "North Region");
        assert_eq!(post_depot.code, "DPT_01");
        assert_eq!(post_depot.name, "Depot 01");
        assert_eq!(post_depot.parent_id, Some(region_node.id));
    }
}
